use super::*;
#[derive(Clone)]
pub(super) struct FeatureEditor {
    document: DocumentId,
    revision: Revision,
    feature: Feature,
    name: String,
    size: [String; 3],
    radius: String,
    suppressed: bool,
}
impl Workspace {
    pub(super) fn open_feature_editor(&mut self, feature: Feature) {
        let Some(d) = self.model.current() else {
            return;
        };
        let size = std::array::from_fn(|i| {
            feature.parameters["size"][i]
                .as_f64()
                .unwrap_or(10.)
                .to_string()
        });
        self.feature_editor = Some(FeatureEditor {
            document: d.document.clone(),
            revision: d.revision,
            name: feature.name.clone(),
            radius: feature.parameters["radius"]
                .as_f64()
                .unwrap_or(1.)
                .to_string(),
            suppressed: feature.suppressed,
            feature,
            size,
        });
        self.inspector_open = true;
        self.model.cancel_preview();
    }
    pub(super) fn feature_properties(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(mut edit) = self.feature_editor.take() else {
            return;
        };
        ui.heading("Edit feature");
        ui.label(format!("{} · {}", edit.feature.kind, edit.feature.id));
        ui.label("Name");
        let r = ui.add_sized(
            [ui.available_width(), 36.],
            egui::TextEdit::singleline(&mut edit.name).id_salt("feature-name"),
        );
        self.geometry.record("feature-name", &r, true);
        let mut valid = !edit.name.trim().is_empty() && edit.name.len() <= 120;
        let mut patch = serde_json::json!({"name":edit.name,"suppressed":edit.suppressed});
        if edit.feature.kind == "box" {
            let mut size = [0.; 3];
            for (i, label) in ["Width", "Depth", "Height"].iter().enumerate() {
                ui.label(*label);
                let r = ui.add_sized(
                    [ui.available_width(), 36.],
                    egui::TextEdit::singleline(&mut edit.size[i]).id_salt(("feature-size", i)),
                );
                self.geometry.record(format!("feature-size-{i}"), &r, true);
                match edit.size[i]
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v > 0. && *v <= 1e6)
                {
                    Some(v) => size[i] = v,
                    None => valid = false,
                }
            }
            patch["size"] = serde_json::json!(size);
        } else {
            ui.label("Radius (mm)");
            let r = ui.add_sized(
                [ui.available_width(), 36.],
                egui::TextEdit::singleline(&mut edit.radius).id_salt("feature-radius"),
            );
            self.geometry.record("feature-radius", &r, true);
            match edit
                .radius
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0. && *v <= 10000.)
            {
                Some(v) => patch["radius"] = serde_json::json!(v),
                None => valid = false,
            }
        }
        let r = ui.checkbox(&mut edit.suppressed, "Suppressed");
        self.geometry.record("feature-suppressed", &r, true);
        patch["suppressed"] = serde_json::json!(edit.suppressed);
        let current = self
            .model
            .current()
            .is_some_and(|d| d.document == edit.document && d.revision == edit.revision);
        if !current {
            ui.colored_label(
                Color32::LIGHT_RED,
                "Document changed. Reopen this feature before applying.",
            );
        }
        let enabled = valid
            && current
            && self.model.connected
            && !self.model.busy
            && self.model.unknown.is_none();
        let r = ui.add_enabled(
            enabled,
            egui::Button::new("Apply & recompute").min_size(Vec2::new(140., 38.)),
        );
        self.geometry.record("feature-apply", &r, enabled);
        if r.clicked() {
            let ticket = self.model.begin_job("Recomputing feature");
            actions.push(Action::Execute {
                ticket,
                command: Command::FeatureEdit {
                    document: edit.document.clone(),
                    revision: edit.revision,
                    feature: edit.feature.id.clone(),
                    patch,
                },
                label: "Feature updated".into(),
            });
        }
        let r = ui.button("Close editor");
        self.geometry.record("feature-close", &r, true);
        if r.clicked() {
            self.inspector_open = false;
            return;
        }
        ui.small(
            "Dependent features recompute atomically. A failed edit keeps the last valid model.",
        );
        if let Some(error) = &self.model.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        self.feature_editor = Some(edit);
    }
}
