//! Native transport/runtime boundary. The egui crate owns no sockets or processes.
use cad_access::Config;
use cad_client::{protocol::*, ClientError, SharedClient};
use cad_ui_egui::{Action, Controller, Event, Failure, Transport, ViewRequest};
use std::{
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::sync::mpsc as asynchronous;
pub struct Bridge {
    pub events: mpsc::Receiver<Event>,
    commands: asynchronous::Sender<Action>,
    views: mpsc::Sender<ViewRequest>,
    host_config: Config,
    host_output: Output,
    tasks: std::sync::Mutex<Option<cad_agent::host::Tasks>>,
    task_namespace: String,
}
impl Bridge {
    pub fn start(config: Config, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (commands, receiver) = asynchronous::channel(32);
        let (sender, events) = mpsc::sync_channel(32);
        let (views, render_rx) = mpsc::channel::<ViewRequest>();
        let output = Output { sender, wake };
        let host_config = config.clone();
        let host_output = output.clone();
        let renders = output.clone();
        let render_config = config.clone();
        std::thread::spawn(move || {
            while let Ok(mut job) = render_rx.recv() {
                for latest in render_rx.try_iter() {
                    job = latest;
                }
                let result = (|| -> Result<cad_render::Frame, String> {
                    let endpoint: cad_protocol::service::ServiceEndpoint = serde_json::from_slice(
                        &std::fs::read(render_config.options.runtime_dir.join("endpoint.json"))
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    if endpoint.service_id != job.service {
                        return Err("service identity changed; reconnect".into());
                    }
                    let view = cad_protocol::service::Viewport {
                        document: job.bound.document.clone(),
                        revision: job.bound.revision,
                        width: job.width,
                        height: job.height,
                        camera: serde_json::to_value(job.camera).map_err(|e| e.to_string())?,
                        epoch: job.epoch,
                        view_id: render_config.client_id.clone(),
                        preview: job.preview_operation.clone(),
                        backend: "auto".into(),
                    };
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    let result = runtime
                        .block_on(SharedClient::control(
                            &endpoint,
                            cad_protocol::service::ServiceRequest::RenderView {
                                session: job.session.clone(),
                                view,
                            },
                        ))
                        .map_err(|e| e.to_string())?;
                    serde_json::from_value(result).map_err(|e| e.to_string())
                })();
                match result {
                    Ok(frame) => {
                        if let Err(message) = frame.validate() {
                            renders.send(Event::Failed {
                                ticket: None,
                                preview: false,
                                failure: Failure {
                                    kind: "render_failed".into(),
                                    message,
                                    request: None,
                                },
                            });
                            continue;
                        }
                        let rgb = frame.viewport_rgb(job.selected.as_deref());
                        if !renders.send(Event::View {
                            epoch: job.epoch,
                            frame: Arc::new(frame),
                            rgb,
                            width: job.width,
                            height: job.height,
                            document: job.bound.document,
                            revision: job.bound.revision,
                            preview: job.preview,
                        }) {
                            break;
                        }
                    }
                    Err(message) => {
                        if !renders.send(Event::Failed {
                            ticket: None,
                            preview: false,
                            failure: Failure {
                                kind: "render_failed".into(),
                                message,
                                request: None,
                            },
                        }) {
                            break;
                        }
                    }
                }
            }
        });
        std::thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(run(config, output, receiver)),
                Err(error) => {
                    output.send(Event::Failed {
                        ticket: None,
                        preview: false,
                        failure: Failure {
                            kind: "service_unavailable".into(),
                            message: error.to_string(),
                            request: None,
                        },
                    });
                }
            }
        });
        Self {
            events,
            commands,
            views,
            host_config,
            host_output,
            tasks: std::sync::Mutex::new(None),
            task_namespace: cad_client::shared::unique_id().expect("OS random source unavailable"),
        }
    }
    pub fn send(&self, action: Action) -> Result<(), String> {
        match action {
            Action::AskAgent {
                id,
                turn,
                service,
                session,
            } => {
                let endpoint: cad_protocol::service::ServiceEndpoint = serde_json::from_slice(
                    &std::fs::read(self.host_config.options.runtime_dir.join("endpoint.json"))
                        .map_err(|_| "CAD endpoint unavailable")?,
                )
                .map_err(|_| "Invalid CAD endpoint")?;
                if endpoint.service_id != service {
                    return Err("CAD service changed; reconnect".into());
                }
                let host = cad_agent::host::Tasks::new(
                    self.host_config.options.runtime_dir.join("agent-tasks"),
                    endpoint,
                    session,
                );
                *self.tasks.lock().unwrap() = Some(host.clone());
                self.run_agent(
                    host,
                    id,
                    format!("{}-{id}", self.task_namespace),
                    Some(turn),
                    false,
                );
                Ok(())
            }
            Action::UndoAgent { id, task } => {
                let host = self
                    .tasks
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or("Agent task host unavailable")?;
                self.run_agent(host, id, task, None, true);
                Ok(())
            }
            Action::ChooseAttachments => {
                self.host_output.send(Event::AttachmentError("Drag text or image files from your file manager into the CAD window to attach them.".into()));
                Ok(())
            }
            Action::LoadAttachments(paths) => {
                let mut files = Vec::new();
                for p in paths.into_iter().take(8) {
                    let result = (|| -> Result<cad_agent::Attachment, String> {
                        if std::fs::metadata(&p)
                            .map_err(|_| "Cannot open attachment")?
                            .len()
                            > 2_250_000
                        {
                            return Err("Attachment exceeds size limit".into());
                        }
                        cad_agent::Attachment::from_bytes(
                            p.file_name().unwrap_or_default().to_string_lossy().into(),
                            &std::fs::read(p).map_err(|_| "Cannot read attachment")?,
                        )
                    })();
                    match result {
                        Ok(a) => files.push(a),
                        Err(e) => {
                            self.host_output.send(Event::AttachmentError(e));
                        }
                    }
                }
                self.host_output.send(Event::Attachments(files));
                Ok(())
            }
            _ => self.commands.try_send(action).map_err(|e| e.to_string()),
        }
    }
    fn run_agent(
        &self,
        host: cad_agent::host::Tasks,
        id: u64,
        task: String,
        turn: Option<cad_agent::Turn>,
        undo: bool,
    ) {
        let output = self.host_output.clone();
        let owner = self.host_config.client_id.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let mut result = if let Some(turn) = turn {
                    host.start(
                        &owner,
                        cad_agent::TaskRequest {
                            id: task.clone(),
                            turn,
                        },
                    )
                    .await
                } else if undo {
                    host.undo(&owner, &task).await
                } else {
                    host.status(&owner, &task).await
                };
                loop {
                    let done = result.as_ref().map_or(true, |s| s.done);
                    let event = Event::AgentReply { id, result };
                    let connected = if done {
                        output.send(event)
                    } else {
                        output.progress(event)
                    };
                    if !connected || done {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    result = host.status(&owner, &task).await;
                }
            });
        });
    }
    pub fn render(&self, view: ViewRequest) -> Result<(), String> {
        self.views.send(view).map_err(|e| e.to_string())
    }
}
#[derive(Clone)]
struct Output {
    sender: mpsc::SyncSender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Output {
    // A minimized window must not stall autonomous modeling when progress fills the UI queue.
    fn progress(&self, event: Event) -> bool {
        match self.sender.try_send(event) {
            Ok(()) => {
                (self.wake)();
                true
            }
            Err(mpsc::TrySendError::Full(_)) => true,
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        }
    }
    fn send(&self, event: Event) -> bool {
        let sent = self.sender.send(event).is_ok();
        if sent {
            (self.wake)();
        }
        sent
    }
}
fn failure(error: ClientError) -> Failure {
    match error {
        ClientError::OutcomeUnknown { request, reason } => Failure {
            kind: "outcome_unknown".into(),
            message: reason,
            request: Some(request),
        },
        ClientError::ConnectionUnusable => Failure {
            kind: "connection_unusable".into(),
            message: "Reconnect and inspect the original request before editing again.".into(),
            request: None,
        },
        ClientError::InvalidRequest(e) => Failure {
            kind: "invalid_params".into(),
            message: e.to_string(),
            request: None,
        },
        ClientError::Transport(message) => {
            let kind = if message.contains("worker_lost") {
                "worker_lost"
            } else {
                "service_unavailable"
            };
            Failure {
                kind: kind.into(),
                message,
                request: None,
            }
        }
    }
}

struct NativeTransport {
    config: Config,
    client: Option<SharedClient>,
}
impl NativeTransport {
    fn client(&mut self) -> Result<&mut SharedClient, Failure> {
        self.client
            .as_mut()
            .ok_or_else(|| failure(ClientError::Transport("not connected".into())))
    }
}
impl Transport for NativeTransport {
    async fn reconnect(&mut self) -> Result<Context, Failure> {
        let next = if let Some(old) = &self.client {
            old.reconnect(&self.config.options).await.map_err(failure)?
        } else {
            let client =
                SharedClient::connect_or_start(&self.config.options, &self.config.client_id)
                    .await
                    .map_err(failure)?;
            let session = if self.config.isolated {
                Some(
                    client
                        .create_session(&self.config.client_id)
                        .await
                        .map_err(failure)?,
                )
            } else {
                self.config.session.clone()
            };
            if let Some(session) = session {
                SharedClient::attach(
                    client.endpoint().clone(),
                    &self.config.client_id,
                    Some(&session),
                )
                .await
                .map_err(failure)?
            } else {
                client
            }
        };
        let mut context = next.context().clone();
        context.service_id = Some(
            ServiceId::new(next.endpoint().service_id.clone())
                .map_err(|e| failure(ClientError::InvalidRequest(e)))?,
        );
        self.client = Some(next);
        Ok(context)
    }
    async fn execute(&mut self, command: Command) -> Result<Response, Failure> {
        self.client()?.execute(command).await.map_err(failure)
    }
    async fn status(&mut self, target: RequestId) -> Result<Response, Failure> {
        self.client()?.status(target).await.map_err(failure)
    }
    async fn snapshot(&mut self) -> Result<serde_json::Value, Failure> {
        self.client()?.snapshot().await.map_err(failure)
    }
    async fn events(&mut self, after: u64) -> Result<serde_json::Value, Failure> {
        self.client()?.events(after).await.map_err(failure)
    }
}
async fn run(config: Config, output: Output, mut actions: asynchronous::Receiver<Action>) {
    let mut controller = Controller::new(
        NativeTransport {
            config,
            client: None,
        },
        move |event| output.send(event),
    );
    if let Err(e) = controller.reconnect().await {
        controller.emit_error(None, false, e);
    }
    let mut interval = tokio::time::interval(Duration::from_millis(450));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            action=actions.recv()=>{let Some(action)=action else{break};controller.act(action).await;},
            _=interval.tick(),if controller.polling=>{if let Err(e)=controller.poll().await{controller.emit_error(None,false,e);}}
        }
    }
}
