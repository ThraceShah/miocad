#![forbid(unsafe_code)]
//! MCP stdio, negotiated 2025-11-25 / 2025-06-18. No HTTP or window dependency.
use cad_access::{Action, Config, Frontend, HELP};
use serde_json::{json, Value};
use tokio::io::{AsyncWriteExt, BufReader};
fn error(id: Value, code: i64, message: impl ToString) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.to_string()}})
}
struct Mcp {
    frontend: Frontend,
    initialized: bool,
    ready: bool,
}
impl Mcp {
    async fn handle(&mut self, request: Value) -> Option<Value> {
        if !request.is_object() || request["jsonrpc"] != "2.0" || !request["method"].is_string() {
            let id = request
                .get("id")
                .filter(|id| id.is_string() || id.is_i64() || id.is_u64())
                .cloned()
                .unwrap_or(Value::Null);
            return Some(error(id, -32600, "invalid JSON-RPC request"));
        }
        let method = request["method"].as_str().unwrap();
        let Some(id) = request.get("id").cloned() else {
            if method == "notifications/initialized"
                && self.initialized
                && request.get("params").is_none_or(Value::is_object)
            {
                self.ready = true;
            }
            // In particular tools/call notifications must not execute CAD commands.
            return None;
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Some(error(
                Value::Null,
                -32600,
                "request ID must be a string or integer",
            ));
        }
        let params = request.get("params").cloned().unwrap_or(json!({}));
        if !params.is_object() {
            return Some(error(id, -32602, "params must be an object"));
        }
        let result = match method {
            "initialize" => {
                if self.initialized {
                    return Some(error(id, -32600, "already initialized"));
                }
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Some(error(
                        id,
                        -32602,
                        "initialize requires protocolVersion, capabilities and clientInfo",
                    ));
                }
                let version = match params["protocolVersion"].as_str().unwrap() {
                    "2025-06-18" => "2025-06-18",
                    _ => "2025-11-25",
                };
                self.initialized = true;
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"ai-cad","version":env!("CARGO_PKG_VERSION")}})
            }
            "ping" => json!({}),
            _ if !self.ready => {
                return Some(error(
                    id,
                    -32002,
                    "initialize first, then notifications/initialized",
                ))
            }
            "tools/list" => {
                if params.get("cursor").is_some() {
                    return Some(error(id, -32602, "tool list has no pagination cursor"));
                }
                match cad_access::tool() {
                    Ok(tool) => json!({"tools":[tool]}),
                    Err(e) => return Some(error(id, -32603, e)),
                }
            }
            "tools/call" => {
                if params["name"] != "cad_command" {
                    return Some(error(id, -32602, "unknown tool"));
                }
                let args = &params["arguments"];
                if !args.is_object()
                    || args.as_object().unwrap().len() != 1
                    || !args["command"].is_object()
                {
                    return Some(error(
                        id,
                        -32602,
                        "arguments must contain only an object command",
                    ));
                }
                let command = args["command"].clone();
                let snapshot = command["op"] == "render.snapshot";
                let output = self.frontend.command(command).await;
                let mut content = vec![json!({"type":"text","text":output.to_string()})];
                if output["ok"] == true && snapshot {
                    content.push(json!({"type":"image","mimeType":"image/png","data":output["result"]["images"]["shaded"]}));
                }
                json!({"content":content,"structuredContent":output,"isError":output["ok"]!=true})
            }
            _ => return Some(error(id, -32601, "method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("cad-mcp: {error}");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), String> {
    let config = match Config::parse()? {
        Action::Help => {
            print!("{HELP}");
            return Ok(());
        }
        Action::Version => {
            println!("cad-mcp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Action::Run(config) => config,
    };
    if config.command.is_some() {
        return Err("--command is only supported by cad-cli".into());
    }
    let mut mcp = Mcp {
        frontend: Frontend::new(config),
        initialized: false,
        ready: false,
    };
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(request) = cad_access::read_input(&mut input).await? {
        let response = match request {
            Ok(request) => mcp.handle(request).await,
            Err(e) => Some(error(Value::Null, -32700, e)),
        };
        if let Some(response) = response {
            let mut bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            output.write_all(&bytes).await.map_err(|e| e.to_string())?;
            output.flush().await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
