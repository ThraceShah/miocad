use cad_protocol::*;
use cad_render::Camera;
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Box,
    Fillet,
}
#[derive(Clone)]
pub struct BoundScene {
    pub document: DocumentId,
    pub revision: Revision,
    pub scene: Arc<Scene>,
}
#[derive(Clone)]
pub struct Selection {
    pub reference: String,
    pub revision: u64,
}
pub struct Preview {
    pub ticket: u64,
    pub document: DocumentId,
    pub revision: Revision,
    pub command: Command,
    pub ready: bool,
    pub scene: Option<BoundScene>,
}
pub struct Model {
    pub connected: bool,
    pub connecting: bool,
    pub documents: Vec<DocumentResult<DocumentState>>,
    pub active: Option<DocumentId>,
    pub scene: Option<BoundScene>,
    pub camera: Camera,
    pub selection: Option<Selection>,
    pub hover: Option<String>,
    pub mode: Mode,
    pub size: [f64; 3],
    pub radius: f64,
    pub preview: Option<Preview>,
    pub busy: bool,
    pub job: u64,
    pub job_label: String,
    pub view_epoch: u64,
    pub notice: String,
    pub error: Option<String>,
    pub unknown: Option<Box<Request>>,
    pub export_path: String,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            connected: false,
            connecting: true,
            documents: Vec::new(),
            active: None,
            scene: None,
            camera: Camera::default(),
            selection: None,
            hover: None,
            mode: Mode::Box,
            size: [10.; 3],
            radius: 1.,
            preview: None,
            busy: false,
            job: 0,
            job_label: String::new(),
            view_epoch: 0,
            notice: "Connecting to shared workspace...".into(),
            error: None,
            unknown: None,
            export_path: "artifacts/model.smb".into(),
        }
    }
}
impl Model {
    pub fn current(&self) -> Option<&DocumentResult<DocumentState>> {
        self.documents
            .iter()
            .find(|d| Some(&d.document) == self.active.as_ref())
    }
    pub fn select(&mut self, document: DocumentId) {
        if self.active.as_ref() == Some(&document) {
            return;
        }
        self.cancel_preview();
        self.active = Some(document);
        self.scene = None;
        self.selection = None;
        self.hover = None;
        self.camera = Camera::default();
        self.view_changed();
    }
    pub fn view_changed(&mut self) {
        self.view_epoch += 1;
        self.hover = None;
    }
    pub fn begin_job(&mut self, label: &str) -> u64 {
        self.job += 1;
        self.busy = true;
        self.job_label = label.into();
        self.error = None;
        self.job
    }
    pub fn finish_job(&mut self) {
        self.busy = false;
        self.job_label.clear();
    }
    pub fn cancel_preview(&mut self) {
        if let Some(p) = self.preview.take() {
            if self.job == p.ticket {
                self.finish_job();
            }
            self.notice = "Preview cancelled. Document and history unchanged.".into();
            self.view_changed();
        }
    }
    pub fn draft_changed(&mut self) {
        self.cancel_preview();
        self.error = None;
    }
    pub fn select_edge(&mut self, reference: String, revision: u64) {
        if self.current().is_some_and(|d| d.revision.get() == revision) && self.preview.is_none() {
            self.selection = Some(Selection {
                reference,
                revision,
            });
            self.view_changed();
        }
    }
    pub fn begin_fillet_preview(&mut self) -> Option<(u64, Command)> {
        self.mode = Mode::Fillet;
        self.begin_preview()
    }
    pub fn begin_preview(&mut self) -> Option<(u64, Command)> {
        if self.busy || !self.connected || self.unknown.is_some() {
            return None;
        }
        let current = self.current()?;
        let document = current.document.clone();
        let revision = current.revision;
        let (operation, command) = match self.mode {
            Mode::Box => {
                let size = self.size.map(Finite::new);
                let size = [
                    size[0].as_ref().ok()?.to_owned(),
                    size[1].as_ref().ok()?.to_owned(),
                    size[2].as_ref().ok()?.to_owned(),
                ];
                (
                    PreviewOperation::Box { size },
                    Command::Box {
                        document: document.clone(),
                        revision,
                        size,
                    },
                )
            }
            Mode::Fillet => {
                let selected = self.selection.as_ref()?;
                if selected.revision != revision.get() {
                    return None;
                }
                let edge = selected.reference.clone();
                let radius = Finite::new(self.radius).ok()?;
                (
                    PreviewOperation::Fillet {
                        edge: edge.clone(),
                        radius,
                    },
                    Command::Fillet {
                        document: document.clone(),
                        revision,
                        edge,
                        radius,
                    },
                )
            }
        };
        let request = Command::Preview {
            document: document.clone(),
            revision,
            operation,
        };
        if let Err(error) = request.value() {
            self.error = Some(error.to_string());
            return None;
        }
        let ticket = self.begin_job("Computing preview");
        self.preview = Some(Preview {
            ticket,
            document,
            revision,
            command,
            ready: false,
            scene: None,
        });
        self.notice = "Computing a temporary candidate. Nothing has been committed.".into();
        Some((ticket, request))
    }
    pub fn accept_preview(&mut self, ticket: u64) -> bool {
        let valid = self.preview.as_ref().is_some_and(|p| {
            p.ticket == ticket
                && self
                    .current()
                    .is_some_and(|d| d.document == p.document && d.revision == p.revision)
        });
        if valid {
            self.preview.as_mut().unwrap().ready = true;
            if self.job == ticket {
                self.finish_job();
            }
            self.notice = "Preview ready. Confirm once to commit, or cancel.".into();
            self.view_changed();
        }
        valid
    }
    pub fn confirm(&mut self) -> Option<Command> {
        if self.busy || !self.connected || self.unknown.is_some() {
            return None;
        }
        let p = self.preview.as_ref()?;
        if !p.ready
            || !self
                .current()
                .is_some_and(|d| d.document == p.document && d.revision == p.revision)
        {
            self.cancel_preview();
            self.notice = "Preview stale. Refresh and preview again.".into();
            return None;
        }
        let command = self.preview.take()?.command;
        self.begin_job("Committing model");
        self.selection = None;
        self.view_changed();
        Some(command)
    }
    pub fn update_documents(&mut self, documents: Vec<DocumentResult<DocumentState>>) {
        let old = self.current().map(|d| d.revision);
        self.documents = documents;
        if self.active.is_none() {
            if let Some(d) = self.documents.first() {
                self.select(d.document.clone());
            }
        }
        if self.active.is_some() && self.current().is_none() {
            self.active = None;
            self.scene = None;
            self.selection = None;
            self.cancel_preview();
            self.view_changed();
        }
        if old.is_some() && old != self.current().map(|d| d.revision) {
            let had = self.preview.is_some();
            self.cancel_preview();
            self.selection = None;
            self.scene = None;
            self.view_changed();
            if had {
                self.notice =
                    "Preview stale: document changed in another entry. Preview again.".into();
            }
        }
    }
    pub fn set_scene(&mut self, bound: BoundScene) {
        if self.active.as_ref() != Some(&bound.document) {
            return;
        }
        if self
            .current()
            .is_some_and(|d| d.revision.get() > bound.revision.get())
        {
            return;
        }
        if self.current().is_some_and(|d| d.revision != bound.revision) {
            let had_preview = self.preview.is_some();
            self.cancel_preview();
            self.selection = None;
            self.hover = None;
            if had_preview {
                self.notice = "Preview stale: authoritative scene advanced. Preview again.".into();
            }
        }
        if let Some(d) = self
            .documents
            .iter_mut()
            .find(|d| d.document == bound.document)
        {
            d.revision = bound.revision;
            d.data = bound.scene.state.clone();
        }
        self.scene = Some(bound);
        self.view_changed();
    }
    pub fn display_scene(&self) -> Option<(&BoundScene, bool)> {
        if let Some(scene) = self
            .preview
            .as_ref()
            .filter(|p| p.ready)
            .and_then(|p| p.scene.as_ref())
        {
            return Some((scene, true));
        }
        self.scene.as_ref().map(|s| (s, false))
    }
    pub fn can_edit(&self) -> bool {
        self.connected && !self.busy && self.unknown.is_none() && self.current().is_some()
    }
}
