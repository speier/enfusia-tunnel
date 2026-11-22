// ENFUSiA web audio. music.s3m is played by rust/src/s3m.rs, the same replayer as the
// native build, compiled into enfusia-tunnel.wasm. An AudioWorklet runs its own instance of
// that wasm and calls enf_audio_render() for every 128-frame block, so the music never
// waits on the render loop. Without AudioWorklet it falls back to a ScriptProcessorNode
// driven by the page's own instance.
//
// Start / stop (Esc) reuse one AudioContext and one node:
//   stop:  disconnect the node (silent at once, whatever the audio thread is doing), tell
//          the player to pause and rewind, suspend the context.
//   start: resume the context, tell the player to play; reconnect only when it confirms,
//          so no old samples can leak out.
"use strict";
const enfAudio = { bytes: null, ctx: null, node: null, mode: "", ready: false, playing: false,
  connected: false, error: "", stats: null };
// build-web.sh loads this file as audio.js?v=<build hash>; load the wasm and the worklet
// with the same tag so a browser cache can never mix two builds.
const ENF_V = (document.currentScript && new URL(document.currentScript.src).search) || "";

// e.g. no audio output device: the intro still runs, just silently
function enfNoDevice(e) { enfAudio.error = String(e); }

function enfConnect(on) {
  const n = enfAudio.node;
  if (!n || on === enfAudio.connected) return;
  if (on) n.connect(enfAudio.ctx.destination); else n.disconnect();
  enfAudio.connected = on;
}

async function enfStartWorklet(ctx) {
  await ctx.audioWorklet.addModule("audio-worklet.js" + ENF_V);
  const node = new AudioWorkletNode(ctx, "enfusia-s3m", {
    numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
    processorOptions: { bytes: enfAudio.bytes, playing: enfAudio.playing },
  });
  node.port.onmessage = (e) => {
    const t = e.data.type;
    if (t === "ready") enfAudio.ready = true;
    else if (t === "playing") enfConnect(enfAudio.playing); // the player has rewound and runs
    else if (t === "stats") enfAudio.stats = e.data;
    else if (t === "error") enfAudio.error = e.data.message;
  };
  enfAudio.node = node;
  enfAudio.mode = "worklet";
  enfConnect(enfAudio.playing);
}

function enfStartScriptProcessor(ctx) {
  if (!wasm_exports.enf_audio_init(ctx.sampleRate)) throw new Error("bad module");
  const sp = ctx.createScriptProcessor(4096, 0, 2);
  sp.onaudioprocess = (e) => {
    const l = e.outputBuffer.getChannelData(0), r = e.outputBuffer.getChannelData(1), n = l.length;
    if (!enfAudio.playing) { l.fill(0); r.fill(0); return; }
    const p = wasm_exports.enf_audio_render(n);
    const pcm = new Float32Array(wasm_memory.buffer, p, 2 * n);
    for (let i = 0; i < n; i++) { l[i] = pcm[2 * i]; r[i] = pcm[2 * i + 1]; }
  };
  enfAudio.node = sp;
  enfAudio.mode = "scriptprocessor";
  enfAudio.ready = true;
  enfConnect(enfAudio.playing);
}

// Called from Rust (Stage::start) inside the click / key / tap handler that starts the intro.
// The first call creates the AudioContext and the node; later calls reuse them.
function enfAudioStart() {
  enfAudio.playing = true;
  const ctx = enfAudio.ctx;
  if (ctx) {
    ctx.resume().catch(enfNoDevice);
    if (enfAudio.mode === "worklet") enfAudio.node.port.postMessage({ type: "play" });
    else enfConnect(true); // ScriptProcessor: rewound in enfAudioStop already
    return;
  }
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) { enfAudio.error = "no Web Audio"; return; }
  enfAudio.ctx = new AC({ latencyHint: "playback" });
  enfAudio.ctx.resume().catch(enfNoDevice);
  const fallback = (e) => {
    if (e) console.warn("AudioWorklet unavailable, using ScriptProcessor:", e);
    try { enfStartScriptProcessor(enfAudio.ctx); } catch (err) { enfAudio.error = String(err); }
  };
  if (enfAudio.ctx.audioWorklet && enfAudio.bytes) enfStartWorklet(enfAudio.ctx).catch(fallback);
  else fallback();
}

// Called from Rust (Stage::stop) when Esc leaves the intro.
function enfAudioStop() {
  enfAudio.playing = false;
  if (!enfAudio.ctx) return;
  enfConnect(false);
  if (enfAudio.mode === "worklet") enfAudio.node.port.postMessage({ type: "stop" });
  else if (enfAudio.mode === "scriptprocessor") wasm_exports.enf_audio_init(enfAudio.ctx.sampleRate);
  enfAudio.ctx.suspend().catch(enfNoDevice);
}

// Some browsers only count touchend / click as a gesture: resume there as well.
for (const ev of ["click", "touchend", "keyup"]) {
  window.addEventListener(ev, () => {
    if (enfAudio.playing && enfAudio.ctx && enfAudio.ctx.state === "suspended") enfAudio.ctx.resume().catch(enfNoDevice);
  }, true);
}

miniquad_add_plugin({
  name: "enfusia_audio",
  version: 2, // = enfusia_audio_crate_version() in main.rs; miniquad logs an error on mismatch
  register_plugin: (importObject) => {
    importObject.env.enf_audio_start = enfAudioStart;
    importObject.env.enf_audio_stop = enfAudioStop;
  },
});

// Fetch the wasm once: miniquad runs it from a blob URL, the worklet gets the same bytes.
function enfLoad(url) {
  url += ENF_V;
  fetch(url)
    .then((r) => { if (!r.ok) throw new Error(url + ": HTTP " + r.status); return r.arrayBuffer(); })
    .then((bytes) => {
      enfAudio.bytes = bytes;
      load(URL.createObjectURL(new Blob([bytes], { type: "application/wasm" })));
    })
    .catch((e) => console.error(e));
}
