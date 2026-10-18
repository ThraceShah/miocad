#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fmt, sync::OnceLock};

pub const CONTRACT_JSON: &str = include_str!("../../../protocol/contract.json");
#[derive(Debug, Clone)]
pub struct ProtocolError(pub String);
impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProtocolError {}
impl From<serde_json::Error> for ProtocolError {
    fn from(e: serde_json::Error) -> Self {
        Self(e.to_string())
    }
}
pub fn contract() -> Result<&'static Value, ProtocolError> {
    static DATA: OnceLock<Result<Value, String>> = OnceLock::new();
    DATA.get_or_init(|| serde_json::from_str(CONTRACT_JSON).map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| ProtocolError(e.clone()))
}
pub fn version() -> Result<&'static str, ProtocolError> {
    contract()?
        .get("protocol_version")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError("missing protocol version".into()))
}
pub fn command_spec(op: &str) -> Result<&'static Value, ProtocolError> {
    contract()?
        .get("commands")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|s| s["id"] == op))
        .ok_or_else(|| ProtocolError(format!("unknown command {op}")))
}
pub fn validate_ref(value: &Value, name: &str) -> Result<(), ProtocolError> {
    validate(value, &json!({"$ref":format!("#/$defs/{name}")}))
}
pub fn validate(value: &Value, schema: &Value) -> Result<(), ProtocolError> {
    fn finite(v: &Value, depth: usize) -> Result<(), ProtocolError> {
        if depth > 64 {
            return Err(ProtocolError("JSON nesting exceeds 64 levels".into()));
        }
        match v {
            Value::Array(a) => {
                for x in a {
                    finite(x, depth + 1)?
                }
            }
            Value::Object(m) => {
                for x in m.values() {
                    finite(x, depth + 1)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn check(v: &Value, s: &Value, depth: usize) -> Result<(), ProtocolError> {
        let fail = |m: &str| ProtocolError(m.into());
        if depth > 64 {
            return Err(fail("schema nesting exceeds 64 levels"));
        }
        if let Some(r) = s.get("$ref").and_then(Value::as_str) {
            let name = r
                .strip_prefix("#/$defs/")
                .ok_or_else(|| fail("unsupported schema reference"))?;
            let target = contract()?
                .get("$defs")
                .and_then(|d| d.get(name))
                .ok_or_else(|| fail("unknown schema reference"))?;
            return check(v, target, depth + 1);
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(a) = s.get(key).and_then(Value::as_array) {
                let count = a
                    .iter()
                    .filter(|item| check(v, item, depth + 1).is_ok())
                    .count();
                if count == 0 || (key == "oneOf" && count != 1) {
                    return Err(fail("schema variant mismatch"));
                }
            }
        }
        let correct = match s.get("type").and_then(Value::as_str) {
            Some("object") => v.is_object(),
            Some("array") => v.is_array(),
            Some("string") => v.is_string(),
            Some("integer") => v.is_i64() || v.is_u64(),
            Some("number") => v.is_number(),
            Some("boolean") => v.is_boolean(),
            Some("null") => v.is_null(),
            _ => true,
        };
        if !correct {
            return Err(fail("incorrect value type"));
        }
        if let Some(c) = s.get("const") {
            if v != c {
                return Err(fail("unexpected constant"));
            }
        }
        if let Some(a) = s.get("enum").and_then(Value::as_array) {
            if !a.contains(v) {
                return Err(fail("value not in enum"));
            }
        }
        if let Some(n) = v.as_f64() {
            if !n.is_finite() {
                return Err(fail("number must be finite"));
            }
            if s.get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|m| n < m)
                || s.get("maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|m| n > m)
                || s.get("exclusiveMinimum")
                    .and_then(Value::as_f64)
                    .is_some_and(|m| n <= m)
            {
                return Err(fail("number outside range"));
            }
        }
        if let Some(text) = v.as_str() {
            if s.get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|n| text.len() < (n as usize))
                || s.get("maxLength")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| text.len() > (n as usize))
            {
                return Err(fail("string length outside range"));
            }
            if s.get("format").and_then(Value::as_str) == Some("identifier")
                && !text
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
            {
                return Err(fail("invalid identifier"));
            }
        }
        if let Some(map) = v.as_object() {
            if let Some(required) = s.get("required").and_then(Value::as_array) {
                for name in required {
                    if !name.as_str().is_some_and(|n| map.contains_key(n)) {
                        return Err(fail("missing required field"));
                    }
                }
            }
            for (name, item) in map {
                if let Some(property) = s.get("properties").and_then(|p| p.get(name)) {
                    check(item, property, depth + 1)?
                } else if s.get("additionalProperties") == Some(&Value::Bool(false)) {
                    return Err(fail("unknown field"));
                }
            }
        }
        if let Some(a) = v.as_array() {
            if s.get("minItems")
                .and_then(Value::as_u64)
                .is_some_and(|n| a.len() < (n as usize))
                || s.get("maxItems")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| a.len() > (n as usize))
            {
                return Err(fail("array length outside range"));
            }
            if let Some(items) = s.get("items") {
                for item in a {
                    check(item, items, depth + 1)?
                }
            }
        }
        Ok(())
    }
    finite(value, 0)?;
    check(value, schema, 0)
}

macro_rules! identifier {
    ($($name:ident),*)=>{$(
        #[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
        #[serde(try_from="String",into="String")]
        pub struct $name(String);
        impl $name {pub fn new(value:impl Into<String>)->Result<Self,ProtocolError>{Self::try_from(value.into())} pub fn as_str(&self)->&str{&self.0}}
        impl TryFrom<String> for $name{type Error=ProtocolError;fn try_from(v:String)->Result<Self,Self::Error>{validate_ref(&json!(v),"Identifier")?;Ok(Self(v))}}
        impl From<$name> for String{fn from(v:$name)->String{v.0}}
    )*}
}
identifier!(ClientId, WorkerId, SessionId, ServiceId, DocumentId);
macro_rules! counter {
    ($($name:ident),*)=>{$(
        #[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
        #[serde(try_from="u64",into="u64")]
        pub struct $name(u64);
        impl $name{pub fn new(v:u64)->Result<Self,ProtocolError>{validate_ref(&json!(v),"Counter")?;Ok(Self(v))}pub fn get(self)->u64{self.0}}
        impl TryFrom<u64> for $name{type Error=ProtocolError;fn try_from(v:u64)->Result<Self,Self::Error>{Self::new(v)}}
        impl From<$name> for u64{fn from(v:$name)->u64{v.0}}
    )*}
}
counter!(RequestId, Revision, EventId);
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "f64")]
pub struct Finite(f64);
impl Finite {
    pub fn new(v: f64) -> Result<Self, ProtocolError> {
        if v.is_finite() {
            Ok(Self(v))
        } else {
            Err(ProtocolError("number must be finite".into()))
        }
    }
    pub fn get(self) -> f64 {
        self.0
    }
}
impl TryFrom<f64> for Finite {
    type Error = ProtocolError;
    fn try_from(v: f64) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl Serialize for Finite {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if !self.0.is_finite() {
            return Err(serde::ser::Error::custom("number must be finite"));
        }
        s.serialize_f64(self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "capabilities")]
    Capabilities,
    #[serde(rename = "commands.list")]
    CommandsList,
    #[serde(rename = "document.create")]
    DocumentCreate,
    #[serde(rename = "document.state")]
    DocumentState { document: DocumentId },
    #[serde(rename = "model.box")]
    Box {
        document: DocumentId,
        revision: Revision,
        size: [Finite; 3],
    },
    #[serde(rename = "model.fillet")]
    Fillet {
        document: DocumentId,
        revision: Revision,
        edge: String,
        radius: Finite,
    },
    #[serde(rename = "document.undo")]
    Undo {
        document: DocumentId,
        revision: Revision,
    },
    #[serde(rename = "document.redo")]
    Redo {
        document: DocumentId,
        revision: Revision,
    },
    #[serde(rename = "topology.edges")]
    Edges { document: DocumentId },
    #[serde(rename = "render.mesh")]
    Mesh { document: DocumentId },
    #[serde(rename = "export.native")]
    ExportNative { document: DocumentId, path: String },
}
impl Command {
    pub fn value(&self) -> Result<Value, ProtocolError> {
        let value = serde_json::to_value(self)?;
        validate(
            &value,
            &command_spec(
                value["op"]
                    .as_str()
                    .ok_or_else(|| ProtocolError("missing op".into()))?,
            )?["input_schema"],
        )?;
        Ok(value)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidRequest,
    InvalidParams,
    UnknownCommand,
    DocumentNotFound,
    EmptyDocument,
    RevisionConflict,
    InvalidReference,
    HistoryUnavailable,
    ValidationFailed,
    KernelError,
    IoError,
    InternalError,
    VersionMismatch,
    WrongInstance,
    WrongSession,
    ClientNotInitialized,
    RequestConflict,
    OutcomeUnavailable,
    OutcomeUnknown,
    ClientLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteError {
    pub code: String,
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub protocol: String,
    pub client_id: ClientId,
    pub worker_id: WorkerId,
    pub session_id: SessionId,
    pub service_id: Option<ServiceId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub kind: String,
    pub event_id: EventId,
    pub worker_id: WorkerId,
    pub session_id: SessionId,
    pub document: DocumentId,
    pub revision: Revision,
    pub client_id: ClientId,
    pub request_id: RequestId,
    pub command: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(flatten)]
    pub context: Context,
    pub request_id: RequestId,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RemoteError>,
    pub events: Vec<Event>,
    pub replayed: bool,
}
impl Response {
    pub fn parse(value: Value) -> Result<Self, ProtocolError> {
        validate_ref(&value, "Response")?;
        Ok(serde_json::from_value(value)?)
    }
    pub fn result_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, ProtocolError> {
        if !self.ok {
            return Err(ProtocolError(
                self.error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| "remote error".into()),
            ));
        }
        Ok(serde_json::from_value(
            self.result
                .clone()
                .ok_or_else(|| ProtocolError("missing result".into()))?,
        )?)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub protocol: String,
    pub method: String,
    pub request_id: RequestId,
    pub client_id: ClientId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<WorkerId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_request_id: Option<RequestId>,
}
impl Request {
    pub fn hello(client_id: ClientId) -> Result<Self, ProtocolError> {
        Ok(Self {
            protocol: version()?.into(),
            method: "hello".into(),
            request_id: RequestId::new(0)?,
            client_id,
            worker_id: None,
            session_id: None,
            command: None,
            target_request_id: None,
        })
    }
    pub fn execute(context: &Context, id: RequestId, command: Command) -> Self {
        Self {
            protocol: context.protocol.clone(),
            method: "execute".into(),
            request_id: id,
            client_id: context.client_id.clone(),
            worker_id: Some(context.worker_id.clone()),
            session_id: Some(context.session_id.clone()),
            command: Some(command),
            target_request_id: None,
        }
    }
    pub fn value(&self) -> Result<Value, ProtocolError> {
        let v = serde_json::to_value(self)?;
        validate_ref(&v, "Request")?;
        if let Some(c) = &self.command {
            c.value()?;
        }
        Ok(v)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentState {
    pub revision: Revision,
    pub units: String,
    pub empty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<Finite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faces: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedDocument {
    pub document: DocumentId,
    pub revision: Revision,
    pub units: String,
    pub empty: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentResult<T> {
    pub document: DocumentId,
    pub revision: Revision,
    pub data: T,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub reference: String,
    pub points: Vec<[Finite; 3]>,
    pub length: Finite,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Triangle {
    pub vertices: [usize; 3],
    pub face: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mesh {
    pub points: Vec<[Finite; 3]>,
    pub triangles: Vec<Triangle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Query,
    SessionWrite,
    DocumentWrite,
    ExternalWrite,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub id: String,
    pub effect: Effect,
    pub input_schema: Value,
    pub output_schema: Value,
    pub preconditions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub kernel: String,
    pub gpu_picking: bool,
    pub persistent_naming: bool,
    pub native_format: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum RequestStatus {
    #[serde(rename = "completed")]
    Completed { response: Box<Response> },
    #[serde(rename = "outcome_unavailable")]
    OutcomeUnavailable,
}
