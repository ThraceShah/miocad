#![forbid(unsafe_code)]
pub use cad_protocol as protocol;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{ClientError, RpcClient, WorkerClient};
