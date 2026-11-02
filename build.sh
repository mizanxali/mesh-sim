#!/usr/bin/env bash
# Builds the engine for the browser into web/pkg. The wasm-bindgen CLI must be
# exactly the crate's version:
#   cargo install wasm-bindgen-cli --version 0.2.105 --locked
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/mesh_sim.wasm
echo "built web/pkg; serve with: python3 -m http.server -d web 8080"
