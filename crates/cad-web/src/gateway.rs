//! Scoped browser gateway. The private service endpoint/token never leaves this process.
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cad_client::{shared::unique_id, SharedClient};
use cad_protocol::{
    service::{ServiceEndpoint, ServiceRequest},
    *,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tower_http::services::ServeDir;
type Error = (StatusCode, &'static str);
const UNAUTHORIZED: Error = (
    StatusCode::UNAUTHORIZED,
    "Browser authorization required; unlock this workspace again.",
);
const FORBIDDEN: Error = (
    StatusCode::FORBIDDEN,
    "Browser request outside authorized scope",
);
const UNAVAILABLE: Error = (
    StatusCode::SERVICE_UNAVAILABLE,
    "CAD service unavailable; reconnect and inspect state. No automatic replay.",
);
#[derive(Clone)]
struct Grant {
    context: Context,
    expires: Instant,
    views: HashSet<String>,
}
struct Access {
    grants: HashMap<String, Grant>,
    attempts: u32,
    window: Instant,
}
pub struct Gateway {
    endpoint: ServiceEndpoint,
    session: String,
    origin: String,
    base: String,
    code: String,
    secure: bool,
    stripped_prefix: bool,
    trusted_access: bool,
    access: Mutex<Access>,
    tasks: cad_agent::host::Tasks,
}
fn same_secret(a: &str, b: &str) -> bool {
    let mut difference = a.len() ^ b.len();
    for (x, y) in a.bytes().zip(b.bytes()) {
        difference |= (x ^ y) as usize;
    }
    difference == 0
}
impl Gateway {
    pub fn new(
        endpoint: ServiceEndpoint,
        session: String,
        origin: String,
        base: String,
        code: String,
    ) -> Self {
        Self {
            tasks: cad_agent::host::Tasks::new(
                std::env::temp_dir().join("miocad-agent-tasks"),
                endpoint.clone(),
                session.clone(),
            ),
            endpoint,
            session,
            secure: origin.starts_with("https://"),
            stripped_prefix: false,
            trusted_access: false,
            origin,
            base,
            code,

            access: Mutex::new(Access {
                grants: HashMap::new(),
                attempts: 0,
                window: Instant::now(),
            }),
        }
    }
    pub fn with_agent_tasks(mut self, root: PathBuf) -> Self {
        self.tasks = cad_agent::host::Tasks::new(root, self.endpoint.clone(), self.session.clone());
        self
    }
    pub fn with_stripped_prefix(mut self, stripped: bool) -> Self {
        self.stripped_prefix = stripped;
        self
    }
    /// Explicit deployment policy; never inferred from Host or forwarded headers.
    pub fn with_trusted_access(mut self, trusted: bool) -> Self {
        self.trusted_access = trusted;
        self
    }
    fn source(&self, headers: &HeaderMap) -> Result<(), Error> {
        if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(&self.origin)
            || headers.get("x-cad-web").and_then(|v| v.to_str().ok()) != Some("1")
        {
            return Err(FORBIDDEN);
        }
        Ok(())
    }
    async fn grant(&self, headers: &HeaderMap) -> Result<Grant, Error> {
        self.source(headers)?;
        let cookie = headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = cookie
            .split(';')
            .filter_map(|s| s.trim().split_once('='))
            .find(|(k, _)| *k == "miocad_web")
            .map(|(_, v)| v)
            .ok_or(UNAUTHORIZED)?;
        let mut access = self.access.lock().await;
        access.grants.retain(|_, g| g.expires > Instant::now());
        let view = headers
            .get("x-cad-view")
            .and_then(|v| v.to_str().ok())
            .ok_or(FORBIDDEN)?;
        if view.len() != 32 || !view.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(FORBIDDEN);
        }
        let grant = access.grants.get_mut(token).ok_or(UNAUTHORIZED)?;
        if !grant.views.contains(view) && grant.views.len() >= 8 {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "Page limit; lock and unlock this browser to start a fresh authorization",
            ));
        }
        grant.views.insert(view.into());
        let mut scoped = grant.clone();
        scoped.context.client_id =
            ClientId::new(format!("{}-{view}", grant.context.client_id.as_str()))
                .map_err(|_| FORBIDDEN)?;
        Ok(scoped)
    }
    async fn control(&self, grant: &Grant, request: ServiceRequest) -> Result<Value, Error> {
        // A grant is tied to the original service and worker, never a newly discovered daemon.
        if grant.context.service_id.as_ref().map(|s| s.as_str()) != Some(&self.endpoint.service_id)
        {
            return Err(UNAVAILABLE);
        }
        let rpc = matches!(&request, ServiceRequest::Rpc { .. });
        let mut result = SharedClient::control(&self.endpoint, request)
            .await
            .map_err(|_| UNAVAILABLE)?;
        if rpc {
            result["service_id"] = json!(self.endpoint.service_id);
        }
        Ok(result)
    }
    fn cookie(&self, token: &str, seconds: u64) -> String {
        format!(
            "miocad_web={token}; HttpOnly; SameSite=Strict; Path={}; Max-Age={seconds}{}",
            if self.stripped_prefix {
                "/"
            } else {
                &self.base
            },
            if self.secure { "; Secure" } else { "" }
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    code: String,
}
async fn access_mode(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    g.source(&headers)?;
    Ok(Json(json!({"requires_code": !g.trusted_access})))
}
async fn login(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(body): Json<Login>,
) -> Result<Response, Error> {
    g.source(&headers)?;
    let mut access = g.access.lock().await;
    if access.window.elapsed() > Duration::from_secs(60) {
        access.window = Instant::now();
        access.attempts = 0;
    }
    if access.attempts >= 12 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Unlock rate limit; wait one minute",
        ));
    }
    access.attempts += 1;
    if !g.trusted_access && !same_secret(&body.code, &g.code) {
        return Err(UNAUTHORIZED);
    }
    access
        .grants
        .retain(|_, grant| grant.expires > Instant::now());
    if access.grants.len() >= 16 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "At most 16 authorized browsers; lock an unused workspace or wait for expiry",
        ));
    }
    let token = unique_id().map_err(|_| UNAVAILABLE)?;
    let client = SharedClient::attach(
        g.endpoint.clone(),
        &format!("web-{}", unique_id().map_err(|_| UNAVAILABLE)?),
        Some(&g.session),
    )
    .await
    .map_err(|_| UNAVAILABLE)?;
    let mut context = client.context().clone();
    context.service_id =
        Some(ServiceId::new(g.endpoint.service_id.clone()).map_err(|_| UNAVAILABLE)?);
    access.grants.insert(
        token.clone(),
        Grant {
            context,
            expires: Instant::now() + Duration::from_secs(8 * 3600),
            views: HashSet::new(),
        },
    );
    let mut response = Json(json!({"ok":true})).into_response();
    response
        .headers_mut()
        .insert("set-cookie", g.cookie(&token, 8 * 3600).parse().unwrap());
    Ok(response)
}
async fn logout(State(g): State<Arc<Gateway>>, headers: HeaderMap) -> Result<Response, Error> {
    g.grant(&headers).await?;
    if let Some(cookie) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        if let Some((_, token)) = cookie
            .split(';')
            .filter_map(|s| s.trim().split_once('='))
            .find(|(k, _)| *k == "miocad_web")
        {
            g.access.lock().await.grants.remove(token);
        }
    }
    let mut response = Json(json!({"ok":true})).into_response();
    response
        .headers_mut()
        .insert("set-cookie", g.cookie("", 0).parse().unwrap());
    Ok(response)
}
async fn hello(State(g): State<Arc<Gateway>>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    let request = Request::hello(grant.context.client_id.clone())
        .map_err(|_| FORBIDDEN)?
        .value()
        .map_err(|_| FORBIDDEN)?;
    let reply = g
        .control(
            &grant,
            ServiceRequest::Rpc {
                session: Some(g.session.clone()),
                request,
            },
        )
        .await?;
    let parsed = cad_protocol::Response::parse(reply.clone()).map_err(|_| UNAVAILABLE)?;
    if parsed.context != grant.context {
        return Err(UNAVAILABLE);
    }
    Ok(Json(reply))
}
/// Explicit allow-list: no server file paths, service lifecycle, session switching or worker hello.
fn scoped_request(value: Value, grant: &Grant) -> Result<Value, Error> {
    validate_ref(&value, "Request").map_err(|_| FORBIDDEN)?;
    let request: Request = serde_json::from_value(value.clone()).map_err(|_| FORBIDDEN)?;
    request.value().map_err(|_| FORBIDDEN)?;
    if request.client_id != grant.context.client_id
        || request.worker_id.as_ref() != Some(&grant.context.worker_id)
        || request.session_id.as_ref() != Some(&grant.context.session_id)
    {
        return Err(FORBIDDEN);
    }
    match (&*request.method, &request.command) {
        (
            "execute",
            Some(
                Command::ExportNative { .. }
                | Command::DocumentSave { .. }
                | Command::DocumentOpen { .. },
            ),
        ) => Err(FORBIDDEN),
        ("execute", Some(_)) | ("request.status", None) => Ok(value),
        _ => Err(FORBIDDEN),
    }
}

