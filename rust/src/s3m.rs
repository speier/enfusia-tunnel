//! A tiny Scream Tracker 3 (.s3m) replayer: just what `music.s3m` needs.
//!
//! It follows libopenmpt's (OpenMPT 0.7) playback of this module step by step, so
//! the output is practically identical:
//!
//! * the same sequencer rules: OpenMPT treats this file as saved by ModPlug Tracker
//!   1.16, with per-effect memory and ST3 period limits,
//! * the same 32.32 fixed-point sample stepping and period/frequency math,
//! * the same 8-tap polyphase Kaiser-windowed sinc resampler (three tables, chosen
//!   by pitch),
//! * the same integer mixer: volume ramps (0.36 ms up, 0.95 ms down, or the whole
//!   tick for smooth volume slides), the old note fading out on a separate voice
//!   when a new one starts, and "pop reduction" when a sample ends.
//!
//! Supported effects: A (speed), T (tempo), D (volume slide incl. fine), E/F
//! (portamento incl. fine/extra fine), G (tone portamento) and O (sample offset),
//! plus the volume column and note cut. Others are ignored. 8-bit mono samples only.
//!
//! The playback rules, mixer arithmetic and resampler tables are modelled on OpenMPT's
//! soundlib (libopenmpt 0.7.13), BSD-3-Clause:
//! Copyright (c) 2004-2025, OpenMPT Project Developers and Contributors;
//! Copyright (c) 1997-2003, Olivier Lapicque. See LICENSE.

const NOTE_NONE: u8 = 0;
const NOTE_CUT: u8 = 254;
const RAMP_BITS: i32 = 12;
const MIN_PERIOD: i32 = 64;
const PHASES: usize = 4096;

#[derive(Clone, Copy)]
struct Cell {
    note: u8, // 0 none, 1..=120 notes (OpenMPT numbering), 254 cut
    ins: u8,
    vol: u8, // 255 = none
    cmd: u8, // effect letter (b'A'..), 0 = none
    param: u8,
}

const EMPTY: Cell = Cell { note: NOTE_NONE, ins: 0, vol: 255, cmd: 0, param: 0 };

#[derive(Default)]
struct Sample {
    data: Vec<i16>, // 8-bit sample data scaled to 16 bit
    loop_start: u32,
    loop_end: u32,
    looped: bool,
    vol: i32, // 0..=256
    c5: u32,
}

#[derive(Clone, Default)]
struct Chn {
    smp: usize, // 0 = none
    length: u32,
    loop_start: u32,
    loop_end: u32,
    looped: bool,
    wrapped: bool,
    pos: u64, // 32.32 fixed point
    inc: u64,
    period: i32,
    porta_dest: i32,
    porta_slide: i32,
    old_porta: u8,
    old_vslide: u8,
    old_offset: u32,
    volume: i32, // 0..=256
    pan: i32,    // 0..=256
    c5: u32,
    new_ins: u8,
    note_fade: bool,
    fade_zero: bool,
    fast_ramp: bool,
    vol_ramp: bool,
    real_vol: i32,
    current: bool,
    mixing: bool, // in this tick's mix list (OpenMPT's ChnMix)
    left: i32,
    right: i32,
    new_left: i32,
    new_right: i32,
    ramp_l: i32,
    ramp_r: i32,
    d_l: i32,
    d_r: i32,
    ramp_len: u32,
    ofs_l: i32,
    ofs_r: i32,
    row: Option<Cell>,
}

pub struct Player {
    samples: Vec<Sample>,
    orders: Vec<usize>,
    patterns: Vec<Vec<Cell>>,
    nch: usize,
    preamp: i32,
    rate: u32,
    sinc: Vec<i16>, // 3 tables: upsampling, 1.33x and 2x downsampling
    chn: Vec<Chn>,  // nch pattern channels + background (fading) voices
    speed: u32,
    tempo: u32,
    ord: usize,
    row: usize,
    tick: u32,
    ofs_l: i32,
    ofs_r: i32,
    buf: Vec<i32>,
    tick_left: usize,
}

