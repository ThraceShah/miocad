#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Install once: rustup target add wasm32-unknown-unknown
# cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo build -p cad-web --lib --target wasm32-unknown-unknown --release --locked
mkdir -p artifacts/web/pkg
wasm-bindgen --target web --out-dir artifacts/web/pkg --out-name cad_web target/wasm32-unknown-unknown/release/cad_web.wasm
cp web/boot.js web/render-worker.js artifacts/web/
cargo build -p cad-web -p cad-service -p cad-cli -p cad-mcp --release --locked
