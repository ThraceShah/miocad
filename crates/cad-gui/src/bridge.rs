//! Native transport/runtime boundary. The egui crate owns no sockets or processes.
use cad_access::Config;
use cad_client::{protocol::*, ClientError, SharedClient};
use cad_ui_egui::{Action, BoundScene, Event, Failure, ViewRequest};
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
                Ok(runtime) => runtime.block_on(Backend::new(config, output).run(receiver)),
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
fn remote(response: Response) -> Result<Response, Failure> {
    if response.ok {
        Ok(response)
    } else {
        let error = response.error.unwrap();
        Err(Failure {
            kind: serde_json::to_value(error.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            message: error.message,
            request: None,
        })
    }
}
fn decode<T: serde::de::DeserializeOwned>(response: Response) -> Result<T, Failure> {
    let response = remote(response)?;
    response.result_as().map_err(|e| Failure {
        kind: "invalid_response".into(),
        message: e.to_string(),
        request: None,
    })
}
struct Backend {
    config: Config,
    output: Output,
    client: Option<SharedClient>,
    active: Option<DocumentId>,
    last_scene: Option<(DocumentId, Revision)>,
    cursor: u64,
    polling: bool,
    uncertain: Option<Request>,
}
impl Backend {
    fn new(config: Config, output: Output) -> Self {
        Self {
            config,
            output,
            client: None,
            active: None,
            last_scene: None,
            cursor: 0,
            polling: false,
            uncertain: None,
        }
    }
    async fn run(mut self, mut actions: asynchronous::Receiver<Action>) {
        if let Err(error) = self.reconnect().await {
            self.emit_error(None, false, error);
        }
        let mut interval = tokio::time::interval(Duration::from_millis(450));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                action=actions.recv()=>{let Some(action)=action else{break};self.act(action).await;},
                _=interval.tick(),if self.polling=>{if let Err(error)=self.poll().await{self.emit_error(None,false,error);}}
            }
        }
    }
    fn emit_error(&mut self, ticket: Option<u64>, preview: bool, error: Failure) {
        if let Some(request) = &error.request {
            self.uncertain = Some((**request).clone());
        }
        if matches!(
            error.kind.as_str(),
            "service_unavailable" | "outcome_unknown" | "connection_unusable" | "worker_lost"
        ) {
            self.polling = false;
        }
        self.output.send(Event::Failed {
            ticket,
            preview,
            failure: error,
        });
    }
    fn client(&mut self) -> Result<&mut SharedClient, Failure> {
        self.client.as_mut().ok_or_else(|| Failure {
            kind: "service_unavailable".into(),
            message: "Connect to the shared service first.".into(),
            request: None,
        })
    }
    async fn reconnect(&mut self) -> Result<(), Failure> {
        let mut client = if let Some(old) = &self.client {
            let next = SharedClient::attach(
                old.endpoint().clone(),
                old.context().client_id.as_str(),
                Some(old.context().session_id.as_str()),
            )
            .await
            .map_err(failure)?;
            if next.context() != old.context() {
                return Err(Failure{kind:"service_unavailable".into(),message:"Service/worker identity changed. Old requests will not be replayed. Reopen to attach a new instance.".into(),request:None});
            }
            next
        } else {
            let mut client =
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
                client = SharedClient::attach(
                    client.endpoint().clone(),
                    &self.config.client_id,
                    Some(&session),
                )
                .await
                .map_err(failure)?;
            }
            client
        };
        let mut reconciliation = None;
        if let Some(request) = &self.uncertain {
            let status = client.status(request.request_id).await.map_err(failure)?;
            let status: RequestStatus = decode(status)?;
            reconciliation=Some(match status{
                RequestStatus::Completed{response}=>{self.uncertain=None;(true,format!("Original request completed (ok={}). State refreshed; nothing was replayed.",response.ok))},
                RequestStatus::OutcomeUnavailable=>(false,"Original result unavailable. Inspect refreshed state before continuing; no command was replayed.".into()),
            });
        }
        self.output.send(Event::Connected {
            service: client.endpoint().service_id.clone(),
            session: client.context().session_id.as_str().into(),
        });
        self.client = Some(client);
        self.polling = true;
        self.last_scene = None;
        self.refresh(true).await?;
        if let Some((completed, message)) = reconciliation {
            self.output.send(Event::Reconciled { completed, message });
        }
        Ok(())
    }
    async fn poll(&mut self) -> Result<(), Failure> {
        let cursor = self.cursor;
        let changes = self.client()?.events(cursor).await.map_err(failure)?;
        if changes["resync_required"] == true || changes["cursor"].as_u64() != Some(self.cursor) {
            self.refresh(false).await?;
        }
        Ok(())
    }
    async fn refresh(&mut self, force: bool) -> Result<(), Failure> {
        let snapshot = self.client()?.snapshot().await.map_err(failure)?;
        let documents: Vec<DocumentResult<DocumentState>> =
            serde_json::from_value(snapshot["documents"].clone()).map_err(|e| Failure {
                kind: "invalid_response".into(),
                message: e.to_string(),
                request: None,
            })?;
        self.cursor = snapshot["cursor"].as_u64().ok_or_else(|| Failure {
            kind: "invalid_response".into(),
            message: "Missing snapshot cursor".into(),
            request: None,
        })?;
        if !self
            .active
            .as_ref()
            .is_some_and(|id| documents.iter().any(|d| &d.document == id))
        {
            self.active = documents.first().map(|d| d.document.clone());
        }
        let selected = documents
            .iter()
            .find(|d| Some(&d.document) == self.active.as_ref())
            .map(|d| (d.document.clone(), d.revision));
        self.output.send(Event::Documents {
            documents,
            cursor: self.cursor,
        });
        if let Some((document, revision)) = selected {
            if force || self.last_scene.as_ref() != Some(&(document.clone(), revision)) {
                let response = self
                    .client()?
                    .execute(Command::Scene {
                        document: document.clone(),
                    })
                    .await
                    .map_err(failure)?;
                let result: DocumentResult<Scene> = decode(response)?;
                self.last_scene = Some((document, result.revision));
                self.output.send(Event::Scene(BoundScene {
                    document: result.document,
                    revision: result.revision,
                    scene: Arc::new(result.data),
                }));
            }
        }
        Ok(())
    }
    async fn act(&mut self, action: Action) {
        let (ticket, preview, result) = match action {
            Action::Select(document) => {
                self.active = Some(document);
                let result = self.refresh(true).await;
                if let Err(e) = result {
                    self.emit_error(None, false, e);
                }
                return;
            }
            Action::Reconnect { ticket } => {
                let result = self.reconnect().await;
                (ticket, false, result)
            }
            Action::Refresh { ticket } => {
                let result = self.refresh(true).await;
                (ticket, false, result)
            }
            Action::NewDocument { ticket } => {
                let result = async {
                    let reply = self
                        .client()?
                        .execute(Command::DocumentCreate)
                        .await
                        .map_err(failure)?;
                    let doc: CreatedDocument = decode(reply)?;
                    self.active = Some(doc.document.clone());
                    self.output.send(Event::Created {
                        ticket,
                        document: doc.document,
                    });
                    self.refresh(true).await
                }
                .await;
                if let Err(e) = result {
                    self.emit_error(Some(ticket), false, e);
                }
                return;
            }
            Action::Preview { ticket, command } => {
                let result = async {
                    let reply = self.client()?.execute(command).await.map_err(failure)?;
                    let scene: DocumentResult<Scene> = decode(reply)?;
                    self.output.send(Event::Preview {
                        ticket,
                        scene: BoundScene {
                            document: scene.document,
                            revision: scene.revision,
                            scene: Arc::new(scene.data),
                        },
                    });
                    Ok::<(), Failure>(())
                }
                .await;
                (ticket, true, result)
            }
            Action::Execute {
                ticket,
                command,
                label,
            } => {
                let result = async {
                    let reply = self.client()?.execute(command).await.map_err(failure)?;
                    remote(reply)?;
                    self.output.send(Event::Completed {
                        ticket,
                        message: label,
                    });
                    if let Err(mut error) = self.refresh(true).await {
                        error.message = format!(
                            "Model command acknowledged. Refresh failed: {}",
                            error.message
                        );
                        self.emit_error(None, false, error);
                    }
                    Ok::<(), Failure>(())
                }
                .await;
                if let Err(e) = result {
                    self.emit_error(Some(ticket), false, e);
                    if self.polling {
                        let _ = self.refresh(true).await;
                    }
                }
                return;
            }
        };
        match result {
            Ok(()) => {
                if !preview {
                    self.output.send(Event::Completed {
                        ticket,
                        message: "Workspace synchronized.".into(),
                    });
                }
            }
            Err(e) => {
                self.emit_error(Some(ticket), preview, e);
                if self.polling {
                    let _ = self.refresh(true).await;
                }
            }
        }
    }
}