fn rd16(d: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?) as usize)
}
fn rd32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

// Bessel function I0 and the Kaiser-windowed sinc table, exactly as OpenMPT builds them.
fn izero(y: f64) -> f64 {
    let (mut s, mut ds, mut d) = (1.0, 1.0, 0.0);
    loop {
        d += 2.0;
        ds = ds * (y * y) / (d * d);
        s += ds;
        if ds <= 1e-7 * s {
            return s;
        }
    }
}

fn sinc_table(out: &mut Vec<i16>, beta: f64, cutoff: f64) {
    let ib = izero(beta);
    let kpi = 4.0 * 1f64.atan() * cutoff;
    for isrc in 0..8 * PHASES {
        let ix = (7 - (isrc & 7)) * PHASES + (isrc >> 3);
        let f = if ix == 4 * PHASES {
            1.0
        } else {
            let x = (ix as f64 - (4 * PHASES) as f64) * (1.0 / PHASES as f64);
            let xpi = x * kpi;
            xpi.sin() * izero(beta * (1.0 - x * x * (1.0 / 16.0)).sqrt()) / (ib * xpi)
        };
        out.push((f * cutoff * 32768.0).round().clamp(-32768.0, 32767.0) as i16);
    }
}

impl Player {
    pub fn new(d: &[u8], rate: u32) -> Option<Player> {
        if d.get(0x2c..0x30)? != b"SCRM" {
            return None;
        }
        let (nord, nins, npat) = (rd16(d, 0x20)?, rd16(d, 0x22)?, rd16(d, 0x24)?);
        let unsigned = rd16(d, 0x2a)? == 2;
        let nch = d[0x40..0x60].iter().rposition(|&c| c != 255)? + 1;
        let mut pan: Vec<i32> = d[0x40..0x40 + nch].iter().map(|&c| if c & 8 != 0 { 0xCC } else { 0x33 }).collect();
        let mut o = 0x60;
        let ord_list = d.get(o..o + nord)?;
        o += nord;
        let ins_ptr = o;
        let pat_ptr = o + 2 * nins;
        if d[0x35] == 252 {
            let p = d.get(pat_ptr + 2 * npat..pat_ptr + 2 * npat + 32)?;
            for (i, pv) in pan.iter_mut().enumerate() {
                if p[i] & 0x20 != 0 {
                    *pv = ((p[i] & 15) as i32 * 256 + 8) / 15;
                }
            }
        }
        let mut samples = vec![Sample::default()];
        for i in 0..nins {
            let h = rd16(d, ins_ptr + 2 * i)? * 16;
            let hd = d.get(h..h + 0x50)?;
            let mut s = Sample { vol: hd[28].min(64) as i32 * 4, c5: rd32(hd, 32)?, ..Default::default() };
            if hd[0] == 1 {
                let len = rd32(hd, 16)? as usize;
                let at = ((hd[13] as usize) << 16 | rd16(hd, 14)?) * 16;
                let raw = d.get(at..at + len)?;
                s.data = raw.iter().map(|&b| (if unsigned { b.wrapping_sub(128) } else { b }) as i8 as i16 * 256).collect();
                s.loop_end = rd32(hd, 24)?.min(len as u32);
                s.loop_start = rd32(hd, 20)?;
                s.looped = hd[31] & 1 != 0 && s.loop_start < s.loop_end;
                if !s.looped {
                    s.loop_start = 0;
                    s.loop_end = 0;
                }
            }
            if s.c5 == 0 {
                s.c5 = 8363;
            }
            samples.push(s);
        }
        let mut patterns = Vec::new();
        for p in 0..npat {
            let mut cells = vec![EMPTY; 64 * nch];
            let at = rd16(d, pat_ptr + 2 * p)? * 16;
            if at != 0 {
                let (mut q, mut r) = (at + 2, 0);
                while r < 64 {
                    let c = *d.get(q)?;
                    q += 1;
                    if c == 0 {
                        r += 1;
                        continue;
                    }
                    let mut cell = EMPTY;
                    if c & 32 != 0 {
                        let n = *d.get(q)?;
                        cell.note = match n {
                            255 => NOTE_NONE,
                            254 => NOTE_CUT,
                            _ if n & 15 < 12 && n >> 4 < 10 => (n >> 4) * 12 + (n & 15) + 13,
                            _ => NOTE_NONE,
                        };
                        cell.ins = *d.get(q + 1)?;
                        q += 2;
                    }
                    if c & 64 != 0 {
                        cell.vol = (*d.get(q)?).min(64);
                        q += 1;
                    }
                    if c & 128 != 0 {
                        let e = *d.get(q)?;
                        cell.cmd = if (1..=26).contains(&e) { b'@' + e } else { 0 };
                        cell.param = *d.get(q + 1)?;
                        q += 2;
                    }
                    if ((c & 31) as usize) < nch {
                        cells[r * nch + (c & 31) as usize] = cell;
                    }
                }
            }
            patterns.push(cells);
        }
        let orders: Vec<usize> = ord_list
            .iter()
            .take_while(|&&x| x != 255)
            .filter(|&&x| (x as usize) < npat)
            .map(|&x| x as usize)
            .collect();
        if orders.is_empty() {
            return None;
        }
        let mut sinc = Vec::with_capacity(3 * 8 * PHASES);
        sinc_table(&mut sinc, 9.6377, 0.97);
        sinc_table(&mut sinc, 8.5, 0.5);
        sinc_table(&mut sinc, 7.0, 0.425);
        let mv = d[0x33] as i32 & 0x7f;
        let chn = pan.iter().map(|&p| Chn { pan: p, ..Default::default() }).collect();
        Some(Player {
            samples,
            orders,
            patterns,
            nch,
            preamp: if mv == 0 { 48 } else { mv.max(16) },
            rate,
            sinc,
            chn,
            speed: (d[0x31] as u32).max(1),
            tempo: (d[0x32] as u32).max(32),
            ord: 0,
            row: 0,
            tick: 0,
            ofs_l: 0,
            ofs_r: 0,
            buf: Vec::new(),
            tick_left: 0,
        })
    }

