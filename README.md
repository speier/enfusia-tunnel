# ENFUSiA tunnel intro: ports

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
| [rust/](rust/) | Rust + miniquad + cpal + own S3M replayer | **651,784 B** stable, **375,360 B** nightly¹ | 9.1× / 5.3× | ✅ native (macOS, Linux, Windows); web is TODO |
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
rust/       the Rust port (Cargo project; embeds ../assets with include_bytes!)
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
| click / any key | start the intro (the start screen is there because audio needs a user gesture) |
| Space | pause or resume the scroller only (it has its own clock; tunnel and music keep going) |
| F or Alt+Enter | toggle fullscreen |
| Esc | quit |

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
| **C: bare miniquad + cpal + own replayer (default)** | **651,784 B** | ~606 KB | **yes** |
| **C, nightly `build-tiny.sh`** (std rebuilt, `panic=immediate-abort`) | **375,360 B** | ~330 KB | **yes** |

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

`rust/build-web.sh` still compiles to WebAssembly and the tunnel renders, but there is
no music yet. See [rust/web/TODO.md](rust/web/TODO.md).

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
