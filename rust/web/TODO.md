# Web build: TODO

`../build-web.sh` still compiles the intro to `wasm32-unknown-unknown`
and `index.html` + `mq_js_bundle.js` load it, so the tunnel and the scroller render.
There is **no music** in the browser yet.

The plan: the S3M replayer (`../src/s3m.rs`) is plain Rust with no dependencies, so the
same code can be compiled into the wasm and fed to Web Audio (an AudioWorklet or a
ScriptProcessor). Native comes first for now.
