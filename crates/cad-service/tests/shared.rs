use cad_client::{protocol::*, ServiceOptions, SharedClient};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
fn options() -> ServiceOptions {
    let root = std::env::temp_dir().join(format!(
        "miocad-test-{}-{}",
        std::process::id(),
        cad_client::shared::unique_id().unwrap()
    ));
    ServiceOptions {
        runtime_dir: root,
        executable: PathBuf::from(env!("CARGO_BIN_EXE_cad-service")),
        worker: std::env::var_os("CAD_SERVER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/cad-server")
            }),
    }
}
#[tokio::test]
async fn shared_lifecycle_replay_events_and_isolation() {
    let options = options();
    // Concurrent discovery/launch must converge on exactly one ready instance.
    let (a, b) = tokio::join!(
        SharedClient::connect_or_start(&options, "a"),
        SharedClient::connect_or_start(&options, "b")
    );
    let mut a = a.unwrap();
    let mut b = b.unwrap();
    assert_eq!(a.endpoint().service_id, b.endpoint().service_id);
    let initial = b.snapshot().await.unwrap();
    assert_eq!(initial["documents"], json!([]));
    let request = a.prepare(Command::DocumentCreate).unwrap();
    let reply = a.send(&request).await.unwrap();
    let doc: CreatedDocument = reply.result_as().unwrap();
    let replay = a.send(&request).await.unwrap();
    assert!(replay.replayed);
    let updates = b.events(0).await.unwrap();
    assert_eq!(updates["events"].as_array().unwrap().len(), 1);
    let snap = b.snapshot().await.unwrap();
    assert_eq!(snap["documents"].as_array().unwrap().len(), 1);
    let size = [Finite::new(10.).unwrap(); 3];
    let command = Command::Box {
        document: doc.document.clone(),
        revision: Revision::new(0).unwrap(),
        size,
    };
    let (one, two) = tokio::join!(a.execute(command.clone()), b.execute(command));
    assert_ne!(one.unwrap().ok, two.unwrap().ok);
    let endpoint = a.endpoint().clone();
    let context = a.context().clone();
    drop(a); // No service/worker termination on client exit.
    let mut reconnected = SharedClient::attach(endpoint, "a", None).await.unwrap();
    assert_eq!(reconnected.context().worker_id, context.worker_id);
    assert!(reconnected.send(&request).await.unwrap().replayed);
    assert_eq!(b.snapshot().await.unwrap()["documents"][0]["revision"], 1);
    let isolated = b.create_session("batch-1").await.unwrap();
    let mut c = SharedClient::attach(b.endpoint().clone(), "isolated", Some(&isolated))
        .await
        .unwrap();
    assert_eq!(c.snapshot().await.unwrap()["documents"], json!([]));
    assert!(c.execute(Command::DocumentCreate).await.unwrap().ok);
    assert_eq!(
        b.snapshot().await.unwrap()["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(b.sessions().await.unwrap().as_array().unwrap().len(), 2);
    // Retention is bounded: reconnecting consumers must explicitly resync.
    for revision in 1..67 {
        assert!(
            b.execute(Command::Box {
                document: doc.document.clone(),
                revision: Revision::new(revision).unwrap(),
                size
            })
            .await
            .unwrap()
            .ok
        );
    }
    assert_eq!(b.events(0).await.unwrap()["resync_required"], true);
    assert!(b.snapshot().await.unwrap()["cursor"].as_u64().unwrap() > 64);
    let diagnostics = b.sessions().await.unwrap();
    let pid = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == b.context().session_id.as_str())
        .unwrap()["worker_pid"]
        .as_u64()
        .unwrap();
    std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let lost = b
        .execute(Command::DocumentState {
            document: doc.document,
        })
        .await
        .unwrap_err();
    assert!(lost.to_string().contains("worker_lost"));
    assert!(b
        .sessions()
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["session_id"] == b.context().session_id.as_str() && s["state"] == "lost"));
    // One worker failure does not destroy other sessions.
    assert_eq!(
        c.snapshot().await.unwrap()["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    b.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(SharedClient::attach(b.endpoint().clone(), "after", None)
        .await
        .is_err());
    // A stale endpoint plus unlocked lock file must recover with a new identity.
    std::fs::write(
        options.runtime_dir.join("endpoint.json"),
        serde_json::to_vec(b.endpoint()).unwrap(),
    )
    .unwrap();
    let fresh = SharedClient::connect_or_start(&options, "fresh")
        .await
        .unwrap();
    assert_ne!(fresh.endpoint().service_id, b.endpoint().service_id);
    let recovered = fresh.snapshot().await.unwrap();
    assert_eq!(recovered["documents"].as_array().unwrap().len(), 1);
    assert_eq!(recovered["documents"][0]["revision"], 67);
    assert_ne!(fresh.context().worker_id, b.context().worker_id);
    fresh.shutdown().await.unwrap();
}

#[tokio::test]
async fn lost_reply_can_be_reconciled_without_repeating_a_commit() {
    use cad_protocol::service::*;
    use tokio::{io::AsyncWriteExt, net::TcpStream};
    let options = options();
    let mut a = SharedClient::connect_or_start(&options, "disconnecting")
        .await
        .unwrap();
    let request = a.prepare(Command::DocumentCreate).unwrap();
    let endpoint = a.endpoint().clone();
    let envelope = ServiceEnvelope {
        protocol: "1.0".into(),
        service_id: endpoint.service_id.clone(),
        token: endpoint.token.clone(),
        request: ServiceRequest::Rpc {
            session: Some(a.context().session_id.as_str().into()),
            request: request.value().unwrap(),
        },
    };
    let mut socket = TcpStream::connect(endpoint.address).await.unwrap();
    cad_client::shared::write_frame(&mut socket, &serde_json::to_value(envelope).unwrap())
        .await
        .unwrap();
    socket.shutdown().await.unwrap();
    drop(socket);
    drop(a); // The reply is deliberately never consumed.
    let b = SharedClient::attach(endpoint.clone(), "observer", None)
        .await
        .unwrap();
    for _ in 0..100 {
        if b.snapshot().await.unwrap()["documents"]
            .as_array()
            .unwrap()
            .len()
            == 1
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        b.snapshot().await.unwrap()["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut recovered = SharedClient::attach(endpoint.clone(), "disconnecting", None)
        .await
        .unwrap();
    let status = recovered.status(request.request_id).await.unwrap();
    assert_eq!(status.result.as_ref().unwrap()["status"], "completed");
    assert!(recovered.send(&request).await.unwrap().replayed);
    assert_eq!(
        b.events(0).await.unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let isolated = b.create_session("retryable-session-key").await.unwrap();
    assert_eq!(
        b.create_session("retryable-session-key").await.unwrap(),
        isolated
    );
    assert!(SharedClient::control(
        &endpoint,
        ServiceRequest::Cancel {
            session: b.context().session_id.as_str().into(),
            client_id: "disconnecting".into(),
            request_id: request.request_id.get()
        }
    )
    .await
    .unwrap_err()
    .to_string()
    .contains("cancellation_unsupported"));
    let mut wrong = endpoint.clone();
    wrong.service_id = "different-instance".into();
    assert!(SharedClient::control(&wrong, ServiceRequest::Ping)
        .await
        .unwrap_err()
        .to_string()
        .contains("identity changed"));
    wrong = endpoint.clone();
    wrong.token = "invalid-token".into();
    assert!(SharedClient::control(&wrong, ServiceRequest::Ping)
        .await
        .unwrap_err()
        .to_string()
        .contains("unauthorized"));
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn daemon_crash_releases_lock_but_never_restores_old_identity() {
    use cad_protocol::service::*;
    let options = options();
    let mut a = SharedClient::connect_or_start(&options, "crash-test")
        .await
        .unwrap();
    let created: CreatedDocument = a
        .execute(Command::DocumentCreate)
        .await
        .unwrap()
        .result_as()
        .unwrap();
    let endpoint = a.endpoint().clone();
    let info = SharedClient::control(&endpoint, ServiceRequest::Ping)
        .await
        .unwrap();
    std::process::Command::new("kill")
        .args(["-KILL", &info["pid"].to_string()])
        .status()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let fresh = SharedClient::connect_or_start(&options, "new-daemon")
        .await
        .unwrap();
    assert_ne!(fresh.endpoint().service_id, endpoint.service_id);
    let recovered = fresh.snapshot().await.unwrap();
    assert_eq!(recovered["documents"].as_array().unwrap().len(), 1);
    assert_eq!(
        recovered["documents"][0]["document"],
        created.document.as_str()
    );
    assert!(a.execute(Command::DocumentCreate).await.is_err());
    fresh.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_client_future_requires_reattach_and_status() {
    use cad_protocol::service::*;
    use std::sync::Arc;
    use tokio::{io::BufReader, net::TcpListener, sync::Notify};
    let options = options();
    let real = SharedClient::connect_or_start(&options, "setup")
        .await
        .unwrap();
    let endpoint = real.endpoint().clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut proxy_endpoint = endpoint.clone();
    proxy_endpoint.address = listener.local_addr().unwrap();
    let committed = Arc::new(Notify::new());
    let signal = committed.clone();
    let proxy = tokio::spawn(async move {
        // Forward the handshake, then commit one request while withholding its response.
        for index in 0..2 {
            let (socket, _) = listener.accept().await.unwrap();
            let (read, mut write) = socket.into_split();
            let envelope: ServiceEnvelope = serde_json::from_value(
                cad_client::shared::read_frame(&mut BufReader::new(read))
                    .await
                    .unwrap(),
            )
            .unwrap();
            let result = SharedClient::control(&endpoint, envelope.request)
                .await
                .unwrap();
            if index == 1 {
                signal.notify_one();
                std::future::pending::<()>().await;
            }
            let reply = ServiceReply {
                protocol: "1.0".into(),
                service_id: endpoint.service_id.clone(),
                result: Some(result),
                error: None,
            };
            cad_client::shared::write_frame(&mut write, &serde_json::to_value(reply).unwrap())
                .await
                .unwrap();
        }
    });
    let mut a = SharedClient::attach(proxy_endpoint, "cancelled-client", None)
        .await
        .unwrap();
    let request = a.prepare(Command::DocumentCreate).unwrap();
    let mut pending = Box::pin(a.send(&request));
    tokio::select! {
        _=committed.notified()=>{},
        result=&mut pending=>panic!("response was supposed to be withheld: {result:?}"),
        _=tokio::time::sleep(Duration::from_secs(5))=>panic!("proxy never committed"),
    }
    drop(pending);
    assert!(matches!(
        a.prepare(Command::DocumentCreate),
        Err(cad_client::ClientError::ConnectionUnusable)
    ));
    proxy.abort();
    let mut recovered = SharedClient::attach(real.endpoint().clone(), "cancelled-client", None)
        .await
        .unwrap();
    assert_eq!(
        recovered
            .status(request.request_id)
            .await
            .unwrap()
            .result
            .unwrap()["status"],
        "completed"
    );
    assert!(recovered.send(&request).await.unwrap().replayed);
    assert_eq!(
        real.snapshot().await.unwrap()["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    real.shutdown().await.unwrap();
}
