use super::*;
use cad_agent::{Attachment, Message, Reference, ReferenceKind, Turn};
#[derive(Default)]
pub struct Chat {
    pub open: bool,
    pub pick_kind: ReferenceKind,
    pub references: Vec<Reference>,
    pub attachments: Vec<Attachment>,
    pub messages: Vec<ChatEntry>,
    pub draft: String,
    pub thinking: bool,
    pub request: u64,
    pub turn: Option<Turn>,
    pub result: Option<cad_agent::TaskState>,
    pub error: Option<String>,
    pub focused: Option<Reference>,
}
#[derive(Clone, serde::Serialize)]
pub struct ChatEntry {
    pub document: DocumentId,
    pub revision: Revision,
    pub role: String,
    pub text: String,
    pub references: Vec<Reference>,
    pub files: Vec<String>,
}
impl Workspace {
    pub(super) fn named_reference(
        &self,
        kind: ReferenceKind,
        target: String,
        name: String,
    ) -> Option<Reference> {
        let d = self.model.current()?;
        Some(Reference {
            name,
            document: d.document.clone(),
            revision: d.revision,
            kind,
            target,
        })
    }
    pub(super) fn reference_current(&self, r: &Reference) -> bool {
        self.model
            .current()
            .is_some_and(|d| d.document == r.document && d.revision == r.revision)
    }
    pub(super) fn add_reference(&mut self, r: Reference) {
        if !self.chat.references.iter().any(|x| {
            x.document == r.document
                && x.revision == r.revision
                && x.kind == r.kind
                && x.target == r.target
        }) {
            self.chat.references.push(r.clone());
        }
        self.chat.focused = Some(r);
    }
    pub(super) fn picked_reference(&mut self, kind: ReferenceKind, target: String) {
        let scene = self.model.scene.as_ref();
        let name = match kind {
            ReferenceKind::Edge => format!(
                "Edge {}",
                scene
                    .and_then(|s| s.scene.edges.iter().position(|e| e.reference == target))
                    .unwrap_or(0)
                    + 1
            ),
            ReferenceKind::Face => {
                let faces: std::collections::BTreeSet<_> = scene
                    .and_then(|s| s.scene.mesh.as_ref())
                    .into_iter()
                    .flat_map(|m| m.triangles.iter().map(|t| &t.face))
                    .collect();
                format!(
                    "Face {}",
                    faces.iter().position(|r| r.as_str() == target).unwrap_or(0) + 1
                )
            }
            ReferenceKind::Body => "Body 1".into(),
            ReferenceKind::Feature => return,
        };
        if kind != ReferenceKind::Edge {
            self.model.selection = None;
        }
        if let Some(r) = self.named_reference(kind, target, name) {
            self.add_reference(r);
        }
    }
    pub(super) fn agent_reply(&mut self, id: u64, result: Result<cad_agent::TaskState, String>) {
        if id != self.chat.request || !self.chat.thinking {
            return;
        }
        self.chat.thinking = false;
        match result {
            Ok(reply) => {
                if !reply.done {
                    self.chat.thinking = true;
                    self.chat.result = Some(reply);
                    return;
                }
                let revision = reply.revision;
                self.chat.messages.push(ChatEntry {
                    document: reply.document.clone(),
                    revision,
                    role: "assistant".into(),
                    text: reply.text.clone(),
                    references: vec![],
                    files: vec![],
                });
                if !reply.steps.is_empty() {
                    self.chat
                        .references
                        .retain(|r| r.document != reply.document || r.revision == reply.revision);
                    if self.chat.focused.as_ref().is_some_and(|r| {
                        r.document == reply.document && r.revision != reply.revision
                    }) {
                        self.chat.focused = None;
                    }
                }
                self.chat.result = Some(reply);
                self.model.view_changed();
            }
            Err(e) => self.chat.error = Some(e),
        }
    }
    pub(super) fn attach(&mut self, files: Vec<Attachment>) {
        for a in files {
            if let Err(e) = a.bytes() {
                self.chat.error = Some(e);
                continue;
            }
            if self.chat.attachments.len() >= 8
                || self
                    .chat
                    .attachments
                    .iter()
                    .map(|a| a.data.len())
                    .sum::<usize>()
                    + a.data.len()
                    > 5_333_336
            {
                self.chat.error = Some("Attachments exceed 8 files / 4 MB total".into());
                break;
            }
            self.chat.attachments.push(a);
        }
        self.chat.open = true;
    }
    pub(super) fn chat_panel(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.chat.open {
            return;
        }
        egui::TopBottomPanel::bottom("agent-conversation")
            .resizable(true)
            .default_height(300.)
            .min_height(180.)
            .max_height(ctx.content_rect().height() * 0.5)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CAD Agent").strong().color(ACCENT));
                    ui.label("Reference > Ask > Agent executes > Result");
                    if ui.small_button("×").clicked() {
                        self.chat.open = false;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Pick from view:");
                    for (kind, label, key) in [
                        (ReferenceKind::Edge, "Edge", "pick-edge"),
                        (ReferenceKind::Face, "Face", "pick-face"),
                        (ReferenceKind::Body, "Body", "pick-body"),
                    ] {
                        let r = ui.selectable_label(self.chat.pick_kind == kind, label);
                        self.geometry.record(key, &r, true);
                        if r.clicked() {
                            self.chat.pick_kind = kind;
                            self.pending_pick = None;
                            self.pick_choices = None;
                        }
                    }
                    let r = ui.button("Attach files / images");
                    self.geometry.record("attach-files", &r, true);
                    if r.clicked() {
                        actions.push(Action::ChooseAttachments);
                    }
                    let r = ui.small_button("Clear references");
                    self.geometry.record("clear-references", &r, true);
                    if r.clicked() {
                        self.chat.references.clear();
                        self.chat.focused = None;
                        self.model.selection = None;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let mut remove = None;
                    for (i, r) in self.chat.references.clone().iter().enumerate() {
                        let current = self.reference_current(r);
                        let label = format!("@{}{}", r.name, if current { "" } else { " (stale)" });
                        let response = ui.selectable_label(current, label).on_hover_text(format!(
                            "{:?} · {} · r{}",
                            r.kind,
                            r.target,
                            r.revision.get()
                        ));
                        self.geometry
                            .record(format!("reference-{i}"), &response, current);
                        if response.clicked() && current {
                            self.chat.focused = Some(r.clone());
                        }
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    }
                    if let Some(i) = remove {
                        let removed = self.chat.references.remove(i);
                        if self.chat.focused.as_ref() == Some(&removed) {
                            self.chat.focused = None;
                        }
                    }
                    let mut remove_file = None;
                    for (i, a) in self.chat.attachments.iter().enumerate() {
                        ui.label(format!("📎 {}", a.name));
                        if ui.small_button("×").clicked() {
                            remove_file = Some(i);
                        }
                    }
                    if let Some(i) = remove_file {
                        self.chat.attachments.remove(i);
                    }
                });
                if self
                    .chat
                    .references
                    .iter()
                    .any(|r| r.kind == ReferenceKind::Feature)
                {
                    ui.small(
                        "Feature highlight shows current body context. Use the pencil to edit parameters.",
                    );
                }
                // Input remains visible; conversation gets the remaining height.
                ui.horizontal(|ui| {
                    let input = ui.add_sized(
                        [ui.available_width() - 82., 40.],
                        egui::TextEdit::singleline(&mut self.chat.draft)
                            .hint_text("Ask about the referenced part…"),
                    );
                    self.geometry.record("chat-input", &input, true);
                    let enabled = !self.chat.thinking
                        && self.model.connected
                        && !self.model.busy
                        && self.model.current().is_some()
                        && !self.chat.draft.trim().is_empty()
                        && self.model.preview.is_none()
                        && self.model.unknown.is_none()
                        && self
                            .chat
                            .references
                            .iter()
                            .all(|r| self.reference_current(r));
                    let send = ui.add_enabled(
                        enabled,
                        egui::Button::new("Send").min_size(Vec2::new(64., 40.)),
                    );
                    self.geometry.record("chat-send", &send, enabled);
                    if enabled
                        && (send.clicked()
                            || (input.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    {
                        let d = self.model.current().unwrap();
                        let turn = Turn {
                            document: d.document.clone(),
                            revision: d.revision,
                            text: self.chat.draft.trim().into(),
                            references: self.chat.references.clone(),
                            attachments: self.chat.attachments.clone(),
                            history: self
                                .chat
                                .messages
                                .iter()
                                .filter(|m| m.document == d.document)
                                .rev()
                                .take(24)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .map(|m| Message {
                                    role: m.role.clone(),
                                    text: format!(
                                        "[document {}, revision {}] {}",
                                        m.document.as_str(),
                                        m.revision.get(),
                                        m.text
                                    ),
                                })
                                .collect(),
                        };
                        match turn.validate() {
                            Ok(()) => {
                                self.chat.messages.push(ChatEntry {
                                    document: turn.document.clone(),
                                    revision: turn.revision,
                                    role: "user".into(),
                                    text: turn.text.clone(),
                                    references: turn.references.clone(),
                                    files: turn
                                        .attachments
                                        .iter()
                                        .map(|a| a.name.clone())
                                        .collect(),
                                });
                                self.chat.draft.clear();
                                self.chat.error = None;
                                self.chat.result = None;
                                self.chat.thinking = true;
                                self.chat.request += 1;
                                self.chat.turn = Some(turn.clone());
                                actions.push(Action::AskAgent {
                                    id: self.chat.request,
                                    turn,
                                    service: self.service.clone(),
                                    session: self.session.clone(),
                                });
                            }
                            Err(e) => self.chat.error = Some(e),
                        }
                    }
                });
                if self.chat.messages.len() > 128 {
                    let n = self.chat.messages.len() - 128;
                    self.chat.messages.drain(..n);
                }
                if self.chat.thinking {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Agent is working autonomously…");
                    });
                }
                if let Some(error) = &self.chat.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                if let Some(result) = self.chat.result.clone() {
                    ui.label(format!(
                        "{} · {} operations · r{}",
                        result.phase,
                        result.steps.len(),
                        result.revision.get()
                    ));
                    let enabled = !self.chat.thinking
                        && result.done
                        && result.can_undo
                        && self.model.current().is_some_and(|d| {
                            d.document == result.document && d.revision == result.revision
                        });
                    if result.can_undo {
                        let r = ui.add_enabled(enabled, egui::Button::new("Undo Agent task"));
                        self.geometry.record("agent-undo", &r, enabled);
                        if r.clicked() {
                            self.chat.thinking = true;
                            actions.push(Action::UndoAgent {
                                id: self.chat.request,
                                task: result.id,
                            });
                        }
                    }
                }
                egui::ScrollArea::vertical()
                    .id_salt("chat-messages")
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for m in self
                            .chat
                            .messages
                            .clone()
                            .into_iter()
                            .filter(|m| self.model.active.as_ref() == Some(&m.document))
                            .collect::<Vec<_>>()
                        {
                            ui.label(
                                RichText::new(if m.role == "user" { "You" } else { "Agent" })
                                    .strong()
                                    .color(ACCENT),
                            );
                            ui.label(&m.text);
                            ui.horizontal_wrapped(|ui| {
                                for r in &m.references {
                                    let current = self.reference_current(r);
                                    let p = ui.selectable_label(
                                        current,
                                        format!(
                                            "@{}{}",
                                            r.name,
                                            if current { "" } else { " (stale)" }
                                        ),
                                    );
                                    if p.clicked() && current {
                                        self.add_reference(r.clone());
                                    }
                                }
                                for a in &m.files {
                                    ui.label(format!("📎 {a}"));
                                }
                            });
                        }
                    });
            });
    }
    pub(super) fn reference_highlights(&self) -> Vec<String> {
        if self.model.preview.is_some() {
            return Vec::new();
        }
        let mut out: Vec<_> = self
            .chat
            .references
            .iter()
            .chain(self.chat.focused.iter())
            .filter(|r| self.reference_current(r))
            .map(|r| {
                if matches!(r.kind, ReferenceKind::Body | ReferenceKind::Feature) {
                    "body".into()
                } else {
                    r.target.clone()
                }
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace() -> Workspace {
        let mut w = Workspace::default();
        let doc = DocumentId::new("doc").unwrap();
        let revision = Revision::new(1).unwrap();
        w.model.documents = vec![DocumentResult {
            document: doc.clone(),
            revision,
            data: DocumentState {
                revision,
                units: "mm".into(),
                empty: false,
                volume: None,
                faces: Some(6),
                edges: Some(12),
                valid: Some(true),
            },
        }];
        w.model.active = Some(doc);
        w.model.connected = true;
        w
    }
    #[test]
    fn named_context_is_deduplicated_and_never_highlights_stale_or_foreign_topology() {
        let mut w = workspace();
        let r = w
            .named_reference(ReferenceKind::Edge, "edge-1".into(), "Edge 1".into())
            .unwrap();
        w.add_reference(r.clone());
        w.add_reference(r);
        assert_eq!(w.chat.references.len(), 1);
        assert_eq!(w.reference_highlights(), vec!["edge-1"]);
        w.model.documents[0].revision = Revision::new(2).unwrap();
        assert!(w.reference_highlights().is_empty());
        w.model.documents[0].revision = Revision::new(1).unwrap();
        w.model.active = Some(DocumentId::new("foreign").unwrap());
        assert!(w.reference_highlights().is_empty());
    }
    #[test]
    fn responses_are_bound_to_the_original_task_and_never_request_approval() {
        let mut w = workspace();
        w.chat.thinking = true;
        w.chat.request = 7;
        let state = cad_agent::TaskState {
            id: "7".into(),
            document: w.model.active.clone().unwrap(),
            before: Revision::new(1).unwrap(),
            revision: Revision::new(2).unwrap(),
            phase: "completed".into(),
            text: "Done".into(),
            steps: vec![],
            can_undo: true,
            done: true,
            atomic: false,
            transaction: None,
        };
        w.agent_reply(6, Ok(state.clone()));
        assert!(w.chat.thinking);
        assert!(w.chat.result.is_none());
        w.agent_reply(7, Ok(state));
        assert!(!w.chat.thinking);
        assert_eq!(w.chat.messages[0].text, "Done");
    }
}
