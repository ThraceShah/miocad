use cad_protocol::*;
use cad_ui_egui::{Action, Controller, Event, Failure, Transport, Workspace};
use eframe::egui;
use futures_util::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use wasm_bindgen::{prelude::*, JsCast};
thread_local! { static UI_GEOMETRY: RefCell<String> = RefCell::new("{}".into()); }
/// Read-only screen-space geometry for assistive tools and GUI automation.
#[wasm_bindgen]
pub fn ui_geometry() -> String {
    UI_GEOMETRY.with(|v| v.borrow().clone())
}
fn failure(message: impl ToString) -> Failure {
    Failure {
        kind: "service_unavailable".into(),
        message: message.to_string(),
        request: None,
    }
}
fn base() -> String {
    js_sys::Reflect::get(&web_sys::window().unwrap(), &"cadBase".into())
        .unwrap()
        .as_string()
        .unwrap()
}
async fn http(path: &str, body: Value) -> Result<gloo_net::http::Response, Failure> {
    let work = async {
        let response = gloo_net::http::Request::post(&format!("{}api/{path}", base()))
            .header("X-CAD-Web", "1")
            .header(
                "X-CAD-View",
                &js_sys::Reflect::get(&web_sys::window().unwrap(), &"cadView".into())
                    .unwrap()
                    .as_string()
                    .unwrap(),
            )
            .json(&body)
            .map_err(failure)?
            .send()
            .await
            .map_err(failure)?;
        if !response.ok() {
            return Err(failure(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }
        Ok(response)
    }
    .fuse();
    let deadline = gloo_timers::future::TimeoutFuture::new(20_000).fuse();
    futures_util::pin_mut!(work, deadline);
    futures_util::select! {reply=work=>reply,_=deadline=>Err(failure("Request timed out. Reconnect; nothing will be replayed."))}
}
async fn json_http(path: &str, body: Value) -> Result<Value, Failure> {
    http(path, body).await?.json().await.map_err(failure)
}
struct BrowserTransport {
    context: Option<Context>,
    next: u64,
    poisoned: bool,
}
impl BrowserTransport {
    async fn send(&mut self, request: Request) -> Result<Response, Failure> {
        if self.poisoned {
            return Err(failure("Reconnect before issuing another request"));
        }
        let value = request.value().map_err(failure)?;
        self.poisoned = true;
        let exchange = async {
            let response = Response::parse(json_http("rpc", value).await?).map_err(failure)?;
            if Some(&response.context) != self.context.as_ref()
                || response.request_id != request.request_id
            {
                return Err(failure("Response correlation/instance mismatch"));
            }
            if response.ok {
                if let Some(command) = &request.command {
                    let command = command.value().map_err(failure)?;
                    validate(
                        response
                            .result
                            .as_ref()
                            .ok_or_else(|| failure("Missing result"))?,
                        &command_spec(command["op"].as_str().unwrap()).map_err(failure)?
                            ["output_schema"],
                    )
                    .map_err(failure)?;
                } else {
                    validate_ref(
                        response
                            .result
                            .as_ref()
                            .ok_or_else(|| failure("Missing status"))?,
                        "StatusResult",
                    )
                    .map_err(failure)?;
                }
            }
            Ok(response)
        }
        .await;
        match exchange {
            Ok(response) => {
                self.poisoned = false;
                Ok(response)
            }
            Err(error) => Err(Failure {
                kind: "outcome_unknown".into(),
                message: error.message,
                request: Some(Box::new(request)),
            }),
        }
    }
    fn prepare(&mut self, command: Command) -> Result<Request, Failure> {
        if self.poisoned {
            return Err(failure("Reconnect to reconcile the previous request first"));
        }
        command.value().map_err(failure)?;
        let context = self
            .context
            .as_ref()
            .ok_or_else(|| failure("Not connected"))?;
        let request = Request::execute(
            context,
            RequestId::new(self.next).map_err(failure)?,
            command,
        );
        self.next += 1;
        Ok(request)
    }
}
impl Transport for BrowserTransport {
    async fn reconnect(&mut self) -> Result<Context, Failure> {
        let response = Response::parse(json_http("hello", json!({})).await?).map_err(failure)?;
        if !response.ok {
            return Err(failure("Worker hello rejected"));
        }
        if self
            .context
            .as_ref()
            .is_some_and(|old| old != &response.context)
        {
            return Err(failure(
                "Service identity changed. Lock and unlock to authorize a new instance; no replay.",
            ));
        }
        let result = response
            .result
            .as_ref()
            .ok_or_else(|| failure("Missing hello result"))?;
        validate_ref(result, "HelloResult").map_err(failure)?;
        self.next = result["last_request_id"]
            .as_u64()
            .ok_or_else(|| failure("Missing high-water"))?
            + 1;
        self.context = Some(response.context.clone());
        self.poisoned = false;
        Ok(response.context)
    }
    async fn execute(&mut self, command: Command) -> Result<Response, Failure> {
        let request = self.prepare(command)?;
        self.send(request).await
    }
    async fn status(&mut self, target: RequestId) -> Result<Response, Failure> {
        let mut request = self.prepare(Command::Capabilities)?;
        request.method = "request.status".into();
        request.command = None;
        request.target_request_id = Some(target);
        self.send(request).await
    }
    async fn snapshot(&mut self) -> Result<Value, Failure> {
        let value = json_http("snapshot", json!({})).await?;
        cad_protocol::service::validate_result("snapshot", &value).map_err(failure)?;
        Ok(value)
    }
    async fn events(&mut self, after: u64) -> Result<Value, Failure> {
        let value = json_http("events", json!({"after":after})).await?;
        cad_protocol::service::validate_result("events", &value).map_err(failure)?;
        Ok(value)
    }
    async fn download(&mut self, document: DocumentId) -> Result<Vec<u8>, Failure> {
        http("download", json!({"document":document}))
            .await?
            .binary()
            .await
            .map_err(failure)
    }
}
async fn run(
    mut actions: futures_channel::mpsc::UnboundedReceiver<Action>,
    output: Rc<RefCell<VecDeque<Event>>>,
    ctx: egui::Context,
) {
    let mut controller = Controller::new(
        BrowserTransport {
            context: None,
            next: 1,
            poisoned: false,
        },
        move |event| {
            output.borrow_mut().push_back(event);
            ctx.request_repaint();
            true
        },
    );
    if let Err(e) = controller.reconnect().await {
        controller.emit_error(None, false, e);
    }
    loop {
        let tick = gloo_timers::future::TimeoutFuture::new(450).fuse();
        let action = actions.next().fuse();
        futures_util::pin_mut!(tick, action);
        futures_util::select! {value=action=>{let Some(value)=value else{break};controller.act(value).await;},_=tick=>{if controller.polling {if let Err(e)=controller.poll().await{controller.emit_error(None,false,e);}}}}
    }
}
#[derive(Serialize, Deserialize)]
struct Rendered {
    epoch: u64,
    document: DocumentId,
    revision: Revision,
    preview: bool,
    width: u32,
    height: u32,
    selected: Option<String>,
    frame: cad_render::Frame,
}
struct Browser {
    workspace: Workspace,
    actions: futures_channel::mpsc::UnboundedSender<Action>,
    events: Rc<RefCell<VecDeque<Event>>>,
    worker: web_sys::Worker,
    rendering: Rc<Cell<bool>>,
    scheduled: u64,
    observed_epoch: u64,
    changed_at: f64,
    _message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _error: Closure<dyn FnMut(web_sys::Event)>,
}
impl Browser {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, JsValue> {
        Workspace::theme(&cc.egui_ctx);
        let gpu = cc
            .wgpu_render_state
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Client GPU unavailable"))?;
        cad_ui_egui::local_view::install(gpu);
        let (sender, receiver) = futures_channel::mpsc::unbounded();
        let events = Rc::new(RefCell::new(VecDeque::new()));
        wasm_bindgen_futures::spawn_local(run(receiver, events.clone(), cc.egui_ctx.clone()));
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);
        let worker =
            web_sys::Worker::new_with_options(&format!("{}render-worker.js", base()), &options)?;
        let rendering = Rc::new(Cell::new(false));
        let busy = rendering.clone();
        let output = events.clone();
        let ctx = cc.egui_ctx.clone();
        let message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            busy.set(false);
            let result = (|| -> Result<Event, String> {
                if js_sys::Reflect::get(&event.data(), &"ok".into())
                    .map_err(|e| format!("{e:?}"))?
                    .as_bool()
                    != Some(true)
                {
                    return Err(js_sys::Reflect::get(&event.data(), &"error".into())
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or_else(|| "Viewport worker failed".into()));
                }
                let value = js_sys::Reflect::get(&event.data(), &"result".into())
                    .map_err(|e| format!("{e:?}"))?
                    .as_string()
                    .ok_or("Missing rendered frame")?;
                let rendered: Rendered = serde_json::from_str(&value).map_err(|e| e.to_string())?;
                rendered.frame.validate()?;
                let rgb = rendered.frame.viewport_rgb(rendered.selected.as_deref());
                Ok(Event::View {
                    epoch: rendered.epoch,
                    frame: Arc::new(rendered.frame),
                    rgb,
                    width: rendered.width,
                    height: rendered.height,
                    document: rendered.document,
                    revision: rendered.revision,
                    preview: rendered.preview,
                })
            })();
            output
                .borrow_mut()
                .push_back(result.unwrap_or_else(|message| Event::Failed {
                    ticket: None,
                    preview: false,
                    failure: Failure {
                        kind: "render_failed".into(),
                        message,
                        request: None,
                    },
                }));
            ctx.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onmessage(Some(message.as_ref().unchecked_ref()));
        let busy = rendering.clone();
        let output = events.clone();
        let ctx = cc.egui_ctx.clone();
        let error = Closure::wrap(Box::new(move |_: web_sys::Event| {
            busy.set(false);
            output.borrow_mut().push_back(Event::Failed {
                ticket: None,
                preview: false,
                failure: Failure {
                    kind: "render_failed".into(),
                    message:
                        "Viewport Web Worker unavailable. Reload after checking browser support."
                            .into(),
                    request: None,
                },
            });
            ctx.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onerror(Some(error.as_ref().unchecked_ref()));
        let mut workspace = Workspace::default();
        workspace.browser_downloads = true;
        workspace.local_backend = Some(format!("{:?}", gpu.adapter.get_info().backend));
        Ok(Self {
            workspace,
            actions: sender,
            events,
            worker,
            rendering,
            scheduled: u64::MAX,
            observed_epoch: u64::MAX,
            changed_at: 0.,
            _message: message,
            _error: error,
        })
    }
}
fn download(bytes: &[u8]) -> Result<(), JsValue> {
    let data = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&data.buffer());
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let document = web_sys::window().unwrap().document().unwrap();
    let anchor = document
        .create_element("a")?
        .dyn_into::<web_sys::HtmlAnchorElement>()?;
    anchor.set_href(&url);
    anchor.set_download("model.smb");
    document.body().unwrap().append_child(&anchor)?;
    anchor.click();
    anchor.remove();
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(30_000).await;
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    Ok(())
}
impl eframe::App for Browser {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Some(event) = { self.events.borrow_mut().pop_front() } {
            if let Event::Download { ticket, ref bytes } = event {
                if let Err(e) = download(bytes) {
                    self.workspace.apply(
                        Event::Failed {
                            ticket: Some(ticket),
                            preview: false,
                            failure: Failure {
                                kind: "download_failed".into(),
                                message: format!("{e:?}"),
                                request: None,
                            },
                        },
                        ctx,
                    );
                    continue;
                }
            }
            self.workspace.apply(event, ctx);
        }
        for action in self.workspace.show(ctx) {
            let _ = self.actions.unbounded_send(action);
        }
        if let Ok(value) = serde_json::to_string(&self.workspace.geometry) {
            UI_GEOMETRY.with(|v| *v.borrow_mut() = value);
        }

        // Display is entirely local. Request an authoritative picking/capture frame only
        // after the camera settles; never queue network work at pointer-event frequency.
        let now = ctx.input(|i| i.time);
        if self.observed_epoch != self.workspace.model.view_epoch {
            self.observed_epoch = self.workspace.model.view_epoch;
            self.changed_at = now;
        }
        let settled = now - self.changed_at >= 0.15;
        if !settled {
            ctx.request_repaint_after(Duration::from_millis(160));
        }
        if settled && !self.rendering.get() && self.scheduled != self.workspace.model.view_epoch {
            self.scheduled = self.workspace.model.view_epoch;
            if let Some(request) = self.workspace.render_request() {
                match serde_json::to_string(&request)
                    .map_err(|e| JsValue::from_str(&e.to_string()))
                    .and_then(|json| {
                        self.worker.post_message(&{
                            let object = js_sys::Object::new();
                            js_sys::Reflect::set(&object, &"job".into(), &json.into())?;
                            let view = js_sys::Reflect::get(
                                &web_sys::window().unwrap(),
                                &"cadView".into(),
                            )?;
                            js_sys::Reflect::set(&object, &"view".into(), &view)?;
                            object.into()
                        })
                    }) {
                    Ok(()) => self.rendering.set(true),
                    Err(e) => self.workspace.apply(
                        Event::Failed {
                            ticket: None,
                            preview: false,
                            failure: Failure {
                                kind: "render_failed".into(),
                                message: format!("{e:?}"),
                                request: None,
                            },
                        },
                        ctx,
                    ),
                }
            }
        }
        if self.workspace.model.busy || self.workspace.model.connecting {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        self.worker.terminate();
    }
}
#[wasm_bindgen]
pub async fn start_web() -> Result<(), JsValue> {
    let canvas = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id("cad-canvas")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions {
                depth_buffer: 24,
                ..Default::default()
            },
            Box::new(|cc| {
                Browser::new(cc)
                    .map(|app| Box::new(app) as Box<dyn eframe::App>)
                    .map_err(|e| format!("{e:?}").into())
            }),
        )
        .await
}
