use cad_client::{protocol::*, ServiceOptions, SharedClient};
use cad_protocol::service::*;
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
fn options() -> ServiceOptions {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    ServiceOptions {
        runtime_dir: std::env::temp_dir().join(format!(
            "p7-viewport-{}",
            cad_client::shared::unique_id().unwrap()
        )),
        executable: PathBuf::from(env!("CARGO_BIN_EXE_cad-service")),
        worker: root.join("build/cad-server"),
    }
}
async fn view(client: &SharedClient, viewport: Viewport) -> Result<Value, cad_client::ClientError> {
    SharedClient::control(
        client.endpoint(),
        ServiceRequest::RenderView {
            session: client.context().session_id.as_str().into(),
            view: viewport,
        },
    )
    .await
}
async fn capture(client: &SharedClient, id: &str) -> Result<Value, cad_client::ClientError> {
    SharedClient::control(
        client.endpoint(),
        ServiceRequest::RenderCapture {
            session: client.context().session_id.as_str().into(),
            render_id: id.into(),
        },
    )
    .await
}
#[tokio::test]
async fn shared_viewport_frame_camera_epoch_revision_preview_and_cpu_fallback() {
    let options = options();
    let mut client = SharedClient::connect_or_start(&options, "viewport-test")
        .await
        .unwrap();
    let doc: CreatedDocument = client
        .execute(Command::DocumentCreate)
        .await
        .unwrap()
        .result_as()
        .unwrap();
    client
        .execute(Command::Box {
            document: doc.document.clone(),
            revision: Revision::new(0).unwrap(),
            size: [Finite::new(10.).unwrap(); 3],
        })
        .await
        .unwrap();
    let mut viewport = Viewport {
        document: doc.document.clone(),
        revision: Revision::new(1).unwrap(),
        width: 384,
        height: 320,
        camera: serde_json::to_value(cad_render::Camera::default()).unwrap(),
        epoch: 1,
        view_id: "desktop-window".into(),
        preview: None,
        backend: "gpu".into(),
    };
    let wire = view(&client, viewport.clone()).await.unwrap();
    let frame: cad_render::Frame = serde_json::from_value(wire.clone()).unwrap();
    frame.validate().unwrap();
    assert_eq!(frame.metadata["backend"], "wgpu");
    assert!(
        serde_json::to_vec(&wire).unwrap().len() < 1_000_000,
        "planes must use bounded compressed wire encoding; actual bytes={}",
        serde_json::to_vec(&wire).unwrap().len()
    );
    let id = frame.metadata["render_id"].as_str().unwrap();
    assert_eq!(capture(&client, id).await.unwrap(), frame.metadata);
    assert_eq!(
        view(&client, viewport.clone()).await.unwrap()["metadata"]["render_id"],
        id
    );
    for (x, y, target) in [(192, 180, "face"), (192, 220, "edge"), (0, 0, "edge")] {
        let picked = SharedClient::control(
            client.endpoint(),
            ServiceRequest::RenderPick {
                session: client.context().session_id.as_str().into(),
                render_id: id.into(),
                x,
                y,
                target: target.into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            picked["reference"],
            frame.pick(x, y, target).unwrap()["reference"]
        );
        assert_eq!(picked["camera"], frame.metadata["camera"]);
    }
    viewport.camera["yaw"] = json!(0.9);
    assert!(view(&client, viewport.clone())
        .await
        .unwrap_err()
        .to_string()
        .contains("view_conflict"));
    viewport.epoch = 2;
    let next = view(&client, viewport.clone()).await.unwrap();
    assert_eq!(next["metadata"]["scene_cache_hit"], true);
    assert!(capture(&client, id)
        .await
        .unwrap_err()
        .to_string()
        .contains("stale_view"));
    viewport.epoch = 1;
    assert!(view(&client, viewport.clone())
        .await
        .unwrap_err()
        .to_string()
        .contains("stale_view"));
    viewport.epoch = 3;
    viewport.preview = Some(PreviewOperation::Box {
        size: [Finite::new(12.).unwrap(); 3],
    });
    let preview = view(&client, viewport.clone()).await.unwrap();
    assert!(
        capture(&client, preview["metadata"]["render_id"].as_str().unwrap())
            .await
            .unwrap_err()
            .to_string()
            .contains("preview_frame")
    );
    let state: DocumentResult<DocumentState> = client
        .execute(Command::DocumentState {
            document: doc.document.clone(),
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert_eq!(state.revision.get(), 1);
    assert_eq!(state.data.volume.unwrap().get(), 1000.);
    viewport.epoch = 4;
    viewport.preview = None;
    viewport.backend = "cpu".into();
    let cpu = view(&client, viewport.clone()).await.unwrap();
    assert_eq!(cpu["metadata"]["backend"], "cpu");
    assert!(cpu["metadata"]["images"]["depth"].is_string());
    client
        .execute(Command::Undo {
            document: doc.document,
            revision: Revision::new(1).unwrap(),
        })
        .await
        .unwrap();
    assert!(
        capture(&client, cpu["metadata"]["render_id"].as_str().unwrap())
            .await
            .unwrap_err()
            .to_string()
            .contains("stale_render")
    );
    assert!(view(&client, viewport)
        .await
        .unwrap_err()
        .to_string()
        .contains("stale_render"));
    client.shutdown().await.unwrap();
}
#[tokio::test]
async fn render_relay_upgrade_and_restart_preserve_original_worker_documents_and_history() {
    let options = options();
    let mut owner = SharedClient::connect_or_start(&options, "original-agent")
        .await
        .unwrap();
    let original = owner.endpoint().clone();
    let doc: CreatedDocument = owner
        .execute(Command::DocumentCreate)
        .await
        .unwrap()
        .result_as()
        .unwrap();
    owner
        .execute(Command::Box {
            document: doc.document.clone(),
            revision: Revision::new(0).unwrap(),
            size: [Finite::new(10.).unwrap(); 3],
        })
        .await
        .unwrap();
    for cycle in 0..2 {
        let mut relay = tokio::process::Command::new(&options.executable)
            .arg("--relay-runtime-dir")
            .arg(&options.runtime_dir)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut endpoint = original.clone();
        for _ in 0..100 {
            if let Ok(bytes) = std::fs::read(options.runtime_dir.join("endpoint.json")) {
                let candidate: ServiceEndpoint = serde_json::from_slice(&bytes).unwrap();
                if candidate.address != original.address {
                    endpoint = candidate;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_ne!(endpoint.address, original.address);
        assert_eq!(endpoint.service_id, original.service_id);
        let reconnected = owner.reconnect(&options).await.unwrap();
        assert_eq!(reconnected.endpoint().address, endpoint.address);
        let client = SharedClient::attach(endpoint, "relay-ui", None)
            .await
            .unwrap();
        assert_eq!(client.context().worker_id, owner.context().worker_id);
        let result = view(
            &client,
            Viewport {
                document: doc.document.clone(),
                revision: Revision::new(1 + cycle).unwrap(),
                width: 192,
                height: 192,
                camera: serde_json::to_value(cad_render::Camera::default()).unwrap(),
                epoch: 1,
                view_id: "relay-view".into(),
                preview: None,
                backend: "gpu".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result["metadata"]["backend"], "wgpu");
        assert_eq!(
            client.snapshot().await.unwrap()["documents"][0]["revision"],
            1 + cycle
        );
        let status = tokio::process::Command::new("kill")
            .arg("-TERM")
            .arg(relay.id().unwrap().to_string())
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert!(relay.wait().await.unwrap().success());
        let restored: ServiceEndpoint = serde_json::from_slice(
            &std::fs::read(options.runtime_dir.join("endpoint.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.address, original.address);
        if cycle == 0 {
            owner
                .execute(Command::Box {
                    document: doc.document.clone(),
                    revision: Revision::new(1).unwrap(),
                    size: [Finite::new(12.).unwrap(); 3],
                })
                .await
                .unwrap();
        }
    }
    let undo = owner
        .execute(Command::Undo {
            document: doc.document,
            revision: Revision::new(2).unwrap(),
        })
        .await
        .unwrap();
    assert!(undo.ok);
    assert_eq!(
        owner.snapshot().await.unwrap()["documents"][0]["data"]["volume"],
        1000.
    );
    owner.shutdown().await.unwrap();
}