    /// Fill `out` with interleaved stereo samples in -1..1. The song loops forever.
    /// Like OpenMPT, it mixes in chunks of at most 512 frames, split at tick boundaries
    /// and at the end of `out`, so the output is the same for any buffer size libopenmpt
    /// would be called with.
    pub fn render(&mut self, mut out: &mut [f32]) {
        while out.len() >= 2 {
            if self.tick_left == 0 {
                self.next_tick();
            }
            let n = (out.len() / 2).min(512).min(self.tick_left);
            self.buf.clear();
            self.buf.resize(2 * n, 0);
            // Start from the decaying "pop reduction" offset, then add every voice.
            if self.ofs_l | self.ofs_r != 0 {
                for i in 0..n {
                    self.ofs_l -= decay(self.ofs_l);
                    self.ofs_r -= decay(self.ofs_r);
                    self.buf[2 * i] = self.ofs_l;
                    self.buf[2 * i + 1] = self.ofs_r;
                }
            }
            for c in 0..self.chn.len() {
                self.mix_voice(c, n);
            }
            let (now, rest) = out.split_at_mut(2 * n);
            for (o, &v) in now.iter_mut().zip(&self.buf) {
                *o = v as f32 * (1.0 / 134217728.0);
            }
            out = rest;
            self.tick_left -= n;
        }
    }

    fn next_tick(&mut self) {
        if self.tick == 0 {
            let pat = &self.patterns[self.orders[self.ord]];
            for c in 0..self.nch {
                self.chn[c].row = Some(pat[self.row * self.nch + c]);
            }
        }
        for c in 0..self.nch {
            self.effects(c);
        }
        let n = (self.rate * 5 / (self.tempo * 2)) as usize;
        for c in 0..self.chn.len() {
            self.update_voice(c, n);
        }
        self.tick_left = n;
        self.tick += 1;
        if self.tick >= self.speed {
            self.tick = 0;
            self.row += 1;
            if self.row >= 64 {
                self.row = 0;
                self.ord = (self.ord + 1) % self.orders.len();
            }
        }
    }

