use cad_protocol::*;
use cad_ui_egui::{Mode, Model};
use serde_json::json;
fn document(revision: u64) -> DocumentResult<DocumentState> {
    serde_json::from_value(json!({"document":"doc","revision":revision,"data":{"revision":revision,"units":"mm","empty":false,"valid":true,"volume":1000,"faces":6,"edges":12}})).unwrap()
}
#[test]
fn preview_cancel_drafts_staleness_and_single_confirmation() {
    let mut m = Model {
        connected: true,
        documents: vec![document(1)],
        ..Default::default()
    };
    m.select(DocumentId::new("doc").unwrap());
    let (ticket, command) = m.begin_preview().unwrap();
    assert!(matches!(command, Command::Preview { .. }));
    m.cancel_preview();
    assert!(!m.accept_preview(ticket));
    assert!(m.confirm().is_none());
    let (ticket, _) = m.begin_preview().unwrap();
    assert!(m.accept_preview(ticket));
    let command = m.confirm().unwrap();
    assert!(matches!(command, Command::Box { .. }));
    assert!(m.confirm().is_none());
    m.finish_job();
    let (ticket, _) = m.begin_preview().unwrap();
    m.draft_changed();
    assert!(!m.accept_preview(ticket));
    let (ticket, _) = m.begin_preview().unwrap();
    assert!(m.accept_preview(ticket));
    m.update_documents(vec![document(2)]);
    assert!(m.confirm().is_none());
    assert!(m.notice.contains("stale"));
}
#[test]
fn camera_and_selection_are_local_and_revision_guarded() {
    let mut a = Model::default();
    let mut b = Model::default();
    for m in [&mut a, &mut b] {
        m.connected = true;
        m.documents = vec![document(7)];
        m.select(DocumentId::new("doc").unwrap());
    }
    a.camera.yaw += 0.7;
    a.view_changed();
    a.select_edge("edge-a".into(), 7);
    a.mode = Mode::Fillet;
    assert!(a.begin_preview().is_some());
    assert!(b.selection.is_none());
    assert!(b.begin_fillet_preview().is_none());
    assert_ne!(a.camera.yaw, b.camera.yaw);
    assert_eq!(a.current().unwrap().revision.get(), 7);
    a.update_documents(vec![document(8)]);
    assert!(a.selection.is_none());
    assert!(a.confirm().is_none());
}

#[test]
fn authoritative_scene_advance_and_late_disconnect_invalidate_preview() {
    use cad_ui_egui::{BoundScene, Event, Failure, Workspace};
    use std::sync::Arc;
    let mut w = Workspace::default();
    w.model.connected = true;
    w.model.documents = vec![document(1)];
    w.model.select(DocumentId::new("doc").unwrap());
    let (ticket, _) = w.model.begin_preview().unwrap();
    assert!(w.model.accept_preview(ticket));
    let scene = Scene {
        state: document(2).data,
        mesh: None,
        edges: vec![],
        can_undo: true,
        can_redo: false,
    };
    w.model.set_scene(BoundScene {
        document: DocumentId::new("doc").unwrap(),
        revision: Revision::new(2).unwrap(),
        scene: Arc::new(scene),
    });
    assert!(w.model.preview.is_none());
    assert!(w.model.notice.contains("stale"));
    let (ticket, _) = w.model.begin_preview().unwrap();
    w.model.cancel_preview();
    w.apply(
        Event::Failed {
            ticket: Some(ticket),
            preview: true,
            failure: Failure {
                kind: "service_unavailable".into(),
                message: "late disconnect".into(),
                request: None,
            },
        },
        &egui::Context::default(),
    );
    assert!(!w.model.connected);
    assert!(w.model.error.unwrap().contains("late disconnect"));
}

#[test]
fn reconciliation_receipt_is_not_hidden_by_refresh_completion() {
    use cad_ui_egui::{Event, Workspace};
    let mut w = Workspace::default();
    let ticket = w.model.begin_job("Reconnecting");
    let ctx = egui::Context::default();
    w.apply(
        Event::Reconciled {
            completed: false,
            message: "Original result unavailable. Inspect refreshed state.".into(),
        },
        &ctx,
    );
    w.apply(
        Event::Completed {
            ticket,
            message: "Workspace synchronized.".into(),
        },
        &ctx,
    );
    assert!(!w.model.busy);
    assert!(w.model.notice.contains("unavailable"));
}
