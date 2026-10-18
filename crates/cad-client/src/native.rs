use cad_protocol::{
    command_spec, validate, ClientId, Command, Context, ProtocolError, Request, RequestId, Response,
};
use serde_json::Value;
use std::{fmt, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    time::timeout,
};

#[derive(Debug)]
pub enum ClientError {
    InvalidRequest(ProtocolError),
    Transport(String),
    OutcomeUnknown {
        request: Box<Request>,
        reason: String,
    },
    ConnectionUnusable,
}
impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(e) => write!(f, "invalid request: {e}"),
            Self::Transport(e) => write!(f, "transport: {e}"),
            Self::OutcomeUnknown { request, reason } => write!(
                f,
                "request {} outcome unknown: {reason}; no automatic replay",
                request.request_id.get()
            ),
            Self::ConnectionUnusable => f.write_str(
                "connection is unusable; reconcile on the same worker before any explicit retry",
            ),
        }
    }
}
impl std::error::Error for ClientError {}
impl From<ProtocolError> for ClientError {
    fn from(e: ProtocolError) -> Self {
        Self::InvalidRequest(e)
    }
}

/// Sequential asynchronous protocol connection. It does not discover, reconnect,
/// launch a shared daemon or retry a command. Those policies belong to P3.
pub struct RpcClient<R, W> {
    reader: R,
    writer: Option<W>,
    context: Option<Context>,
    next: u64,
    poisoned: bool,
    deadline: Duration,
}
impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> RpcClient<R, W> {
    pub async fn open(
        reader: R,
        writer: W,
        client: ClientId,
        deadline: Duration,
    ) -> Result<Self, ClientError> {
        let mut connection = Self {
            reader,
            writer: Some(writer),
            context: None,
            next: 1,
            poisoned: false,
            deadline,
        };
        let hello = Request::hello(client)?;
        let reply = connection.send(&hello).await?;
        if !reply.ok {
            return Err(ClientError::Transport(format!(
                "handshake rejected: {:?}",
                reply.error
            )));
        }
        let high_water = reply
            .result
            .as_ref()
            .and_then(|v| v.get("last_request_id"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ClientError::Transport("handshake omitted request high-water mark".into())
            })?;
        connection.next = high_water
            .checked_add(1)
            .ok_or_else(|| ClientError::Transport("request IDs exhausted".into()))?;
        connection.context = Some(reply.context);
        Ok(connection)
    }
    pub fn context(&self) -> Result<&Context, ClientError> {
        self.context
            .as_ref()
            .ok_or_else(|| ClientError::Transport("handshake incomplete".into()))
    }
    pub fn prepare(&mut self, command: Command) -> Result<Request, ClientError> {
        if self.poisoned {
            return Err(ClientError::ConnectionUnusable);
        }
        command.value()?;
        let id = RequestId::new(self.next)?;
        let request = Request::execute(self.context()?, id, command);
        request.value()?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| ClientError::Transport("request IDs exhausted".into()))?;
        Ok(request)
    }
    pub async fn execute(&mut self, command: Command) -> Result<Response, ClientError> {
        let request = self.prepare(command)?;
        self.send(&request).await
    }
    pub async fn status(&mut self, target: RequestId) -> Result<Response, ClientError> {
        let mut request = self.prepare(Command::Capabilities)?;
        request.method = "request.status".into();
        request.command = None;
        request.target_request_id = Some(target);
        self.send(&request).await
    }
    /// An explicit resend must retain the exact Request and instance context.
    /// On EOF, timeout, cancellation or malformed replies the stream is poisoned.
    pub async fn send(&mut self, request: &Request) -> Result<Response, ClientError> {
        if self.poisoned {
            return Err(ClientError::ConnectionUnusable);
        }
        let value = request.value()?;
        if let Some(context) = &self.context {
            if request.client_id != context.client_id
                || request.worker_id.as_ref() != Some(&context.worker_id)
                || request.session_id.as_ref() != Some(&context.session_id)
            {
                return Err(ClientError::Transport(
                    "request belongs to a different client/worker/session".into(),
                ));
            }
        }
        let mut bytes =
            serde_json::to_vec(&value).map_err(|e| ClientError::Transport(e.to_string()))?;
        bytes.push(b'\n');
        // Set before the first await: dropping a pending future must not allow a
        // later call to consume its late reply as the next request's response.
        self.poisoned = true;
        let result = timeout(self.deadline, async {
            let writer = self
                .writer
                .as_mut()
                .ok_or_else(|| "writer closed".to_string())?;
            writer.write_all(&bytes).await.map_err(|e| e.to_string())?;
            writer.flush().await.map_err(|e| e.to_string())?;
            let mut line = Vec::new();
            let count = (&mut self.reader)
                .take(16 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .await
                .map_err(|e| e.to_string())?;
            if count == 0 {
                return Err("worker disconnected".into());
            }
            if count > 16 * 1024 * 1024 || line.last() != Some(&b'\n') {
                return Err(
                    "response is incomplete or exceeds 16 MiB; use a future mesh transfer channel"
                        .into(),
                );
            }
            let value: Value = serde_json::from_slice(&line).map_err(|e| e.to_string())?;
            let response = Response::parse(value).map_err(|e| e.to_string())?;
            if response.request_id != request.request_id
                || response.context.client_id != request.client_id
            {
                return Err("response correlation mismatch".into());
            }
            if let Some(context) = &self.context {
                if response.context.worker_id != context.worker_id
                    || response.context.session_id != context.session_id
                    || response.context.service_id != context.service_id
                {
                    return Err("response instance changed".into());
                }
            }
            if response.ok && (request.method == "hello" || request.method == "request.status") {
                cad_protocol::validate_ref(
                    response
                        .result
                        .as_ref()
                        .ok_or_else(|| "missing result".to_string())?,
                    if request.method == "hello" {
                        "HelloResult"
                    } else {
                        "StatusResult"
                    },
                )
                .map_err(|e| e.to_string())?;
            }
            if response.ok {
                if let Some(command) = &request.command {
                    let command_value = command.value().map_err(|e| e.to_string())?;
                    let op = command_value["op"]
                        .as_str()
                        .ok_or_else(|| "missing op".to_string())?;
                    let output = response
                        .result
                        .as_ref()
                        .ok_or_else(|| "missing result".to_string())?;
                    validate(
                        output,
                        &command_spec(op).map_err(|e| e.to_string())?["output_schema"],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            Ok::<_, String>(response)
        })
        .await;
        match result {
            Ok(Ok(response)) => {
                self.poisoned = false;
                Ok(response)
            }
            failure => Err(ClientError::OutcomeUnknown {
                request: Box::new(request.clone()),
                reason: match failure {
                    Ok(Err(message)) => message,
                    _ => "response timeout".into(),
                },
            }),
        }
    }
}

/// Owned, isolated worker transport for P2 tests and batch examples. It is not a
/// user-facing CLI, the P3 default daemon, or a hidden Python bridge.
pub struct WorkerClient {
    rpc: RpcClient<BufReader<ChildStdout>, ChildStdin>,
    child: Child,
}
impl WorkerClient {
    pub async fn spawn(path: impl AsRef<Path>, client: ClientId) -> Result<Self, ClientError> {
        let mut child = tokio::process::Command::new(path.as_ref())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let writer = child
            .stdin
            .take()
            .ok_or_else(|| ClientError::Transport("missing worker stdin".into()))?;
        let reader = child
            .stdout
            .take()
            .ok_or_else(|| ClientError::Transport("missing worker stdout".into()))?;
        let rpc = RpcClient::open(
            BufReader::new(reader),
            writer,
            client,
            Duration::from_secs(10),
        )
        .await?;
        Ok(Self { rpc, child })
    }
    pub async fn shutdown(mut self) -> std::io::Result<()> {
        self.rpc.writer.take();
        match timeout(Duration::from_secs(2), self.child.wait()).await {
            Ok(result) => {
                let status = result?;
                if status.success() {
                    Ok(())
                } else {
                    Err(std::io::Error::other(format!("worker exited {status}")))
                }
            }
            Err(_) => {
                self.child.kill().await?;
                Ok(())
            }
        }
    }
}
impl std::ops::Deref for WorkerClient {
    type Target = RpcClient<BufReader<ChildStdout>, ChildStdin>;
    fn deref(&self) -> &Self::Target {
        &self.rpc
    }
}
impl std::ops::DerefMut for WorkerClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rpc
    }
}