    fn period_of(note: u8, c5: u32) -> i32 {
        const T: [u64; 12] = [1712, 1616, 1524, 1440, 1356, 1280, 1208, 1140, 1076, 1016, 960, 907];
        let n = note as u64 - 1;
        let c5 = (if c5 == 0 { 8363 } else { c5 } as u64).min(u32::MAX as u64 >> (n / 12));
        (8363 * (T[(n % 12) as usize] << 5) / (c5 << (n / 12))) as i32
    }

    fn set_sample(&mut self, c: usize, s: usize) {
        let smp = &self.samples[s];
        let ch = &mut self.chn[c];
        ch.smp = s;
        ch.length = if smp.looped { smp.loop_end } else { smp.data.len() as u32 };
        ch.loop_start = smp.loop_start;
        ch.loop_end = smp.loop_end;
        ch.looped = smp.looped;
        ch.wrapped = false;
    }

    // OpenMPT's InstrumentChange() for a sample-based S3M.
    fn instrument_change(&mut self, c: usize, ins: usize, porta: bool, upd_vol: bool) {
        let s = if ins < self.samples.len() { ins } else { 0 };
        let has_data = !self.samples[s].data.is_empty();
        let (vol, c5) = (self.samples[s].vol, self.samples[s].c5);
        let ch = &mut self.chn[c];
        if upd_vol && has_data {
            ch.volume = vol;
        }
        ch.fast_ramp = true;
        if porta && s == ch.smp && s != 0 && ch.length != 0 {
            return;
        }
        ch.note_fade = false;
        ch.c5 = c5;
        ch.porta_dest = 0;
        self.set_sample(c, s);
    }

    // A new note: the old one keeps playing on a background voice and fades out fast.
    fn check_nna(&mut self, c: usize) {
        let ch = &self.chn[c];
        if ch.length == 0 || ch.left | ch.right == 0 {
            return;
        }
        let mut bg = ch.clone();
        bg.note_fade = true;
        bg.fade_zero = true;
        bg.fast_ramp = true;
        bg.row = None;
        let slot = (self.nch..self.chn.len()).find(|&i| self.chn[i].length == 0);
        match slot {
            Some(i) => self.chn[i] = bg,
            None => self.chn.push(bg),
        }
        let ch = &mut self.chn[c];
        ch.length = 0;
        ch.pos = 0;
        ch.ofs_l = 0;
        ch.ofs_r = 0;
        ch.left = 0;
        ch.right = 0;
    }

    fn note_change(&mut self, c: usize, note: u8, mut porta: bool) {
        let ch = &mut self.chn[c];
        if note == NOTE_CUT {
            ch.note_fade = true;
            ch.fast_ramp = true;
            ch.volume = 0;
            ch.fade_zero = true;
            return;
        }
        if ch.smp == 0 {
            return;
        }
        let period = Self::period_of(note, ch.c5);
        if period != 0 {
            if !porta || ch.period == 0 {
                ch.period = period;
            }
            ch.porta_dest = period;
            if !porta {
                let s = ch.smp;
                self.set_sample(c, s);
                let ch = &mut self.chn[c];
                ch.pos = 0;
            }
        } else {
            porta = false;
        }
        let ch = &mut self.chn[c];
        ch.note_fade = false;
        ch.fade_zero = false;
        if !porta {
            ch.fast_ramp = true;
            ch.left = 0;
            ch.right = 0;
        }
    }

    fn freq_slide(ch: &mut Chn, amount: i32, tone_porta: bool) {
        if ch.period == 0 || amount == 0 {
            return;
        }
        ch.period -= amount;
        if ch.period < 1 {
            ch.period = 1;
            if !tone_porta {
                ch.fade_zero = true;
                ch.note_fade = true;
                ch.fast_ramp = true;
            }
        }
    }

