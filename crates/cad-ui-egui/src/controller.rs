//! Shared asynchronous workspace policy. Hosts supply transport and scheduling only.
use crate::{Action, BoundScene, Event, Failure};
use cad_protocol::*;
use std::sync::Arc;
#[allow(async_fn_in_trait)]
pub trait Transport {
    async fn reconnect(&mut self) -> Result<Context, Failure>;
    async fn execute(&mut self, command: Command) -> Result<Response, Failure>;
    async fn status(&mut self, target: RequestId) -> Result<Response, Failure>;
    async fn snapshot(&mut self) -> Result<serde_json::Value, Failure>;
    async fn events(&mut self, after: u64) -> Result<serde_json::Value, Failure>;
    async fn download(&mut self, _document: DocumentId) -> Result<Vec<u8>, Failure> {
        Err(Failure {
            kind: "unsupported".into(),
            message: "This host does not provide browser downloads".into(),
            request: None,
        })
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
pub struct Controller<T: Transport> {
    output: Box<dyn Fn(Event) -> bool>,
    transport: T,
    active: Option<DocumentId>,
    last_scene: Option<(DocumentId, Revision)>,
    cursor: u64,
    pub polling: bool,
    uncertain: Option<Request>,
}
impl<T: Transport> Controller<T> {
    pub fn new(transport: T, output: impl Fn(Event) -> bool + 'static) -> Self {
        Self {
            transport,
            output: Box::new(output),
            active: None,
            last_scene: None,
            cursor: 0,
            polling: false,
            uncertain: None,
        }
    }
    pub fn emit_error(&mut self, ticket: Option<u64>, preview: bool, error: Failure) {
        if let Some(request) = &error.request {
            if self.uncertain.is_none() {
                self.uncertain = Some((**request).clone());
            }
        }
        if matches!(
            error.kind.as_str(),
            "service_unavailable" | "outcome_unknown" | "connection_unusable" | "worker_lost"
        ) {
            self.polling = false;
        }
        (self.output)(Event::Failed {
            ticket,
            preview,
            failure: error,
        });
    }
    fn client(&mut self) -> Result<&mut T, Failure> {
        Ok(&mut self.transport)
    }
    pub async fn reconnect(&mut self) -> Result<(), Failure> {
        let context = self.transport.reconnect().await?;
        let mut reconciliation = None;
        if let Some(request) = &self.uncertain {
            let status = self.transport.status(request.request_id).await?;
            let status: RequestStatus = decode(status)?;
            reconciliation=Some(match status{
                RequestStatus::Completed{response}=>{self.uncertain=None;(true,format!("Original request completed (ok={}). State refreshed; nothing was replayed.",response.ok))},
                RequestStatus::OutcomeUnavailable=>(false,"Original result unavailable. Inspect refreshed state before continuing; no command was replayed.".into()),
            });
        }
        (self.output)(Event::Connected {
            service: context
                .service_id
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("unknown")
                .into(),
            session: context.session_id.as_str().into(),
        });
        self.polling = true;
        self.last_scene = None;
        let refreshed = self.refresh(true).await;
        if let Some((completed, message)) = reconciliation {
            (self.output)(Event::Reconciled { completed, message });
        }
        refreshed?;
        Ok(())
    }
    pub async fn poll(&mut self) -> Result<(), Failure> {
        let cursor = self.cursor;
        let changes = self.client()?.events(cursor).await?;
        if changes["resync_required"] == true || changes["cursor"].as_u64() != Some(self.cursor) {
            self.refresh(false).await?;
        }
        Ok(())
    }
    async fn refresh(&mut self, force: bool) -> Result<(), Failure> {
        let snapshot = self.client()?.snapshot().await?;
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
        (self.output)(Event::Documents {
            documents,
            cursor: self.cursor,
        });
        let history = self.client()?.events(0).await?;
        let events = serde_json::from_value(history["events"].clone()).unwrap_or_default();
        (self.output)(Event::History {
            events,
            truncated: history["resync_required"] == true,
        });
        if let Some((document, revision)) = selected {
            if force || self.last_scene.as_ref() != Some(&(document.clone(), revision)) {
                let response = self
                    .client()?
                    .execute(Command::Scene {
                        document: document.clone(),
                    })
                    .await?;
                let result: DocumentResult<Scene> = decode(response)?;
                self.last_scene = Some((document, result.revision));
                (self.output)(Event::Scene(BoundScene {
                    document: result.document,
                    revision: result.revision,
                    scene: Arc::new(result.data),
                }));
            }
        }
        Ok(())
    }
    pub async fn act(&mut self, action: Action) {
        let (ticket, preview, result) = match action {
            Action::AskAgent { id, .. } | Action::UndoAgent { id, .. } => {
                (self.output)(Event::AgentReply {
                    id,
                    result: Err("Agent host adapter unavailable".into()),
                });
                return;
            }
            Action::ChooseAttachments | Action::LoadAttachments(_) => {
                (self.output)(Event::AttachmentError(
                    "Drop files into the desktop window, or use the browser file picker".into(),
                ));
                return;
            }
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
                    let reply = self.client()?.execute(Command::DocumentCreate).await?;
                    let doc: CreatedDocument = decode(reply)?;
                    self.active = Some(doc.document.clone());
                    (self.output)(Event::Created {
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
                    let reply = self.client()?.execute(command).await?;
                    let scene: DocumentResult<Scene> = decode(reply)?;
                    (self.output)(Event::Preview {
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
            Action::DownloadNative { ticket, document } => {
                let result = self.transport.download(document).await;
                match result {
                    Ok(bytes) => {
                        (self.output)(Event::Download { ticket, bytes });
                    }
                    Err(error) => self.emit_error(Some(ticket), false, error),
                }
                return;
            }
            Action::Execute {
                ticket,
                command,
                label,
            } => {
                let result = async {
                    let reply = self.client()?.execute(command).await?;
                    remote(reply)?;
                    (self.output)(Event::Completed {
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
                    (self.output)(Event::Completed {
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
