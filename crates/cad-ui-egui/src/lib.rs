#![forbid(unsafe_code)]
mod controller;
mod model;
pub use controller::*;
mod chrome;
mod geometry;
pub mod local_view;
mod widgets;
use cad_protocol::*;
pub use geometry::*;
pub use model::*;
use std::sync::Arc;
pub use widgets::*;
#[derive(Clone)]
pub enum Action {
    AskAgent {
        id: u64,
        turn: cad_agent::Turn,
        service: String,
        session: String,
    },
    UndoAgent {
        id: u64,
        task: String,
    },
    ChooseAttachments,
    LoadAttachments(Vec<std::path::PathBuf>),
    NewDocument {
        ticket: u64,
    },
    Select(DocumentId),
    DownloadNative {
        ticket: u64,
        document: DocumentId,
    },
    Refresh {
        ticket: u64,
    },
    Reconnect {
        ticket: u64,
    },
    Preview {
        ticket: u64,
        command: Command,
    },
    Execute {
        ticket: u64,
        command: Command,
        label: String,
    },
}
#[derive(Clone, Debug)]
pub struct Failure {
    pub kind: String,
    pub message: String,
    pub request: Option<Box<Request>>,
}
pub enum Event {
    ViewFailed {
        epoch: Option<u64>,
        message: String,
    },
    AgentReply {
        id: u64,
        result: Result<cad_agent::TaskState, String>,
    },
    Attachments(Vec<cad_agent::Attachment>),
    AttachmentError(String),
    History {
        events: Vec<cad_protocol::Event>,
        truncated: bool,
    },
    Download {
        ticket: u64,
        bytes: Vec<u8>,
    },
    Connected {
        service: String,
        session: String,
    },
    Documents {
        documents: Vec<DocumentResult<DocumentState>>,
        cursor: u64,
    },
    Scene(BoundScene),
    Preview {
        ticket: u64,
        scene: BoundScene,
    },
    Created {
        ticket: u64,
        document: DocumentId,
    },
    Completed {
        ticket: u64,
        message: String,
    },
    Failed {
        ticket: Option<u64>,
        preview: bool,
        failure: Failure,
    },
    Reconciled {
        completed: bool,
        message: String,
    },
    View {
        epoch: u64,
        frame: Arc<cad_render::Frame>,
        rgb: Vec<u8>,
        width: u32,
        height: u32,
        document: DocumentId,
        revision: Revision,
        preview: bool,
    },
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ViewRequest {
    pub service: String,
    pub session: String,
    pub preview_operation: Option<PreviewOperation>,
    pub epoch: u64,
    pub bound: BoundScene,
    pub preview: bool,
    pub camera: cad_render::Camera,
    pub selected: Option<String>,
    pub width: u32,
    pub height: u32,
}
