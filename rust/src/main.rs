//! ENFUSiA tunnel intro
//!
//! Original: SPeY / ENFUSiA, April 2002 -- Delphi + OpenGL 1.1 + BASSMOD (`intro.pas`).
//! This port: Rust on bare miniquad (OpenGL / WebGL) + cpal (native) or Web Audio (wasm).
//!
//! Everything is embedded in the binary, just like the original single-file exe:
//! the tunnel texture (`tunel.bmp`), the scroller text, a 4 KB CP437 font and the original
//! 20 KB `music.s3m` (Aryx by Karsten Koch, 1995), played live by our own tiny S3M
//! replayer (`s3m.rs`), which follows libopenmpt sample for sample. No external libraries.

use miniquad::*;
use std::sync::atomic::{AtomicBool, Ordering};

mod s3m;

// --- embedded resources (the original linked these in via ENFUSiA.rc) ---
const TUNEL_BMP: &[u8] = include_bytes!("../../assets/tunel.bmp");
/// The original tune, played live (BASSMOD in 2002, s3m.rs now).
const MUSIC_S3M: &[u8] = include_bytes!("../../assets/music.s3m");
/// Scroller text, CP437: the 2003 ENFUSiA NFO art by Yce*Remorse, with demo text inside.
const NFO: &[u8] = include_bytes!("../../assets/scroller.nfo");
/// CP437 font, 256 glyphs x 16 rows, 1 bit per pixel (MSB = left).
const FONT: &[u8; 4096] = include_bytes!("../../assets/cp437_8x16.bin");

// --- constants straight from intro.pas ---
const TUNNEL_SPEED: f64 = 1.0 / 50.0;
const PI_8: f64 = std::f64::consts::PI / 8.0;
const SIDES: usize = 16; // I := 0 to 15 (17 rows of vertices, the last closes the tube)
const DEPTH: usize = 32; // J := 0 to 31 (33 rings)
const FADE_IN_MS: f64 = 500.0;

// --- text ---
const GLYPH_W: f32 = 8.0;
const GLYPH_H: f32 = 16.0;
const SCROLL_PX_PER_SEC: f32 = 64.0; // at 1x text scale: ~4 lines/s, like the 2002 original
/// Text is sized so the screen looks like the original 1024x768 at any resolution.
const REF_HEIGHT: f32 = 768.0;
/// Lines 1..11 of scroller.nfo are the big ENFUSiA logo, reused on the start screen.
const LOGO: std::ops::Range<usize> = 0..11;
const MUSIC_CREDIT: &[u8] = b"music: Aryx by Karsten Koch, 1995";
const MAX_QUADS: usize = 16_000;

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const LOGO_BLUE: [f32; 4] = [0.55, 0.7, 1.0, 1.0];
const GREY: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
const DARK: [f32; 4] = [0.35, 0.35, 0.35, 1.0];

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

const VERTEX: &str = r#"#version 100
attribute vec3 in_pos;
attribute vec2 in_uv;
attribute vec4 in_color;
uniform mat4 mvp;
varying lowp vec4 color;
varying mediump vec2 uv;
void main() {
    gl_Position = mvp * vec4(in_pos, 1.0);
    color = in_color;
    uv = in_uv;
}"#;

const FRAGMENT: &str = r#"#version 100
precision mediump float;
varying lowp vec4 color;
varying mediump vec2 uv;
uniform sampler2D tex;
void main() {
    gl_FragColor = texture2D(tex, uv) * color;
}"#;

/// gluPerspective, column-major.
fn perspective(fovy_deg: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fovy_deg.to_radians() / 2.0).tan();
    let nf = near - far;
    [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, (far + near) / nf, -1.0,
        0.0, 0.0, 2.0 * far * near / nf, 0.0,
    ]
}