    fn effects(&mut self, c: usize) {
        let first = self.tick == 0;
        let cell = self.chn[c].row.unwrap_or(EMPTY);
        let porta = cell.cmd == b'G';
        if first {
            let (mut note, mut ins) = (cell.note, cell.ins as usize);
            if ins != 0 {
                self.chn[c].new_ins = ins as u8;
            }
            if note == NOTE_NONE && ins != 0 {
                if let Some(s) = self.samples.get(ins) {
                    if !s.data.is_empty() {
                        self.chn[c].volume = s.vol;
                        self.chn[c].fast_ramp = true;
                    }
                }
                ins = 0;
            }
            if note == NOTE_CUT {
                ins = 0;
            }
            let is_note = note != NOTE_NONE && note != NOTE_CUT;
            if is_note && !porta {
                self.check_nna(c);
            }
            if ins != 0 {
                self.instrument_change(c, ins, porta, true);
                self.chn[c].new_ins = 0;
            }
            if note != NOTE_NONE {
                let ni = self.chn[c].new_ins as usize;
                if ins == 0 && ni != 0 && is_note {
                    let upd = self.chn[c].smp == 0;
                    self.instrument_change(c, ni, porta, upd);
                    self.chn[c].new_ins = 0;
                }
                self.note_change(c, note, porta);
                note = 0;
            }
            let _ = note;
            if cell.vol != 255 {
                self.chn[c].volume = cell.vol as i32 * 4;
                self.chn[c].fast_ramp = true;
            }
        }
        let p = cell.param;
        let ch = &mut self.chn[c];
        match cell.cmd {
            b'A' if first && p != 0 => self.speed = p as u32,
            b'T' if first && p >= 0x20 => self.tempo = p as u32,
            b'D' => {
                let p = if p != 0 {
                    ch.old_vslide = p;
                    p
                } else {
                    ch.old_vslide
                };
                let (hi, lo) = ((p >> 4) as i32, (p & 15) as i32);
                let mut v = ch.volume;
                if lo == 15 && hi != 0 {
                    if first {
                        v += hi * 4;
                    }
                } else if hi == 15 && lo != 0 {
                    if first {
                        v -= lo * 4;
                    }
                } else {
                    if first && (lo == 15 || hi == 15) {
                        v += if lo == 15 { -60 } else { 60 };
                    }
                    if !first {
                        v += if lo != 0 { -lo * 4 } else { hi * 4 };
                    }
                }
                ch.volume = v.clamp(0, 256);
            }
            b'E' | b'F' => {
                let p = if p != 0 {
                    ch.old_porta = p;
                    p
                } else {
                    ch.old_porta
                };
                let sign = if cell.cmd == b'F' { 1 } else { -1 };
                if p >= 0xE0 {
                    if p & 15 != 0 && first {
                        let amt = (p & 15) as i32 * if p >= 0xF0 { 4 } else { 1 };
                        Self::freq_slide(ch, sign * amt, false);
                        if ch.period > 0xFFFF {
                            ch.period = 0xFFFF;
                        }
                    }
                } else if !first {
                    Self::freq_slide(ch, sign * p as i32 * 4, false);
                }
            }
            b'G' => {
                if p != 0 {
                    ch.porta_slide = p as i32;
                }
                let delta = ch.porta_slide * 4;
                if ch.period != 0 && ch.porta_dest != 0 && !first {
                    if ch.period < ch.porta_dest {
                        Self::freq_slide(ch, -delta, true);
                        ch.period = ch.period.min(ch.porta_dest);
                    } else if ch.period > ch.porta_dest {
                        Self::freq_slide(ch, delta, true);
                        ch.period = ch.period.max(ch.porta_dest);
                    }
                }
            }
            b'O' if first => {
                let mut off = (p as u32) << 8;
                if off != 0 {
                    ch.old_offset = off;
                } else {
                    off = ch.old_offset;
                }
                if cell.note == NOTE_NONE || cell.note == NOTE_CUT {
                    return;
                }
                if ch.looped && off >= ch.loop_end && ch.loop_end > 0 {
                    off = (off - ch.loop_start) % (ch.loop_end - ch.loop_start) + ch.loop_start;
                }
                if off >= ch.length || (ch.looped && off >= ch.loop_end) {
                    off = ch.loop_start;
                }
                ch.pos = (off as u64) << 32;
            }
            _ => {}
        }
    }

