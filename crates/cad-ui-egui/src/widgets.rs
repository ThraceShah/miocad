use crate::*;
use egui::{Color32, RichText, Vec2};
const ACCENT: Color32 = Color32::from_rgb(44, 210, 190);
const SELECTED: Color32 = Color32::from_rgb(255, 189, 64);
const MUTED: Color32 = Color32::from_rgb(141, 157, 174);
const CANVAS: Color32 = Color32::from_rgb(242, 245, 248);
struct PendingPick {
    document: DocumentId,
    revision: u64,
    epoch: u64,
    point: [f64; 2],
    radius: [f64; 2],
    deadline: f64,
}
struct PickChoices {
    document: DocumentId,
    revision: u64,
    epoch: u64,
    references: Vec<String>,
}
pub struct Workspace {
    pub model: Model,
    pub browser_downloads: bool,
    pub local_backend: Option<String>,
    pub service: String,
    pub session: String,
    pub cursor: u64,
    reconciliation_job: Option<u64>,
    pending_pick: Option<PendingPick>,
    pick_choices: Option<PickChoices>,
    choice_hover: Option<String>,
    texture: Option<egui::TextureHandle>,
    frame: Option<Arc<cad_render::Frame>>,
    frame_epoch: u64,
    frame_document: Option<DocumentId>,
    frame_revision: u64,
    frame_preview: bool,
    pub viewport_size: [u32; 2],
    pub geometry: UiGeometry,
    inspector_open: bool,
    tree_open: bool,
    size_text: [String; 3],
    radius_text: String,
    history: Vec<cad_protocol::Event>,
    history_truncated: bool,
    multi_active: bool,
    touch_guard: bool,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            model: Model::default(),
            browser_downloads: false,
            local_backend: None,
            service: String::new(),
            session: String::new(),
            cursor: 0,
            reconciliation_job: None,
            pending_pick: None,
            pick_choices: None,
            choice_hover: None,
            texture: None,
            frame: None,
            frame_epoch: 0,
            frame_document: None,
            frame_revision: 0,
            frame_preview: false,
            viewport_size: [640, 640],
            geometry: UiGeometry::default(),
            inspector_open: false,
            tree_open: true,
            size_text: ["10".into(), "10".into(), "10".into()],
            radius_text: "1".into(),
            history: Vec::new(),
            history_truncated: false,
            multi_active: false,
            touch_guard: false,
        }
    }
}
impl Workspace {
    pub fn theme(ctx: &egui::Context) {
        ctx.set_theme(egui::Theme::Dark);
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(25, 32, 40);
        style.visuals.window_fill = Color32::from_rgb(25, 32, 40);
        style.visuals.faint_bg_color = Color32::from_rgb(31, 40, 50);
        style.visuals.extreme_bg_color = Color32::from_rgb(17, 24, 31);
        style.visuals.selection.bg_fill = Color32::from_rgb(22, 84, 80);
        style.visuals.selection.stroke = egui::Stroke::new(1., ACCENT);
        style.spacing.item_spacing = Vec2::new(10., 10.);
        style.spacing.button_padding = Vec2::new(12., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(13.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
        ctx.set_style(style);
    }
    pub fn apply(&mut self, event: Event, ctx: &egui::Context) {
        match event {
            Event::History { events, truncated } => {
                self.history = events;
                self.history_truncated = truncated;
            }
            Event::Download { ticket, .. } => {
                self.apply(
                    Event::Completed {
                        ticket,
                        message: "SMB downloaded".into(),
                    },
                    ctx,
                );
            }
            Event::Connected { service, session } => {
                self.service = service;
                self.session = session;
                self.model.connected = true;
                self.model.connecting = false;
                self.model.error = None;
                self.model.notice = "Connected. Changes are shared with CLI and Agents.".into();
            }
            Event::Documents { documents, cursor } => {
                self.cursor = cursor;
                self.model.update_documents(documents);
            }
            Event::Scene(scene) => self.model.set_scene(scene),
            Event::Preview { ticket, scene } => {
                if self.model.accept_preview(ticket) {
                    self.model.preview.as_mut().unwrap().scene = Some(scene);
                }
            }
            Event::Created { ticket, document } => {
                if self.model.job == ticket {
                    self.model.finish_job();
                    self.model.select(document);
                    self.model.mode = Mode::Box;
                    self.inspector_open = true;
                    self.model.notice =
                        "Document created. Set dimensions and preview your first Box.".into();
                }
            }
            Event::Completed { ticket, message } => {
                if self.model.job == ticket {
                    if self.model.job_label == "Committing model" {
                        self.inspector_open = false;
                    }
                    self.model.finish_job();
                    if self.reconciliation_job.take() != Some(ticket) {
                        self.model.notice = message;
                    }
                    self.model.selection = None;
                }
            }
            Event::Failed {
                ticket,
                preview,
                failure,
            } => {
                let disconnected = matches!(
                    failure.kind.as_str(),
                    "service_unavailable"
                        | "outcome_unknown"
                        | "connection_unusable"
                        | "worker_lost"
                );
                if preview
                    && self
                        .model
                        .preview
                        .as_ref()
                        .is_none_or(|p| Some(p.ticket) != ticket)
                    && !disconnected
                {
                    return;
                }
                if let Some(ticket) = ticket {
                    if ticket == self.model.job {
                        self.model.finish_job();
                    } else if !disconnected {
                        return;
                    }
                }
                if preview || disconnected {
                    self.model.cancel_preview();
                }
                if failure.request.is_some() {
                    self.model.unknown = failure.request;
                }
                if disconnected {
                    self.model.connected = false;
                    self.model.connecting = false;
                }
                self.model.error = Some(format!("{}: {}", failure.kind, failure.message));
                self.model.notice = if failure.message.starts_with("Model command acknowledged") {
                    "Model command acknowledged; scene refresh failed. Reconnect to inspect.".into()
                } else {
                    "Operation did not report success. Refresh or reconnect before continuing."
                        .into()
                };
            }
            Event::Reconciled { completed, message } => {
                self.reconciliation_job = Some(self.model.job);
                if completed {
                    self.model.unknown = None;
                }
                self.model.notice = message;
            }
            Event::View {
                epoch,
                frame,
                rgb,
                width,
                height,
                document,
                revision,
                preview,
            } => {
                if epoch != self.model.view_epoch || self.model.active.as_ref() != Some(&document) {
                    return;
                }
                let camera =
                    serde_json::from_value::<cad_render::Camera>(frame.metadata["camera"].clone())
                        .ok();
                if frame.dimensions() != self.viewport_size
                    || frame.dimensions() != [width, height]
                    || frame.metadata["document"] != document.as_str()
                    || frame.metadata["revision"].as_u64() != Some(revision.get())
                    || camera != Some(self.model.camera)
                    || (!preview && self.model.current().is_none_or(|d| d.revision != revision))
                {
                    return;
                }
                let image = egui::ColorImage::from_rgb([width as usize, height as usize], &rgb);
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture =
                        Some(ctx.load_texture("cad-viewport", image, egui::TextureOptions::LINEAR));
                }
                self.frame = Some(frame);
                self.frame_epoch = epoch;
                self.frame_document = Some(document);
                self.frame_revision = revision.get();
                self.frame_preview = preview;
            }
        }
    }
    pub fn frame_current(&self) -> bool {
        self.frame.is_some()
            && self.frame_epoch == self.model.view_epoch
            && self.frame_document == self.model.active
            && self
                .model
                .current()
                .is_some_and(|d| d.revision.get() == self.frame_revision)
            && self
                .model
                .display_scene()
                .is_some_and(|(_, preview)| preview == self.frame_preview)
    }
    pub fn render_request(&self) -> Option<ViewRequest> {
        let (bound, preview) = self.model.display_scene()?;
        bound.scene.mesh.as_ref()?;
        Some(ViewRequest {
            service: self.service.clone(),
            session: self.session.clone(),
            preview_operation: if preview {
                self.model.preview.as_ref().and_then(|p| match &p.command {
                    Command::Preview { operation, .. } => Some(operation.clone()),
                    Command::Box { size, .. } => Some(PreviewOperation::Box { size: *size }),
                    Command::Fillet { edge, radius, .. } => Some(PreviewOperation::Fillet {
                        edge: edge.clone(),
                        radius: *radius,
                    }),
                    _ => None,
                })
            } else {
                None
            },
            epoch: self.model.view_epoch,
            bound: bound.clone(),
            preview,
            camera: self.model.camera,
            selected: self.model.selection.as_ref().map(|s| s.reference.clone()),
            width: self.viewport_size[0],
            height: self.viewport_size[1],
        })
    }
    fn properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, actions: &mut Vec<Action>) {
        ui.label(
            RichText::new(match self.model.mode {
                Mode::Box => "Box",
                Mode::Fillet => "Fillet",
            })
            .size(19.)
            .strong(),
        );
        ui.label(RichText::new("FEATURE PARAMETERS").size(10.).color(MUTED));
        ui.add_space(12.);
        let mut fields_valid = true;
        match self.model.mode {
            Mode::Box => {
                ui.label(RichText::new("Box dimensions").strong());
                ui.label(
                    RichText::new("Replaces the document's single body.")
                        .size(11.)
                        .color(MUTED),
                );
                ui.add_space(6.);
                for (i, name) in ["Width", "Depth", "Height"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([64., 38.], egui::Label::new(*name));
                        let response = ui.add_sized(
                            [105., 38.],
                            egui::TextEdit::singleline(&mut self.size_text[i]),
                        );
                        self.geometry
                            .record(["width", "depth", "height"][i], &response, true);
                        let value = self.size_text[i]
                            .parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite() && (0.01..=10000.).contains(v));
                        fields_valid &= value.is_some();
                        if response.changed() {
                            if let Some(value) = value {
                                self.model.size[i] = value;
                                self.model.draft_changed();
                            }
                        }
                        ui.label("mm");
                    });
                }
            }
            Mode::Fillet => {
                ui.label(RichText::new("Edge fillet").strong());
                ui.label(
                    RichText::new("Tap near a visible edge to select it.")
                        .size(11.)
                        .color(MUTED),
                );
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    let response = ui.add_sized(
                        [105., 38.],
                        egui::TextEdit::singleline(&mut self.radius_text),
                    );
                    self.geometry.record("radius", &response, true);
                    let value = self
                        .radius_text
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite() && (0.001..=10000.).contains(v));
                    fields_valid = value.is_some();
                    if response.changed() {
                        if let Some(value) = value {
                            self.model.radius = value;
                            self.model.draft_changed();
                        }
                    }
                    ui.label("mm");
                });
                ui.label(
                    RichText::new(if self.model.selection.is_some() {
                        "1 edge selected · amber"
                    } else {
                        "No edge selected"
                    })
                    .color(if self.model.selection.is_some() {
                        SELECTED
                    } else {
                        MUTED
                    }),
                );
            }
        }
        ui.add_space(15.);
        if !fields_valid {
            ui.colored_label(SELECTED, "Enter a positive dimension in mm.");
        }
        let preview_enabled = fields_valid
            && self.model.can_edit()
            && (self.model.mode == Mode::Box || self.model.selection.is_some());
        let response = ui.add_enabled(
            preview_enabled,
            egui::Button::new("Preview shape").min_size(Vec2::new(ui.available_width(), 40.)),
        );
        self.geometry.record("preview", &response, preview_enabled);
        if response.clicked() {
            if let Some((ticket, command)) = self.model.begin_preview() {
                actions.push(Action::Preview { ticket, command });
            }
        }
        ui.horizontal(|ui| {
            let ready = self.model.preview.as_ref().is_some_and(|p| p.ready)
                && self.model.can_edit()
                && self.frame_current();
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.;
            let response = ui.add_enabled(
                ready,
                egui::Button::new(RichText::new("Confirm").color(Color32::from_rgb(8, 30, 30)))
                    .fill(ACCENT)
                    .min_size(Vec2::new(width, 40.)),
            );
            self.geometry.record("confirm", &response, ready);
            if response.clicked() {
                if let Some(command) = self.model.confirm() {
                    actions.push(Action::Execute {
                        ticket: self.model.job,
                        command,
                        label: "Model committed".into(),
                    });
                }
            }
            let enabled = self.model.preview.is_some();
            let response = ui.add_enabled(
                enabled,
                egui::Button::new("Cancel").min_size(Vec2::new(width, 40.)),
            );
            self.geometry.record("cancel", &response, enabled);
            if response.clicked() {
                self.model.cancel_preview();
            }
        });
        if let Some(preview) = &self.model.preview {
            ui.label(
                RichText::new("PREVIEW  /  not committed")
                    .size(11.)
                    .color(ACCENT),
            );
            if let Some(scene) = &preview.scene {
                if let Some(volume) = scene.scene.state.volume {
                    ui.label(
                        RichText::new(format!("Valid candidate  /  {:.3} mm³", volume.get()))
                            .size(11.)
                            .color(MUTED),
                    );
                }
            }
        }
        ui.add_space(10.);
        ui.separator();
        if let Some(error) = &self.model.error {
            ui.colored_label(Color32::from_rgb(248, 166, 139), error);
            ui.add_space(5.);
        }
        if let Some(request) = &self.model.unknown {
            ui.label(
                RichText::new(format!(
                    "Request {} outcome is uncertain. Reconnect to reconcile; do not repeat it.",
                    request.request_id.get()
                ))
                .color(Color32::from_rgb(244, 187, 98))
                .size(12.),
            );
            if self.model.connected
                && !self.model.busy
                && ui.button("Use refreshed state").clicked()
            {
                self.model.unknown = None;
                self.model.notice = "Using refreshed state. No command was replayed.".into();
            }
        }
        if let Some(doc) = self.model.current() {
            ui.label(RichText::new("DOCUMENT").size(11.).color(MUTED));
            ui.add_space(6.);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}...",
                        &doc.document.as_str()[..8.min(doc.document.as_str().len())]
                    ))
                    .monospace(),
                );
                if ui.small_button("Copy ID").clicked() {
                    ctx.copy_text(doc.document.as_str().into());
                }
            });
            ui.label(format!("Revision {}", doc.revision.get()));
            if let Some(volume) = doc.data.volume {
                ui.label(format!("Volume  {:.4} mm³", volume.get()));
            }
            ui.label(if doc.data.empty {
                "No solid body"
            } else if doc.data.valid == Some(true) {
                "Solid valid"
            } else {
                "Validation unavailable"
            });
        }
        ui.add_space(12.);
        egui::CollapsingHeader::new("Native export").show(ui, |ui| {
            if !self.browser_downloads {
                ui.label(
                    RichText::new("SMB path (parent directory must exist)")
                        .size(11.)
                        .color(MUTED),
                );
                ui.text_edit_singleline(&mut self.model.export_path);
            }
            if ui
                .add_enabled(
                    self.model.can_edit() && self.model.current().is_some_and(|d| !d.data.empty),
                    egui::Button::new(if self.browser_downloads {
                        "Download SMB"
                    } else {
                        "Export SMB"
                    }),
                )
                .clicked()
            {
                let document = self.model.current().unwrap().document.clone();
                let command = Command::ExportNative {
                    document,
                    path: self.model.export_path.clone(),
                };
                let ticket = self.model.begin_job("Exporting SMB");
                if self.browser_downloads {
                    actions.push(Action::DownloadNative {
                        ticket,
                        document: self.model.current().unwrap().document.clone(),
                    });
                } else {
                    actions.push(Action::Execute {
                        ticket,
                        command,
                        label: "SMB exported".into(),
                    });
                }
            }
        });
        ui.add_space(8.);
        egui::CollapsingHeader::new("Connection").show(ui, |ui| {
            if let Some(frame) = &self.frame {
                if let Some(id) = frame.metadata["render_id"].as_str() {
                    ui.label(format!("Renderer: {}", frame.metadata["backend"]));
                    if let Some(adapter) = frame.metadata.get("adapter") {
                        ui.label(format!(
                            "{} / {}",
                            adapter["name"].as_str().unwrap_or("unknown"),
                            adapter["device_type"].as_str().unwrap_or("unknown")
                        ));
                    }
                    if ui.button("Copy render ID").clicked() {
                        ctx.copy_text(id.into());
                    }
                    if let Some(reason) = frame.metadata["fallback_reason"].as_str() {
                        ui.label(reason);
                    }
                }
            }

            ui.label(
                RichText::new(format!(
                    "Service {}",
                    self.service.get(..12).unwrap_or(&self.service)
                ))
                .monospace()
                .size(10.),
            );
            ui.label(
                RichText::new(format!(
                    "Session {}",
                    self.session.get(..12).unwrap_or(&self.session)
                ))
                .monospace()
                .size(10.),
            );
            ui.label(RichText::new(format!("Event cursor {}", self.cursor)).size(10.));
        });
    }
    fn resolve_pick(&mut self, now: f64) {
        let Some(pending) = self.pending_pick.as_ref() else {
            return;
        };
        if self.model.active.as_ref() != Some(&pending.document)
            || self.model.view_epoch != pending.epoch
            || self
                .model
                .current()
                .is_none_or(|d| d.revision.get() != pending.revision)
            || self.model.preview.is_some()
            || !self.model.connected
            || self.model.busy
        {
            self.pending_pick = None;
            return;
        }
        if now > pending.deadline {
            self.pending_pick = None;
            self.model.notice =
                "Picking frame unavailable. Tap the edge again after reconnecting.".into();
            return;
        }
        if !self.frame_current() {
            return;
        }
        let pending = self.pending_pick.take().unwrap();
        if let Some(frame) = &self.frame {
            if let Ok(pick) = frame.pick_edge_near(pending.point, pending.radius) {
                if let Some(reference) = pick["reference"].as_str() {
                    self.model.select_edge(reference.into(), pending.revision);
                    self.model.mode = Mode::Fillet;
                    self.inspector_open = true;
                    self.model.notice =
                        "Edge selected (amber). Preview a fillet or tap another edge.".into();
                } else if pick["ambiguous"] == true {
                    let references = pick["candidates"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    self.pick_choices = Some(PickChoices {
                        document: pending.document,
                        revision: pending.revision,
                        epoch: pending.epoch,
                        references,
                    });
                    self.model.notice =
                        "Overlapping edges: choose the intended edge from the list.".into();
                } else {
                    if self.model.selection.take().is_some() {
                        self.model.view_changed();
                    }
                    self.model.notice = "No visible edge nearby. Tap closer to an edge.".into();
                }
            }
        }
    }
    fn choices_current(&self) -> bool {
        self.pick_choices.as_ref().is_some_and(|p| {
            self.model.active.as_ref() == Some(&p.document)
                && self
                    .model
                    .current()
                    .is_some_and(|d| d.revision.get() == p.revision)
                && self.model.view_epoch == p.epoch
                && self.model.connected
                && !self.model.busy
                && self.model.preview.is_none()
                && self.frame_current()
        })
    }
    fn choose_edge(&mut self, reference: &str) {
        if self.choices_current()
            && self
                .pick_choices
                .as_ref()
                .is_some_and(|p| p.references.iter().any(|r| r == reference))
        {
            let revision = self.pick_choices.take().unwrap().revision;
            self.model.select_edge(reference.into(), revision);
            self.model.mode = Mode::Fillet;
            self.inspector_open = true;
            self.model.notice =
                "Edge selected (amber). Preview a fillet or tap another edge.".into();
        }
    }
    fn edge_choices(&mut self, ctx: &egui::Context) {
        if !self.choices_current() {
            self.pick_choices = None;
            self.choice_hover = None;
            return;
        }
        let refs = self.pick_choices.as_ref().unwrap().references.clone();
        let mut selected = None;
        let mut hovered = None;
        let mut open = true;
        let mut cancel = false;
        let position = ctx.content_rect().center() - Vec2::new(170., 130.);
        egui::Window::new("Choose an edge")
            .id(egui::Id::new("edge-disambiguation"))
            .collapsible(false)
            .resizable(false)
            .fixed_pos(position)
            .default_width(340.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Several edges overlap in this view.");
                ui.label("Select the intended edge below.");
                ui.add_space(8.);
                egui::ScrollArea::vertical()
                    .id_salt("edge-choice-list")
                    .max_height(260.)
                    .show(ui, |ui| {
                        for (i, reference) in refs.iter().enumerate() {
                            let edge = self.model.display_scene().and_then(|(s, _)| {
                                s.scene
                                    .edges
                                    .iter()
                                    .enumerate()
                                    .find(|(_, e)| e.reference == *reference)
                            });
                            let label = edge
                                .map(|(index, e)| {
                                    format!(
                                        "{} · Edge {} · {:.4} mm",
                                        i + 1,
                                        index + 1,
                                        e.length.get()
                                    )
                                })
                                .unwrap_or_else(|| format!("{} · Edge", i + 1));
                            let response = ui.add_sized([320., 40.], egui::Button::new(label));
                            self.geometry.record(format!("choice-{i}"), &response, true);
                            if response.hovered() {
                                hovered = Some(reference.clone());
                            }
                            let key = [
                                egui::Key::Num1,
                                egui::Key::Num2,
                                egui::Key::Num3,
                                egui::Key::Num4,
                                egui::Key::Num5,
                                egui::Key::Num6,
                                egui::Key::Num7,
                                egui::Key::Num8,
                            ]
                            .get(i);
                            if response.clicked()
                                || key.is_some_and(|key| ctx.input(|input| input.key_pressed(*key)))
                            {
                                selected = Some(reference.clone());
                            }
                        }
                    });
                ui.add_space(4.);
                if ui.button("Cancel (Esc)").clicked()
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    cancel = true;
                }
            });
        if self.choice_hover != hovered {
            self.choice_hover = hovered;
            ctx.request_repaint();
        }
        if let Some(reference) = selected {
            self.choose_edge(&reference);
            ctx.request_repaint();
        }
        if !open || cancel {
            self.pick_choices = None;
        }
    }
    fn viewport(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        self.geometry.viewport = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        let multi = ctx.input(|i| i.multi_touch());
        if let Some(gesture) = multi.filter(|g| rect.contains(g.start_pos) || self.multi_active) {
            self.multi_active = true;
            self.touch_guard = true;
            let before = self.model.camera;
            self.model.camera.zoom_at(
                gesture.zoom_delta as f64,
                [
                    (gesture.center_pos.x - rect.left()) as f64 / rect.width() as f64,
                    (gesture.center_pos.y - rect.top()) as f64 / rect.height() as f64,
                ],
            );
            self.model.camera.pan_by(
                [
                    gesture.translation_delta.x as f64,
                    gesture.translation_delta.y as f64,
                ],
                [rect.width() as f64, rect.height() as f64],
            );
            if self.model.camera != before {
                self.model.view_changed();
            }
        }
        let gesture_guard = self.touch_guard;

        for x in (rect.left() as i32..rect.right() as i32).step_by(32) {
            painter.line_segment(
                [
                    egui::pos2(x as f32, rect.top()),
                    egui::pos2(x as f32, rect.bottom()),
                ],
                egui::Stroke::new(0.5, Color32::from_rgb(226, 232, 237)),
            );
        }
        for y in (rect.top() as i32..rect.bottom() as i32).step_by(32) {
            painter.line_segment(
                [
                    egui::pos2(rect.left(), y as f32),
                    egui::pos2(rect.right(), y as f32),
                ],
                egui::Stroke::new(0.5, Color32::from_rgb(226, 232, 237)),
            );
        }
        let factor = (768. / rect.width().max(rect.height()).max(1.)).min(1.);
        let size = [
            (rect.width() * factor).round().clamp(64., 768.) as u32,
            (rect.height() * factor).round().clamp(64., 768.) as u32,
        ];
        if self.viewport_size != size {
            self.viewport_size = size;
            self.model.view_changed();
        }
        if !gesture_guard && response.dragged() && ctx.input(|i| i.pointer.delta() != Vec2::ZERO) {
            let delta = ctx.input(|i| i.pointer.delta());
            if ctx.input(|i| i.modifiers.shift)
                || response.dragged_by(egui::PointerButton::Secondary)
            {
                self.model.camera.pan_by(
                    [delta.x as f64, delta.y as f64],
                    [rect.width() as f64, rect.height() as f64],
                );
            } else {
                self.model.camera.orbit(
                    [delta.x as f64, delta.y as f64],
                    [rect.width() as f64, rect.height() as f64],
                );
            }
            self.model.view_changed();
        }
        if response.hovered() && !gesture_guard {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01 {
                let pos = response.hover_pos().unwrap_or(rect.center());
                self.model.camera.zoom_at(
                    (scroll as f64 * 0.002).exp(),
                    [
                        (pos.x - rect.left()) as f64 / rect.width() as f64,
                        (pos.y - rect.top()) as f64 / rect.height() as f64,
                    ],
                );
                self.model.view_changed();
            }
        }
        if response.double_clicked() && !gesture_guard {
            self.model.camera.fit();
            self.model.view_changed();
        }
        let can_pick = self.model.preview.is_none() && !self.model.busy && self.model.connected;
        let touch = ctx.input(|i| {
            i.any_touches()
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::End,
                            ..
                        }
                    )
                })
        });
        if response.clicked() && can_pick && !gesture_guard {
            // Touch release can already have removed hover_pos; the interaction owns its
            // release position independently of hover state.
            if let (Some(pos), Some(document)) =
                (response.interact_pointer_pos(), self.model.active.clone())
            {
                let point = [
                    (pos.x - rect.left()) as f64 / rect.width() as f64
                        * self.viewport_size[0] as f64,
                    (pos.y - rect.top()) as f64 / rect.height() as f64
                        * self.viewport_size[1] as f64,
                ];
                let slop = if touch { 14. } else { 6. };
                self.pick_choices = None;
                self.pending_pick = Some(PendingPick {
                    document,
                    revision: self.model.current().map_or(0, |d| d.revision.get()),
                    epoch: self.model.view_epoch,
                    point,
                    radius: [
                        slop * self.viewport_size[0] as f64 / rect.width() as f64,
                        slop * self.viewport_size[1] as f64 / rect.height() as f64,
                    ],
                    deadline: ctx.input(|i| i.time) + 3.,
                });
                if !self.frame_current() {
                    self.model.notice = "Updating pick map… your tap is queued.".into();
                }
            }
        }
        self.resolve_pick(ctx.input(|i| i.time));
        if self.pending_pick.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.model.hover = None;
        if response.hovered()
            && !response.dragged()
            && !gesture_guard
            && can_pick
            && self.frame_current()
        {
            if let (Some(pos), Some(frame)) = (response.hover_pos(), &self.frame) {
                let point = [
                    (pos.x - rect.left()) as f64 / rect.width() as f64
                        * self.viewport_size[0] as f64,
                    (pos.y - rect.top()) as f64 / rect.height() as f64
                        * self.viewport_size[1] as f64,
                ];
                if let Ok(pick) = frame.pick_edge_near(
                    point,
                    [
                        6. * self.viewport_size[0] as f64 / rect.width() as f64,
                        6. * self.viewport_size[1] as f64 / rect.height() as f64,
                    ],
                ) {
                    self.model.hover = pick["reference"].as_str().map(str::to_owned);
                }
            }
        }
        if self.choices_current() {
            self.model.hover = self.choice_hover.clone();
        }
        let has_scene = self
            .model
            .display_scene()
            .is_some_and(|(s, _)| s.scene.mesh.is_some());
        if has_scene && self.local_backend.is_some() {
            if let Some((bound, _)) = self.model.display_scene() {
                painter.rect_filled(rect, 0., CANVAS);
                painter.add(
                    crate::local_view::View {
                        scene: bound.scene.clone(),
                        camera: self.model.camera,
                        selected: self.model.selection.as_ref().map(|s| s.reference.clone()),
                        hovered: self.model.hover.clone(),
                        display_size: [rect.width(), rect.height()],
                        size: self.viewport_size,
                    }
                    .callback(rect),
                );
            }
        } else if has_scene {
            if let Some(texture) = self.texture.as_ref().filter(|_| self.frame_current()) {
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                    Color32::WHITE,
                );
            }
        } else {
            painter.text(
                rect.center() - Vec2::new(0., 15.),
                egui::Align2::CENTER_CENTER,
                if self.model.current().is_some() {
                    "Your document is ready"
                } else {
                    "A shared space for precise ideas"
                },
                egui::FontId::proportional(23.),
                Color32::from_rgb(57, 78, 94),
            );
            painter.text(
                rect.center() + Vec2::new(0., 18.),
                egui::Align2::CENTER_CENTER,
                if self.model.current().is_some() {
                    "Set Box dimensions, then preview and confirm."
                } else {
                    "Create a document or connect your Agent to begin."
                },
                egui::FontId::proportional(13.),
                Color32::from_rgb(107, 126, 141),
            );
        }
        if has_scene && self.local_backend.is_none() && !self.frame_current() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Updating viewport…",
                egui::FontId::proportional(16.),
                Color32::from_rgb(97, 117, 133),
            );
        }
        painter.text(
            rect.left_top() + Vec2::new(22., 22.),
            egui::Align2::LEFT_TOP,
            if self.model.preview.as_ref().is_some_and(|p| p.ready) {
                "CANDIDATE PREVIEW"
            } else if self.tree_open {
                ""
            } else {
                "ORTHOGRAPHIC VIEW"
            },
            egui::FontId::proportional(11.),
            Color32::from_rgb(97, 117, 133),
        );
        painter.text(
            rect.left_bottom() + Vec2::new(20., -20.),
            egui::Align2::LEFT_BOTTOM,
            if self.model.hover.is_some() {
                "Visible edge · tap/click to select"
            } else {
                "One finger: rotate   ·   Two fingers: zoom / pan   ·   Fit: center model"
            },
            egui::FontId::proportional(11.),
            Color32::from_rgb(97, 117, 133),
        );
        if !ctx.input(|i| i.any_touches()) {
            self.multi_active = false;
            self.touch_guard = false;
        }
        let origin = rect.right_bottom() - Vec2::new(42., 42.);
        for (axis, label, color) in [
            ([1., 0., 0.], "X", Color32::from_rgb(203, 91, 81)),
            ([0., 1., 0.], "Y", Color32::from_rgb(73, 152, 115)),
            ([0., 0., 1.], "Z", Color32::from_rgb(72, 121, 184)),
        ] {
            let p = self.model.camera.projected_axis(axis);
            let offset = Vec2::new(p[0] as f32 * 27., p[1] as f32 * 27.);
            painter.line_segment([origin, origin + offset], egui::Stroke::new(1.5, color));
            painter.text(
                origin + offset,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(10.),
                color,
            );
        }
    }
    fn export(&mut self, actions: &mut Vec<Action>) {
        let Some(document) = self
            .model
            .current()
            .filter(|d| !d.data.empty)
            .map(|d| d.document.clone())
        else {
            return;
        };
        let ticket = self.model.begin_job("Exporting SMB");
        if self.browser_downloads {
            actions.push(Action::DownloadNative { ticket, document });
        } else {
            actions.push(Action::Execute {
                ticket,
                command: Command::ExportNative {
                    document,
                    path: self.model.export_path.clone(),
                },
                label: "SMB exported".into(),
            });
        }
    }
    fn model_tree(&mut self, ctx: &egui::Context) {
        if !self.tree_open || self.model.current().is_none() {
            return;
        }
        let [x, y, w, h] = self.geometry.viewport;
        let width = 200f32.min((w * 0.38).max(145.));
        egui::Area::new(egui::Id::new("model-tree-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x + 12., y))
            .show(ctx, |ui| {
                ui.set_width(width);
                let frame = egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(247, 250, 252, 205))
                    .stroke(egui::Stroke::new(1., Color32::from_black_alpha(25)))
                    .corner_radius(8)
                    .inner_margin(10);
                let inner_height = (h - frame.total_margin().sum().y).max(0.);
                let panel = frame.show(ui, |ui| {
                    ui.set_height(inner_height);
                    ui.visuals_mut().override_text_color = Some(Color32::from_rgb(44, 65, 79));
                    let response = egui::CollapsingHeader::new("FeatureManager")
                        .id_salt("model-tree")
                        .default_open(true)
                        .show(ui, |ui| {
                            let Some(doc) = self.model.current() else {
                                return;
                            };
                            let id = doc.document.clone();
                            let revision = doc.revision;
                            ui.label(
                                RichText::new(format!("Solid body · r{}", revision.get()))
                                    .strong()
                                    .color(Color32::from_rgb(44, 65, 79)),
                            );
                            ui.label(format!(
                                "{} faces · {} edges",
                                doc.data.faces.unwrap_or(0),
                                doc.data.edges.unwrap_or(0)
                            ));
                            egui::ScrollArea::vertical()
                                .id_salt("tree-scroll")
                                .max_height(ui.available_height())
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    egui::CollapsingHeader::new("Operations")
                                        .id_salt("tree-history")
                                        .default_open(true)
                                        .show(ui, |ui| {
                                            let mut boxes = 0;
                                            let mut fillets = 0;
                                            for e in self.history.iter().filter(|e| {
                                                e.document == id
                                                    && e.revision.get() <= revision.get()
                                            }) {
                                                let label = match e.command.as_str() {
                                                    "model.box" => {
                                                        boxes += 1;
                                                        format!("Box {boxes}")
                                                    }
                                                    "model.fillet" => {
                                                        fillets += 1;
                                                        format!("Fillet {fillets}")
                                                    }
                                                    "document.undo" => "Undo".into(),
                                                    "document.redo" => "Redo".into(),
                                                    _ => continue,
                                                };
                                                ui.label(format!(
                                                    "{label} · r{}",
                                                    e.revision.get()
                                                ))
                                                .on_hover_text("Committed operation record");
                                            }
                                            if self.history_truncated {
                                                ui.small("Earlier records unavailable");
                                            }
                                            if boxes + fillets == 0 {
                                                ui.small("No operation records");
                                            }
                                        });
                                    let tree_scene = self
                                        .model
                                        .scene
                                        .as_ref()
                                        .filter(|s| s.revision == revision)
                                        .map(|s| s.scene.clone());
                                    let edges = tree_scene
                                        .as_ref()
                                        .map(|s| s.edges.as_slice())
                                        .unwrap_or(&[]);
                                    egui::CollapsingHeader::new(format!("Edges ({})", edges.len()))
                                        .id_salt("tree-edges")
                                        .show(ui, |ui| {
                                            for (i, e) in edges.iter().enumerate() {
                                                let enabled = self.model.can_edit()
                                                    && self.model.preview.is_none();
                                                if ui
                                                    .add_enabled(
                                                        enabled,
                                                        egui::Button::selectable(
                                                            self.model
                                                                .selection
                                                                .as_ref()
                                                                .is_some_and(|s| {
                                                                    s.reference == e.reference
                                                                }),
                                                            format!("Edge {}", i + 1),
                                                        ),
                                                    )
                                                    .clicked()
                                                {
                                                    self.model.select_edge(
                                                        e.reference.clone(),
                                                        revision.get(),
                                                    );
                                                    self.model.mode = Mode::Fillet;
                                                    self.inspector_open = true;
                                                }
                                            }
                                        });
                                });
                        });
                    self.geometry
                        .record("tree-header", &response.header_response, true);
                });
                self.geometry.record("tree-panel", &panel.response, true);
            });
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Vec<Action> {
        use crate::chrome::{tool, Icon};
        let mut actions = Vec::new();
        self.geometry = UiGeometry::default();
        egui::TopBottomPanel::top("brand")
            .exact_height(32.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(17, 24, 31))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("AI / CAD").strong().color(ACCENT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(if self.model.connected {
                                "SHARED / CONNECTED"
                            } else {
                                "OFFLINE"
                            })
                            .size(10.)
                            .color(MUTED),
                        );
                    });
                });
            });
        egui::TopBottomPanel::top("document-tabs")
            .exact_height(40.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 32, 40))
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("tabs-scroll")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let docs: Vec<_> = self
                                .model
                                .documents
                                .iter()
                                .map(|d| d.document.clone())
                                .collect();
                            for (i, id) in docs.iter().enumerate() {
                                let enabled = !self.model.busy || self.model.preview.is_some();
                                let response = ui.add_enabled(
                                    enabled,
                                    egui::Button::selectable(
                                        self.model.active.as_ref() == Some(id),
                                        format!("Document {:02}", i + 1),
                                    )
                                    .min_size(Vec2::new(122., 30.)),
                                );
                                self.geometry
                                    .record(format!("document-{i}"), &response, enabled);
                                if response.clicked() {
                                    self.model.select(id.clone());
                                    self.inspector_open = false;
                                    actions.push(Action::Select(id.clone()));
                                }
                            }
                            let enabled = self.model.connected
                                && !self.model.busy
                                && self.model.unknown.is_none();
                            let response = ui
                                .add_enabled(
                                    enabled,
                                    egui::Button::new("+").min_size(Vec2::new(38., 30.)),
                                )
                                .on_hover_text("New document");
                            self.geometry.record("new-document", &response, enabled);
                            if response.clicked() {
                                let ticket = self.model.begin_job("Creating document");
                                actions.push(Action::NewDocument { ticket });
                            }
                        });
                    });
            });
        egui::TopBottomPanel::top("command-ribbon")
            .exact_height(76.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(30, 39, 49))
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("ribbon-scroll")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.;
                        ui.horizontal(|ui| {
                            for (mode, icon, name, label) in [
                                (Mode::Box, Icon::Box, "box", "Box"),
                                (Mode::Fillet, Icon::Fillet, "fillet", "Fillet"),
                            ] {
                                let enabled = self.model.current().is_some() && !self.model.busy;
                                let response = tool(
                                    ui,
                                    icon,
                                    label,
                                    self.inspector_open && self.model.mode == mode,
                                    enabled,
                                );
                                self.geometry.record(name, &response, enabled);
                                if response.clicked() && enabled {
                                    if self.model.mode != mode {
                                        self.model.mode = mode;
                                        self.model.draft_changed();
                                    }
                                    self.inspector_open = true;
                                }
                            }
                            for (icon, name, label, active) in [
                                (
                                    Icon::Parameters,
                                    "parameters",
                                    "Params",
                                    self.inspector_open,
                                ),
                                (Icon::Tree, "tree", "Tree", self.tree_open),
                            ] {
                                let response = tool(ui, icon, label, active, true);
                                self.geometry.record(name, &response, true);
                                if response.clicked() {
                                    if name == "tree" {
                                        self.tree_open = !self.tree_open;
                                    } else {
                                        self.inspector_open = !self.inspector_open;
                                    }
                                }
                            }
                            ui.separator();
                            for (icon, name, label) in [
                                (Icon::Plus, "zoom-in", "Zoom +"),
                                (Icon::Minus, "zoom-out", "Zoom −"),
                                (Icon::Fit, "fit", "Fit"),
                            ] {
                                let response = tool(ui, icon, label, false, true);
                                self.geometry.record(name, &response, true);
                                if response.clicked() {
                                    if name == "fit" {
                                        self.model.camera.fit();
                                    } else {
                                        self.model.camera.zoom_at(
                                            if name == "zoom-in" { 1.25 } else { 0.8 },
                                            [0.5, 0.5],
                                        );
                                    }
                                    self.model.view_changed();
                                }
                            }
                            ui.separator();
                            let history = self
                                .model
                                .scene
                                .as_ref()
                                .filter(|s| {
                                    self.model
                                        .current()
                                        .is_some_and(|d| d.revision == s.revision)
                                })
                                .map(|s| (s.scene.can_undo, s.scene.can_redo))
                                .unwrap_or((false, false));
                            for (icon, name, label, available, redo) in [
                                (Icon::Undo, "undo", "Undo", history.0, false),
                                (Icon::Redo, "redo", "Redo", history.1, true),
                            ] {
                                let enabled = available
                                    && self.model.can_edit()
                                    && self.model.preview.is_none();
                                let response = tool(ui, icon, label, false, enabled);
                                self.geometry.record(name, &response, enabled);
                                if response.clicked() && enabled {
                                    let d = self.model.current().unwrap();
                                    let command = if redo {
                                        Command::Redo {
                                            document: d.document.clone(),
                                            revision: d.revision,
                                        }
                                    } else {
                                        Command::Undo {
                                            document: d.document.clone(),
                                            revision: d.revision,
                                        }
                                    };
                                    let ticket = self.model.begin_job(label);
                                    actions.push(Action::Execute {
                                        ticket,
                                        command,
                                        label: label.into(),
                                    });
                                }
                            }
                            for (icon, name, label) in [
                                (Icon::Refresh, "refresh", "Refresh"),
                                (Icon::Connect, "reconnect", "Connect"),
                                (Icon::Download, "download", "SMB"),
                            ] {
                                let enabled = !self.model.busy
                                    && (name != "download"
                                        || self.model.can_edit()
                                            && self.model.current().is_some_and(|d| !d.data.empty));
                                let response = tool(ui, icon, label, false, enabled);
                                self.geometry.record(name, &response, enabled);
                                if response.clicked() && enabled {
                                    match name {
                                        "download" => self.export(&mut actions),
                                        "refresh" => {
                                            let ticket = self.model.begin_job("Refreshing");
                                            actions.push(Action::Refresh { ticket });
                                        }
                                        _ => {
                                            let ticket = self.model.begin_job("Reconnecting");
                                            self.model.connecting = true;
                                            actions.push(Action::Reconnect { ticket });
                                        }
                                    }
                                }
                            }
                        });
                    });
            });
        egui::TopBottomPanel::bottom("status")
            .exact_height(28.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(17, 24, 31))
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.model.busy {
                        ui.spinner();
                    }
                    let text = self.model.error.as_deref().unwrap_or(&self.model.notice);
                    ui.add(
                        egui::Label::new(RichText::new(text).size(11.).color(
                            if self.model.error.is_some() {
                                SELECTED
                            } else {
                                MUTED
                            },
                        ))
                        .truncate(),
                    )
                    .on_hover_text(text);
                });
            });
        if self.inspector_open {
            let width = if ctx.content_rect().width() < 900. {
                242.
            } else {
                274.
            };
            egui::SidePanel::left("parameters")
                .exact_width(width)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(25, 32, 40))
                        .inner_margin(14),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("PropertyManager").size(11.).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let r = ui.button("×").on_hover_text("Close parameters");
                            self.geometry.record("close-parameters", &r, true);
                            if r.clicked() {
                                self.inspector_open = false;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("properties-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.properties(ui, ctx, &mut actions));
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(CANVAS).inner_margin(0))
            .show(ctx, |ui| self.viewport(ui, ctx));
        self.model_tree(ctx);
        self.edge_choices(ctx);
        self.geometry.parameters_open = self.inspector_open;
        self.geometry.tree_open = self.tree_open;
        self.geometry.scale = ctx.zoom_factor();
        self.geometry.selected = self.model.selection.as_ref().map(|s| s.reference.clone());
        self.geometry.document = self.model.active.as_ref().map(|d| d.as_str().to_owned());
        self.geometry.camera = self.model.camera;
        self.geometry.view_epoch = self.model.view_epoch;
        self.geometry.history_entries = self
            .history
            .iter()
            .filter(|e| self.model.active.as_ref() == Some(&e.document))
            .count();
        self.geometry.preview = self.model.preview.is_some();
        self.geometry.frame_current = self.frame_current();
        self.geometry.connected = self.model.connected;
        self.geometry.uncertain = self.model.unknown.is_some();
        actions
    }
}