/// Pixel-space ortho, origin top-left, column-major.
fn ortho(w: f32, h: f32) -> [f32; 16] {
    [
        2.0 / w, 0.0, 0.0, 0.0,
        0.0, -2.0 / h, 0.0, 0.0,
        0.0, 0.0, -1.0, 0.0,
        -1.0, 1.0, 0.0, 1.0,
    ]
}

/// Minimal 24-bit BMP loader, like LoadTexture in the original: rows are kept bottom-up,
/// which is exactly how glTexImage2D received them in 2002, so the texture coordinates match.
fn load_bmp_rgba(bmp: &[u8]) -> (u32, u32, Vec<u8>) {
    let u32_at = |o: usize| u32::from_le_bytes(bmp[o..o + 4].try_into().unwrap());
    let offset = u32_at(10) as usize;
    let (w, h) = (u32_at(18) as usize, (u32_at(22) as i32).unsigned_abs() as usize);
    let stride = (w * 3 + 3) & !3;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let p = offset + y * stride + x * 3;
            rgba.extend_from_slice(&[bmp[p + 2], bmp[p + 1], bmp[p], 255]); // BGR -> RGB (SwapRGB)
        }
    }
    (w as u32, h as u32, rgba)
}

/// Expand the 1-bit font into a 128x256 white-on-transparent atlas (16x16 glyphs).
fn font_atlas() -> Vec<u8> {
    let mut rgba = vec![0u8; 128 * 256 * 4];
    for g in 0..256 {
        let (gx, gy) = ((g % 16) * 8, (g / 16) * 16);
        for r in 0..16 {
            let bits = FONT[g * 16 + r];
            for x in 0..8 {
                if bits & (0x80 >> x) != 0 {
                    let p = ((gy + r) * 128 + gx + x) * 4;
                    rgba[p..p + 4].copy_from_slice(&[255; 4]);
                }
            }
        }
    }
    rgba
}

/// procedure drawDemo -- the tunnel, vertex for vertex (texture coordinates use GL_REPEAT,
/// so the ever-growing v is wrapped to [0,1) on the CPU to keep shader precision).
fn tunnel_vertices(demo_ms: f64, out: &mut Vec<Vertex>) {
    let angle = demo_ms / 14.0;
    let a1 = (angle / 27.0).sin();
    let a2 = (angle / 25.0).cos();
    let a3 = (angle / 13.0).cos();
    let a4 = (angle / 17.0).sin();

    let mut tunnels = [[[0f32; 3]; DEPTH + 1]; SIDES + 1];
    for (i, ring) in tunnels.iter_mut().enumerate() {
        let x = (PI_8 * i as f64).cos();
        let y = (PI_8 * i as f64).sin();
        for (j, v) in ring.iter_mut().enumerate() {
            let jf = j as f64;
            let k = angle + 2.0 * jf;
            let r = 3.0 - jf / 12.0;
            let vx = r * x + 2.0 * (k / 27.0).sin() + (k / 13.0).cos() - 2.0 * a1 - a3;
            let vy = r * y + 2.5 * (k / 25.0).cos() + (k / 17.0).sin() - 2.0 * a2 - a4;
            *v = [vx as f32, vy as f32, (-jf - 2.0) as f32]; // glTranslatef(0, 0, -2)
        }
    }

    out.clear();
    let v_off = (angle * TUNNEL_SPEED).fract();
    for j in 0..DEPTH {
        let j1 = (j as f64 / 32.0 + v_off) as f32;
        let j2 = ((j + 1) as f64 / 32.0 + v_off) as f32;
        // near the end of the tunnel, fade the effect away
        let mut c = if j > 24 { 1.0 - (j as f64 - 24.0) / 10.0 } else { 1.0 };
        // fade in tunnel
        if demo_ms < FADE_IN_MS {
            c *= demo_ms / FADE_IN_MS;
        }
        let color = [c as f32, c as f32, c as f32, 1.0];
        for i in 0..SIDES {
            let u0 = (i as f32 - 1.0) / 8.0;
            let u1 = i as f32 / 8.0;
            for (pos, uv) in [
                (tunnels[i][j], [u0, j1]),
                (tunnels[i + 1][j], [u1, j1]),
                (tunnels[i + 1][j + 1], [u1, j2]),
                (tunnels[i][j + 1], [u0, j2]),
            ] {
                out.push(Vertex { pos, uv, color });
            }
        }
    }
}

