//! Local service control plane. Modeling payloads remain the neutral worker protocol.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceEndpoint {
    pub protocol: String,
    pub address: std::net::SocketAddr,
    pub service_id: String,
    pub token: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServiceRequest {
    Ping,
    Sessions,
    CreateSession {
        key: String,
    },
    Rpc {
        session: Option<String>,
        request: Value,
    },
    Snapshot {
        session: String,
    },
    Events {
        session: String,
        after: u64,
    },
    Cancel {
        session: String,
        client_id: String,
        request_id: u64,
    },
    RenderSnapshot {
        session: String,
        document: String,
        width: u32,
        height: u32,
        directory: Option<String>,
    },
    RenderPick {
        session: String,
        render_id: String,
        x: u32,
        y: u32,
        target: String,
    },
    Shutdown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceEnvelope {
    pub protocol: String,
    pub service_id: String,
    pub token: String,
    pub request: ServiceRequest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceReply {
    pub protocol: String,
    pub service_id: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Validate both languages against the same service control contract.
pub fn validate_envelope(value: &Value) -> Result<(), crate::ProtocolError> {
    crate::validate_ref(value, "ServiceEnvelope")
}
pub fn validate_result(method: &str, value: &Value) -> Result<(), crate::ProtocolError> {
    let contract = crate::contract()?;
    let spec = contract["service_methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|spec| spec["method"] == method)
        .expect("typed service method is registered");
    crate::validate(value, &spec["output_schema"])
}