#[cfg(test)]
mod picking_tests {
    use super::*;
    use serde_json::json;
    fn workspace() -> (Workspace, Event, egui::Context) {
        let mut w = Workspace {
            viewport_size: [96, 96],
            ..Default::default()
        };
        w.model.connected = true;
        let state:DocumentState=serde_json::from_value(json!({"revision":1,"units":"mm","empty":false,"valid":true,"volume":1,"faces":1,"edges":1})).unwrap();
        let doc = DocumentId::new("doc").unwrap();
        w.model.update_documents(vec![DocumentResult {
            document: doc.clone(),
            revision: Revision::new(1).unwrap(),
            data: state.clone(),
        }]);
        w.model.select(doc.clone());
        let mesh:Mesh=serde_json::from_value(json!({"points":[[0,0,0],[10,0,0],[0,10,0]],"triangles":[{"vertices":[0,1,2],"face":"face"}]})).unwrap();
        let edges: Vec<Edge> = serde_json::from_value(
            json!([{"reference":"edge","points":[[0,0,0],[10,0,0]],"length":10}]),
        )
        .unwrap();
        w.model.set_scene(BoundScene {
            document: doc.clone(),
            revision: Revision::new(1).unwrap(),
            scene: Arc::new(Scene {
                state,
                mesh: Some(mesh.clone()),
                edges: edges.clone(),
                can_undo: false,
                can_redo: false,
            }),
        });
        let mut frame = cad_render::render_view(&mesh, &edges, 96, 96, w.model.camera).unwrap();
        frame.metadata["document"] = json!("doc");
        frame.metadata["revision"] = json!(1);
        let point = (0..96)
            .flat_map(|y| (0..96).map(move |x| (x, y)))
            .find(|(x, y)| frame.pick(*x, *y, "edge").unwrap()["reference"] == "edge")
            .unwrap();
        w.pending_pick = Some(PendingPick {
            document: doc.clone(),
            revision: 1,
            epoch: w.model.view_epoch,
            point: [point.0 as f64 + 0.5, point.1 as f64 + 0.5],
            radius: [6., 6.],
            deadline: 10.,
        });
        let event = Event::View {
            epoch: w.model.view_epoch,
            rgb: frame.viewport_rgb(None),
            frame: Arc::new(frame),
            width: 96,
            height: 96,
            document: doc,
            revision: Revision::new(1).unwrap(),
            preview: false,
        };
        (w, event, egui::Context::default())
    }
    #[test]
    fn tap_waits_for_matching_frame_and_then_selects_once() {
        let (mut w, event, ctx) = workspace();
        w.resolve_pick(0.);
        assert!(w.model.selection.is_none());
        assert!(w.pending_pick.is_some());
        w.apply(event, &ctx);
        w.resolve_pick(1.);
        assert_eq!(w.model.selection.as_ref().unwrap().reference, "edge");
        assert!(w.pending_pick.is_none());
        let epoch = w.model.view_epoch;
        w.resolve_pick(2.);
        assert_eq!(w.model.view_epoch, epoch);
    }
    #[test]
    fn queued_tap_is_discarded_on_camera_or_document_revision_change_and_timeout() {
        let (mut w, event, ctx) = workspace();
        w.model.view_changed();
        w.apply(event, &ctx);
        w.resolve_pick(1.);
        assert!(w.model.selection.is_none());
        assert!(w.pending_pick.is_none());
        let (mut w, event, ctx) = workspace();
        w.model.documents[0].revision = Revision::new(2).unwrap();
        w.apply(event, &ctx);
        w.resolve_pick(1.);
        assert!(w.model.selection.is_none());
        assert!(w.pending_pick.is_none());
        let (mut w, _, _) = workspace();
        w.resolve_pick(11.);
        assert!(w.pending_pick.is_none());
        assert!(w.model.selection.is_none());
    }
    #[test]
    fn explicit_candidate_choice_is_scoped_and_never_accepts_arbitrary_edges() {
        for reference in ["edge", "second-edge"] {
            let (mut w, event, ctx) = workspace();
            w.apply(event, &ctx);
            let scene = &mut Arc::make_mut(&mut w.model.scene.as_mut().unwrap().scene).edges;
            let mut second = scene[0].clone();
            second.reference = "second-edge".into();
            scene.push(second);
            w.pending_pick = None;
            w.pick_choices = Some(PickChoices {
                document: w.model.active.clone().unwrap(),
                revision: 1,
                epoch: w.model.view_epoch,
                references: vec!["edge".into(), "second-edge".into()],
            });
            w.choose_edge("not-a-candidate");
            assert!(w.model.selection.is_none());
            w.choose_edge(reference);
            assert_eq!(w.model.selection.as_ref().unwrap().reference, reference);
            assert!(w.pick_choices.is_none());
        }
        let (mut w, event, ctx) = workspace();
        w.apply(event, &ctx);
        w.pending_pick = None;
        w.pick_choices = Some(PickChoices {
            document: w.model.active.clone().unwrap(),
            revision: 1,
            epoch: w.model.view_epoch,
            references: vec!["edge".into()],
        });
        w.model.view_changed();
        w.choose_edge("edge");
        assert!(w.model.selection.is_none());
    }
    #[test]
    fn portrait_layout_reserves_viewport_and_keeps_parameters_on_the_left() {
        for size in [[768., 1024.], [820., 1180.], [1280., 820.]] {
            let (mut w, _, ctx) = workspace();
            w.pending_pick = None;
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(size[0], size[1]),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input(), |ctx| {
                w.show(ctx);
            });
            assert!((w.geometry.viewport[2] - size[0]).abs() < 2.);
            assert!(w.geometry.viewport[3] > size[1] * 0.75);
            assert!(w.geometry.controls["box"].rect[1] < w.geometry.viewport[1]);
            let tree = w.geometry.controls["tree-panel"].rect;
            assert!((tree[1] - w.geometry.viewport[1]).abs() < 1.);
            assert!(
                (tree[3] - w.geometry.viewport[3]).abs() < 1.,
                "{tree:?} {:?}",
                w.geometry.viewport
            );
            let ratio = w.geometry.viewport[2] / w.geometry.viewport[3];
            assert!((w.viewport_size[0] as f32 / w.viewport_size[1] as f32 - ratio).abs() < 0.003);
            w.inspector_open = true;
            let _ = ctx.run(input(), |ctx| {
                w.show(ctx);
            });
            assert!(w.geometry.viewport[2] > size[0] * 0.65);
            for key in ["width", "depth", "height", "preview", "confirm", "cancel"] {
                let r = w.geometry.controls[key].rect;
                assert!(
                    r[0] + r[2] <= w.geometry.viewport[0] + 1.,
                    "{key}: {r:?}, {:?}",
                    w.geometry.viewport
                );
            }
        }
    }
}