fn quad_indices(quads: usize) -> Vec<u16> {
    (0..quads as u16)
        .flat_map(|q| {
            let b = q * 4;
            [b, b + 1, b + 2, b, b + 2, b + 3]
        })
        .collect()
}

/// Text layout in physical pixels. Glyph cells are a whole number of pixels so the CP437
/// block characters tile without seams, and they scale with the window height so the screen
/// looks like the 1024x768 original at any size (HiDPI / retina included).
struct Text {
    w: f32, // glyph cell width, px
    h: f32, // glyph cell height, px
    s: f32, // font pixel -> screen px
    quads: Vec<Vertex>,
}

impl Text {
    fn new(screen_w: f32, screen_h: f32) -> Self {
        let mut t = Self { w: GLYPH_W, h: GLYPH_H, s: 1.0, quads: Vec::new() };
        t.fit(screen_w, screen_h, 80, 0.0);
        t
    }

    /// Size the glyphs like the 1024x768 original (by window height), but never so big that
    /// `cols` columns overflow the width or `rows` rows the height (phones in portrait, odd
    /// window shapes; `rows` 0 = no height limit). Below 1:1 only when that is needed to fit.
    fn fit(&mut self, screen_w: f32, screen_h: f32, cols: usize, rows: f32) {
        let cols = cols.max(1) as f32;
        let fit_w = screen_w / (cols * GLYPH_W);
        let fit_h = screen_h / (rows * GLYPH_H);
        let s = (screen_h / REF_HEIGHT).max(fit_w.min(1.0)).min(fit_w).min(fit_h).max(0.25);
        self.set_scale(s, screen_w / cols);
    }

    /// Whole-pixel 1:2 glyph cells for font scale `s`, at most `max_w` px wide.
    fn set_scale(&mut self, s: f32, max_w: f32) {
        let w = (GLYPH_W * s).round().max(2.0);
        if w <= max_w {
            self.w = w;
            self.h = (GLYPH_H * s).round().max(4.0);
        } else {
            self.w = max_w.floor().max(2.0);
            self.h = 2.0 * self.w;
        }
        self.s = s;
    }

    fn width(&self, line: &[u8]) -> f32 {
        line.len() as f32 * self.w
    }

    fn line(&mut self, x: f32, y: f32, line: &[u8], color: [f32; 4]) {
        let (x, y) = (x.round(), y.round());
        for (k, &b) in line.iter().enumerate() {
            if b <= b' ' || self.quads.len() / 4 >= MAX_QUADS {
                continue;
            }
            let (u0, v0) = ((b % 16) as f32 / 16.0, (b / 16) as f32 / 16.0);
            let (u1, v1) = (u0 + 1.0 / 16.0, v0 + 1.0 / 16.0);
            let (x0, x1, y1) = (x + k as f32 * self.w, x + (k + 1) as f32 * self.w, y + self.h);
            for (px, py, u, v) in [(x0, y, u0, v0), (x1, y, u1, v0), (x1, y1, u1, v1), (x0, y1, u0, v1)] {
                self.quads.push(Vertex { pos: [px, py, 0.0], uv: [u, v], color });
            }
        }
    }

    fn centered(&mut self, screen_w: f32, y: f32, line: &[u8], color: [f32; 4]) {
        let x = (screen_w - self.width(line)) / 2.0;
        self.line(x, y, line, color);
    }
}

fn nfo_lines() -> Vec<&'static [u8]> {
    let nfo = NFO.strip_suffix(b"\x1a").unwrap_or(NFO); // DOS end-of-file marker
    let nfo = nfo.strip_suffix(b"\r\n").unwrap_or(nfo);
    nfo.split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .map(|l| &l[..l.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1)])
        .collect()
}

