use cad_access::Config;
use cad_client::{protocol::*, ServiceOptions, SharedClient};
use cad_gui::bridge::Bridge;
use cad_ui_egui::{Action, Event};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
fn config() -> Config {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    Config {
        options: ServiceOptions {
            runtime_dir: std::env::temp_dir().join(format!(
                "cad-gui-test-{}",
                cad_client::shared::unique_id().unwrap()
            )),
            executable: root.join("target/debug/cad-service"),
            worker: std::env::var_os("CAD_SERVER")
                .map(PathBuf::from)
                .unwrap_or(root.join("build/cad-server")),
        },
        client_id: format!("gui-{}", cad_client::shared::unique_id().unwrap()),
        session: None,
        isolated: false,
        command: None,
    }
}
fn wait(bridge: &Bridge, mut predicate: impl FnMut(&Event) -> bool) -> Event {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let event = bridge
            .events
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .expect("GUI background did not publish expected event");
        if predicate(&event) {
            return event;
        }
    }
}
fn revision(event: &Event, expected: u64) -> bool {
    matches!(event,Event::Scene(scene)if scene.revision.get()==expected)
}
#[test]
fn real_gui_backend_shares_previews_commits_and_survives_window_exit() {
    let config = config();
    let options = config.options.clone();
    let bridge = Bridge::start(config.clone(), Arc::new(|| {}));
    let Event::Connected { service, .. } = wait(&bridge, |e| matches!(e, Event::Connected { .. }))
    else {
        unreachable!()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut peer = runtime
        .block_on(SharedClient::connect_or_start(&options, "agent-peer"))
        .unwrap();
    assert_eq!(peer.endpoint().service_id, service);
    bridge.send(Action::NewDocument { ticket: 1 }).unwrap();
    let Event::Created { document, .. } =
        wait(&bridge, |e| matches!(e, Event::Created { ticket: 1, .. }))
    else {
        unreachable!()
    };
    wait(&bridge, |e| revision(e, 0));
    bridge
        .send(Action::Preview {
            ticket: 2,
            command: Command::Preview {
                document: document.clone(),
                revision: Revision::new(0).unwrap(),
                operation: PreviewOperation::Box {
                    size: [Finite::new(10.).unwrap(); 3],
                },
            },
        })
        .unwrap();
    let Event::Preview { scene, .. } =
        wait(&bridge, |e| matches!(e, Event::Preview { ticket: 2, .. }))
    else {
        unreachable!()
    };
    assert_eq!(scene.scene.state.volume.unwrap().get(), 1000.);
    let state: DocumentResult<DocumentState> = runtime
        .block_on(peer.execute(Command::DocumentState {
            document: document.clone(),
        }))
        .unwrap()
        .result_as()
        .unwrap();
    assert!(state.data.empty);
    assert_eq!(state.revision.get(), 0);
    // Cancel is local discard. Agent then changes the authoritative document.
    assert!(
        runtime
            .block_on(peer.execute(Command::Box {
                document: document.clone(),
                revision: Revision::new(0).unwrap(),
                size: [Finite::new(10.).unwrap(); 3]
            }))
            .unwrap()
            .ok
    );
    let Event::Scene(scene) = wait(&bridge, |e| revision(e, 1)) else {
        unreachable!()
    };
    bridge
        .send(Action::Execute {
            ticket: 3,
            command: Command::Box {
                document: document.clone(),
                revision: Revision::new(0).unwrap(),
                size: [Finite::new(2.).unwrap(); 3],
            },
            label: "should not commit stale preview".into(),
        })
        .unwrap();
    let Event::Failed { failure, .. } = wait(&bridge, |e| {
        matches!(
            e,
            Event::Failed {
                ticket: Some(3),
                ..
            }
        )
    }) else {
        unreachable!()
    };
    assert_eq!(failure.kind, "revision_conflict");
    let edge = scene.scene.edges[0].reference.clone();
    bridge
        .send(Action::Preview {
            ticket: 4,
            command: Command::Preview {
                document: document.clone(),
                revision: Revision::new(1).unwrap(),
                operation: PreviewOperation::Fillet {
                    edge: edge.clone(),
                    radius: Finite::new(1.).unwrap(),
                },
            },
        })
        .unwrap();
    wait(&bridge, |e| matches!(e, Event::Preview { ticket: 4, .. }));
    assert_eq!(
        runtime.block_on(peer.snapshot()).unwrap()["documents"][0]["revision"],
        1
    );
    bridge
        .send(Action::Execute {
            ticket: 5,
            command: Command::Fillet {
                document: document.clone(),
                revision: Revision::new(1).unwrap(),
                edge,
                radius: Finite::new(1.).unwrap(),
            },
            label: "Fillet committed".into(),
        })
        .unwrap();
    let Event::Scene(scene) = wait(&bridge, |e| revision(e, 2)) else {
        unreachable!()
    };
    assert!(scene.scene.state.valid.unwrap());
    assert!(scene.scene.state.volume.unwrap().get() < 1000.);
    runtime
        .block_on(peer.execute(Command::Undo {
            document: document.clone(),
            revision: Revision::new(2).unwrap(),
        }))
        .unwrap();
    wait(&bridge, |e| revision(e, 3));
    runtime
        .block_on(peer.execute(Command::Redo {
            document: document.clone(),
            revision: Revision::new(3).unwrap(),
        }))
        .unwrap();
    wait(&bridge, |e| revision(e, 4));
    drop(bridge);
    assert_eq!(
        runtime.block_on(peer.snapshot()).unwrap()["documents"][0]["revision"],
        4
    );
    let mut next = config.clone();
    next.client_id = "reopened-gui".into();
    let reopened = Bridge::start(next, Arc::new(|| {}));
    let Event::Connected {
        service: next_service,
        ..
    } = wait(&reopened, |e| matches!(e, Event::Connected { .. }))
    else {
        unreachable!()
    };
    assert_eq!(next_service, service);
    wait(&reopened, |e| revision(e, 4));
    runtime.block_on(peer.shutdown()).unwrap();
    let Event::Failed { failure, .. } = wait(&reopened, |e| matches!(e, Event::Failed { .. }))
    else {
        unreachable!()
    };
    assert_eq!(failure.kind, "service_unavailable");
}
#[test]
fn unavailable_service_is_reported_without_a_ui_thread_wait() {
    let mut config = config();
    config.options.executable = "/missing-ai-cad-service".into();
    let started = Instant::now();
    let bridge = Bridge::start(config, Arc::new(|| {}));
    assert!(started.elapsed() < Duration::from_millis(100));
    let Event::Failed { failure, .. } = wait(&bridge, |e| matches!(e, Event::Failed { .. })) else {
        unreachable!()
    };
    assert_eq!(failure.kind, "service_unavailable");
    assert!(failure.message.contains("No such file"));
}