fn task_owner(grant: &Grant) -> &str {
    grant
        .context
        .client_id
        .as_str()
        .rsplit_once('-')
        .map(|(root, _)| root)
        .unwrap_or(grant.context.client_id.as_str())
}
async fn agent_turn(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(request): Json<cad_agent::TaskRequest>,
) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    let result = g.tasks.start(task_owner(&grant), request).await;
    Ok(Json(match result {
        Ok(task) => json!({"ok":true,"task":task}),
        Err(error) => json!({"ok":false,"error":error}),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentTaskId {
    id: String,
}
async fn agent_status(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(body): Json<AgentTaskId>,
) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    let result = g.tasks.status(task_owner(&grant), &body.id).await;
    Ok(Json(match result {
        Ok(task) => json!({"ok":true,"task":task}),
        Err(error) => json!({"ok":false,"error":error}),
    }))
}
async fn agent_undo(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(body): Json<AgentTaskId>,
) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    let result = g.tasks.undo(task_owner(&grant), &body.id).await;
    Ok(Json(match result {
        Ok(task) => json!({"ok":true,"task":task}),
        Err(error) => json!({"ok":false,"error":error}),
    }))
}

async fn rpc(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    let request = scoped_request(body, &grant)?;
    Ok(Json(
        g.control(
            &grant,
            ServiceRequest::Rpc {
                session: Some(g.session.clone()),
                request,
            },
        )
        .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewBody {
    service: String,
    session: String,
    view: cad_protocol::service::Viewport,
}
async fn view(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(mut body): Json<ViewBody>,
) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    if body.service != g.endpoint.service_id || body.session != g.session {
        return Err(FORBIDDEN);
    }
    body.view.view_id = grant.context.client_id.as_str().into();
    Ok(Json(
        g.control(
            &grant,
            ServiceRequest::RenderView {
                session: g.session.clone(),
                view: body.view,
            },
        )
        .await?,
    ))
}
async fn snapshot(State(g): State<Arc<Gateway>>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let grant = g.grant(&headers).await?;
    Ok(Json(
        g.control(
            &grant,
            ServiceRequest::Snapshot {
                session: g.session.clone(),
            },
        )
        .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct After {
    after: u64,
}
async fn events(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(body): Json<After>,
) -> Result<Json<Value>, Error> {
    if body.after > 9_000_000_000_000_000 {
        return Err(FORBIDDEN);
    }
    let grant = g.grant(&headers).await?;
    Ok(Json(
        g.control(
            &grant,
            ServiceRequest::Events {
                session: g.session.clone(),
                after: body.after,
            },
        )
        .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Download {
    document: DocumentId,
}
async fn download(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(body): Json<Download>,
) -> Result<Response, Error> {
    let _grant = g.grant(&headers).await?;
    // A unique internal client avoids consuming/colliding with the browser request high-water.
    // Directory is private, generated here, and removed on every normal/error path.
    let mut client = SharedClient::attach(g.endpoint.clone(), "web-download", Some(&g.session))
        .await
        .map_err(|_| UNAVAILABLE)?;
    // Serialize downloads on the same logical client through a dedicated lock below.
    let _lock = DOWNLOAD_LOCK.lock().await;
    // Refresh high-water *after* acquiring the lock.
    client = SharedClient::attach(client.endpoint().clone(), "web-download", Some(&g.session))
        .await
        .map_err(|_| UNAVAILABLE)?;
    let directory = PrivateDirectory::new().map_err(|_| UNAVAILABLE)?;
    let path = directory.0.join("model.smb");
    let reply = client
        .execute(Command::ExportNative {
            document: body.document,
            path: path.to_str().ok_or(UNAVAILABLE)?.into(),
        })
        .await
        .map_err(|_| UNAVAILABLE)?;
    if !reply.ok {
        return Err((
            StatusCode::CONFLICT,
            "Native export failed; inspect the current model",
        ));
    }
    let metadata = std::fs::metadata(&path).map_err(|_| UNAVAILABLE)?;
    if metadata.len() > 32 * 1024 * 1024 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "Native download exceeds 32 MiB",
        ));
    }
    let bytes = std::fs::read(&path).map_err(|_| UNAVAILABLE)?;
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("content-disposition", "attachment; filename=\"model.smb\""),
        ],
        bytes,
    )
        .into_response())
}
static DOWNLOAD_LOCK: Mutex<()> = Mutex::const_new(());
struct PrivateDirectory(PathBuf);
impl PrivateDirectory {
    fn new() -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!("miocad-download-{}", unique_id()?));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn headers(request: axum::extract::Request, next: Next) -> Response {
    let mut reply = next.run(request).await;
    for (key,value) in [("cache-control","no-store"),("x-content-type-options","nosniff"),("referrer-policy","no-referrer"),("content-security-policy","default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; worker-src 'self'; connect-src 'self'; img-src 'self' blob: data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'")] { reply.headers_mut().insert(key,HeaderValue::from_static(value)); }
    reply
}
pub fn router(gateway: Gateway, assets: PathBuf) -> Router {
    let base = gateway.base.clone();
    let stripped_prefix = gateway.stripped_prefix;
    let html = include_str!("../../../web/index.html").replace("__BASE__", &base);
    let root_html = html.clone();
    let app = Router::new()
        .route(
            "/",
            get(move || {
                let html = root_html.clone();
                async move { axum::response::Html(html) }
            }),
        )
        .route("/health", get(|| async { "ok" }))
        .route("/api/access", post(access_mode))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/hello", post(hello))
        .route("/api/rpc", post(rpc))
        .route(
            "/api/agent",
            post(agent_turn).layer(DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route("/api/agent/status", post(agent_status))
        .route("/api/agent/undo", post(agent_undo))
        .route("/api/snapshot", post(snapshot))
        .route("/api/view", post(view))
        .route("/api/events", post(events))
        .route("/api/download", post(download))
        .nest_service("/pkg", ServeDir::new(assets.join("pkg")))
        .route_service(
            "/boot.js",
            tower_http::services::ServeFile::new(assets.join("boot.js")),
        )
        .route_service(
            "/render-worker.js",
            tower_http::services::ServeFile::new(assets.join("render-worker.js")),
        )
        .fallback(get(move || {
            let html = html.clone();
            async move { axum::response::Html(html) }
        }))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(Arc::new(gateway));
    // The portal uses strip-prefix=false; all routes/cookies/resources share this base.
    let app = if stripped_prefix || base == "/" {
        app
    } else {
        {
            let destination = base.clone();
            Router::new()
                .route(
                    base.trim_end_matches('/'),
                    get(move || {
                        let destination = destination.clone();
                        async move { axum::response::Redirect::temporary(&destination) }
                    }),
                )
                .nest(&base, app)
        }
    };
    app.layer(middleware::from_fn(headers))
}
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = match cad_access::Config::parse()? {
        cad_access::Action::Run(c) => c,
        cad_access::Action::Help => {
            println!("cad-web uses cad-cli connection flags. Environment: HOST(loopback), PORT, BASE_PATH, CAD_WEB_ORIGIN, CAD_WEB_ASSETS, CAD_WEB_TRUSTED_ACCESS=1 (explicit credential-free deployment). Access code: <runtime-dir>/web-access-code (0600). Browser grants expire after 8h; restart revokes grants. No Python runtime.");
            return Ok(());
        }
        cad_access::Action::Version => {
            println!("cad-web {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    };
    if config.command.is_some() || config.isolated {
        return Err("Use --session for an authorized existing session; --command/--isolated are unsupported".into());
    }
    let host = std::env::var("HOST")
        .unwrap_or("127.0.0.1".into())
        .parse::<std::net::IpAddr>()?;
    if !host.is_loopback() {
        return Err("gateway must listen on loopback".into());
    }
    let port = std::env::var("PORT")
        .unwrap_or("8787".into())
        .parse::<u16>()?;
    let base = std::env::var("BASE_PATH").unwrap_or("/apps/miocad/".into());
    if !base.starts_with('/')
        || !base.ends_with('/')
        || base.contains("..")
        || base
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '/' && c != '-' && c != '_')
    {
        return Err("invalid BASE_PATH".into());
    }
    let origin =
        std::env::var("CAD_WEB_ORIGIN").unwrap_or("https://example.ts.net".into());
    if !(origin.starts_with("https://")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://localhost:"))
        || origin.ends_with('/')
        || origin.contains(['\r', '\n', '\"', '<', '>'])
    {
        return Err("invalid CAD_WEB_ORIGIN".into());
    }
    let client = if std::env::var("CAD_WEB_ATTACH_ONLY").as_deref() == Ok("1") {
        let endpoint: ServiceEndpoint = serde_json::from_slice(&std::fs::read(
            config.options.runtime_dir.join("endpoint.json"),
        )?)?;
        if std::env::var("CAD_WEB_REQUIRE_RELAY").as_deref() == Ok("1") {
            let upstream: ServiceEndpoint = serde_json::from_slice(&std::fs::read(
                config.options.runtime_dir.join("render-upstream.json"),
            )?)?;
            if endpoint.address == upstream.address || endpoint.service_id != upstream.service_id {
                return Err("render relay is not published yet".into());
            }
        }
        SharedClient::attach(endpoint, "web-gateway", config.session.as_deref()).await?
    } else {
        SharedClient::connect_or_start(&config.options, "web-gateway").await?
    };
    let session = config
        .session
        .unwrap_or(client.context().session_id.as_str().into());
    // Fail startup when the explicitly authorized session does not exist.
    SharedClient::attach(client.endpoint().clone(), "web-gateway", Some(&session)).await?;
    let code_path = config.options.runtime_dir.join("web-access-code");
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let code = match std::fs::read_to_string(&code_path) {
        Ok(code) => code.trim().to_string(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let code = unique_id()?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&code_path)?;
            file.write_all(code.as_bytes())?;
            code
        }
        Err(error) => return Err(error.into()),
    };
    if code.len() != 48 || !code.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid private web-access-code file".into());
    }
    let assets = std::env::var_os("CAD_WEB_ASSETS")
        .map(PathBuf::from)
        .unwrap_or(
            std::env::current_exe()?
                .parent()
                .ok_or("no executable parent")?
                .join("../web"),
        );
    if !assets.join("pkg/cad_web_bg.wasm").is_file() {
        return Err("Web assets not built; run scripts/build-web.sh".into());
    }
    let app = router(
        Gateway::new(client.endpoint().clone(), session, origin, base, code)
            .with_agent_tasks(config.options.runtime_dir.join("agent-tasks"))
            .with_stripped_prefix(std::env::var("CAD_WEB_STRIP_PREFIX").as_deref() == Ok("1"))
            .with_trusted_access(std::env::var("CAD_WEB_TRUSTED_ACCESS").as_deref() == Ok("1")),
        assets,
    );
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!(
        "CAD WebUI listening on {}. Credential-free access: {}. Optional unlock code file: {}.",
        listener.local_addr()?,
        std::env::var("CAD_WEB_TRUSTED_ACCESS").as_deref() == Ok("1"),
        code_path.display()
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn expired_grants_and_missing_page_identity_are_rejected_before_service_access() {
        let context = Context {
            protocol: "1.0".into(),
            client_id: ClientId::new("web-root").unwrap(),
            worker_id: WorkerId::new("worker").unwrap(),
            session_id: SessionId::new("session").unwrap(),
            service_id: Some(ServiceId::new("service").unwrap()),
        };
        let gateway = Gateway::new(
            ServiceEndpoint {
                protocol: "1.0".into(),
                address: "127.0.0.1:1".parse().unwrap(),
                service_id: "service".into(),
                token: "never-exposed".into(),
            },
            "session".into(),
            "https://cad.example".into(),
            "/apps/miocad/".into(),
            "test-code".into(),
        );
        gateway.access.lock().await.grants.insert(
            "cookie".into(),
            Grant {
                context: context.clone(),
                expires: Instant::now() - Duration::from_secs(1),
                views: HashSet::new(),
            },
        );
        let mut headers = HeaderMap::new();
        for (key, value) in [
            ("origin", "https://cad.example"),
            ("x-cad-web", "1"),
            ("cookie", "miocad_web=cookie"),
            ("x-cad-view", "0123456789abcdef0123456789abcdef"),
        ] {
            headers.insert(key, value.parse().unwrap());
        }
        assert!(matches!(
            gateway.grant(&headers).await,
            Err((StatusCode::UNAUTHORIZED, _))
        ));
        gateway.access.lock().await.grants.insert(
            "cookie".into(),
            Grant {
                context,
                expires: Instant::now() + Duration::from_secs(60),
                views: HashSet::new(),
            },
        );
        headers.remove("x-cad-view");
        assert!(matches!(
            gateway.grant(&headers).await,
            Err((StatusCode::FORBIDDEN, _))
        ));
    }
}
