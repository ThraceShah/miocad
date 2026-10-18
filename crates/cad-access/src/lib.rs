#![forbid(unsafe_code)]
//! Shared product command adapter. CLI/MCP contain only transport-specific code.
use cad_client::{protocol::*, ClientError, ServiceOptions, SharedClient};
use cad_protocol::service::ServiceRequest;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};
pub const HELP:&str="AI-CAD Rust frontend\nRead one JSON command per line (CLI) or MCP JSON-RPC per line (MCP).\n  --runtime-dir DIR  Shared daemon discovery directory\n  --service PATH     cad-service executable (or CAD_SERVICE)\n  --worker PATH      C++ cad-server executable (or CAD_SERVER)\n  --client-id ID     Stable logical identity for reconciliation; one active frontend per ID\n  --session ID       Attach an existing session\n  --isolated         Create an isolated session in the shared daemon\n  --command JSON     Execute one command (CLI only)\n  --help / --version\nEOF closes the frontend, not the shared service. Explicit op=service.shutdown stops it.\n";
#[derive(Clone, Debug)]
pub struct Config {
    pub options: ServiceOptions,
    pub client_id: String,
    pub session: Option<String>,
    pub isolated: bool,
    pub command: Option<String>,
}
pub enum Action {
    Run(Config),
    Help,
    Version,
}
impl Config {
    pub fn parse() -> Result<Action, String> {
        let bin = std::env::current_exe().map_err(|e| e.to_string())?;
        let bin = bin.parent().ok_or("no executable directory")?;
        // Cargo examples live one directory below the product binaries.
        let bin = if bin.file_name().is_some_and(|name| name == "examples") {
            bin.parent().unwrap_or(bin)
        } else {
            bin
        };
        let worker = std::env::var_os("CAD_SERVER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let sibling = bin.join("cad-server");
                if sibling.exists() {
                    sibling
                } else {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/cad-server")
                }
            });
        let service = std::env::var_os("CAD_SERVICE")
            .map(PathBuf::from)
            .unwrap_or_else(|| bin.join("cad-service"));
        let mut config = Self {
            options: ServiceOptions::new(service, worker),
            client_id: format!(
                "frontend-{}",
                cad_client::shared::unique_id().map_err(|e| e.to_string())?
            ),
            session: None,
            isolated: false,
            command: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--help" | "-h" => return Ok(Action::Help),
                "--version" => return Ok(Action::Version),
                "--isolated" => config.isolated = true,
                "--runtime-dir" => {
                    config.options.runtime_dir =
                        args.next().ok_or("--runtime-dir needs a path")?.into()
                }
                "--service" => {
                    config.options.executable = args.next().ok_or("--service needs a path")?.into()
                }
                "--worker" => {
                    config.options.worker = args.next().ok_or("--worker needs a path")?.into()
                }
                "--client-id" => config.client_id = args.next().ok_or("--client-id needs an ID")?,
                "--session" => config.session = Some(args.next().ok_or("--session needs an ID")?),
                "--command" => config.command = Some(args.next().ok_or("--command needs JSON")?),
                _ => return Err(format!("unknown option {flag}; use --help")),
            }
        }
        ClientId::new(config.client_id.clone()).map_err(|e| e.to_string())?;
        if config.isolated && config.session.is_some() {
            return Err("--isolated conflicts with --session".into());
        }
        Ok(Action::Run(config))
    }
}
pub fn catalog() -> Result<Vec<Value>, ProtocolError> {
    let contract = contract()?;
    Ok(contract["commands"]
        .as_array()
        .unwrap()
        .iter()
        .chain(contract["access_commands"].as_array().unwrap())
        .map(|spec| {
            let mut spec = spec.clone();
            spec["input_schema"]["additionalProperties"] = json!(false);
            spec
        })
        .collect())
}
pub fn tool() -> Result<Value, ProtocolError> {
    fn expand(value: &Value) -> Result<Value, ProtocolError> {
        if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
            let name = reference
                .strip_prefix("#/$defs/")
                .expect("local neutral references");
            return expand(&contract()?["$defs"][name]);
        }
        Ok(match value {
            Value::Array(array) => {
                Value::Array(array.iter().map(expand).collect::<Result<_, _>>()?)
            }
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), expand(v)?)))
                    .collect::<Result<_, ProtocolError>>()?,
            ),
            _ => value.clone(),
        })
    }
    let schemas = catalog()?
        .into_iter()
        .map(|spec| expand(&spec["input_schema"]))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        json!({"name":"cad_command","description":"Execute a CAD command against the default shared service. Start with commands.list, service.snapshot or document.create. Mutations need document/revision. render.snapshot returns CAD scene PNGs; render.pick needs render_id,x,y and rejects stale frames. Closing MCP preserves documents. service.shutdown explicitly stops the whole service. Results with outcome_unknown require service.reconnect then request.status/snapshot; never blindly repeat a mutation.",
        "inputSchema":{"type":"object","properties":{"command":{"oneOf":schemas}},"required":["command"],"additionalProperties":false},
        "outputSchema":{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]},
        "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":false}}),
    )
}
pub fn failure(kind: &str, message: impl ToString) -> Value {
    json!({"ok":false,"error":{"code":"command_failed","kind":kind,"message":message.to_string(),"retryable":false}})
}
fn report(error: ClientError) -> Value {
    match error {
        ClientError::InvalidRequest(e) => failure("invalid_params", e),
        ClientError::ConnectionUnusable => failure(
            "connection_unusable",
            "service.reconnect required; then inspect request.status and document state",
        ),
        ClientError::OutcomeUnknown { request, reason } => {
            let mut output = failure("outcome_unknown", reason);
            output["error"]["request"] = serde_json::to_value(request).unwrap();
            output
        }
        ClientError::Transport(message) => {
            let prefix = message.split(':').next().unwrap_or("");
            let kind = if !prefix.is_empty()
                && prefix.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
            {
                prefix
            } else {
                "transport_error"
            };
            failure(kind, &message)
        }
    }
}
fn local(message: impl ToString) -> ClientError {
    ClientError::Transport(message.to_string())
}
pub struct Frontend {
    config: Config,
    client: Option<SharedClient>,
}
impl Frontend {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            client: None,
        }
    }
    async fn connect(&mut self) -> Result<&mut SharedClient, ClientError> {
        if self.client.is_none() {
            let mut client =
                SharedClient::connect_or_start(&self.config.options, &self.config.client_id)
                    .await?;
            let session = if self.config.isolated {
                Some(client.create_session(&self.config.client_id).await?)
            } else {
                self.config.session.clone()
            };
            if let Some(session) = session {
                client = SharedClient::attach(
                    client.endpoint().clone(),
                    &self.config.client_id,
                    Some(&session),
                )
                .await?;
            }
            self.client = Some(client);
        }
        Ok(self.client.as_mut().unwrap())
    }
    pub async fn command(&mut self, command: Value) -> Value {
        let id = command.get("id").cloned();
        let mut result = match self.try_command(command).await {
            Ok(value) => value,
            Err(error) => report(error),
        };
        if let Some(id) = id {
            result["id"] = id;
        }
        result
    }
    async fn try_command(&mut self, mut command: Value) -> Result<Value, ClientError> {
        let op = command
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| local("invalid_params: command must contain string op"))?
            .to_string();
        let specs = catalog()?;
        let Some(spec) = specs.iter().find(|spec| spec["id"] == op) else {
            return Ok(failure(
                "unknown_command",
                format!("unknown operation: {op}"),
            ));
        };
        validate(&command, &spec["input_schema"])?;
        if op == "commands.list" {
            return Ok(json!({"ok":true,"result":specs}));
        }
        let client = self.connect().await?;
        let session = client.context().session_id.as_str().to_string();
        let text = |name: &str| command[name].as_str().unwrap().to_string();
        let request = match op.as_str() {
            "render.snapshot" => Some(ServiceRequest::RenderSnapshot {
                session: session.clone(),
                document: text("document"),
                width: command["width"].as_u64().unwrap_or(384) as u32,
                height: command["height"].as_u64().unwrap_or(384) as u32,
                directory: command
                    .get("directory")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }),
            "render.pick" => Some(ServiceRequest::RenderPick {
                session: session.clone(),
                render_id: text("render_id"),
                x: command["x"].as_u64().unwrap() as u32,
                y: command["y"].as_u64().unwrap() as u32,
                target: command["target"].as_str().unwrap_or("edge").into(),
            }),
            "service.info" => Some(ServiceRequest::Ping),
            "service.sessions" => Some(ServiceRequest::Sessions),
            "service.snapshot" => Some(ServiceRequest::Snapshot {
                session: session.clone(),
            }),
            "service.events" => Some(ServiceRequest::Events {
                session: session.clone(),
                after: command["after"].as_u64().unwrap(),
            }),
            "service.session.create" => Some(ServiceRequest::CreateSession { key: text("key") }),
            "service.shutdown" => Some(ServiceRequest::Shutdown),
            _ => None,
        };
        if let Some(request) = request {
            let result = SharedClient::control(client.endpoint(), request).await?;
            validate(&result, &spec["output_schema"])?;
            return Ok(json!({"ok":true,"result":result}));
        }
        if op == "service.attach" || op == "service.reconnect" {
            let target = if op == "service.attach" {
                text("session")
            } else {
                session
            };
            let next = SharedClient::attach(
                client.endpoint().clone(),
                client.context().client_id.as_str(),
                Some(&target),
            )
            .await?;
            if op == "service.reconnect" && next.context() != client.context() {
                return Err(local(
                    "wrong_instance: refusing to reconnect to a changed worker",
                ));
            }
            let context = serde_json::to_value(next.context()).map_err(local)?;
            self.client = Some(next);
            return Ok(json!({"ok":true,"result":context}));
        }
        let response = if op == "request.status" {
            client
                .status(RequestId::new(
                    command["target_request_id"].as_u64().unwrap(),
                )?)
                .await?
        } else {
            command
                .as_object_mut()
                .ok_or_else(|| local("invalid_params: expected object"))?
                .remove("id");
            let typed: Command = serde_json::from_value(command)
                .map_err(|e| local(format!("invalid_params: {e}")))?;
            client.execute(typed).await?
        };
        serde_json::to_value(response).map_err(local)
    }
}
/// Bounded stdio input. JSON errors are recoverable; framing/resource errors close the stream.
pub async fn read_input<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Result<Value, String>>, String> {
    let mut bytes = Vec::new();
    let count = reader
        .take(cad_client::shared::MAX_FRAME + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    if count == 0 {
        return Ok(None);
    }
    if count as u64 > cad_client::shared::MAX_FRAME {
        return Err("input exceeds 16 MiB".into());
    }
    Ok(Some(
        serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
    ))
}
