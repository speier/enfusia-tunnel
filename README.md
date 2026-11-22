# ENFUSiA tunnel intro: ports

**▶ [Run it in your browser](https://speier.github.io/enfusia-tunnel/)**: no download.
Click to start, with sound on. The music is the same S3M replayer as the native build,
compiled to WebAssembly.

In April 2002 I (SPeY / ENFUSiA) wrote a small tunnel intro in Delphi: raw Win32,
OpenGL 1.1 immediate mode and BASSMOD playing a Scream Tracker module.

It shipped as **one self-contained 71,249-byte `ENFUSiA.exe`**. Everything it needed was
linked into the exe as Win32 resources:

| embedded resource | size |
|---|---|
| `bassmod.dll` (the module player) | 36,044 B |
| `music.s3m` (the tune) | 20,800 B |
| `tunel.bmp` (the tunnel texture) | 12,342 B |

On startup a small `Res2File` helper wrote `bassmod.dll` and `music.s3m` to disk next to
the exe. The intro then `LoadLibrary`'d BASSMOD, played the module, and `DeleteFile`'d
both files on exit. The texture never touched the disk: `LoadResTexture` read it
straight from the resource. So the user got one file to run, with nothing to install and
nothing left behind.

(The 19,968-byte `.old/winapi/ENFUSiA.exe` on the same backup is a different
experiment: rotating 3D "SPeY" text, a different tune and an external DLL. It is not
this intro.)

Found again 25 years later, it is now a collection of ports of that intro. Each one
keeps the same tunnel, vertex for vertex, and the same tune.

## The rule for every port

**One self-contained binary.** All assets and the music player live inside the
executable, and there are no external libraries to install. That is the same
embedded, portable approach the 2002 exe used. The operating system's own frameworks
(OpenGL, CoreAudio, AppKit, ALSA, Win32) are fine; anything else must be compiled in.

## Ports

| Port | Language / stack | Binary size | vs 2002 exe | Status |
|---|---|---|---|---|
| [original/](original/intro.pas) | Delphi + Win32 + OpenGL 1.1 + BASSMOD (DLL embedded) | **71,249 B** | 1× | the reference |
| [rust/](rust/) | Rust + miniquad + cpal + own S3M replayer | **654,000 B** stable, **377,544 B** nightly¹ | 9.2× / 5.3× | ✅ native (macOS, Linux, Windows), ✅ [web](https://speier.github.io/enfusia-tunnel/) (214,221 B wasm, 93,507 B gzipped) |
| C | C | – | – | 🗓 planned |
| Zig | Zig | – | – | 🗓 planned |
| asm | assembly | – | – | 🗓 planned |

¹ Stripped Linux x86_64 builds, each including 45,711 B of embedded assets. "Nightly"
is `rust/build-tiny.sh`. These were measured on Linux; a macOS build will differ a
little.

## Layout

```
assets/     shared by every port: music.s3m, tunel.bmp, scroller.nfo (the 2003
            ENFUSiA NFO art by Yce\*Remorse with demo text inside),
            cp437_8x16.bin (font bitmap), ENFUSiA.ico
original/   intro.pas, the 2002 Delphi source
rust/       the Rust port (Cargo project; embeds ../assets with include_bytes!);
            rust/web is the browser page around the wasm build (sources)
tools/      make_font.py + the Px437 TTF it bakes cp437_8x16.bin from
```

## The Rust port

### Build and run

```sh
cd rust && cargo run --release
```

That is all on macOS and Windows. On Linux, cpal needs the ALSA headers to build
(`apt install libasound2-dev`); at runtime the binary only uses the system's
`libasound`, like any other sound app.

To build the smallest binary, run `rust/build-tiny.sh`. It needs nightly and
rust-src, and gives the same output; a panic just aborts without printing a message.

### Controls

| Key | Action |
|---|---|
| click / tap, Enter or Space | start the intro (the start screen is there because audio needs a user gesture). Other keys, keys with Cmd / Ctrl / Alt held, key repeats and (in the browser) a key right after switching back to the window are ignored |
| Space (in the intro) | pause or resume the scroller only (it has its own clock; tunnel and music keep going) |
| F or Alt+Enter | toggle fullscreen (in the browser also a double-click or the corner button) |
| Esc | in the intro: back to the start screen (music, tunnel and scroller restart from the beginning next time); on the start screen: quit (native only) |

Developer options:
- `--size WxH`, `--fullscreen`
- `--freeze MS`: render a fixed frame.
- `--dump-pcm out.f32 --seconds N [--rate HZ] [--block FRAMES] [--s3m FILE]`: write the
  music as stereo f32 without opening an audio device, then exit.

### How close is it to 2002?

* **Tunnel:** the same math as `intro.pas`: 16 sides × 33 rings, angle = ms/14,
  `gluPerspective(45, aspect, 1, 100)` and the same texture scroll. Texture `v` is
  wrapped on the CPU so it stays precise after hours.
* **Music:** the original 20 KB `music.s3m` is embedded and played live by
  [`rust/src/s3m.rs`](rust/src/s3m.rs). It is a ~700-line S3M replayer that
  implements only what this module uses:
  - effects A, T, D, E, F, G and O, the volume column and note cut;
  - libopenmpt's sequencer rules (OpenMPT treats this file as saved by ModPlug
    Tracker 1.16) and its 32.32 fixed-point pitch math;
  - libopenmpt's 8-tap Kaiser-windowed sinc resampler (three tables);
  - libopenmpt's integer mixer: volume ramps, the fade-out voice for the old note,
    and pop reduction when a sample ends.

  The result is **bit-identical to libopenmpt 0.7.13**, which is the engine behind
  OpenMPT and chiptune.js, so it sounds the same:
  - 100.00% of samples are equal over 300 s, including two passes through the loop
    point.
  - It is also identical at 22.05, 44.1 and 96 kHz, with several callback sizes,
    and for every one of the 12 channels played solo.

  The song loops seamlessly, like `BASS_MUSIC_LOOP`. The replayer adds about 10 KB of
  code, and rendering takes about 1% of one CPU core.
* **Scroller:** ENFUSiA's 2003 NFO, ASCII art by Yce\*Remorse, kept byte for byte: the
  big logo, the side frame, the section headers and the bottom logo. Only the text
  inside the frame is new (about, credits, controls, news, greets). Its big logo is
  also the start screen.
* **Text:** IBM VGA 8×16 (CP437) font. The NFO block is always centred and scales in
  whole-pixel steps with the window height, HiDPI-aware.

### Binary size: what was tried

All Linux x86_64 release builds (`opt-level="z"`, LTO, 1 codegen unit, `panic=abort`,
stripped). The current builds embed 45,711 B of assets: s3m 20,800 + bmp 12,342 + NFO
8,473 + font 4,096. Variants A and B were measured with the earlier 2,090 B NFO (39,328 B
of assets).

| Variant | Size | Code (size − assets) | Self-contained? |
|---|---|---|---|
| A: macroquad + cpal, libopenmpt via dlopen | 1,126,144 B | ~1,087 KB | no, needs libopenmpt |
| B: bare miniquad + cpal, libopenmpt via dlopen | 633,056 B | ~594 KB | no, needs libopenmpt |
| B + libopenmpt (and libstdc++) linked statically | ≈ 2,358 KB | ~2,319 KB | yes, but 3.7× bigger |
| **C: bare miniquad + cpal + own replayer (default)** | **654,000 B** | ~606 KB | **yes** |
| **C, nightly `build-tiny.sh`** (std rebuilt, `panic=immediate-abort`) | **377,544 B** | ~330 KB | **yes** |

Where C's code goes (cargo-bloat, approximate):

| Crate | stable (.text ≈ 396 KB) | nightly tiny (.text ≈ 188 KB) |
|---|---|---|
| Rust std / core / alloc / hashbrown | 261 KB | 49 KB |
| miniquad | 66 KB | 87 KB (inlines more) |
| the intro + replayer | 32 KB (replayer ≈ 10 KB) | 30 KB |
| cpal (audio output) | 20 KB | 18 KB |
| pkg_config / alsa (Linux) | 11 KB | 2.5 KB |

**Recommendation:** variant C. It is the only small option that follows the rule, and
it adds only about 12 KB of code over B (which needed libopenmpt installed). It is 1.7 MB
smaller than linking libopenmpt in statically. Use `build-tiny.sh` for releases if
nightly is acceptable. Getting near the 71 KB original is a job for the C, Zig and asm
ports.

### Web

The same `main.rs` and `s3m.rs`, compiled to `wasm32-unknown-unknown`:

```sh
rust/build-web.sh                                         # -> rust/target/web/
python3 -m http.server 8080 --directory rust/target/web   # open http://localhost:8080/
```

* **One wasm, everything inside:** code, texture, font, NFO and `music.s3m`, embedded as in
  the native binary. The page only serves a few files:

  | file | size | gzip -9 |
  |---|---|---|
  | `enfusia-tunnel.wasm` | 214,221 B | 93,507 B |
  | `mq_js_bundle.js` (miniquad 0.4.11's `gl.js`, minified) | 31,541 B | 8,579 B |
  | `audio.js` + `audio-worklet.js` (the Web Audio glue) | 8,475 B | 3,124 B |
  | `index.html` (+ `favicon.ico`) | 5,954 B (+ 766 B) | 2,468 B (+ 469 B) |

* **Music:** the wasm exports `enf_audio_init(rate)` and `enf_audio_render(frames)`.
  - When the start-screen click or key reaches `Stage::start`, Rust calls into
    `audio.js`. Because that happens inside the browser's own user-gesture event, the
    AudioContext is allowed to start.
  - An AudioWorklet (`audio-worklet.js`) then runs its own instance of the same wasm,
    with the miniquad imports stubbed out. It renders each 128-frame block on the audio
    thread, so the music never waits on the render loop.
  - The wasm is fetched only once.
  - Browsers without AudioWorklet fall back to a ScriptProcessorNode on the page's own
    instance.
* **Same sound as native:** 30 s rendered through the real worklet in Chrome
  (OfflineAudioContext, at 44.1 and 48 kHz) are bit-identical to the native
  `--dump-pcm --block 128` render of the same player.
* **Controls:** as in the native build, plus touch.
  - A tap starts the intro and the music on phones and tablets.
* **Phones and tablets:** the layout follows the screen, in portrait too, and changes on
  rotation.
  - Text is sized by the window height, like the 1024x768 original, but never wider than
    the screen. The scroller fits the whole 79-column NFO across the width.
  - On a narrow screen the start screen splits its long lines. On a touch screen it says
    "tap to start" and leaves out the keyboard hints.
  - In portrait the tunnel keeps a 45° horizontal field of view, so it still fills the
    width.
  - The canvas renders at the device pixel ratio and fills the visible viewport
    (`position: fixed`, not `100vh`). The fullscreen button stays clear of notches
    (`viewport-fit=cover` + `env(safe-area-inset-*)`).
  - The native build uses the same layout code, so tall or narrow windows fit there too.
  - Fullscreen toggles with F, Alt+Enter, a double-click, or the small corner-bracket button in the
    top-right corner. The button appears when the mouse moves or the screen is touched,
    and hides after 2 s idle, so it also works without a keyboard. It is hidden where
    the page cannot go fullscreen, such as iPhone Safari.
  - All of this is handled in `index.html`, because browsers only allow fullscreen from
    a key, click or tap handler.
  - Esc in the intro goes back to the start screen, and the music stops at once:
    - The node is disconnected, the player is rewound and the AudioContext is
      suspended.
    - The next start resumes the same context and worklet. The node is reconnected
      only after the player confirms it is back at the first row, so the tune always
      starts from the top.
  - Esc on the start screen does nothing, because a page cannot close its tab.
  - In browser fullscreen, Esc only leaves fullscreen; the same keypress never also
    reaches the intro. A second Esc goes back to the start screen.
* **Cache-safe builds:** `build-web.sh` copies the page into `rust/target/web` and tags
  `audio.js`, `mq_js_bundle.js`, the worklet and the wasm with `?v=<content hash>`.
  GitHub Pages lets browsers cache files for 10 minutes. Without the tag, a cached
  `audio.js` from the previous build could run with the new wasm. That happened once:
  Esc went back to the start screen while the music kept playing, because the old
  `audio.js` had no stop function.
* **Deploy:** `.github/workflows/pages.yml` builds this on every push to `main` and
  publishes `rust/target/web` to GitHub Pages. No build output is committed.

## Credits

* **Code:** SPeY / ENFUSiA, 2002 and 2026.
* **NFO art:** ASCII by **Yce\*Remorse** (ENFUSiA 2k3 NFO, last mod. 07/15/03).
* **Music:** Aryx by **Karsten Koch**, 1995. It is not mine; all rights remain with its
  author. It is included because the original intro played it.
* **Font:** "Px437 IBM VGA 8x16" from The Ultimate Oldschool PC Font Pack by **VileR**
  (int10h.org), CC BY-SA 4.0.
* **Replayer:** `s3m.rs` is modelled on [OpenMPT / libopenmpt](https://lib.openmpt.org/)
  (BSD-3-Clause, © OpenMPT Project Developers and Contributors, © Olivier Lapicque).
* **Libraries:** [miniquad](https://github.com/not-fl3/miniquad) (MIT/Apache-2.0) and
  [cpal](https://github.com/RustAudio/cpal) (Apache-2.0).

## License

MIT (see [LICENSE](LICENSE)). This does not cover the music or the font. See the end
of the LICENSE file for those, and for OpenMPT's notice that applies to `s3m.rs`.