/// The ShowScroller the original meant to write: the text glides upward, forever, as one
/// block centred horizontally (lines keep their relative alignment so the art stays intact).
fn layout_scroller(t: &mut Text, lines: &[&[u8]], scroll_ms: f64, sw: f32, sh: f32) {
    // the whole 79-column frame always fits the width (phones in portrait get smaller text)
    t.fit(sw, sh, lines.iter().map(|l| l.len()).max().unwrap_or(80), 0.0);
    let block_w = lines.iter().map(|l| t.width(l)).fold(0.0, f32::max);
    let x = ((sw - block_w) / 2.0).max(0.0);
    let cycle = sh + lines.len() as f32 * t.h;
    let offset = ((scroll_ms / 1000.0) as f32 * SCROLL_PX_PER_SEC * t.s) % cycle;
    for (i, line) in lines.iter().enumerate() {
        let y = sh - offset + i as f32 * t.h;
        if y > -t.h && y < sh {
            t.line(x, y, line, WHITE);
        }
    }
}

/// The "click to start" screen (browsers need a user gesture before audio, too).
/// The "click to start" screen (browsers need a user gesture before audio, too). On a narrow
/// screen (a phone in portrait) the long lines are split so the text can stay large; on a
/// touch screen the hints talk about tapping and the keyboard-only line is left out.
fn layout_start_screen(t: &mut Text, lines: &[&[u8]], time: f64, sw: f32, sh: f32) {
    let touch = TOUCH.load(Ordering::Relaxed);
    let compact = sw < 54.0 * GLYPH_W * (sh / REF_HEIGHT).max(1.0);
    let mut text: Vec<(&[u8], [f32; 4])> = vec![(b"tunnel intro  -  2002 by SPeY / ENFUSiA", WHITE)];
    if compact {
        text.push((b"Delphi + OpenGL + BASSMOD  >>", GREY));
        text.push((b"ported to Rust in 2026", GREY));
    } else {
        text.push((b"Delphi + OpenGL + BASSMOD  >>  ported to Rust in 2026", GREY));
    }
    text.push((MUSIC_CREDIT, GREY));
    text.push((b"", WHITE));
    let prompt: &[u8] = match (touch, compact, cfg!(target_arch = "wasm32")) {
        (true, _, true) => b"[ tap to start  -  sound on ]",
        (true, _, false) => b"[ tap to start ]",
        (false, false, true) => b"[ click or press enter to start  -  sound on ]",
        (false, true, true) => b"[ click or enter to start  -  sound on ]",
        (false, _, false) => b"[ click or press enter to start ]",
    };
    let blink_on = (time * 2.0) as i64 % 2 == 0;
    text.push((if blink_on { prompt } else { b"" }, WHITE));
    if !touch {
        text.push((b"", WHITE));
        let hint: &[u8] = match (compact, cfg!(target_arch = "wasm32")) {
            (false, true) => b"Space: pause scroller   F: fullscreen   Esc: back",
            (false, false) => b"Space: pause scroller   F: fullscreen   Esc: quit",
            (true, true) => b"Space: pause  F: fullscreen  Esc: back",
            (true, false) => b"Space: pause  F: fullscreen  Esc: quit",
        };
        text.push((hint, DARK));
    }

    // logo: as wide as fits; text: fits on its own, at most 1.6x the logo's scale
    let logo = &lines[LOGO.start..LOGO.end.min(lines.len())];
    let logo_cols = logo.iter().map(|l| l.len()).max().unwrap_or(1);
    let text_cols = text.iter().map(|(l, _)| l.len()).chain([prompt.len()]).max().unwrap_or(1);
    let rows = (logo.len() + 1 + text.len()) as f32;
    t.fit(sw, sh, logo_cols, rows);
    let (logo_s, logo_h) = (t.s, logo.len() as f32 * t.h);
    let logo_x = (sw - logo_cols as f32 * t.w) / 2.0;
    let mut text_t = Text { w: t.w, h: t.h, s: t.s, quads: Vec::new() };
    // same height rule as the logo, but the text rows must fit below it
    let text_rows = (text.len() + 1) as f32 * sh / (sh - logo_h).max(1.0);
    text_t.fit(sw, sh, text_cols, text_rows);
    if text_t.s > logo_s * 1.6 {
        text_t.set_scale(logo_s * 1.6, sw / text_cols as f32);
    }
    let total = logo_h + (1 + text.len()) as f32 * text_t.h;
    let mut y = ((sh - total) / 2.0).max(0.0);
    for line in logo {
        t.line(logo_x, y, line, LOGO_BLUE);
        y += t.h;
    }
    y += text_t.h;
    let (w, h) = (t.w, t.h);
    t.w = text_t.w;
    t.h = text_t.h;
    for (line, color) in text {
        t.centered(sw, y, line, color);
        y += t.h;
    }
    (t.w, t.h) = (w, h);
}

