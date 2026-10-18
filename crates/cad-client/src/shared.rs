//! Native, loopback-only shared service discovery and explicit session attachment.
use crate::{protocol::*, ClientError};
use cad_protocol::service::*;
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};
pub const MAX_FRAME: u64 = 16 * 1024 * 1024;
pub fn unique_id() -> std::io::Result<String> {
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Value, String> {
    let mut line = Vec::new();
    let n = reader
        .take(MAX_FRAME + 1)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 || n as u64 > MAX_FRAME || line.last() != Some(&b'\n') {
        return Err("disconnected or invalid/oversized frame".into());
    }
    serde_json::from_slice(&line).map_err(|e| e.to_string())
}
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 >= MAX_FRAME {
        return Err("frame exceeds 16 MiB".into());
    }
    bytes.push(b'\n');
    writer.write_all(&bytes).await.map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}
fn transport(e: impl ToString) -> ClientError {
    ClientError::Transport(e.to_string())
}
#[derive(Clone, Debug)]
pub struct ServiceOptions {
    pub runtime_dir: PathBuf,
    pub executable: PathBuf,
    pub worker: PathBuf,
}
impl ServiceOptions {
    pub fn new(executable: impl Into<PathBuf>, worker: impl Into<PathBuf>) -> Self {
        let root = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir)
                    .join(".cache")
            });
        Self {
            runtime_dir: root.join("miocad"),
            executable: executable.into(),
            worker: worker.into(),
        }
    }
}
/// Every exchange has a fresh socket. Disconnecting never owns the worker lifetime.
/// An uncertain modeling outcome still poisons the logical client until reattachment.
pub struct SharedClient {
    endpoint: ServiceEndpoint,
    context: Context,
    next: u64,
    uncertain: bool,
}
impl SharedClient {
    pub fn endpoint(&self) -> &ServiceEndpoint {
        &self.endpoint
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    pub async fn control(
        endpoint: &ServiceEndpoint,
        request: ServiceRequest,
    ) -> Result<Value, ClientError> {
        if endpoint.protocol != version()? || !endpoint.address.ip().is_loopback() {
            return Err(transport("unsupported/non-loopback endpoint"));
        }
        let method = serde_json::to_value(&request).map_err(transport)?["method"]
            .as_str()
            .unwrap()
            .to_string();
        let envelope = ServiceEnvelope {
            protocol: version()?.into(),
            service_id: endpoint.service_id.clone(),
            token: endpoint.token.clone(),
            request,
        };
        let envelope = serde_json::to_value(envelope).map_err(transport)?;
        validate_envelope(&envelope)?;
        timeout(Duration::from_secs(15), async {
            let mut socket = TcpStream::connect(endpoint.address)
                .await
                .map_err(transport)?;
            write_frame(&mut socket, &envelope)
                .await
                .map_err(transport)?;
            let value = read_frame(&mut BufReader::new(socket))
                .await
                .map_err(transport)?;
            cad_protocol::validate_ref(&value, "ServiceReply")?;
            let reply: ServiceReply = serde_json::from_value(value).map_err(transport)?;
            if reply.protocol != version()? || reply.service_id != endpoint.service_id {
                return Err(transport("service identity changed; no retry"));
            }
            match (reply.result, reply.error) {
                (Some(result), None) => {
                    validate_result(&method, &result)?;
                    Ok(result)
                }
                (_, Some(error)) => Err(transport(error)),
                _ => Err(transport("invalid service reply")),
            }
        })
        .await
        .map_err(|_| transport("service response timeout; outcome may be unknown"))?
    }
    pub async fn attach(
        endpoint: ServiceEndpoint,
        client: &str,
        session: Option<&str>,
    ) -> Result<Self, ClientError> {
        let request = Request::hello(ClientId::new(client)?)?;
        let reply = Self::control(
            &endpoint,
            ServiceRequest::Rpc {
                session: session.map(str::to_string),
                request: request.value()?,
            },
        )
        .await?;
        let reply = Response::parse(reply)?;
        if !reply.ok {
            return Err(transport(format!("hello rejected: {:?}", reply.error)));
        }
        if reply.context.client_id.as_str() != client
            || session.is_some_and(|s| s != reply.context.session_id.as_str())
        {
            return Err(transport("hello identity mismatch"));
        }
        cad_protocol::validate_ref(
            reply
                .result
                .as_ref()
                .ok_or_else(|| transport("hello omitted result"))?,
            "HelloResult",
        )?;
        let next = reply.result.as_ref().unwrap()["last_request_id"]
            .as_u64()
            .ok_or_else(|| transport("invalid high water"))?
            + 1;
        Ok(Self {
            endpoint,
            context: reply.context,
            next,
            uncertain: false,
        })
    }
    pub async fn connect_or_start(
        options: &ServiceOptions,
        client: &str,
    ) -> Result<Self, ClientError> {
        ClientId::new(client)?;
        let path = options.runtime_dir.join("endpoint.json");
        if let Ok(endpoint) = read_endpoint(&path) {
            if let Ok(c) = Self::attach(endpoint, client, None).await {
                return Ok(c);
            }
        }
        // Each contender may launch a process; only the lifetime lock holder may publish.
        std::fs::create_dir_all(&options.runtime_dir).map_err(transport)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&options.runtime_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(transport)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(options.runtime_dir.join("service.log"))
            .map_err(transport)?;
        let mut child = std::process::Command::new(&options.executable)
            .arg("--runtime-dir")
            .arg(&options.runtime_dir)
            .arg("--worker")
            .arg(&options.worker)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(transport)?;
        // Reap both losing contenders and the eventual daemon without tying lifetime to the client.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < until {
            if let Ok(endpoint) = read_endpoint(&path) {
                if let Ok(c) = Self::attach(endpoint, client, None).await {
                    return Ok(c);
                }
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        Err(transport(
            "shared service did not become ready; inspect runtime service.log",
        ))
    }
    pub fn prepare(&mut self, command: Command) -> Result<Request, ClientError> {
        if self.uncertain {
            return Err(ClientError::ConnectionUnusable);
        }
        command.value()?;
        let request = Request::execute(&self.context, RequestId::new(self.next)?, command);
        self.next += 1;
        Ok(request)
    }
    pub async fn execute(&mut self, command: Command) -> Result<Response, ClientError> {
        let req = self.prepare(command)?;
        self.send(&req).await
    }
    pub async fn send(&mut self, request: &Request) -> Result<Response, ClientError> {
        if self.uncertain {
            return Err(ClientError::ConnectionUnusable);
        }
        if request.client_id != self.context.client_id
            || request.worker_id.as_ref() != Some(&self.context.worker_id)
            || request.session_id.as_ref() != Some(&self.context.session_id)
        {
            return Err(transport(
                "request belongs to another instance/session/client",
            ));
        }
        let value = request.value()?;
        self.uncertain = true;
        let exchange = async {
            let value = Self::control(
                &self.endpoint,
                ServiceRequest::Rpc {
                    session: Some(self.context.session_id.as_str().into()),
                    request: value,
                },
            )
            .await?;
            let response = Response::parse(value)?;
            if response.request_id != request.request_id || response.context != self.context {
                return Err(transport("response correlation/instance mismatch"));
            }
            if response.ok {
                if let Some(command) = &request.command {
                    let command = command.value()?;
                    validate(
                        response
                            .result
                            .as_ref()
                            .ok_or_else(|| transport("missing command result"))?,
                        &command_spec(command["op"].as_str().unwrap())?["output_schema"],
                    )?;
                } else if request.method == "request.status" {
                    cad_protocol::validate_ref(
                        response
                            .result
                            .as_ref()
                            .ok_or_else(|| transport("missing status result"))?,
                        "StatusResult",
                    )?;
                }
            }
            Ok::<_, ClientError>(response)
        }
        .await;
        match exchange {
            Ok(response) => {
                self.uncertain = false;
                Ok(response)
            }
            Err(e) => Err(ClientError::OutcomeUnknown {
                request: Box::new(request.clone()),
                reason: e.to_string(),
            }),
        }
    }
    pub async fn status(&mut self, target: RequestId) -> Result<Response, ClientError> {
        let mut req = self.prepare(Command::Capabilities)?;
        req.method = "request.status".into();
        req.command = None;
        req.target_request_id = Some(target);
        self.send(&req).await
    }
    pub async fn sessions(&self) -> Result<Value, ClientError> {
        Self::control(&self.endpoint, ServiceRequest::Sessions).await
    }
    pub async fn create_session(&self, key: &str) -> Result<String, ClientError> {
        let v = Self::control(
            &self.endpoint,
            ServiceRequest::CreateSession { key: key.into() },
        )
        .await?;
        Ok(v["session_id"]
            .as_str()
            .ok_or_else(|| transport("session id missing"))?
            .into())
    }
    pub async fn snapshot(&self) -> Result<Value, ClientError> {
        Self::control(
            &self.endpoint,
            ServiceRequest::Snapshot {
                session: self.context.session_id.as_str().into(),
            },
        )
        .await
    }
    pub async fn events(&self, after: u64) -> Result<Value, ClientError> {
        Self::control(
            &self.endpoint,
            ServiceRequest::Events {
                session: self.context.session_id.as_str().into(),
                after,
            },
        )
        .await
    }
    pub async fn shutdown(&self) -> Result<Value, ClientError> {
        Self::control(&self.endpoint, ServiceRequest::Shutdown).await
    }
}
fn read_endpoint(path: &Path) -> Result<ServiceEndpoint, ClientError> {
    serde_json::from_slice(&std::fs::read(path).map_err(transport)?).map_err(transport)
}