    // OpenMPT's per-tick channel update: pitch, volume and the volume ramp.
    fn update_voice(&mut self, c: usize, tick_len: usize) {
        let rate = self.rate as u64;
        let preamp = self.preamp;
        let has_data = !self.samples[self.chn[c].smp].data.is_empty();
        let ch = &mut self.chn[c];
        if ch.note_fade && ch.fade_zero && ch.left | ch.right == 0 {
            ch.length = 0;
        }
        ch.mixing = false;
        if c >= self.nch && ch.length == 0 {
            return;
        }
        ch.inc = 0;
        ch.real_vol = 0;
        if ch.period != 0 && ch.length != 0 {
            let vol = if ch.note_fade { 0 } else { ch.volume.clamp(0, 256) };
            ch.real_vol = vol << 6; // * 256 (global) * 64 (channel) * 64 (sample) >> 20
            let mut period = ch.period;
            if period <= MIN_PERIOD {
                ch.length = 0; // ST3 stops notes above its highest pitch
                period = MIN_PERIOD;
            }
            let freq = 8363u64 * ((1712 << 8) << 4) / ((period as u64) << 8);
            ch.inc = ((freq << 32) / (rate << 4)).max(1);
        }
        ch.vol_ramp = ch.real_vol | ch.left | ch.right != 0;
        ch.new_left = 0;
        ch.new_right = 0;
        ch.current = has_data && ch.length != 0 && ch.inc != 0;
        if !ch.current {
            ch.left = 0;
            ch.right = 0;
            ch.length = 0;
            ch.mixing = ch.ofs_l | ch.ofs_r != 0; // still needed for pop reduction
            return;
        }
        ch.mixing = true;
        let realvol = ch.real_vol * preamp / 128 / 2;
        ch.new_left = realvol * (256 - ch.pan) / 256 / 2;
        ch.new_right = realvol * ch.pan / 256 / 2;
        // ProcessRamping()
        ch.d_l = 0;
        ch.d_r = 0;
        ch.ramp_len = 0;
        if ch.vol_ramp && (ch.left != ch.new_left || ch.right != ch.new_right) {
            let up = ch.new_left > ch.left || ch.new_right > ch.right;
            let global = ((if up { 363 } else { 952 }) * rate as i64 + 500_000) / 1_000_000;
            let mut len = global.max(1) as i32;
            if ch.left | ch.right != 0 && ch.new_left | ch.new_right != 0 && !ch.fast_ramp {
                len = (tick_len as i32).clamp(len, 1 << (RAMP_BITS - 1));
            }
            ch.d_l = ((ch.new_left - ch.left) << RAMP_BITS) / len;
            ch.d_r = ((ch.new_right - ch.right) << RAMP_BITS) / len;
            ch.left = ch.new_left - (ch.d_l * len) / (1 << RAMP_BITS);
            ch.right = ch.new_right - (ch.d_r * len) / (1 << RAMP_BITS);
            if ch.d_l | ch.d_r != 0 {
                ch.ramp_len = len as u32;
            } else {
                ch.left = ch.new_left;
                ch.right = ch.new_right;
            }
        } else {
            ch.left = ch.new_left;
            ch.right = ch.new_right;
        }
        ch.ramp_l = ch.left << RAMP_BITS;
        ch.ramp_r = ch.right << RAMP_BITS;
        ch.fast_ramp = false;
    }

