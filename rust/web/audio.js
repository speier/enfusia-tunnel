// ENFUSiA web audio. music.s3m is played by rust/src/s3m.rs, the same replayer as the
// native build, compiled into enfusia-tunnel.wasm. An AudioWorklet runs its own instance of
// that wasm and calls enf_audio_render() for every 128-frame block, so the music never
// waits on the render loop. Without AudioWorklet it falls back to a ScriptProcessorNode
// driven by the page's own instance.
"use strict";
const enfAudio = { bytes: null, ctx: null, node: null, mode: "", ready: false, playing: false, error: "", stats: null };

// The worklet instance only needs the player; stub every import miniquad's code would use.
function enfStubImports() {
  return new Proxy({}, { get: () => new Proxy({}, { get: () => () => 0 }) });
}

async function enfStartWorklet(ctx) {
  await ctx.audioWorklet.addModule("audio-worklet.js");
  const node = new AudioWorkletNode(ctx, "enfusia-s3m", {
    numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
    processorOptions: { bytes: enfAudio.bytes, playing: enfAudio.playing },
  });
  node.port.onmessage = (e) => {
    if (e.data.type === "ready") enfAudio.ready = true;
    else if (e.data.type === "stats") enfAudio.stats = e.data;
    else if (e.data.type === "error") enfAudio.error = e.data.message;
  };
  node.connect(ctx.destination);
  enfAudio.node = node;
  enfAudio.mode = "worklet";
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
  sp.connect(ctx.destination);
  enfAudio.node = sp;
  enfAudio.mode = "scriptprocessor";
  enfAudio.ready = true;
}

// Called from Rust (Stage::start) inside the click / key handler that starts the intro.
// The first call creates the AudioContext and the worklet; later calls reuse them.
function enfAudioStart() {
  enfAudio.playing = true;
  if (enfAudio.ctx) {
    if (enfAudio.ctx.state === "suspended") enfAudio.ctx.resume();
    if (enfAudio.mode === "worklet") enfAudio.node.port.postMessage({ type: "play" });
    return;
  }
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) { enfAudio.error = "no Web Audio"; return; }
  const ctx = new AC({ latencyHint: "playback" });
  enfAudio.ctx = ctx;
  ctx.resume();
  const fallback = (e) => {
    if (e) console.warn("AudioWorklet unavailable, using ScriptProcessor:", e);
    try { enfStartScriptProcessor(ctx); } catch (err) { enfAudio.error = String(err); }
  };
  if (ctx.audioWorklet && enfAudio.bytes) enfStartWorklet(ctx).catch(fallback);
  else fallback();
}

// Called from Rust (Stage::stop) when Esc leaves the intro: silence, and rewind the player
// right away so the next start begins at the first row again.
function enfAudioStop() {
  enfAudio.playing = false;
  if (enfAudio.mode === "worklet") enfAudio.node.port.postMessage({ type: "stop" });
  else if (enfAudio.mode === "scriptprocessor") wasm_exports.enf_audio_init(enfAudio.ctx.sampleRate);
}

// Some browsers only count touchend / click as a gesture: resume there as well.
for (const ev of ["click", "touchend", "keyup"]) {
  window.addEventListener(ev, () => {
    if (enfAudio.ctx && enfAudio.ctx.state === "suspended") enfAudio.ctx.resume();
  }, true);
}

miniquad_add_plugin({
  name: "enfusia_audio",
  version: 1,
  register_plugin: (importObject) => {
    importObject.env.enf_audio_start = enfAudioStart;
    importObject.env.enf_audio_stop = enfAudioStop;
  },
});

// Fetch the wasm once: miniquad runs it from a blob URL, the worklet gets the same bytes.
function enfLoad(url) {
  fetch(url)
    .then((r) => { if (!r.ok) throw new Error(url + ": HTTP " + r.status); return r.arrayBuffer(); })
    .then((bytes) => {
      enfAudio.bytes = bytes;
      load(URL.createObjectURL(new Blob([bytes], { type: "application/wasm" })));
    })
    .catch((e) => console.error(e));
}
