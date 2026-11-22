// AudioWorklet side of web/audio.js: its own instance of enfusia-tunnel.wasm, used only
// for the S3M player exports (enf_audio_init / enf_audio_render).
"use strict";
class EnfusiaS3M extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.ex = null;
    this.frames = 0; this.sumsq = 0; this.peak = 0; this.nonzero = 0; this.sent = 0;
    const stub = new Proxy({}, { get: () => new Proxy({}, { get: () => () => 0 }) });
    WebAssembly.instantiate(options.processorOptions.bytes, stub)
      .then(({ instance }) => {
        if (!instance.exports.enf_audio_init(sampleRate)) throw new Error("bad module");
        this.ex = instance.exports;
        this.port.postMessage({ type: "ready", sampleRate });
      })
      .catch((e) => this.port.postMessage({ type: "error", message: String(e) }));
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    if (!this.ex || !out || !out[0]) return true;
    const n = out[0].length;
    const p = this.ex.enf_audio_render(n);
    const pcm = new Float32Array(this.ex.memory.buffer, p, 2 * n);
    const l = out[0], r = out[1] || null;
    for (let i = 0; i < n; i++) {
      const a = pcm[2 * i], b = pcm[2 * i + 1];
      if (r) { l[i] = a; r[i] = b; } else l[i] = 0.5 * (a + b);
      this.sumsq += a * a + b * b;
      const m = Math.max(Math.abs(a), Math.abs(b));
      if (m > this.peak) this.peak = m;
      if (m !== 0) this.nonzero++;
    }
    this.frames += n;
    if (this.frames - this.sent >= sampleRate / 2) { // a small status report twice a second
      this.sent = this.frames;
      this.port.postMessage({ type: "stats", frames: this.frames, nonzero: this.nonzero,
        rms: Math.sqrt(this.sumsq / (2 * this.frames)), peak: this.peak });
    }
    return true;
  }
}
registerProcessor("enfusia-s3m", EnfusiaS3M);
