//! Screen-space UI geometry for accessibility and host automation; never authoritative CAD state.
use std::collections::BTreeMap;
#[derive(Default, Clone, serde::Serialize)]
pub struct ControlGeometry {
    pub rect: [f32; 4],
    pub enabled: bool,
}
#[derive(Default, Clone, serde::Serialize)]
pub struct UiGeometry {
    pub model_error: Option<String>,
    pub agent_open: bool,
    pub agent_draft: String,
    pub references: Vec<cad_agent::Reference>,
    pub agent_thinking: bool,
    pub agent_messages: Vec<String>,
    pub agent_task: Option<cad_agent::TaskState>,
    pub agent_error: Option<String>,
    pub highlights: Vec<String>,
    pub viewport: [f32; 4],
    pub controls: BTreeMap<String, ControlGeometry>,
    pub parameters_open: bool,
    pub tree_open: bool,
    pub scale: f32,
    pub selected: Option<String>,
    pub document: Option<String>,
    pub preview: bool,
    pub frame_current: bool,
    pub camera: cad_render::Camera,
    pub view_epoch: u64,
    pub history_entries: usize,
    pub connected: bool,
    pub uncertain: bool,
}
impl UiGeometry {
    pub fn record(&mut self, name: impl Into<String>, response: &egui::Response, enabled: bool) {
        let r = response.rect;
        self.controls.insert(
            name.into(),
            ControlGeometry {
                rect: [r.min.x, r.min.y, r.width(), r.height()],
                enabled,
            },
        );
    }
}
