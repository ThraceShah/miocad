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
}
impl Bridge {
    pub fn start(config: Config, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (commands, receiver) = asynchronous::channel(32);
        let (sender, events) = mpsc::sync_channel(32);
        let (views, render_rx) = mpsc::channel::<ViewRequest>();
        let output = Output { sender, wake };
        let renders = output.clone();
        std::thread::spawn(move || {
            while let Ok(mut job) = render_rx.recv() {
                for latest in render_rx.try_iter() {
                    job = latest;
                }
                let Some(mesh) = &job.bound.scene.mesh else {
                    continue;
                };
                match cad_render::render_view(
                    mesh,
                    &job.bound.scene.edges,
                    job.width,
                    job.height,
                    job.camera,
                ) {
                    Ok(frame) => {
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
        }
    }
    pub fn send(&self, action: Action) -> Result<(), String> {
        self.commands.try_send(action).map_err(|e| e.to_string())
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
            let next = SharedClient::attach(
                old.endpoint().clone(),
                old.context().client_id.as_str(),
                Some(old.context().session_id.as_str()),
            )
            .await
            .map_err(failure)?;
            if next.context() != old.context() {
                return Err(failure(ClientError::Transport(
                    "Service/worker identity changed; reopen to attach a new instance. No replay."
                        .into(),
                )));
            }
            next
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