/// A touch screen has been seen (web: reported by the page, see audio.js).
static TOUCH: AtomicBool = AtomicBool::new(false);

struct Stage {
    ctx: Box<dyn RenderingBackend>,
    tunnel_pipe: Pipeline,
    text_pipe: Pipeline,
    tunnel_bind: Bindings,
    text_bind: Bindings,
    lines: Vec<&'static [u8]>,
    verts: Vec<Vertex>,
    #[cfg(not(target_arch = "wasm32"))]
    stream: Option<cpal::Stream>,
    started: bool,
    start_time: f64,
    scroll_ms: f64,
    scroll_paused: bool,
    last: f64,
    #[cfg(not(target_arch = "wasm32"))]
    fullscreen: bool,
    /// --freeze MS: draw the intro at a fixed time (for screenshots)
    freeze: Option<f64>,
}

impl Stage {
    fn new(fullscreen: bool, freeze: Option<f64>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = fullscreen; // the page handles fullscreen in the browser
        let mut ctx = window::new_rendering_backend();

        let (tw, th, rgba) = load_bmp_rgba(TUNEL_BMP);
        // GL_REPEAT + GL_LINEAR as in 2002 (its mipmaps were never sampled: MIN_FILTER = LINEAR)
        let tunnel_tex = ctx.new_texture_from_data_and_format(
            &rgba,
            TextureParams {
                width: tw,
                height: th,
                format: TextureFormat::RGBA8,
                wrap: TextureWrap::Repeat,
                min_filter: FilterMode::Linear,
                mag_filter: FilterMode::Linear,
                ..Default::default()
            },
        );
        let font_tex = ctx.new_texture_from_data_and_format(
            &font_atlas(),
            TextureParams {
                width: 128,
                height: 256,
                format: TextureFormat::RGBA8,
                min_filter: FilterMode::Nearest,
                mag_filter: FilterMode::Nearest,
                ..Default::default()
            },
        );

        let shader = ctx
            .new_shader(
                ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT },
                ShaderMeta {
                    images: vec!["tex".to_string()],
                    uniforms: UniformBlockLayout { uniforms: vec![UniformDesc::new("mvp", UniformType::Mat4)] },
                },
            )
            .expect("shader");
        let attrs = [
            VertexAttribute::new("in_pos", VertexFormat::Float3),
            VertexAttribute::new("in_uv", VertexFormat::Float2),
            VertexAttribute::new("in_color", VertexFormat::Float4),
        ];
        let tunnel_pipe = ctx.new_pipeline(
            &[BufferLayout::default()],
            &attrs,
            shader,
            PipelineParams { depth_test: Comparison::Less, depth_write: true, ..Default::default() },
        );
        let text_pipe = ctx.new_pipeline(
            &[BufferLayout::default()],
            &attrs,
            shader,
            PipelineParams {
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
        );

        let tunnel_vb = ctx.new_buffer(BufferType::VertexBuffer, BufferUsage::Stream, BufferSource::empty::<Vertex>(DEPTH * SIDES * 4));
        let tunnel_ib = ctx.new_buffer(BufferType::IndexBuffer, BufferUsage::Immutable, BufferSource::slice(&quad_indices(DEPTH * SIDES)));
        let text_vb = ctx.new_buffer(BufferType::VertexBuffer, BufferUsage::Stream, BufferSource::empty::<Vertex>(MAX_QUADS * 4));
        let text_ib = ctx.new_buffer(BufferType::IndexBuffer, BufferUsage::Immutable, BufferSource::slice(&quad_indices(MAX_QUADS)));

        let now = date::now();
        let mut stage = Stage {
            tunnel_bind: Bindings { vertex_buffers: vec![tunnel_vb], index_buffer: tunnel_ib, images: vec![tunnel_tex] },
            text_bind: Bindings { vertex_buffers: vec![text_vb], index_buffer: text_ib, images: vec![font_tex] },
            ctx,
            tunnel_pipe,
            text_pipe,
            lines: nfo_lines(),
            verts: Vec::with_capacity(MAX_QUADS * 4),
            #[cfg(not(target_arch = "wasm32"))]
            stream: None,
            started: false,
            start_time: now,
            scroll_ms: 0.0,
            scroll_paused: false,
            last: now,
            #[cfg(not(target_arch = "wasm32"))]
            fullscreen,
            freeze,
        };
        if freeze.is_some() {
            stage.started = true;
        }
        stage
    }

    fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.start_time = date::now();
        self.last = self.start_time;
        // BASS_MUSIC_LOOP: the replayer loops the module itself, seamlessly.
        #[cfg(not(target_arch = "wasm32"))]
        match music::play(MUSIC_S3M) {
            Ok(s) => self.stream = Some(s),
            Err(e) => eprintln!("music disabled: {e}"),
        }
        // the browser plays it from an AudioWorklet (see web/audio.js); this call runs inside
        // the click / key handler, so the browser lets the AudioContext start
        #[cfg(target_arch = "wasm32")]
        web_audio::start();
        #[cfg(not(target_arch = "wasm32"))]
        window::show_mouse(false);
    }

    /// Esc during the intro: back to the start screen. Music, tunnel clock and scroller all
    /// start from the beginning next time.
    fn stop(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        self.freeze = None;
        self.scroll_ms = 0.0;
        self.scroll_paused = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.stream = None; // dropping the stream stops it; start() makes a new player
            window::show_mouse(true);
        }
        #[cfg(target_arch = "wasm32")]
        web_audio::stop();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        window::set_fullscreen(self.fullscreen);
    }
}

impl EventHandler for Stage {
    fn update(&mut self) {}

    fn draw(&mut self) {
        let (sw, sh) = window::screen_size(); // physical pixels
        let now = date::now();
        let (demo_ms, scroll_ms) = match self.freeze {
            Some(ms) => (ms, ms),
            None => {
                if self.started && !self.scroll_paused {
                    self.scroll_ms += ((now - self.last) * 1000.0).min(100.0); // no jump after a stall
                }
                self.last = now;
                ((now - self.start_time) * 1000.0, self.scroll_ms)
            }
        };

        self.ctx.begin_default_pass(PassAction::clear_color(0.0, 0.0, 0.0, 1.0));
        if self.started {
            tunnel_vertices(demo_ms, &mut self.verts);
            self.ctx.buffer_update(self.tunnel_bind.vertex_buffers[0], BufferSource::slice(&self.verts));
            self.ctx.apply_pipeline(&self.tunnel_pipe);
            self.ctx.apply_bindings(&self.tunnel_bind);
            // gluPerspective(45.0, Width / Height, 1.0, 100.0)
            let aspect = sw / sh.max(1.0);
            // in portrait (phones) widen the vertical angle so the tube still fills the width
            let fovy = if aspect < 1.0 { 2.0 * ((22.5f32).to_radians().tan() / aspect).atan().to_degrees() } else { 45.0 };
            self.ctx.apply_uniforms(UniformsSource::table(&perspective(fovy, aspect, 1.0, 100.0)));
            self.ctx.draw(0, (DEPTH * SIDES * 6) as i32, 1);
        }

        let mut t = Text::new(sw, sh);
        if self.started {
            layout_scroller(&mut t, &self.lines, scroll_ms, sw, sh);
        } else {
            layout_start_screen(&mut t, &self.lines, now, sw, sh);
        }
        if !t.quads.is_empty() {
            self.ctx.buffer_update(self.text_bind.vertex_buffers[0], BufferSource::slice(&t.quads));
            self.ctx.apply_pipeline(&self.text_pipe);
            self.ctx.apply_bindings(&self.text_bind);
            self.ctx.apply_uniforms(UniformsSource::table(&ortho(sw, sh)));
            self.ctx.draw(0, (t.quads.len() / 4 * 6) as i32, 1);
        }
        self.ctx.end_render_pass();
        self.ctx.commit_frame();
    }

