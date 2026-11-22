#!/usr/bin/env sh
# Build the WebGL/WASM version into ./web (serve that folder with any static file server).
set -e
cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build --release --target wasm32-unknown-unknown
# NOTE: no music in the browser yet, see web/TODO.md
cp target/wasm32-unknown-unknown/release/enfusia-tunnel.wasm web/
echo "Built web/enfusia-tunnel.wasm - now run e.g.:  python3 -m http.server 8080 --directory web"
