#![forbid(unsafe_code)]
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
pub mod gateway;