    fn key_down_event(&mut self, key: KeyCode, mods: KeyMods, repeat: bool) {
        if repeat {
            return;
        }
        // (in the browser, web/index.html handles F / Alt+Enter: it must call requestFullscreen
        // itself, inside the real key handler)
        #[cfg(not(target_arch = "wasm32"))]
        if (key == KeyCode::F && !mods.ctrl && !mods.logo) || (key == KeyCode::Enter && mods.alt) {
            self.toggle_fullscreen();
        }
        // nothing else happens while Cmd / Ctrl / Alt is held (Cmd+Tab, shortcuts, ...)
        if mods.ctrl || mods.alt || mods.logo {
            return;
        }
        // Esc: in the intro, back to the start screen; on the start screen, quit (natively; a
        // web page cannot close its tab, and Esc in browser fullscreen only leaves fullscreen,
        // see web/index.html)
        if key == KeyCode::Escape {
            if self.started {
                self.stop();
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                window::order_quit();
            }
            return;
        }
        let enter_or_space = matches!(key, KeyCode::Enter | KeyCode::KpEnter | KeyCode::Space);
        if !self.started {
            if enter_or_space {
                self.start(); // the key that starts the intro does nothing else
            }
        } else if key == KeyCode::Space {
            self.scroll_paused = !self.scroll_paused;
        }
    }

    fn mouse_button_down_event(&mut self, _b: MouseButton, _x: f32, _y: f32) {
        self.start();
    }

    fn touch_event(&mut self, phase: TouchPhase, _id: u64, _x: f32, _y: f32) {
        TOUCH.store(true, Ordering::Relaxed);
        if phase == TouchPhase::Started {
            self.start();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|p| args.get(p + 1)).cloned();
    let fullscreen = args.iter().any(|a| a == "--fullscreen" || a == "-f");

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = arg("--dump-pcm") {
        // render N seconds of the music (stereo f32, no audio device), for testing;
        // --s3m FILE plays another module instead of the embedded one
        let secs: f64 = arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(10.0);
        let data = arg("--s3m").map(|f| std::fs::read(f).expect("read s3m"));
        let rate: u32 = arg("--rate").and_then(|v| v.parse().ok()).unwrap_or(48_000);
        let block: usize = arg("--block").and_then(|v| v.parse().ok()).unwrap_or(4096);
        let mut m = s3m::Player::new(data.as_deref().unwrap_or(MUSIC_S3M), rate).expect("module");
        let mut pcm = vec![0f32; (secs * rate as f64) as usize * 2];
        for block in pcm.chunks_mut(2 * block) {
            m.render(block); // same block size as the libopenmpt reference renders
        }
        let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(&path, bytes).expect("write pcm");
        eprintln!("wrote {secs} s of {rate} Hz stereo f32 to {path}");
        return;
    }

    let (w, h) = arg("--size")
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1024, 768));
    let freeze = arg("--freeze").and_then(|v| v.parse().ok());
    let conf = conf::Conf {
        window_title: "ENFUSiA".to_owned(),
        window_width: w,
        window_height: h,
        fullscreen,
        high_dpi: true, // full retina resolution on macOS / devicePixelRatio on the web
        window_resizable: true,
        ..Default::default()
    };
    miniquad::start(conf, move || Box::new(Stage::new(fullscreen, freeze)));
}

