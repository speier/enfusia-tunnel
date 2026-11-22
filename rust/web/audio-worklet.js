// AudioWorklet side of web/audio.js: its own instance of enfusia-tunnel.wasm, used only
// for the S3M player exports (enf_audio_init / enf_audio_render).
"use strict";
class EnfusiaS3M extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.ex = null;
    this.playing = options.processorOptions.playing !== false;
    this.rewound = true;  // the player is at the very beginning
    this.pos = 0;     // frames since the player was (re)created
    this.head = 0;    // sum of |samples| over the first second after (re)start, for tests
    this.frames = 0; this.sumsq = 0; this.peak = 0; this.nonzero = 0; this.sent = 0;
    const stub = new Proxy({}, { get: () => new Proxy({}, { get: () => () => 0 }) });
    WebAssembly.instantiate(options.processorOptions.bytes, stub)
      .then(({ instance }) => {
        if (!instance.exports.enf_audio_init(sampleRate)) throw new Error("bad module");
        this.ex = instance.exports;
        this.port.postMessage({ type: "ready", sampleRate });
      })
      .catch((e) => this.port.postMessage({ type: "error", message: String(e) }));
    this.port.onmessage = (e) => {
      if (e.data.type === "stop") {
        this.playing = false;
        this.rewind();
      } else if (e.data.type === "play") {
        if (!this.rewound) this.rewind();
        this.playing = true;
        this.port.postMessage({ type: "playing" }); // audio.js reconnects the node now
      }
    };
  }

  rewind() { // a fresh player: the tune starts again at order 0, row 0
    if (this.ex) this.ex.enf_audio_init(sampleRate);
    this.pos = 0; this.head = 0; this.rewound = true;
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    if (!this.ex || !this.playing || !out || !out[0]) return true; // outputs stay silent
    const n = out[0].length;
    this.rewound = false;
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
      if (this.pos + i < sampleRate) this.head += Math.abs(a) + Math.abs(b);
    }
    this.pos += n;
    this.frames += n;
    if (this.frames - this.sent >= sampleRate / 2) { // a small status report twice a second
      this.sent = this.frames;
      this.port.postMessage({ type: "stats", frames: this.frames, nonzero: this.nonzero,
        rms: Math.sqrt(this.sumsq / (2 * this.frames)), peak: this.peak,
        pos: this.pos, head: this.pos >= sampleRate ? this.head : null });
    }
    return true;
  }
}
registerProcessor("enfusia-s3m", EnfusiaS3M);
