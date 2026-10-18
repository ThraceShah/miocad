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
const EVENT_LIMIT: usize = 64;
struct Worker {
    child: Child,
    reader: BufReader<ChildStdout>,
    writer: ChildStdin,
    context: Value,
    docs: BTreeSet<String>,
    events: VecDeque<Value>,
    cursor: u64,
    lost: Option<String>,
    internal_id: u64,
}
impl Worker {
    async fn start(path: &PathBuf) -> Result<Self, String> {
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
            child,
            reader,
            writer,
            context: Value::Null,
            docs: BTreeSet::new(),
            events: VecDeque::new(),
            cursor: 0,
            lost: None,
            internal_id: 0,
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
        Ok(w)
    }
    async fn exchange(&mut self, req: &Value) -> Result<Value, String> {
        if let Some(reason) = &self.lost {
            return Err(format!(
                "worker_lost: {reason}; documents unavailable; no automatic restart/retry"
            ));
        }
        let result = timeout(Duration::from_secs(60), async {
            write_frame(&mut self.writer, req).await?;
            let value = read_frame(&mut self.reader).await?;
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
            let _ = self.child.kill().await;
        }
        result.map_err(|e| format!("worker_lost: {e}; in-flight outcome unknown; no replay"))
    }
    async fn snapshot(&mut self) -> Result<Value, String> {
        self.internal_id += 1;
        let request = json!({
            "protocol": self.context["protocol"], "method": "execute",
            "client_id": "service-observer", "request_id": self.internal_id,
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
    fn diagnostics(&mut self) -> Value {
        if self.lost.is_none() {
            match self.child.try_wait() {
                Ok(Some(status)) => self.lost = Some(format!("worker exited {status}")),
                Err(e) => self.lost = Some(e.to_string()),
                _ => {}
            }
        }
        json!({"session_id":self.context["session_id"],"worker_id":self.context["worker_id"],"worker_pid":self.child.id(),"state":if self.lost.is_some(){"lost"}else{"ready"},"reason":self.lost,"cursor":self.cursor,"documents":self.docs.len()})
    }
}
type Session = Arc<Mutex<Worker>>;
struct Service {
    endpoint: ServiceEndpoint,
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
                let worker = Worker::start(&self.worker).await?;
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
                let result = session.lock().await.exchange(&request).await;
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
        self.sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| "session_not_found".into())
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
    let first = Worker::start(&worker).await?;
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
        endpoint,
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
        let _ = w.child.kill().await;
        let _ = w.child.wait().await;
    }
    // Hold the singleton lock until endpoint removal and all workers have stopped.
    let _ = std::fs::remove_file(path);
    drop(lock);
    Ok(())
}