/// Live S3M playback: our own replayer (s3m.rs) feeding a cpal output stream.
#[cfg(not(target_arch = "wasm32"))]
mod music {
    use crate::s3m::Player;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};

    /// Start playback on the default output device. Keep the returned stream alive.
    pub fn play(data: &'static [u8]) -> Result<cpal::Stream, String> {
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let player = Player::new(data, config.sample_rate).ok_or("bad module")?;
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, player),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, player),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, player),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, player),
            f => return Err(format!("unsupported sample format {f}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(stream)
    }

    fn build<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        mut player: Player,
    ) -> Result<cpal::Stream, String> {
        let channels = config.channels as usize;
        let mut buf = Vec::<f32>::new();
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    buf.resize(data.len() / channels * 2, 0.0);
                    player.render(&mut buf);
                    for (frame, lr) in data.chunks_mut(channels).zip(buf.chunks(2)) {
                        for (c, s) in frame.iter_mut().enumerate() {
                            let v = match (channels, c) {
                                (1, _) => (lr[0] + lr[1]) * 0.5,
                                (_, 0) => lr[0],
                                (_, 1) => lr[1],
                                _ => 0.0,
                            };
                            *s = T::from_sample(v);
                        }
                    }
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())
    }
}

/// Web Audio glue: web/audio.js calls these from an AudioWorklet that runs its own instance of
/// this same wasm module, so the music never waits on the render loop.
#[cfg(target_arch = "wasm32")]
mod web_audio {
    use crate::s3m::Player;
    use std::cell::RefCell;

    thread_local! {
        static PLAYER: RefCell<Option<(Player, Vec<f32>)>> = const { RefCell::new(None) };
    }

    extern "C" {
        fn enf_audio_start();
        fn enf_audio_stop();
    }

    /// Ask the page to start (or restart) the music; call from a user-gesture event.
    pub fn start() {
        unsafe { enf_audio_start() }
    }

    /// Silence the music and rewind it to the beginning.
    pub fn stop() {
        unsafe { enf_audio_stop() }
    }

    /// Lets miniquad's JS loader know the `enfusia_audio` plugin in audio.js is in use.
    #[no_mangle]
    pub extern "C" fn enfusia_audio_crate_version() -> u32 {
        2 // must match `version` of the enfusia_audio plugin in web/audio.js
    }

    /// The page reports a touch screen (coarse pointer) before the first frame.
    #[no_mangle]
    pub extern "C" fn enf_set_touch(on: u32) {
        super::TOUCH.store(on != 0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Create the player for the output sample rate. Returns 1 on success.
    #[no_mangle]
    pub extern "C" fn enf_audio_init(rate: u32) -> u32 {
        let p = Player::new(crate::MUSIC_S3M, rate);
        let ok = p.is_some() as u32;
        PLAYER.with(|s| *s.borrow_mut() = p.map(|p| (p, Vec::new())));
        ok
    }

    /// Render `frames` interleaved stereo f32 frames; returns a pointer into wasm memory.
    #[no_mangle]
    pub extern "C" fn enf_audio_render(frames: u32) -> *const f32 {
        PLAYER.with(|s| match s.borrow_mut().as_mut() {
            Some((p, buf)) => {
                buf.resize(frames as usize * 2, 0.0);
                p.render(buf);
                buf.as_ptr()
            }
            None => std::ptr::null(),
        })
    }
}