    fn mix_voice(&mut self, c: usize, n: usize) {
        let Self { chn, samples, sinc, buf, ofs_l, ofs_r, .. } = self;
        let ch = &mut chn[c];
        if !ch.mixing {
            return;
        }
        let data = &samples[ch.smp].data[..];
        let mut i = 0;
        let table = if ch.inc > 0x1_8000_0000 {
            2
        } else if ch.inc > 0x1_3000_0000 {
            1
        } else {
            0
        };
        let lut_base = &sinc[table * 8 * PHASES..(table + 1) * 8 * PHASES];
        while i < n {
            let mut ipos = (ch.pos >> 32) as u32;
            if !ch.current || ch.length == 0 {
                break;
            }
            if ipos >= ch.length {
                if !ch.looped {
                    break;
                }
                ch.pos -= ((ch.length - ch.loop_start) as u64) << 32;
                ipos = (ch.pos >> 32) as u32;
                ch.wrapped = true;
                if ipos >= ch.length {
                    break;
                }
            }
            if ch.ramp_len == 0 && ch.left | ch.right == 0 {
                ch.pos += ch.inc;
                ch.ofs_l = 0;
                ch.ofs_r = 0;
                i += 1;
                continue;
            }
            let (ls, le) = (ch.loop_start as i64, ch.loop_end as i64);
            let near_start = ipos >= ch.loop_start && ipos < ch.loop_start + 16;
            if !near_start {
                ch.wrapped = false;
            }
            let ahead = ch.looped && (ipos as i64 >= (le - 16).max(ls) || (ch.wrapped && near_start));
            let last = data.len() as i64 - 1;
            let lut = &lut_base[((ch.pos as u32 >> 20) as usize & (PHASES - 1)) * 8..][..8];
            let mut acc = 0i32;
            for (k, &w) in lut.iter().enumerate() {
                let mut j = ipos as i64 + k as i64 - 3;
                if ahead {
                    j = ls + (j - ls).rem_euclid(le - ls);
                } else {
                    j = j.clamp(0, last);
                }
                acc += w as i32 * data[j as usize] as i32;
            }
            let s = acc / 32768;
            let (vl, vr) = if ch.ramp_len > 0 {
                ch.ramp_l += ch.d_l;
                ch.ramp_r += ch.d_r;
                (ch.ramp_l >> RAMP_BITS, ch.ramp_r >> RAMP_BITS)
            } else {
                (ch.left, ch.right)
            };
            ch.ofs_l = s * vl;
            ch.ofs_r = s * vr;
            buf[2 * i] += ch.ofs_l;
            buf[2 * i + 1] += ch.ofs_r;
            ch.pos += ch.inc;
            i += 1;
            if ch.ramp_len > 0 {
                ch.ramp_len -= 1;
                if ch.ramp_len == 0 {
                    ch.left = ch.new_left;
                    ch.right = ch.new_right;
                    ch.d_l = 0;
                    ch.d_r = 0;
                    if ch.note_fade && ch.fade_zero {
                        ch.length = 0;
                        ch.current = false;
                    }
                } else {
                    ch.left = ch.ramp_l >> RAMP_BITS;
                    ch.right = ch.ramp_r >> RAMP_BITS;
                }
            }
        }
        if i < n {
            // The voice stopped: let its last output value decay instead of popping.
            if ch.ofs_l | ch.ofs_r != 0 {
                for j in i..n {
                    ch.ofs_l -= decay(ch.ofs_l);
                    ch.ofs_r -= decay(ch.ofs_r);
                    buf[2 * j] += ch.ofs_l;
                    buf[2 * j + 1] += ch.ofs_r;
                }
                *ofs_l += ch.ofs_l;
                *ofs_r += ch.ofs_r;
            }
            ch.ofs_l = 0;
            ch.ofs_r = 0;
            ch.current = false;
            ch.length = 0;
            ch.pos = 0;
            ch.ramp_len = 0;
        }
    }
}

fn decay(x: i32) -> i32 {
    (x + if x > 0 { 255 } else { 0 }) >> 8
}
