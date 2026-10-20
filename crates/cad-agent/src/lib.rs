//! Agent-neutral context, bounded autonomous tasks and Commander operation contracts.
#![forbid(unsafe_code)]
use cad_protocol::*;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
pub mod host;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    #[default]
    Edge,
    Face,
    Body,
    Feature,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub name: String,
    pub document: DocumentId,
    pub revision: Revision,
    pub kind: ReferenceKind,
    pub target: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: String,
}
impl Attachment {
    pub fn from_bytes(name: String, bytes: &[u8]) -> Result<Self, String> {
        use base64::Engine;
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[255, 216, 255]) {
            "image/jpeg"
        } else {
            "text/plain"
        };
        let value = Self {
            name,
            mime: mime.into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        value.bytes()?;
        Ok(value)
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        use base64::Engine;
        if self.name.is_empty() || self.name.len() > 200 || self.data.len() > 3_000_000 {
            return Err("Attachment name or size exceeds limit".into());
        }
        let b = base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .map_err(|_| "Invalid attachment encoding")?;
        match self.mime.as_str() {
            "image/png" if b.starts_with(b"\x89PNG\r\n\x1a\n") => (),
            "image/jpeg" if b.starts_with(&[255, 216, 255]) => (),
            "text/plain" if std::str::from_utf8(&b).is_ok() => (),
            _ => return Err("Only UTF-8 text, PNG and JPEG attachments are supported".into()),
        }
        Ok(b)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub document: DocumentId,
    pub revision: Revision,
    pub text: String,
    pub references: Vec<Reference>,
    pub attachments: Vec<Attachment>,
    pub history: Vec<Message>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Proposal {
    Fillet {
        reference: String,
        radius: f64,
    },
    Box {
        size: [f64; 3],
    },
    FeatureEdit {
        feature: String,
        patch: serde_json::Value,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    #[serde(default)]
    pub failed: bool,
    pub text: String,
    pub proposal: Option<Proposal>,
}
impl Turn {
    pub fn validate(&self) -> Result<(), String> {
        if self.text.trim().is_empty()
            || self.text.len() > 16000
            || self.references.len() > 32
            || self.attachments.len() > 8
            || self.history.len() > 24
        {
            return Err("Empty message or context exceeds limits".into());
        }
        if self
            .history
            .iter()
            .any(|m| !["user", "assistant"].contains(&m.role.as_str()) || m.text.len() > 16000)
        {
            return Err("Invalid conversation history".into());
        }
        let mut total = 0;
        for a in &self.attachments {
            total += a.bytes()?.len();
        }
        if total > 4_000_000 {
            return Err("Attachments exceed 4 MB total".into());
        }
        for r in &self.references {
            if r.document != self.document
                || r.revision != self.revision
                || r.name.is_empty()
                || r.name.len() > 120
                || r.target.len() > 512
            {
                return Err(
                    "Reference is stale or belongs to another document; select it again".into(),
                );
            }
        }
        Ok(())
    }
    pub fn validate_reply(&self, reply: &Reply) -> Result<(), String> {
        if reply.text.trim().is_empty() || reply.text.len() > 16000 {
            return Err("Invalid Agent response text".into());
        }
        match &reply.proposal {
            Some(Proposal::Fillet { reference, radius }) => {
                if !radius.is_finite()
                    || !(0.001..=10000.).contains(radius)
                    || !self
                        .references
                        .iter()
                        .any(|r| r.kind == ReferenceKind::Edge && &r.target == reference)
                {
                    return Err("Agent proposal must use an explicitly referenced edge and a valid constant radius".into());
                }
            }
            Some(Proposal::FeatureEdit { feature, patch }) => {
                validate_ref(patch, "FeaturePatch").map_err(|e| e.to_string())?;
                if patch.as_object().is_none_or(|p| p.is_empty())
                    || !self.references.iter().any(|r| {
                        r.kind == ReferenceKind::Feature && r.target == format!("feature:{feature}")
                    })
                {
                    return Err("Feature edit must target a current named feature".into());
                }
            }
            Some(Proposal::Box { size })
                if size
                    .iter()
                    .any(|v| !v.is_finite() || !(0.01..=10000.).contains(v)) =>
            {
                return Err("Invalid proposed Box size".into())
            }
            _ => (),
        }
        Ok(())
    }
}
/// Validate against the authoritative current scene; names are labels, never lookup keys.
pub fn validate_references(
    turn: &Turn,
    scene: &DocumentResult<Scene>,
    events: &[Event],
) -> Result<(), String> {
    turn.validate()?;
    if scene.document != turn.document || scene.revision != turn.revision {
        return Err("Document changed; select references again".into());
    }
    for r in &turn.references {
        let exists =
            match r.kind {
                ReferenceKind::Edge => scene.data.edges.iter().any(|e| e.reference == r.target),
                ReferenceKind::Face => scene
                    .data
                    .mesh
                    .as_ref()
                    .is_some_and(|m| m.triangles.iter().any(|t| t.face == r.target)),
                ReferenceKind::Body => r.target == "body" && !scene.data.state.empty,
                ReferenceKind::Feature => {
                    scene.data.features.as_ref().is_some_and(|fs| {
                        fs.iter().any(|f| r.target == format!("feature:{}", f.id))
                    }) || (scene.data.features.is_none()
                        && events.iter().any(|e| {
                            e.document == turn.document
                                && e.revision.get() <= turn.revision.get()
                                && matches!(e.command.as_str(), "model.box" | "model.fillet")
                                && r.target == format!("event:{}", e.event_id.get())
                        }))
                }
            };
        if !exists {
            return Err(format!(
                "Reference {} no longer exists in this document",
                r.name
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn turn() -> Turn {
        Turn {
            document: DocumentId::new("doc").unwrap(),
            revision: Revision::new(1).unwrap(),
            text: "fillet this edge".into(),
            references: vec![Reference {
                name: "Edge 1".into(),
                document: DocumentId::new("doc").unwrap(),
                revision: Revision::new(1).unwrap(),
                kind: ReferenceKind::Edge,
                target: "edge-ref".into(),
            }],
            attachments: vec![],
            history: vec![],
        }
    }
    #[test]
    fn only_named_current_edges_can_receive_fillet_proposals() {
        let mut t = turn();
        t.validate().unwrap();
        let mut r = Reply {
            failed: false,
            text: "Preview this fillet".into(),
            proposal: Some(Proposal::Fillet {
                reference: "edge-ref".into(),
                radius: 1.,
            }),
        };
        t.validate_reply(&r).unwrap();
        r.proposal = Some(Proposal::Fillet {
            reference: "invented-edge".into(),
            radius: 1.,
        });
        assert!(t.validate_reply(&r).is_err());
        t.references[0].revision = Revision::new(2).unwrap();
        assert!(t.validate().is_err());
    }
    #[test]
    fn rejects_unbounded_or_disguised_attachments_and_unknown_operations() {
        assert!(Attachment {
            name: "x.png".into(),
            mime: "image/png".into(),
            data: "aGVsbG8=".into()
        }
        .bytes()
        .is_err());
        assert!(serde_json::from_str::<Reply>(
            r#"{"text":"ok","proposal":{"kind":"shell","command":"rm"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Reply>(r#"{"text":"ok","proposal":{"kind":"fillet","reference":"edge-ref","radius":1,"end_radius":2}}"#).is_err());
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn authoritative_scene_rejects_missing_topology_and_cross_document_references() {
        let document = DocumentId::new("doc").unwrap();
        let revision = Revision::new(2).unwrap();
        let state = DocumentState {
            revision,
            units: "mm".into(),
            empty: false,
            volume: None,
            faces: Some(0),
            edges: Some(1),
            valid: Some(true),
        };
        let scene = DocumentResult {
            document: document.clone(),
            revision,
            data: Scene {
                state,
                mesh: None,
                edges: vec![Edge {
                    reference: "actual-edge".into(),
                    points: vec![],
                    length: Finite::new(10.).unwrap(),
                }],
                can_undo: true,
                can_redo: false,
                features: None,
                timeline: None,
            },
        };
        let mut turn = Turn {
            document: document.clone(),
            revision,
            text: "use this edge".into(),
            references: vec![Reference {
                name: "named edge".into(),
                document,
                revision,
                kind: ReferenceKind::Edge,
                target: "actual-edge".into(),
            }],
            attachments: vec![],
            history: vec![],
        };
        validate_references(&turn, &scene, &[]).unwrap();
        turn.references[0].target = "missing-edge".into();
        assert!(validate_references(&turn, &scene, &[]).is_err());
        turn.references[0].kind = ReferenceKind::Feature;
        turn.references[0].target = "event:123".into();
        assert!(validate_references(&turn, &scene, &[]).is_err());
        turn.references[0].kind = ReferenceKind::Body;
        turn.references[0].target = "body".into();
        validate_references(&turn, &scene, &[]).unwrap();
        turn.references[0].document = DocumentId::new("another-doc").unwrap();
        assert!(validate_references(&turn, &scene, &[]).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRequest {
    pub id: String,
    pub turn: Turn,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepReceipt {
    pub operation: Proposal,
    pub revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub document: DocumentId,
    pub before: Revision,
    pub revision: Revision,
    pub phase: String,
    pub text: String,
    pub steps: Vec<StepReceipt>,
    pub can_undo: bool,
    pub done: bool,
    #[serde(default)]
    pub atomic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
}
