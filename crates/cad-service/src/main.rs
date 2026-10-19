#![forbid(unsafe_code)]
use cad_client::shared::{read_frame, unique_id, write_frame};
use cad_protocol::{service::*, ClientId, Request, Response};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::OpenOptions,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::BufReader,
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, ChildStdout},
    sync::{watch, Mutex},
    time::timeout,
};
mod client_ledger;
const EVENT_LIMIT: usize = 64;
struct Worker {
    clients: Option<client_ledger::Clients>,
    child: Option<Child>,
    reader: Option<BufReader<ChildStdout>>,
    writer: Option<ChildStdin>,
    upstream: Option<ServiceEndpoint>,
    observer: String,
    context: Value,
    docs: BTreeSet<String>,
    events: VecDeque<Value>,
    cursor: u64,
    lost: Option<String>,
    internal_id: u64,
    frames: VecDeque<(String, Arc<cad_render::Frame>)>,
    renderer: Arc<std::sync::Mutex<Option<cad_render::Renderer>>>,
    scene_cache: Option<(String, Value)>,
    views: BTreeMap<String, u64>,
}
impl Worker {
    async fn start(path: &PathBuf, client_root: PathBuf) -> Result<Self, String> {
        let mut child = tokio::process::Command::new(path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(child.stdout.take().ok_or("worker stdout missing")?);
        let writer = child.stdin.take().ok_or("worker stdin missing")?;
        let mut w = Self {
            clients: None,
            child: Some(child),
            reader: Some(reader),
            writer: Some(writer),
            upstream: None,
            observer: "service-observer".into(),
            context: Value::Null,
            docs: BTreeSet::new(),
            events: VecDeque::new(),
            cursor: 0,
            lost: None,
            internal_id: 0,
            frames: VecDeque::new(),
            renderer: Arc::new(std::sync::Mutex::new(None)),
            scene_cache: None,
            views: BTreeMap::new(),
        };
        let hello = Request::hello(ClientId::new("service-observer").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            .value()
            .map_err(|e| e.to_string())?;
        let reply = w.exchange(&hello).await?;
        if reply["ok"] != true {
            return Err("worker handshake failed".into());
        }
        cad_protocol::validate_ref(&reply["result"], "HelloResult").map_err(|e| e.to_string())?;
        w.context = reply;
        w.clients = Some(client_ledger::Clients::new(client_root, &w.context));
        Ok(w)
    }
    async fn remote(
        endpoint: ServiceEndpoint,
        session: &str,
        client_root: PathBuf,
    ) -> Result<Self, String> {
        let observer = format!(
            "render-observer-{}",
            &endpoint.service_id[..16.min(endpoint.service_id.len())]
        );
        let request = Request::hello(ClientId::new(observer.clone()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            .value()
            .map_err(|e| e.to_string())?;
        let context = cad_client::SharedClient::control(
            &endpoint,
            ServiceRequest::Rpc {
                session: Some(session.into()),
                request,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        if context["ok"] != true {
            return Err("upstream worker hello rejected".into());
        }
        let internal_id = context["result"]["last_request_id"]
            .as_u64()
            .ok_or("upstream high-water missing")?;
        Ok(Self {
            clients: Some(client_ledger::Clients::new(client_root, &context)),
            child: None,
            reader: None,
            writer: None,
            upstream: Some(endpoint),
            observer,
            context,
            internal_id,
            docs: BTreeSet::new(),
            events: VecDeque::new(),
            cursor: 0,
            lost: None,
            frames: VecDeque::new(),
            renderer: Arc::new(std::sync::Mutex::new(None)),
            scene_cache: None,
            views: BTreeMap::new(),
        })
    }
    async fn client_exchange(&mut self, req: &Value) -> Result<Value, String> {
        self.diagnostics();
        if let Some(reason) = &self.lost {
            return Err(format!("worker_lost: {reason}; no replay"));
        }
        if let Some(clients) = self.clients.take() {
            let result = clients.exchange(self, req).await;
            self.clients = Some(clients);
            result
        } else {
            self.exchange(req).await
        }
    }
    async fn exchange(&mut self, req: &Value) -> Result<Value, String> {
        if let Some(reason) = &self.lost {
            return Err(format!(
                "worker_lost: {reason}; documents unavailable; no automatic restart/retry"
            ));
        }
        let result = timeout(Duration::from_secs(60), async {
            let value = if let Some(endpoint) = &self.upstream {
                cad_client::SharedClient::control(
                    endpoint,
                    ServiceRequest::Rpc {
                        session: self.context["session_id"].as_str().map(str::to_string),
                        request: req.clone(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?
            } else {
                write_frame(self.writer.as_mut().ok_or("worker stdin missing")?, req).await?;
                read_frame(self.reader.as_mut().ok_or("worker stdout missing")?).await?
            };
            let reply = Response::parse(value.clone()).map_err(|e| e.to_string())?;
            if value["client_id"] != req["client_id"] || value["request_id"] != req["request_id"] {
                return Err("worker response correlation mismatch".into());
            }
            if !self.context.is_null()
                && (value["worker_id"] != self.context["worker_id"]
                    || value["session_id"] != self.context["session_id"])
            {
                return Err("worker identity changed".into());
            }
            if reply.ok && !reply.replayed {
                for event in reply.events {
                    self.cursor += 1;
                    self.docs.insert(event.document.as_str().into());
                    let mut event = serde_json::to_value(event).map_err(|e| e.to_string())?;
                    event["event_id"] = json!(self.cursor);
                    self.events.push_back(event);
                    while self.events.len() > EVENT_LIMIT {
                        self.events.pop_front();
                    }
                }
            }
            Ok::<_, String>(value)
        })
        .await
        .unwrap_or_else(|_| {
            Err("worker response exceeded 60s; worker terminated; in-flight outcome unknown".into())
        });
        if let Err(e) = &result {
            self.lost = Some(e.clone());
            if let Some(child) = &mut self.child {
                let _ = child.kill().await;
            }
        }
        result.map_err(|e| format!("worker_lost: {e}; in-flight outcome unknown; no replay"))
    }
    async fn snapshot(&mut self) -> Result<Value, String> {
        self.internal_id += 1;
        let request = json!({
            "protocol": self.context["protocol"], "method": "execute",
            "client_id": self.observer, "request_id": self.internal_id,
            "worker_id": self.context["worker_id"], "session_id": self.context["session_id"],
            "command": {"op": "document.list"}
        });
        let reply = self.exchange(&request).await?;
        if reply["ok"] != true {
            return Err(format!("snapshot failed: {}", reply["error"]));
        }
        let docs = reply["result"].clone();

        Ok(
            json!({"worker_id":self.context["worker_id"],"session_id":self.context["session_id"],"cursor":self.cursor,"documents":docs}),
        )
    }
    async fn query(&mut self, op: &str, document: &str) -> Result<Value, String> {
        self.internal_id += 1;
        let request = json!({
            "protocol": self.context["protocol"], "method": "execute",
            "client_id": self.observer, "request_id": self.internal_id,
            "worker_id": self.context["worker_id"], "session_id": self.context["session_id"],
            "command": {"op": op, "document": document}
        });
        let reply = self.exchange(&request).await?;
        if reply["ok"] != true {
            return Err(format!(
                "{}: {}",
                reply["error"]["kind"].as_str().unwrap_or("kernel_error"),
                reply["error"]["message"]
            ));
        }
        Ok(reply["result"].clone())
    }
    #[allow(clippy::too_many_arguments)]
    async fn render_snapshot(
        &mut self,
        document: &str,
        width: u32,
        height: u32,
        directory: Option<String>,
        camera: Option<Value>,
        backend: String,
        revision: Option<u64>,
        view: Option<(String, u64)>,
        preview: Option<cad_protocol::PreviewOperation>,
    ) -> Result<Value, String> {
        if let Some((view_id, epoch)) = &view {
            if let Some(current) = self.views.get(view_id) {
                if epoch < current {
                    return Err("stale_view: a newer viewport was published".into());
                }
                if epoch == current {
                    let frame = self
                        .frames
                        .iter()
                        .find(|(_, f)| {
                            f.metadata["view_id"] == *view_id && f.metadata["epoch"] == *epoch
                        })
                        .ok_or("render_not_found: equal epoch frame evicted; advance epoch")?;
                    let m = &frame.1.metadata;
                    let expected_camera = camera.as_ref().ok_or("missing viewport camera")?;
                    let same_preview = m.get("preview_operation") == Some(&json!(preview));
                    let same_camera = ["yaw", "pitch", "zoom", "pan"]
                        .iter()
                        .all(|field| m["camera"][field] == expected_camera[field]);
                    if m["document"] == document
                        && m["revision"].as_u64() == revision
                        && m["width"] == width
                        && m["height"] == height
                        && m["preview"] == preview.is_some()
                        && same_camera
                        && same_preview
                        && m["backend_request"] == backend
                    {
                        let cached = m.clone();
                        let state = self.query("document.state", document).await?;
                        if state["revision"].as_u64() != revision {
                            return Err("stale_render: document changed".into());
                        }
                        return Ok(cached);
                    }
                    return Err(
                        "view_conflict: epoch reused with different viewport; advance epoch".into(),
                    );
                }
            } else if self.views.len() >= 128 {
                return Err("view_limit: 128 view identities per session".into());
            }
        }
        let camera = camera
            .map(serde_json::from_value::<cad_render::Camera>)
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut scene_hit = false;
        let scene = if let Some(operation) = preview.clone() {
            self.internal_id += 1;
            let req = json!({"protocol":self.context["protocol"],"method":"execute","client_id":self.observer,"request_id":self.internal_id,"worker_id":self.context["worker_id"],"session_id":self.context["session_id"],"command":{"op":"model.preview","document":document,"revision":revision,"operation":operation}});
            let result = self.exchange(&req).await?;
            if result["ok"] != true {
                return Err(format!("preview_failed: {}", result["error"]));
            }
            result["result"].clone()
        } else {
            let state = self.query("document.state", document).await?;
            if let Some((cached_id, cached)) = &self.scene_cache {
                if cached_id == document && cached["revision"] == state["revision"] {
                    scene_hit = true;
                    cached.clone()
                } else {
                    let result = self.query("render.scene", document).await?;
                    self.scene_cache = Some((document.into(), result.clone()));
                    result
                }
            } else {
                let result = self.query("render.scene", document).await?;
                self.scene_cache = Some((document.into(), result.clone()));
                result
            }
        };
        if revision.is_some_and(|revision| scene["revision"].as_u64() != Some(revision)) {
            return Err("stale_render: requested document revision changed".into());
        }
        if let Some((view_id, epoch)) = &view {
            if self
                .views
                .get(view_id)
                .is_some_and(|current| epoch < current)
            {
                return Err("stale_view: a newer viewport request was published".into());
            }
        }
        let mesh_data: cad_protocol::Mesh = serde_json::from_value(scene["data"]["mesh"].clone())
            .map_err(|_| "empty_document: no mesh".to_string())?;
        let edge_data: Vec<cad_protocol::Edge> =
            serde_json::from_value(scene["data"]["edges"].clone()).map_err(|e| e.to_string())?;
        let requested_backend = backend.clone();
        let renderer = self.renderer.clone();
        let fit = camera.is_none();
        let configured = std::env::var("CAD_RENDER_BACKEND").ok();
        let forced_cpu = configured.as_deref() == Some("cpu");
        let backend = if backend == "auto" {
            configured.unwrap_or(backend)
        } else {
            backend
        };
        let mut frame = tokio::task::spawn_blocking(move || {
            let mut guard = renderer.lock().map_err(|e| e.to_string())?;
            let renderer = guard.get_or_insert_with(|| {
                if forced_cpu {
                    cad_render::Renderer::cpu("CAD_RENDER_BACKEND=cpu")
                } else {
                    cad_render::Renderer::new()
                }
            });
            let frame = renderer.render(
                &mesh_data,
                &edge_data,
                width,
                height,
                camera.unwrap_or_default(),
                fit,
                &backend,
            )?;
            Ok::<_, String>(frame)
        })
        .await
        .map_err(|e| e.to_string())??;
        frame.metadata["backend_request"] = json!(requested_backend);
        frame.metadata["scene_cache_hit"] = json!(scene_hit);
        frame.metadata["preview"] = json!(preview.is_some());
        frame.metadata["preview_operation"] = json!(preview);
        if let Some((view_id, epoch)) = view {
            frame.metadata["view_id"] = json!(view_id);
            frame.metadata["epoch"] = json!(epoch);
            self.views.insert(view_id, epoch);
        }
        let id = unique_id().map_err(|e| e.to_string())?;
        frame.metadata["render_id"] = json!(id);
        frame.metadata["document"] = json!(document);
        frame.metadata["revision"] = scene["revision"].clone();
        frame.metadata["session_id"] = self.context["session_id"].clone();
        frame.metadata["worker_id"] = self.context["worker_id"].clone();
        frame.metadata["files"] = match directory {
            Some(directory) => frame.save(&directory)?,
            None => json!({}),
        };
        let metadata = frame.metadata.clone();
        self.frames.push_back((id, Arc::new(frame)));
        while self.frames.len() > 4 {
            self.frames.pop_front();
        }
        Ok(metadata)
    }
    async fn capture(&mut self, render_id: &str) -> Result<Value, String> {
        let frame = self
            .frames
            .iter()
            .find(|(id, _)| id == render_id)
            .ok_or("render_not_found: unknown or evicted frame")?
            .1
            .clone();
        let m = &frame.metadata;
        if m["preview"] == true {
            return Err(
                "preview_frame: candidate cannot be picked or captured as committed geometry"
                    .into(),
            );
        }
        if let (Some(view), Some(epoch)) = (m["view_id"].as_str(), m["epoch"].as_u64()) {
            if self.views.get(view) != Some(&epoch) {
                return Err("stale_view: viewport has advanced".into());
            }
        }
        let state = self
            .query(
                "document.state",
                m["document"].as_str().ok_or("invalid frame")?,
            )
            .await?;
        if state["revision"] != m["revision"] {
            return Err("stale_render: document changed".into());
        }
        Ok(m.clone())
    }
    async fn render_pick(
        &mut self,
        render_id: &str,
        x: u32,
        y: u32,
        target: &str,
    ) -> Result<Value, String> {
        self.capture(render_id).await?;
        let metadata = {
            let frame = &self.frames.iter().find(|(id, _)| id == render_id)
                .ok_or("render_not_found: unknown, evicted or different-session frame; request a new snapshot")?.1;
            let mut metadata = serde_json::Map::new();
            for name in [
                "render_id",
                "document",
                "revision",
                "session_id",
                "worker_id",
            ] {
                metadata.insert(name.into(), frame.metadata[name].clone());
            }
            Value::Object(metadata)
        };
        let document = metadata["document"]
            .as_str()
            .ok_or("invalid frame document")?;
        let state = self.query("document.state", document).await?;
        if state["revision"] != metadata["revision"] {
            return Err("stale_render: document changed; request a new snapshot".into());
        }
        let frame = &self
            .frames
            .iter()
            .find(|(id, _)| id == render_id)
            .ok_or("render_not_found")?
            .1;
        let mut result = frame.pick(x, y, target)?;
        for name in [
            "render_id",
            "document",
            "revision",
            "session_id",
            "worker_id",
        ] {
            result[name] = metadata[name].clone();
        }
        for name in ["camera", "width", "height", "view_id", "epoch"] {
            if let Some(value) = frame.metadata.get(name) {
                result[name] = value.clone();
            }
        }
        Ok(result)
    }
    fn diagnostics(&mut self) -> Value {
        if self.lost.is_none() {
            if let Some(child) = &mut self.child {
                match child.try_wait() {
                    Ok(Some(status)) => self.lost = Some(format!("worker exited {status}")),
                    Err(e) => self.lost = Some(e.to_string()),
                    _ => {}
                }
            }
        }
        json!({"session_id":self.context["session_id"],"worker_id":self.context["worker_id"],"worker_pid":self.child.as_ref().and_then(|child|child.id()),"state":if self.lost.is_some(){"lost"}else{"ready"},"reason":self.lost,"cursor":self.cursor,"documents":self.docs.len()})
    }
}
type Session = Arc<Mutex<Worker>>;
struct Service {
    client_root: PathBuf,
    endpoint: ServiceEndpoint,
    upstream: Option<ServiceEndpoint>,
    worker: PathBuf,
    default: String,
    sessions: Mutex<BTreeMap<String, Session>>,
    keys: Mutex<BTreeMap<String, String>>,
    stop: watch::Sender<bool>,
}
impl Service {
    async fn handle(&self, envelope: ServiceEnvelope) -> Result<Value, String> {
        if envelope.protocol != self.endpoint.protocol {
            return Err("version_mismatch".into());
        }
        if envelope.service_id != self.endpoint.service_id {
            return Err("wrong_instance: service changed; never replay".into());
        }
        if envelope.token != self.endpoint.token {
            return Err("unauthorized".into());
        }
        if *self.stop.borrow() {
            return Err("service_stopping: no new work accepted".into());
        }
        if let Some(upstream) = &self.upstream {
            if !matches!(
                &envelope.request,
                ServiceRequest::Rpc { .. }
                    | ServiceRequest::RenderSnapshot { .. }
                    | ServiceRequest::RenderView { .. }
                    | ServiceRequest::RenderCapture { .. }
                    | ServiceRequest::RenderFrames { .. }
                    | ServiceRequest::RenderPick { .. }
            ) {
                let stopping = matches!(&envelope.request, ServiceRequest::Shutdown);
                let result = cad_client::SharedClient::control(upstream, envelope.request)
                    .await
                    .map_err(|e| e.to_string());
                if stopping {
                    let _ = self.stop.send(true);
                }
                return result;
            }
        }
        match envelope.request {
            ServiceRequest::Ping => Ok(json!({
                "ready": true, "service_id": self.endpoint.service_id,
                "default_session": self.default, "pid": std::process::id()
            })),
            ServiceRequest::Sessions => {
                let sessions: Vec<_> = self.sessions.lock().await.values().cloned().collect();
                let mut list = Vec::new();
                for session in sessions {
                    list.push(session.lock().await.diagnostics());
                }
                Ok(json!(list))
            }
            ServiceRequest::CreateSession { key } => {
                cad_protocol::validate_ref(&json!(key), "Identifier").map_err(|e| e.to_string())?;
                // The idempotency key lock covers spawn, so retries converge.
                let mut keys = self.keys.lock().await;
                if let Some(id) = keys.get(&key) {
                    return Ok(json!({"session_id": id}));
                }
                if keys.len() >= 15 {
                    return Err("session_limit: 16 workers per service".into());
                }
                let worker = Worker::start(&self.worker,self.client_root.clone()).await?;
                let id = worker.context["session_id"].as_str().ok_or("invalid session")?.to_string();
                self.sessions.lock().await.insert(id.clone(), Arc::new(Mutex::new(worker)));
                keys.insert(key, id.clone());
                Ok(json!({"session_id": id}))
            }
            ServiceRequest::Rpc { session, request } => {
                cad_protocol::validate_ref(&request, "Request").map_err(|e| e.to_string())?;
                if request["client_id"] == "service-observer" {
                    return Err("reserved_client_id".into());
                }
                let session = self.session(session.as_deref().unwrap_or(&self.default)).await?;
                let result = session.lock().await.client_exchange(&request).await;
                result
            }
            ServiceRequest::Snapshot { session } => {
                let session = self.session(&session).await?;
                let mut worker = session.lock().await;
                if worker.diagnostics()["state"] == "lost" {
                    return Err("worker_lost: snapshot unavailable".into());
                }
                worker.snapshot().await
            }
            ServiceRequest::Events { session, after } => {
                let session = self.session(&session).await?;
                let mut worker = session.lock().await;
                if worker.diagnostics()["state"] == "lost" {
                    return Err("worker_lost: event stream ended".into());
                }
                let oldest = worker.events.front().and_then(|v| v["event_id"].as_u64()).unwrap_or(worker.cursor + 1);
                let missed = after > worker.cursor || after < oldest - 1;
                let events: Vec<_> = if missed {
                    Vec::new()
                } else {
                    worker.events.iter().filter(|v| v["event_id"].as_u64().unwrap() > after).cloned().collect()
                };
                Ok(json!({"cursor": worker.cursor, "resync_required": missed, "events": events}))
            }
            ServiceRequest::RenderSnapshot { session, document, width, height, directory, camera, backend, revision } => {
                let session = self.session(&session).await?;
                let result = session.lock().await.render_snapshot(&document, width, height, directory,camera,backend,revision,None,None).await;
                result
            }
            ServiceRequest::RenderView {session,view} => {
                let session=self.session(&session).await?;let mut worker=session.lock().await;
                let metadata=worker.render_snapshot(view.document.as_str(),view.width,view.height,None,Some(view.camera),view.backend,Some(view.revision.get()),Some((view.view_id,view.epoch)),view.preview).await?;
                let frame=&worker.frames.iter().find(|(id,_)|Some(id.as_str())==metadata["render_id"].as_str()).ok_or("render_not_found")?.1;
                serde_json::to_value(frame).map_err(|e|e.to_string())
            }
            ServiceRequest::RenderCapture {session,render_id} => {let session=self.session(&session).await?;let result=session.lock().await.capture(&render_id).await;result}
            ServiceRequest::RenderFrames {session} => {let session=self.session(&session).await?;let worker=session.lock().await;Ok(json!(worker.frames.iter().map(|(_,frame)|{let mut info=frame.metadata.clone();let object=info.as_object_mut().unwrap();object.remove("images");object.remove("files");info}).collect::<Vec<_>>()))}
            ServiceRequest::RenderPick { session, render_id, x, y, target } => {
                let session = self.session(&session).await?;
                let result = session.lock().await.render_pick(&render_id,x,y,&target).await;
                result
            }
            ServiceRequest::Cancel { .. } => Err("cancellation_unsupported: accepted kernel calls continue; disconnect is not cancellation; inspect request.status".into()),
            ServiceRequest::Shutdown => {
                let _ = self.stop.send(true);
                Ok(json!({"state": "stopping", "durable": false}))
            }
        }
    }

    async fn dispatch(&self, value: Value) -> Result<Value, String> {
        validate_envelope(&value).map_err(|e| format!("invalid_request: {e}"))?;
        let envelope: ServiceEnvelope = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let request_value = serde_json::to_value(&envelope.request).map_err(|e| e.to_string())?;
        let method = request_value["method"].as_str().ok_or("missing method")?;
        let result = self.handle(envelope).await?;
        validate_result(method, &result)
            .map_err(|e| format!("outcome_unknown: invalid service result: {e}"))?;
        Ok(result)
    }
    async fn session(&self, id: &str) -> Result<Session, String> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(id) {
            return Ok(session.clone());
        }
        if let Some(upstream) = &self.upstream {
            if sessions.len() >= 16 {
                return Err("session_limit".into());
            }
            let worker = Worker::remote(upstream.clone(), id, self.client_root.clone()).await?;
            let worker = Arc::new(Mutex::new(worker));
            sessions.insert(id.into(), worker.clone());
            return Ok(worker);
        }
        Err("session_not_found".into())
    }
}
async fn connection(socket: TcpStream, service: Arc<Service>) {
    let (reader, mut writer) = socket.into_split();
    let result = match timeout(
        Duration::from_secs(5),
        read_frame(&mut BufReader::new(reader)),
    )
    .await
    {
        Ok(Ok(value)) => service.dispatch(value).await,
        Ok(Err(error)) => Err(error),
        Err(_) => Err("request read timeout".into()),
    };
    let reply = ServiceReply {
        protocol: service.endpoint.protocol.clone(),
        service_id: service.endpoint.service_id.clone(),
        result: result.as_ref().ok().cloned(),
        error: result.err(),
    };
    // Accepted work is independent of socket lifetime; send failure never cancels it.
    if let Ok(value) = serde_json::to_value(reply) {
        let _ = timeout(Duration::from_secs(5), write_frame(&mut writer, &value)).await;
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("cad-service: {e}");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    if std::env::args().nth(1).as_deref() == Some("--relay-runtime-dir") {
        return relay(PathBuf::from(
            std::env::args_os()
                .nth(2)
                .ok_or("--relay-runtime-dir requires directory")?,
        ))
        .await;
    }
    let mut root = None;
    let mut worker = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--runtime-dir") => root = args.next().map(PathBuf::from),
            Some("--worker") => worker = args.next().map(PathBuf::from),
            _ => return Err("usage: cad-service --runtime-dir DIR --worker CAD_SERVER".into()),
        }
    }
    let root = root.ok_or("--runtime-dir required")?;
    let worker = worker.ok_or("--worker required")?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("service.lock"))
        .map_err(|e| e.to_string())?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    }
    // Kernel initialization and handshake must finish before endpoint publication.
    let first = Worker::start(&worker, root.join("owner-client-ledger")).await?;
    let default = first.context["session_id"]
        .as_str()
        .ok_or("missing session")?
        .to_string();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let endpoint = ServiceEndpoint {
        protocol: cad_protocol::version().map_err(|e| e.to_string())?.into(),
        address: listener.local_addr().map_err(|e| e.to_string())?,
        service_id: unique_id().map_err(|e| e.to_string())?,
        token: unique_id().map_err(|e| e.to_string())?,
    };
    let path = root.join("endpoint.json");
    let temp = root.join("endpoint.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec(&endpoint).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temp, &path).map_err(|e| e.to_string())?;
    eprintln!(
        "ready service={} address={} default_session={default}",
        endpoint.service_id, endpoint.address
    );
    let (stop, mut stopping) = watch::channel(false);
    let service = Arc::new(Service {
        client_root: root.join("owner-client-ledger"),
        endpoint,
        upstream: None,
        worker,
        default: default.clone(),
        sessions: Mutex::new(BTreeMap::from([(default, Arc::new(Mutex::new(first)))])),
        keys: Mutex::new(BTreeMap::new()),
        stop,
    });
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (socket, _) = accepted.map_err(|e| e.to_string())?;
                // Include clients that connect without ever sending a frame.
                if tasks.len() >= 128 {
                    drop(socket);
                    continue;
                }
                tasks.spawn(connection(socket, service.clone()));
            }
            _ = stopping.changed() => break,
            _ = tokio::signal::ctrl_c() => {
                let _ = service.stop.send(true);
                break;
            }
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}

        }
    }
    drop(listener);
    // Drain accepted requests before worker shutdown. Kernel watchdog bounds each call.
    while tasks.join_next().await.is_some() {}
    let sessions: Vec<_> = service.sessions.lock().await.values().cloned().collect();
    for s in sessions {
        let mut w = s.lock().await;
        if let Some(child) = &mut w.child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
    // Hold the singleton lock until endpoint removal and all workers have stopped.
    let _ = std::fs::remove_file(path);
    drop(lock);
    Ok(())
}

// Upgrade rendering without stopping the existing document owner. The original endpoint
// remains pinned privately; only discovery's address changes, never service/worker identity.
async fn relay(root: PathBuf) -> Result<(), String> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("render-relay.lock"))
        .map_err(|e| e.to_string())?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Err("render relay already running".into()),
        Err(e) => return Err(e.to_string()),
    }
    let endpoint_path = root.join("endpoint.json");
    let upstream_path = root.join("render-upstream.json");
    let current: ServiceEndpoint =
        serde_json::from_slice(&std::fs::read(&endpoint_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let saved = std::fs::read(&upstream_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<ServiceEndpoint>(&b).ok());
    let upstream = if let Some(saved) = saved {
        if saved.service_id == current.service_id
            && cad_client::SharedClient::control(&saved, ServiceRequest::Ping)
                .await
                .is_ok()
        {
            saved
        } else {
            current
        }
    } else {
        current
    };
    let ping = cad_client::SharedClient::control(&upstream, ServiceRequest::Ping)
        .await
        .map_err(|e| e.to_string())?;
    let default = ping["default_session"]
        .as_str()
        .ok_or("upstream default missing")?
        .to_string();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let endpoint = ServiceEndpoint {
        address: listener.local_addr().map_err(|e| e.to_string())?,
        ..upstream.clone()
    };
    let first =
        Worker::remote(upstream.clone(), &default, root.join("relay-client-ledger")).await?;
    let (stop, mut stopping) = watch::channel(false);
    let service = Arc::new(Service {
        client_root: root.join("relay-client-ledger"),
        endpoint: endpoint.clone(),
        upstream: Some(upstream.clone()),
        worker: PathBuf::new(),
        default: default.clone(),
        sessions: Mutex::new(BTreeMap::from([(default, Arc::new(Mutex::new(first)))])),
        keys: Mutex::new(BTreeMap::new()),
        stop,
    });
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(root.join("render-upstream.tmp"))
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(&upstream).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(root.join("render-upstream.tmp"), &upstream_path).map_err(|e| e.to_string())?;
    let publish = |endpoint: &ServiceEndpoint| -> Result<(), String> {
        let path = root.join("render-endpoint.tmp");
        std::fs::write(
            &path,
            serde_json::to_vec(endpoint).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(path, &endpoint_path).map_err(|e| e.to_string())
    };
    publish(&endpoint)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    let mut tasks = tokio::task::JoinSet::new();
    eprintln!("render relay ready; document owner remains alive");
    loop {
        tokio::select! {
            accepted=listener.accept()=>{let (socket,_)=accepted.map_err(|e|e.to_string())?;if tasks.len()<128{tasks.spawn(connection(socket,service.clone()));}},
            _=stopping.changed()=>break,
            _=tokio::signal::ctrl_c()=>break,
            _=terminate.recv()=>break,
            Some(_)=tasks.join_next(),if !tasks.is_empty()=>{}
        }
    }
    drop(listener);
    let _ = service.stop.send(true);
    while tasks.join_next().await.is_some() {}
    if cad_client::SharedClient::control(&upstream, ServiceRequest::Ping)
        .await
        .is_ok()
    {
        publish(&upstream)?;
    }
    drop(lock);
    Ok(())
}
