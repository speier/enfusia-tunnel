#!/usr/bin/env sh
# Build the browser version into ./target/web: index.html + mq_js_bundle.js + audio.js +
# audio-worklet.js + enfusia-tunnel.wasm (everything else, music included, is inside the
# wasm). Serve that folder with any static file server.
#
# The scripts and the wasm get a ?v=<content hash> tag, so a browser that still has the
# previous build cached can never mix an old audio.js with a new wasm.
set -e
cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build --release --target wasm32-unknown-unknown
OUT=target/web
rm -rf "$OUT" && mkdir -p "$OUT"
cp web/index.html web/mq_js_bundle.js web/audio.js web/audio-worklet.js web/favicon.ico "$OUT/"
cp target/wasm32-unknown-unknown/release/enfusia-tunnel.wasm "$OUT/"
V=$(cat "$OUT"/*.js "$OUT"/*.wasm | { sha256sum 2>/dev/null || shasum -a 256; } | cut -c1-12)
sed -i.bak -e "s|src=\"mq_js_bundle.js\"|src=\"mq_js_bundle.js?v=$V\"|" \
           -e "s|src=\"audio.js\"|src=\"audio.js?v=$V\"|" "$OUT/index.html"
rm -f "$OUT/index.html.bak"
grep -q "audio.js?v=$V" "$OUT/index.html"
echo "Built $OUT (build $V) - now run e.g.:  python3 -m http.server 8080 --directory $(pwd)/$OUT"
