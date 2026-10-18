use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use cad_client::{ServiceOptions, SharedClient};
use cad_protocol::*;
use cad_web::gateway::{router, Gateway};
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;
const ORIGIN: &str = "https://cad.example";
const BASE: &str = "/apps/miocad/";
const CODE: &str = "111111111111111111111111111111111111111111111111";
async fn post(
    app: &Router,
    path: &str,
    body: Value,
    cookie: &str,
    origin: &str,
) -> (StatusCode, Value, String, Vec<u8>) {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("{BASE}api/{path}"))
        .header("content-type", "application/json")
        .header("X-CAD-Web", "1")
        .header("X-CAD-View", "0123456789abcdef0123456789abcdef");
    if !origin.is_empty() {
        request = request.header("Origin", origin);
    }
    if !cookie.is_empty() {
        request = request.header("Cookie", cookie);
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
        bytes,
    )
}
async fn rpc(app: &Router, context: &Context, id: u64, command: Command, cookie: &str) -> Value {
    let request = cad_protocol::Request::execute(context, RequestId::new(id).unwrap(), command)
        .value()
        .unwrap();
    let (status, reply, _, _) = post(app, "rpc", request, cookie, ORIGIN).await;
    assert_eq!(status, StatusCode::OK);
    reply
}
#[tokio::test]
async fn authorized_web_and_agent_share_transactions_with_scoped_files_and_reconciliation() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let options = ServiceOptions {
        runtime_dir: std::env::temp_dir().join(format!(
            "cad-web-test-{}",
            cad_client::shared::unique_id().unwrap()
        )),
        executable: root.join("target/debug/cad-service"),
        worker: std::env::var_os("CAD_SERVER")
            .map(PathBuf::from)
            .unwrap_or(root.join("build/cad-server")),
    };
    let mut peer = SharedClient::connect_or_start(&options, "web-test-agent")
        .await
        .unwrap();
    let app = router(
        Gateway::new(
            peer.endpoint().clone(),
            peer.context().session_id.as_str().into(),
            ORIGIN.into(),
            BASE.into(),
            CODE.into(),
        ),
        root.join("web"),
    );
    assert_eq!(
        post(&app, "access", json!({}), "", ORIGIN).await.1["requires_code"],
        true
    );
    assert_eq!(
        post(&app, "login", json!({"code":""}), "", ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post(&app, "snapshot", json!({}), "", ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    for origin in ["", "https://evil.example", "null"] {
        assert_eq!(
            post(&app, "login", json!({"code":CODE}), "", origin)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        post(&app, "login", json!({"code":"wrong"}), "", ORIGIN)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, _, cookie, _) = post(&app, "login", json!({"code":CODE}), "", ORIGIN).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.contains("HttpOnly; SameSite=Strict; Path=/apps/miocad/"));
    assert!(cookie.contains("Secure"));
    let cookie = cookie.split(';').next().unwrap();
    let (hello_status, hello, _, hello_bytes) =
        post(&app, "hello", json!({}), cookie, ORIGIN).await;
    assert_eq!(
        hello_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&hello_bytes)
    );
    let response =
        cad_protocol::Response::parse(hello.clone()).unwrap_or_else(|e| panic!("{e}: {hello}"));
    let context = response.context;
    assert_eq!(
        context.service_id.as_ref().unwrap().as_str(),
        peer.endpoint().service_id
    );
    assert_eq!(context.session_id, peer.context().session_id);
    assert_eq!(
        post(&app, "snapshot", json!({}), cookie, "https://evil.example")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let created = rpc(&app, &context, 1, Command::DocumentCreate, cookie).await;
    let document = DocumentId::new(created["result"]["document"].as_str().unwrap()).unwrap();
    let boxed = Command::Box {
        document: document.clone(),
        revision: Revision::new(0).unwrap(),
        size: [Finite::new(10.).unwrap(); 3],
    };
    let committed = rpc(&app, &context, 2, boxed.clone(), cookie).await;
    assert_eq!(committed["ok"], true);
    // A second page has a separate logical identity even with the same browser cookie.
    let other = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{BASE}api/hello"))
                .header("Origin", ORIGIN)
                .header("X-CAD-Web", "1")
                .header("X-CAD-View", "fedcba9876543210fedcba9876543210")
                .header("Cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::OK);
    let other: cad_protocol::Response =
        serde_json::from_slice(&to_bytes(other.into_body(), 100_000).await.unwrap()).unwrap();
    assert_ne!(other.context.client_id, context.client_id);
    assert_eq!(other.context.worker_id, context.worker_id);
    // Lost HTTP acknowledgement: status and exact replay identify one committed history step.
    let mut status_request =
        cad_protocol::Request::execute(&context, RequestId::new(3).unwrap(), Command::Capabilities);
    status_request.method = "request.status".into();
    status_request.command = None;
    status_request.target_request_id = Some(RequestId::new(2).unwrap());
    let status = post(&app, "rpc", status_request.value().unwrap(), cookie, ORIGIN)
        .await
        .1;
    assert_eq!(status["result"]["status"], "completed");
    let replay = rpc(&app, &context, 2, boxed, cookie).await;
    assert_eq!(replay["replayed"], true);
    let preview = rpc(
        &app,
        &context,
        4,
        Command::Preview {
            document: document.clone(),
            revision: Revision::new(1).unwrap(),
            operation: PreviewOperation::Box {
                size: [Finite::new(12.).unwrap(); 3],
            },
        },
        cookie,
    )
    .await;
    assert_eq!(preview["ok"], true);
    let state: DocumentResult<DocumentState> = peer
        .execute(Command::DocumentState {
            document: document.clone(),
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert_eq!(state.revision.get(), 1);
    assert_eq!(state.data.volume.unwrap().get(), 1000.);
    assert!(
        peer.execute(Command::Undo {
            document: document.clone(),
            revision: Revision::new(1).unwrap()
        })
        .await
        .unwrap()
        .ok
    );
    let stale = rpc(
        &app,
        &context,
        5,
        Command::Box {
            document: document.clone(),
            revision: Revision::new(1).unwrap(),
            size: [Finite::new(8.).unwrap(); 3],
        },
        cookie,
    )
    .await;
    assert_eq!(stale["error"]["kind"], "revision_conflict");
    let (_, snapshot, _, _) = post(&app, "snapshot", json!({}), cookie, ORIGIN).await;
    assert_eq!(snapshot["documents"][0]["revision"], 2);
    assert_eq!(
        post(&app, "events", json!({"after":0}), cookie, ORIGIN)
            .await
            .1["cursor"],
        3
    );
    let export = cad_protocol::Request::execute(
        &context,
        RequestId::new(6).unwrap(),
        Command::ExportNative {
            document: document.clone(),
            path: "/tmp/web-must-not-write.smb".into(),
        },
    )
    .value()
    .unwrap();
    assert_eq!(
        post(&app, "rpc", export, cookie, ORIGIN).await.0,
        StatusCode::FORBIDDEN
    );
    let mut cross =
        cad_protocol::Request::execute(&context, RequestId::new(6).unwrap(), Command::Capabilities);
    cross.session_id = Some(SessionId::new("another-session").unwrap());
    assert_eq!(
        post(&app, "rpc", cross.value().unwrap(), cookie, ORIGIN)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&app, "rpc", json!({"method":"shutdown"}), cookie, ORIGIN)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(
            &app,
            "download",
            json!({"document":document,"path":"/etc/passwd"}),
            cookie,
            ORIGIN
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        peer.execute(Command::Redo {
            document: document.clone(),
            revision: Revision::new(2).unwrap()
        })
        .await
        .unwrap()
        .ok
    );
    let (status, _, _, bytes) = post(
        &app,
        "download",
        json!({"document":document}),
        cookie,
        ORIGIN,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!bytes.is_empty());
    let raw = String::from_utf8_lossy(&serde_json::to_vec(&snapshot).unwrap()).to_string();
    assert!(!raw.contains(&peer.endpoint().token));
    assert!(!raw.contains(&peer.endpoint().address.to_string()));
    assert_eq!(
        post(&app, "logout", json!({}), cookie, ORIGIN).await.0,
        StatusCode::OK
    );
    assert_eq!(
        post(&app, "hello", json!({}), cookie, ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    let home = app
        .clone()
        .oneshot(Request::builder().uri(BASE).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(home.status(), StatusCode::OK);
    // Rate limiting includes failed unlocks and never bypasses the origin check.
    for _ in 0..9 {
        assert_eq!(
            post(&app, "login", json!({"code":"wrong"}), "", ORIGIN)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        post(&app, "login", json!({"code":CODE}), "", ORIGIN)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Static nested route uses the stable absolute base; secrets do not enter HTML.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/apps/miocad/workspace/doc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        to_bytes(response.into_body(), 100_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("<base href=\"/apps/miocad/\">"));
    assert!(!html.contains(CODE));
    // Credential-free mode is opt-in and still issues scoped, revocable browser grants.
    let trusted = router(
        Gateway::new(
            peer.endpoint().clone(),
            peer.context().session_id.as_str().into(),
            ORIGIN.into(),
            BASE.into(),
            CODE.into(),
        )
        .with_trusted_access(true),
        root.join("web"),
    );
    for origin in ["", "https://evil.example", "null"] {
        for path in ["access", "login"] {
            assert_eq!(
                post(&trusted, path, json!({"code":""}), "", origin).await.0,
                StatusCode::FORBIDDEN
            );
        }
    }
    assert_eq!(
        post(&trusted, "access", json!({}), "", ORIGIN).await.1["requires_code"],
        false
    );
    assert_eq!(
        post(&trusted, "snapshot", json!({}), "", ORIGIN).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, _, trusted_cookie, _) =
        post(&trusted, "login", json!({"code":""}), "", ORIGIN).await;
    assert_eq!(status, StatusCode::OK);
    assert!(trusted_cookie.contains("HttpOnly; SameSite=Strict; Path=/apps/miocad/"));
    assert!(trusted_cookie.contains("Secure"));
    let trusted_cookie = trusted_cookie.split(';').next().unwrap();
    let (_, trusted_hello, _, _) = post(&trusted, "hello", json!({}), trusted_cookie, ORIGIN).await;
    let trusted_context = cad_protocol::Response::parse(trusted_hello)
        .unwrap()
        .context;
    assert_eq!(trusted_context.session_id, context.session_id);
    assert_ne!(trusted_context.client_id, context.client_id);
    assert_eq!(
        post(&trusted, "snapshot", json!({}), trusted_cookie, ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        post(&trusted, "logout", json!({}), trusted_cookie, ORIGIN)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        post(&trusted, "hello", json!({}), trusted_cookie, ORIGIN)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    peer.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(options.runtime_dir);
}
