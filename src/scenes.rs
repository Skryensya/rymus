// The full screen visualizer (V): twelve scenes in the spirit of Windows Media Player's (Battery's endless feedback
// tunnels, Alchemy's shapes that turn into each other, Ambience's slow clouds, the classic Bars and Waves), with our own
// touch. Each song opens on its own scene; strong moments of the song (a drop, a big rise) move on to another, at most
// every 30 s and never in a song's first 15 s, with a soft dissolve. [ ] change it by hand and . holds it.
//
// What the song sounds like is read once per frame into `Feel` (bass, mids, highs, real loudness, a beat pulse, the
// tempo) and every scene draws from that, so all of them react the same calm way. A frame never brightens by much more
// than a little at once: nothing flashes, whatever the music does.
use crate::trip::Trip;
use crate::ui::Rgb;
use std::f64::consts::{PI, TAU};
use std::fmt::Write;
use unicode_width::UnicodeWidthChar;

// ---------- cells ----------

// One screen cell: a character in fg on bg, colors 0..1. A half block (▀) shows fg above and bg below.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: [f32; 3],
    pub bg: [f32; 3],
}
const SKIP: char = '\0'; // the second half of a wide character

pub fn to_rows(cells: &[Cell], w: usize, h: usize) -> Vec<String> {
    // shortcut: 32 steps per channel, so neighbours share one escape code; finer if banding shows
    let q = |c: [f32; 3]| c.map(|n| ((n.clamp(0.0, 1.0) * 255.0).round() as u8) & 0xf8);
    (0..h)
        .map(|j| {
            let mut line = String::with_capacity(w * 6);
            let (mut fg, mut bg) = (None, None);
            for c in &cells[j * w..(j + 1) * w] {
                if c.ch == SKIP {
                    continue;
                }
                let (f, b) = (q(c.fg), q(c.bg));
                // a half block in one colour is a space; a space needs no foreground. Each colour is sent only when
                // it changes: the screen is the same, with far less for the terminal to read
                let ch = if c.ch == '▀' && f == b { ' ' } else { c.ch };
                if bg != Some(b) {
                    let _ = write!(line, "\x1b[48;2;{};{};{}m", b[0], b[1], b[2]);
                    bg = Some(b);
                }
                if ch != ' ' && fg != Some(f) {
                    let _ = write!(line, "\x1b[38;2;{};{};{}m", f[0], f[1], f[2]);
                    fg = Some(f);
                }
                line.push(ch);
            }
            line.push_str("\x1b[39;49m");
            line
        })
        .collect()
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}
// How bright the picture looks overall, 0..1.
fn brightness(cells: &[Cell]) -> f32 {
    let one = |c: &Cell| match c.ch {
        '▀' => (lum(c.fg) + lum(c.bg)) / 2.0,
        ' ' | SKIP => lum(c.bg),
        _ => lum(c.bg) * 0.7 + lum(c.fg) * 0.3,
    };
    cells.iter().map(one).sum::<f32>() / cells.len().max(1) as f32
}

// ---------- what the song sounds like ----------

#[derive(Clone, Default)]
pub struct Feel {
    pub t: f64,  // the scene clock: playback time, stops while paused
    pub dt: f64, // since the last frame; 0 when paused
    pub bands: Vec<f64>, // the spectrum, bass first, 0..1, eased
    pub bass: f64,
    pub mid: f64,
    pub high: f64,
    pub energy: f64, // how loud it really is: soft songs stay calm
    pub beat: f64,   // a pulse on each beat, fading
    pub tempo: f64,  // 0 = 60 BPM, 1 = 180
    pub seed: u32,   // from the song: each draws its scenes a little differently
}

#[derive(Default)]
struct Ears {
    avg_bass: f64,
    cool: f64,
    kick: f64,
    beats: Vec<f64>,
    fast: f64, // loudness over about a second
    slow: f64, // and over about twelve: a strong moment is when the first rises well above the second
}

fn avg(a: &[f64]) -> f64 {
    if a.is_empty() { 0.0 } else { a.iter().sum::<f64>() / a.len() as f64 }
}

impl Ears {
    fn listen(&mut self, f: &mut Feel, raw: &[f64], energy: f64) {
        let dt = f.dt;
        if f.bands.len() != raw.len() {
            f.bands = vec![0.0; raw.len()];
        }
        if dt <= 0.0 {
            return;
        }
        // up in about 0.12 s, down in about 0.5 s: raw levels jump, eased ones flow
        let ease = |from: f64, to: f64| from + (to - from) * (dt / if to > from { 0.12 } else { 0.5 }).min(1.0);
        let gain = 0.3 + 0.7 * energy;
        for (b, r) in f.bands.iter_mut().zip(raw) {
            *b = ease(*b, r * gain);
        }
        let n = raw.len().max(1);
        f.bass = avg(&f.bands[..(n * 3 / 16).max(1)]);
        f.mid = avg(&f.bands[n * 3 / 16..n * 10 / 16]);
        f.high = avg(&f.bands[n * 10 / 16..]);
        f.energy = ease(f.energy, energy);
        // a beat: the raw bass jumps above its recent average
        let bass = avg(&raw[..(n * 3 / 16).max(1)]) * gain;
        self.avg_bass += (bass - self.avg_bass) * (dt * 2.0).min(1.0);
        self.cool -= dt;
        if self.cool <= 0.0 && bass > self.avg_bass * 1.3 + 0.08 {
            self.cool = 0.25;
            self.kick = 0.4 + 0.6 * energy;
            self.beats.push(f.t);
            if self.beats.len() > 16 {
                self.beats.remove(0);
            }
        }
        // the pulse rises over a few frames (no jolt) and fades over half a second
        f.beat += (self.kick - f.beat) * (dt / 0.06).min(1.0);
        self.kick *= (-dt * 6.0).exp();
        f.beat *= (-dt * 2.5).exp();
        let mut gaps: Vec<f64> = self.beats.windows(2).map(|b| b[1] - b[0]).filter(|&g| g > 0.25 && g < 1.5).collect();
        gaps.sort_by(f64::total_cmp);
        let bpm = if gaps.len() >= 3 { 60.0 / gaps[gaps.len() / 2] } else { 110.0 };
        f.tempo += (((bpm - 60.0) / 120.0).clamp(0.0, 1.0) - f.tempo) * (dt / 3.0).min(1.0);
        self.fast += (energy - self.fast) * (dt / 1.0).min(1.0);
        self.slow += (energy - self.slow) * (dt / 12.0).min(1.0);
    }
    fn strong_moment(&self) -> bool {
        self.fast > 0.3 && self.fast - self.slow > 0.15
    }
}

// ---------- colors ----------

type Pal = [[f32; 3]; 4]; // dark to light

fn to_pal(colors: &[Rgb]) -> Pal {
    let colors: Vec<[f32; 3]> = if colors.is_empty() { vec![[0.4, 0.8, 0.5]] } else { colors.iter().map(|c| c.map(|v| v as f32 / 255.0)).collect() };
    std::array::from_fn(|i| {
        let x = i as f32 / 3.0 * (colors.len() - 1) as f32;
        let (a, b) = (x.floor() as usize, (x.ceil() as usize).min(colors.len() - 1));
        mix(colors[a], colors[b], x - x.floor())
    })
}
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn scale(c: [f32; 3], k: f64) -> [f32; 3] {
    c.map(|v| (v * k as f32).clamp(0.0, 1.0))
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| (a[i] + b[i]).min(1.0))
}
// Along the palette and back: any x lands on it, 0 dark, 1 light, 2 dark again. Dark ends are lifted so nothing is
// lost in black.
fn paint(p: &Pal, x: f64) -> [f32; 3] {
    let u = 1.0 - ((x.rem_euclid(2.0)) - 1.0).abs();
    let k = u * 3.0;
    let i = (k.floor() as usize).min(3);
    let c = mix(p[i], p[(i + 1).min(3)], (k - i as f64) as f32);
    let top = c.iter().cloned().fold(0.0, f32::max);
    if top < 0.35 { if top > 0.0 { c.map(|v| v * 0.35 / top) } else { [0.35; 3] } } else { c }
}
const CLASSIC: Pal = [[0.0, 0.55, 0.25], [0.2, 0.85, 0.3], [0.95, 0.85, 0.15], [1.0, 0.3, 0.15]]; // green to red
const PHOSPHOR: [f32; 3] = [0.35, 1.0, 0.55];

// ---------- drawing surfaces ----------

// What a scene keeps between frames.
#[derive(Default)]
struct Mem {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>, // w × 2h pixels (half blocks), or w × h cell colors
    dots: Vec<f32>,    // 2w × 4h braille dots
    peaks: Vec<f64>,
}
impl Mem {
    fn fit(&mut self, w: usize, h: usize) {
        if (self.w, self.h) != (w, h) {
            *self = Mem { w, h, px: vec![[0.0; 3]; w * h * 2], dots: vec![0.0; w * h * 8], peaks: vec![] };
        }
    }
}

// Half block pixels: x across, y down, both measured in half heights so a circle is round.
struct Px<'a> {
    px: &'a mut [[f32; 3]],
    pw: usize,
    ph: usize,
}
impl Px<'_> {
    fn norm(&self, i: usize, j: usize) -> (f64, f64) {
        let half = self.ph as f64 / 2.0;
        ((i as f64 + 0.5 - self.pw as f64 / 2.0) / half, (j as f64 + 0.5 - half) / half)
    }
    fn at(&self, x: f64, y: f64) -> (f64, f64) {
        let half = self.ph as f64 / 2.0;
        (x * half + self.pw as f64 / 2.0 - 0.5, y * half + half - 0.5)
    }
    fn sample(&self, x: f64, y: f64) -> [f32; 3] {
        let (fx, fy) = self.at(x, y);
        if fx < 0.0 || fy < 0.0 || fx >= (self.pw - 1) as f64 || fy >= (self.ph - 1) as f64 {
            return [0.0; 3];
        }
        let (i, j) = (fx as usize, fy as usize);
        let (u, v) = ((fx - i as f64) as f32, (fy - j as f64) as f32);
        let p = |a: usize, b: usize| self.px[b * self.pw + a];
        mix(mix(p(i, j), p(i + 1, j), u), mix(p(i, j + 1), p(i + 1, j + 1), u), v)
    }
    // Every pixel becomes the old picture seen through `from` (where it was before this frame), a little dimmer.
    fn feedback(&mut self, keep: f64, from: impl Fn(f64, f64) -> (f64, f64)) {
        let mut old: Vec<[f32; 3]> = self.px.to_vec();
        let src = Px { px: &mut old, pw: self.pw, ph: self.ph };
        for j in 0..self.ph {
            for i in 0..self.pw {
                let (x, y) = src.norm(i, j);
                let (sx, sy) = from(x, y);
                self.px[j * self.pw + i] = scale(src.sample(sx, sy), keep);
            }
        }
    }
    fn dot(&mut self, x: f64, y: f64, c: [f32; 3]) {
        let (fx, fy) = self.at(x, y);
        let (i, j) = (fx.round() as i64, fy.round() as i64);
        for (di, dj, k) in [(0, 0, 1.0), (1, 0, 0.35), (-1, 0, 0.35), (0, 1, 0.35), (0, -1, 0.35)] {
            let (a, b) = (i + di, j + dj);
            if a >= 0 && b >= 0 && (a as usize) < self.pw && (b as usize) < self.ph {
                let p = &mut self.px[b as usize * self.pw + a as usize];
                *p = add(*p, scale(c, k));
            }
        }
    }
    fn line(&mut self, (x0, y0): (f64, f64), (x1, y1): (f64, f64), c: [f32; 3]) {
        let n = ((x1 - x0).hypot(y1 - y0) * self.ph as f64 / 2.0).ceil().max(1.0) as usize;
        for s in 0..=n {
            let f = s as f64 / n as f64;
            self.dot(x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, c);
        }
    }
    fn each(&mut self, f: impl Fn(f64, f64) -> [f32; 3]) {
        for j in 0..self.ph {
            for i in 0..self.pw {
                let (x, y) = self.norm(i, j);
                self.px[j * self.pw + i] = f(x, y);
            }
        }
    }
    fn cells(&self) -> Vec<Cell> {
        let h = self.ph / 2;
        let mut out = Vec::with_capacity(self.pw * h);
        for j in 0..h {
            for i in 0..self.pw {
                out.push(Cell { ch: '▀', fg: self.px[2 * j * self.pw + i], bg: self.px[(2 * j + 1) * self.pw + i] });
            }
        }
        out
    }
}

// Braille: 2 × 4 dots per cell, square spacing, measured like Px.
struct Dots<'a> {
    dots: &'a mut [f32],
    color: &'a mut [[f32; 3]], // one per cell
    w: usize,
    h: usize,
}
impl Dots<'_> {
    fn fade(&mut self, keep: f32) {
        self.dots.iter_mut().for_each(|d| *d *= keep);
        self.color.iter_mut().for_each(|c| *c = c.map(|v| v * keep));
    }
    fn plot(&mut self, x: f64, y: f64, c: [f32; 3]) {
        let (dw, dh) = (self.w * 2, self.h * 4);
        let half = dh as f64 / 2.0;
        let (i, j) = ((x * half + dw as f64 / 2.0) as i64, (y * half + half) as i64);
        if i < 0 || j < 0 || i as usize >= dw || j as usize >= dh {
            return;
        }
        let (i, j) = (i as usize, j as usize);
        self.dots[j * dw + i] = 1.0;
        let cell = &mut self.color[(j / 4) * self.w + i / 2];
        *cell = c.iter().zip(cell.iter()).map(|(a, b)| a.max(*b)).collect::<Vec<_>>().try_into().unwrap_or(c);
    }
    fn line(&mut self, (x0, y0): (f64, f64), (x1, y1): (f64, f64), c: [f32; 3]) {
        let n = ((x1 - x0).hypot(y1 - y0) * self.h as f64 * 2.0).ceil().max(1.0) as usize;
        for s in 0..=n {
            let f = s as f64 / n as f64;
            self.plot(x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, c);
        }
    }
    fn cells(&self, bg: [f32; 3]) -> Vec<Cell> {
        const BIT: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
        let dw = self.w * 2;
        let mut out = Vec::with_capacity(self.w * self.h);
        for cj in 0..self.h {
            for ci in 0..self.w {
                let mut bits = 0;
                for (dx, col) in BIT.iter().enumerate() {
                    for (dy, bit) in col.iter().enumerate() {
                        if self.dots[(cj * 4 + dy) * dw + ci * 2 + dx] > 0.35 {
                            bits |= bit;
                        }
                    }
                }
                let ch = if bits == 0 { ' ' } else { char::from_u32(0x2800 + bits).unwrap_or(' ') };
                out.push(Cell { ch, fg: self.color[cj * self.w + ci], bg });
            }
        }
        out
    }
}

// ---------- the scenes ----------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Family {
    Battery,
    Alchemy,
    Ambience,
    Bars,
}
pub const SCENES: [(&str, Family); 12] = [
    ("my tornado is resting", Family::Battery),
    ("the lake remembers", Family::Battery),
    ("a quiet engine", Family::Battery),
    ("spider in the bathtub", Family::Alchemy),
    ("glass orchard", Family::Alchemy),
    ("nine moths agree", Family::Alchemy),
    ("the old smoke", Family::Ambience),
    ("slow milk in tea", Family::Ambience),
    ("weather inside a shell", Family::Ambience),
    ("the choir in the walls", Family::Bars),
    ("a wire hums back", Family::Bars),
    ("circular hymn", Family::Bars),
];

// The level of the spectrum at u (0 bass .. 1 treble), between bands.
fn band(f: &Feel, u: f64) -> f64 {
    let n = f.bands.len();
    if n == 0 {
        return 0.0;
    }
    let x = u.clamp(0.0, 1.0) * (n - 1) as f64;
    let i = x.floor() as usize;
    let j = (i + 1).min(n - 1);
    f.bands[i] + (f.bands[j] - f.bands[i]) * (x - i as f64)
}

// For the old smoke: what that scene needs that the others do not.
struct Song<'a> {
    id: &'a str,
    raw: &'a [f64],
    colors: &'a [Rgb],
    energy: f64,
}

#[allow(clippy::too_many_arguments, clippy::needless_range_loop)] // one function per scene's state; bands index several arrays
fn draw(k: usize, m: &mut Mem, f: &Feel, p: &Pal, w: usize, h: usize, smoke: &mut Trip, song: &Song) -> Vec<Cell> {
    m.fit(w, h);
    let live = f.dt > 0.0; // paused: the picture holds still
    let k25 = f.dt * 25.0; // frame rate independent steps
    let spin = if f.seed.is_multiple_of(2) { 1.0 } else { -1.0 };
    let aspect = w as f64 / h as f64 / 2.0; // half the width, in half heights
    match k {
        // ----- Battery: each frame is the last one, moved a little and dimmed, with new light drawn on top
        0 => {
            let mut px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live {
                let rot = (0.012 + 0.03 * f.mid) * k25 * spin;
                let zoom = 1.0 - (0.01 + 0.035 * f.beat) * k25;
                let (s, c) = rot.sin_cos();
                px.feedback(0.9f64.powf(k25), |x, y| ((x * c - y * s) * zoom, (x * s + y * c) * zoom));
                let r0 = 0.22 + 0.18 * f.bass;
                for b in 0..48 {
                    let u = b as f64 / 48.0;
                    let a = u * TAU + f.t * 0.4 * spin;
                    let r = r0 + band(f, (u * 2.0 - 1.0).abs()) * 0.4;
                    px.dot(a.cos() * r, a.sin() * r, scale(paint(p, u * 2.0 + f.t * 0.05), 0.35 + 0.6 * band(f, u)));
                }
            }
            px.cells()
        }
        1 => {
            let mut px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live {
                let zoom = 1.0 + (0.012 + 0.02 * f.energy) * k25;
                let ripple = 0.006 * f.beat;
                px.feedback(0.92f64.powf(k25), |x, y| {
                    let r = x.hypot(y);
                    let wob = (r * 12.0 - f.t * 5.0).sin() * ripple;
                    (x * zoom + wob * x, y * zoom + wob * y)
                });
                let amp = 0.08 * (0.4 + f.energy);
                let n = (w * 2).max(20);
                let mut last = None;
                for s in 0..=n {
                    let x = (s as f64 / n as f64 * 2.0 - 1.0) * aspect;
                    let y: f64 = (0..12).map(|b| f.bands.get(b * f.bands.len() / 12).unwrap_or(&0.0) * (x * (b + 1) as f64 * 1.3 + f.t * (1.0 + b as f64 * 0.3)).sin()).sum::<f64>() * amp;
                    if let Some(prev) = last {
                        px.line(prev, (x, y), scale(paint(p, 0.6 + x * 0.2 + f.t * 0.03), 0.8));
                    }
                    last = Some((x, y));
                }
            }
            px.cells()
        }
        2 => {
            let mut px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live {
                let fold = 5.0 + (f.seed % 3) as f64; // a kaleidoscope of 5 to 7 mirrors
                let rot = 0.008 * k25 * spin;
                let zoom = 1.0 - 0.012 * k25;
                px.feedback(0.9f64.powf(k25), |x, y| {
                    let r = x.hypot(y) * zoom;
                    let slice = PI / fold;
                    let a = (y.atan2(x) + rot).rem_euclid(2.0 * slice);
                    let a = if a > slice { 2.0 * slice - a } else { a };
                    (a.cos() * r, a.sin() * r)
                });
                let sides = 3 + (f.tempo * 4.0).round() as usize;
                let r = 0.25 + 0.35 * f.energy + 0.1 * f.beat;
                let turn = f.t * 0.5 * spin;
                let corner = |i: usize| {
                    let a = turn + i as f64 / sides as f64 * TAU;
                    (a.cos() * r, a.sin() * r)
                };
                let c = scale(paint(p, f.t * 0.07 + 0.5), 0.9);
                for i in 0..sides {
                    px.line(corner(i), corner(i + 1), c);
                }
            }
            px.cells()
        }
        // ----- Alchemy: shapes that slowly turn into others
        3 => {
            let mut d = Dots { dots: &mut m.dots, color: &mut m.px[..w * h], w, h };
            if live {
                d.fade(0.3f32.powf(k25 as f32));
                // few loops, slowly changing how many: a figure, not a ball of wool
                let morph = f.t * 0.04 + f.seed as f64 * 0.1;
                let a = 1.0 + 2.0 * (0.5 + 0.5 * morph.sin());
                let b = a + 1.0;
                let size = 0.55 + 0.3 * f.energy + 0.12 * f.beat;
                for curve in 0..2 {
                    let delta = curve as f64 * 1.3 + f.t * (0.25 + 0.5 * f.bass);
                    let color = paint(p, curve as f64 / 2.0 + f.t * 0.03 + 0.4);
                    let mut last = None;
                    for s in 0..=300 {
                        let u = s as f64 / 300.0 * TAU;
                        let pt = ((a * u + delta).sin() * size * aspect.min(1.6), (b * u).sin() * size * 0.92);
                        if let Some(prev) = last {
                            d.line(prev, pt, color);
                        }
                        last = Some(pt);
                    }
                }
            }
            d.cells(scale(p[0], 0.12))
        }
        4 => {
            let mut px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live {
                let layers = [(1.0, 1.0), (0.62, -1.4), (0.36, 2.0)];
                let size = 0.5 + 0.25 * f.energy + 0.08 * f.beat;
                // the superformula depends on the angle only: worked out once per frame for 720 angles per layer
                const STEPS: usize = 720;
                let shapes: Vec<(f64, Vec<f64>, f64)> = layers
                    .iter()
                    .enumerate()
                    .map(|(l, &(s, speed))| {
                        let m_ = 3.0 + 5.0 * (0.5 + 0.5 * (f.t * 0.06 + l as f64 * 2.1 + f.seed as f64).sin());
                        let turn = f.t * 0.15 * speed * spin;
                        // from a triangle to a star to a flower as m_ changes
                        let edge = (0..STEPS)
                            .map(|k| {
                                let a = k as f64 / STEPS as f64 * TAU + turn;
                                let sf = ((m_ * a / 4.0).cos().abs().powf(1.2) + (m_ * a / 4.0).sin().abs().powf(1.2)).powf(-1.0 / 1.4);
                                s * size * sf.clamp(0.4, 1.6)
                            })
                            .collect();
                        (l as f64, edge, band(f, l as f64 / 2.0))
                    })
                    .collect();
                let bg = scale(p[0], 0.1);
                let colors: Vec<[f32; 3]> = (0..3).map(|l| paint(p, l as f64 * 0.3 + f.t * 0.02 + 0.5)).collect();
                px.each(|x, y| {
                    let r = x.hypot(y);
                    let th = y.atan2(x);
                    let k = ((th.rem_euclid(TAU) / TAU * STEPS as f64) as usize).min(STEPS - 1);
                    let mut c = bg;
                    for (l, edge, lvl) in &shapes {
                        let d = r - edge[k];
                        let glow = (-(d.abs() * 16.0)).exp() * (0.5 + 0.7 * lvl) + if d < 0.0 { 0.05 } else { 0.0 };
                        c = add(c, scale(colors[*l as usize], glow));
                    }
                    c
                });
            }
            px.cells()
        }
        5 => {
            const RAMP: [char; 10] = [' ', '.', '·', ':', '-', '=', '+', '*', '#', '%'];
            let fold = 5.0 + (f.seed % 5) as f64;
            let bg = scale(p[0], 0.1);
            if live || m.px[0] == [0.0; 3] {
                m.px[0] = [0.001; 3]; // drawn at least once while paused
            }
            (0..w * h)
                .map(|n| {
                    let (i, j) = (n % w, n / w);
                    let x = (i as f64 + 0.5 - w as f64 / 2.0) / h as f64; // a cell is twice as tall as wide
                    let y = (j as f64 + 0.5 - h as f64 / 2.0) / (h as f64 / 2.0);
                    let r = x.hypot(y);
                    let th = y.atan2(x);
                    let v = 0.5 + 0.5 * (r * 9.0 - f.t * 1.2 + band(f, r.min(1.0)) * 3.0).sin() * (th * fold + f.t * 0.25 * spin + r * 2.0 * (f.t * 0.1).sin()).cos();
                    let v = (v * (0.45 + 0.7 * f.energy) + 0.25 * f.beat * (-(r - 0.6).powi(2) * 8.0).exp()).clamp(0.0, 1.0).powi(2); // room to breathe
                    let ch = RAMP[((v * RAMP.len() as f64) as usize).min(RAMP.len() - 1)];
                    Cell { ch, fg: scale(paint(p, r * 0.6 + f.t * 0.03 + v * 0.3), 0.35 + v), bg }
                })
                .collect()
        }
        // ----- Ambience: soft clouds that flow
        6 => smoke.cells(w, h, f.t, song.raw, song.colors, song.id, song.energy),
        7 => {
            let px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live || px.px[0] == [0.0; 3] {
                let (t, bass) = (f.t, f.bass);
                let bright = 0.35 + 0.5 * f.energy;
                // three waves, each sin(a(x) + b(y)) = sin a·cos b + cos a·sin b: the sines and cosines are worked out
                // once per column and once per row instead of for every pixel
                let cols: Vec<[f64; 6]> = (0..px.pw)
                    .map(|i| {
                        let x = px.norm(i, 0).0;
                        let (a, d, e) = (x * 1.3 + t * 0.21, 1.2 * (x * 2.1 + t * 0.11).sin(), x * 0.9);
                        [a.sin(), a.cos(), d.sin(), d.cos(), e.sin(), e.cos()]
                    })
                    .collect();
                let rows: Vec<[f64; 6]> = (0..px.ph)
                    .map(|j| {
                        let y = px.norm(0, j).1;
                        let (b, c, e) = (1.5 * (y * 1.7 - t * 0.17).sin() * (0.6 + bass), y * 1.1 - t * 0.13, y * 0.9 + t * 0.07);
                        [b.sin(), b.cos(), c.sin(), c.cos(), e.sin(), e.cos()]
                    })
                    .collect();
                for (j, r) in rows.iter().enumerate() {
                    for (i, c) in cols.iter().enumerate() {
                        let w1 = c[0] * r[1] + c[1] * r[0];
                        let w2 = r[2] * c[3] + r[3] * c[2];
                        let w3 = c[4] * r[5] + c[5] * r[4];
                        let v = 0.5 + (w1 + w2 + w3) / 6.0;
                        px.px[j * px.pw + i] = scale(paint(p, v * 0.9 + t * 0.015), bright + 0.2 * v);
                    }
                }
            }
            px.cells()
        }
        8 => {
            let mut px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live || px.px[0] == [0.0; 3] {
                let balls: Vec<(f64, f64, f64)> = (0..7)
                    .map(|b| {
                        let k = b as f64;
                        let x = (f.t * 0.13 * (k + 1.0) / 3.0 + k).sin() * 0.85 * aspect;
                        let y = (f.t * 0.17 * (k + 2.0) / 3.0 + k * 2.0).cos() * 0.7;
                        (x, y, 0.1 + 0.28 * band(f, k / 6.0) + 0.05 * f.energy)
                    })
                    .collect();
                let deep = scale(p[0], 0.15);
                px.each(|x, y| {
                    let field: f64 = balls.iter().map(|&(bx, by, r)| r * r / ((x - bx).powi(2) + (y - by).powi(2) + 0.002)).sum();
                    let v = ((field - 0.6) / 1.0).clamp(0.0, 1.0);
                    let v = v * v * (3.0 - 2.0 * v);
                    mix(deep, paint(p, 0.3 + field.min(4.0) * 0.12 + f.t * 0.02), v as f32)
                });
            }
            px.cells()
        }
        // ----- Bars and Waves
        9 => {
            let ph = 2 * h;
            let n = (w / 3).max(4);
            if m.peaks.len() != n {
                m.peaks = vec![0.0; n];
            }
            let base = (ph as f64 * 0.78) as usize; // reflection below
            let mut px = vec![[0.0f32; 3]; w * ph];
            let back = scale(p[0], 0.06);
            px.iter_mut().for_each(|c| *c = back);
            for b in 0..n {
                let level = band(f, b as f64 / (n - 1) as f64).min(1.0);
                if live {
                    m.peaks[b] = (m.peaks[b] - f.dt * 0.35).max(level);
                }
                let top = (level * base as f64) as usize;
                let peak = ((m.peaks[b] * base as f64) as usize).min(base.saturating_sub(1));
                for i in b * 3..(b * 3 + 2).min(w) {
                    for k in 0..top {
                        px[(base - 1 - k) * w + i] = paint(&CLASSIC, k as f64 / base as f64);
                        let mirror = base + k / 3;
                        if mirror < ph {
                            px[mirror * w + i] = scale(paint(&CLASSIC, k as f64 / base as f64), 0.22);
                        }
                    }
                    px[(base - 1 - peak) * w + i] = [0.9, 0.9, 0.95];
                }
            }
            Px { px: &mut px, pw: w, ph }.cells()
        }
        10 => {
            let mut d = Dots { dots: &mut m.dots, color: &mut m.px[..w * h], w, h };
            if live {
                d.fade(0.55f32.powf(k25 as f32)); // phosphor that glows on a moment
                let amp = 0.55 * (0.3 + 0.7 * f.energy);
                let color = mix(PHOSPHOR, paint(p, 0.9), 0.3);
                let n = w * 2;
                let norm: f64 = f.bands.iter().sum::<f64>().max(1.0);
                let mut last = None;
                for s in 0..=n {
                    let x = (s as f64 / n as f64 * 2.0 - 1.0) * aspect;
                    let y = f.bands.iter().enumerate().map(|(b, l)| l * (x * (1.0 + b as f64) * 2.1 + f.t * (2.0 + b as f64 * 0.7) + b as f64).sin()).sum::<f64>() / norm * amp * 3.0;
                    let pt = (x, y.clamp(-0.95, 0.95));
                    if let Some(prev) = last {
                        d.line(prev, pt, color);
                    }
                    last = Some(pt);
                }
            }
            d.cells([0.0, 0.04, 0.02])
        }
        _ => {
            let mut px = Px { px: &mut m.px, pw: w, ph: 2 * h };
            if live {
                let rot = 0.004 * k25 * spin;
                let (s, c) = rot.sin_cos();
                px.feedback(0.78f64.powf(k25), |x, y| (x * c - y * s, x * s + y * c));
                let inner = 0.12 + 0.05 * f.beat;
                for i in 0..64 {
                    let u = i as f64 / 64.0;
                    let a = u * TAU + f.t * 0.1 * spin;
                    let len = 0.05 + band(f, 1.0 - (u * 2.0 - 1.0).abs()) * 0.7 * (0.6 + f.energy);
                    let (ca, sa) = (a.cos(), a.sin());
                    px.line((ca * inner, sa * inner), (ca * (inner + len), sa * (inner + len)), scale(paint(p, u * 2.0 + f.t * 0.04), 0.7));
                }
            }
            px.cells()
        }
    }
}

// ---------- the show ----------

const TRANSITION: f64 = 1.8; // seconds of dissolve between scenes
const SETTLE: f64 = 15.0; // no automatic change in a song's first seconds
const EVERY: f64 = 30.0; // and at most this often
const MAX_RISE: f32 = 0.06; // how much brighter a frame may get than the one before

fn hash(s: &str) -> u32 {
    s.chars().fold(2166136261u32, |h, c| (h ^ c as u32).wrapping_mul(16777619))
}
// The scene a song opens on: the same every time it plays.
pub fn opening(id: &str) -> usize {
    hash(id) as usize % SCENES.len()
}

pub struct Show {
    ears: Ears,
    feel: Feel,
    pal: Pal,
    mems: Vec<Mem>,
    smoke: Trip,
    scene: usize,
    from: Option<(usize, f64)>, // dissolving away from this scene, since then
    changed_at: f64,
    title_at: f64,
    song: String,
    pub locked: bool,
    last_t: f64,
    last_lum: f32,
    cache: Option<(String, Vec<String>)>,
}

impl Default for Show {
    fn default() -> Show {
        Show {
            ears: Ears::default(), feel: Feel::default(), pal: to_pal(&[]), mems: (0..SCENES.len()).map(|_| Mem::default()).collect(),
            smoke: Trip::new(), scene: 0, from: None, changed_at: 0.0, title_at: 0.0, song: String::new(), locked: false,
            last_t: 0.0, last_lum: 0.0, cache: None,
        }
    }
}

impl Show {
    pub fn scene_name(&self) -> &'static str {
        SCENES[self.scene].0
    }
    // The trip was opened: the song's name is shown again.
    pub fn opened(&mut self, t: f64) {
        self.title_at = t;
        self.cache = None;
    }
    pub fn step(&mut self, dir: i64) {
        let n = SCENES.len() as i64;
        self.go((self.scene as i64 + dir).rem_euclid(n) as usize, true);
    }
    pub fn toggle_lock(&mut self) {
        self.locked = !self.locked;
        self.cache = None;
    }
    fn go(&mut self, to: usize, by_hand: bool) {
        if to == self.scene {
            return;
        }
        self.from = Some((self.scene, self.last_t));
        self.scene = to;
        self.changed_at = self.last_t;
        self.mems[to] = Mem::default(); // feedback scenes start from dark, not from a stale picture
        if by_hand {
            self.cache = None;
        }
    }

    // h rows of exactly w columns. t: playback time (stops while paused); pos: where the song is, in seconds.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(&mut self, w: usize, h: usize, t: f64, pos: f64, raw: &[f64], energy: f64, colors: &[Rgb], id: &str, title: &str, artist: &str, lyric: Option<&str>) -> Vec<String> {
        // paused and nothing changed: the very same picture, not even worked out again
        let key = format!("{w}x{h}|{t}|{id}|{}|{}|{:?}|{title}|{:?}", self.scene, self.locked, lyric, self.from.map(|f| f.0));
        if let Some((k, rows)) = &self.cache {
            if *k == key {
                return rows.clone();
            }
        }
        let cells = self.cells(w, h, t, pos, raw, energy, colors, id, title, artist, lyric);
        let rows = to_rows(&cells, w, h);
        let key = format!("{w}x{h}|{t}|{id}|{}|{}|{:?}|{title}|{:?}", self.scene, self.locked, lyric, self.from.map(|f| f.0));
        self.cache = Some((key, rows.clone()));
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn cells(&mut self, w: usize, h: usize, t: f64, pos: f64, raw: &[f64], energy: f64, colors: &[Rgb], id: &str, title: &str, artist: &str, lyric: Option<&str>) -> Vec<Cell> {
        let (w, h) = (w.max(1), h.max(1));
        let dt = (t - self.last_t).clamp(0.0, 0.1);
        // a new song: its own scene, its name shown, colors that drift over to its cover
        if id != self.song {
            let first = self.song.is_empty();
            self.song = id.to_string();
            self.feel.seed = hash(id);
            self.title_at = t;
            if first {
                self.scene = opening(id);
                self.last_t = t;
                self.pal = to_pal(colors);
            } else if !self.locked {
                self.last_t = t;
                self.go(opening(id), false);
            }
        }
        self.last_t = t;
        self.feel.t = t;
        self.feel.dt = dt;
        self.ears.listen(&mut self.feel, raw, energy);
        let target = to_pal(colors);
        let k = (dt / 2.0).min(1.0) as f32;
        for (c, to) in self.pal.iter_mut().zip(target) {
            *c = mix(*c, to, k);
        }
        // a strong moment of the song moves on to another scene
        if !self.locked && pos >= SETTLE && t - self.changed_at >= EVERY && self.ears.strong_moment() && self.from.is_none() {
            let n = SCENES.len();
            let to = (self.scene + 1 + (hash(&format!("{id}{t}")) as usize % (n - 1))) % n;
            self.go(to, false);
        }
        let song = Song { id, raw, colors, energy };
        let mut cells = draw(self.scene, &mut self.mems[self.scene], &self.feel, &self.pal, w, h, &mut self.smoke, &song);
        if let Some((old, since)) = self.from {
            let p = if dt > 0.0 { (t - since) / TRANSITION } else { 1.0 }; // paused: no dissolve to wait for
            if p >= 1.0 {
                self.from = None;
            } else {
                let before = draw(old, &mut self.mems[old], &self.feel, &self.pal, w, h, &mut self.smoke, &song);
                dissolve(&mut cells, &before, w, p);
            }
        }
        self.overlays(&mut cells, w, h, t, dt, title, artist, lyric);
        // no flashes: a frame may only get a little brighter than the last
        let b = brightness(&cells);
        if dt > 0.0 && b > self.last_lum + MAX_RISE && b > 0.0 {
            let k = (self.last_lum + MAX_RISE) / b;
            cells.iter_mut().for_each(|c| {
                c.fg = c.fg.map(|v| v * k);
                c.bg = c.bg.map(|v| v * k);
            });
        }
        self.last_lum = brightness(&cells);
        cells
    }

    #[allow(clippy::too_many_arguments)]
    fn overlays(&self, cells: &mut [Cell], w: usize, h: usize, t: f64, dt: f64, title: &str, artist: &str, lyric: Option<&str>) {
        // fades in over 0.4 s, stays, fades out over 0.8 s; shown at once while paused
        let alpha = |age: f64, stay: f64| {
            if !(0.0..stay + 0.8).contains(&age) {
                0.0
            } else if age < 0.4 && dt > 0.0 {
                age / 0.4
            } else if age > stay {
                1.0 - (age - stay) / 0.8
            } else {
                1.0
            }
        };
        let light = mix(self.pal[3], [1.0; 3], 0.6);
        if h >= 6 && !title.is_empty() {
            let text = if artist.is_empty() { title.to_string() } else { format!("{title} — {artist}") };
            label(cells, w, 1, 2, &text, alpha(t - self.title_at, 4.0) as f32, light);
        }
        if h >= 4 {
            let name = format!("{}{}", SCENES[self.scene].0, if self.locked { " · held" } else { "" });
            let a = if self.locked { 0.55 } else { alpha(t - self.changed_at, 3.0) as f32 };
            label(cells, w, h - 2, 2, &name, a, mix(self.pal[2], [1.0; 3], 0.4));
        }
        if let Some(line) = lyric.filter(|l| !l.trim().is_empty() && h >= 8) {
            let n = line.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>().min(w.saturating_sub(4));
            label(cells, w, h - 4, w.saturating_sub(n) / 2, line, 1.0, [1.0; 3]);
        }
    }
}

// Writes text on a darkened band, so it reads on any scene. alpha 0 draws nothing.
fn label(cells: &mut [Cell], w: usize, row: usize, col: usize, text: &str, alpha: f32, color: [f32; 3]) {
    if alpha <= 0.01 || col >= w {
        return;
    }
    let start = row * w;
    let mut i = col;
    let dark = |c: &mut Cell, k: f32| {
        c.fg = c.fg.map(|v| v * (1.0 - 0.7 * k));
        c.bg = c.bg.map(|v| v * (1.0 - 0.7 * k));
    };
    if col > 0 {
        dark(&mut cells[start + col - 1], alpha);
    }
    for ch in text.chars().filter(|c| !c.is_control()) {
        let cw = ch.width().unwrap_or(0);
        if cw == 0 {
            continue;
        }
        if i + cw > w.saturating_sub(1) {
            break;
        }
        let c = &mut cells[start + i];
        dark(c, alpha);
        c.ch = ch;
        c.fg = mix(c.bg, color, alpha);
        if cw == 2 {
            let next = &mut cells[start + i + 1];
            next.ch = SKIP;
        }
        i += cw;
    }
    if i < w {
        dark(&mut cells[start + i], alpha);
    }
}

// From `before` to `cells` as p goes 0 to 1: colors blend, and each cell takes the new character at its own moment.
fn dissolve(cells: &mut [Cell], before: &[Cell], w: usize, p: f64) {
    let p = (p * p * (3.0 - 2.0 * p)) as f32;
    for (n, (c, b)) in cells.iter_mut().zip(before).enumerate() {
        let order = (((n % w) as u32).wrapping_mul(73) ^ ((n / w) as u32).wrapping_mul(151)) % 97;
        let switched = (order as f32 / 97.0) < p;
        let (fg, bg) = (mix(b.fg, c.fg, p), mix(b.bg, c.bg, p));
        // a wide character's second half follows its first
        let ch = if switched { c.ch } else { b.ch };
        *c = Cell { ch, fg, bg };
    }
    // keep wide characters whole: a SKIP must follow a wide character
    for n in 0..cells.len() {
        let wide = cells[n].ch.width().unwrap_or(1) == 2;
        if wide && (n + 1) % w != 0 {
            cells[n + 1].ch = SKIP;
        } else if cells[n].ch == SKIP && (n % w == 0 || cells[n - 1].ch.width().unwrap_or(1) != 2) {
            cells[n].ch = ' ';
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::visible;

    const PAL: [Rgb; 3] = [[30, 60, 20], [200, 40, 40], [240, 220, 120]];

    fn bands(loud: bool, t: f64) -> Vec<f64> {
        let hit = loud && t % 0.5 < 0.06;
        (0..32).map(|k| if !loud { 0.0 } else if k < 6 { if hit { 1.0 } else { 0.3 } } else { 0.5 + 0.4 * ((t + k as f64) * 3.0).sin() }).collect()
    }

    #[test]
    fn every_scene_fills_the_screen_exactly() {
        for (scene, _) in SCENES.iter().enumerate() {
            for (w, h) in [(80, 24), (37, 9), (200, 60), (12, 4)] {
                for loud in [false, true] {
                    let mut show = Show::default();
                    show.cells(w, h, 0.0, 0.0, &bands(loud, 0.0), 0.5, &PAL, "song", "Title 日本 ♫", "Band", Some("la la"));
                    show.scene = scene;
                    let mut t = 0.0;
                    for _ in 0..4 {
                        t += 0.04;
                        let rows = show.frame(w, h, t, 20.0, &bands(loud, t), if loud { 0.9 } else { 0.0 }, &PAL, "song", "Title 日本 ♫", "Band", Some("la la"));
                        assert_eq!(rows.len(), h);
                        for r in &rows {
                            assert_eq!(crate::ui::width(&visible(r)), w, "{} at {w}x{h} loud {loud}", SCENES[scene].0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_song_always_opens_the_same_and_songs_differ() {
        assert_eq!(opening("abc"), opening("abc"));
        let seen: std::collections::HashSet<usize> = (0..40).map(|i| opening(&format!("song-{i}"))).collect();
        assert!(seen.len() >= 8, "{seen:?}");
        let mut show = Show::default();
        show.frame(40, 10, 0.0, 0.0, &bands(false, 0.0), 0.0, &PAL, "abc", "", "", None);
        assert_eq!(show.scene, opening("abc"));
    }

    // Drives a song for `secs`: quiet, then loud from `drop` on. Returns when the scene changed (song time).
    fn changes(show: &mut Show, secs: f64, drop: f64) -> Vec<f64> {
        let (mut t, mut out, mut scene) = (0.0, vec![], show.scene);
        while t < secs {
            t += 0.04;
            let loud = t >= drop;
            show.cells(30, 8, t, t, &bands(loud, t), if loud { 0.9 } else { 0.1 }, &PAL, "song", "", "", None);
            if show.scene != scene {
                out.push(t);
                scene = show.scene;
            }
        }
        out
    }

    #[test]
    fn strong_moments_change_the_scene_but_not_too_soon_nor_too_often() {
        let mut show = Show::default();
        let at = changes(&mut show, 9.0, 5.0);
        assert!(at.is_empty(), "nothing in the first 15 s: {at:?}");
        let mut show = Show::default();
        let at = changes(&mut show, 120.0, 40.0);
        assert!(!at.is_empty() && at[0] >= 40.0, "the drop moves it on: {at:?}");
        assert!(at.windows(2).all(|p| p[1] - p[0] >= EVERY), "{at:?}");
        let mut show = Show::default();
        show.toggle_lock();
        assert!(changes(&mut show, 120.0, 40.0).is_empty(), "held");
    }

    #[test]
    fn nothing_flashes() {
        for (scene, _) in SCENES.iter().enumerate() {
            let mut show = Show::default();
            show.cells(60, 20, 0.0, 0.0, &bands(false, 0.0), 0.0, &PAL, "song", "", "", None);
            show.scene = scene;
            let (mut t, mut last) = (0.0, None::<f32>);
            for n in 0..200 {
                t += 0.04;
                let loud = n % 6 < 3; // loud and silent every few frames: the worst case
                let c = show.cells(60, 20, t, 20.0, &bands(loud, t), if loud { 1.0 } else { 0.0 }, &PAL, "song", "", "", None);
                let b = brightness(&c);
                if let Some(l) = last {
                    assert!(b <= l + MAX_RISE + 0.005, "{} jumps from {l} to {b}", SCENES[scene].0);
                }
                last = Some(b);
            }
        }
    }

    #[test]
    fn paused_holds_still_and_keys_still_work() {
        let mut show = Show::default();
        let mut t = 0.0;
        for _ in 0..30 {
            t += 0.04;
            show.frame(50, 12, t, 20.0, &bands(true, t), 0.8, &PAL, "song", "T", "A", None);
        }
        let a = show.frame(50, 12, t, 20.0, &bands(true, t), 0.8, &PAL, "song", "T", "A", None);
        let b = show.frame(50, 12, t, 20.0, &bands(false, t), 0.1, &PAL, "song", "T", "A", None);
        assert_eq!(a, b, "paused: the same picture whatever the sound does");
        let before = show.scene;
        show.step(1);
        let c = show.frame(50, 12, t, 20.0, &bands(true, t), 0.8, &PAL, "song", "T", "A", None);
        assert_ne!(show.scene, before);
        assert_ne!(a, c, "a scene picked by hand shows while paused");
        assert!(visible(&c[10]).contains(SCENES[show.scene].0), "with its name");
    }
}
#[cfg(test)]
mod leak {
    use super::*;
    #[test]
    fn no_escape_is_cut() {
        let mut show = Show::default();
        let mut t = 0.0;
        for n in 0..300 {
            t += 0.04;
            let rows = show.frame(76, 20, t, 20.0, &[0.5; 32], 0.7, &[[200, 40, 40]], "song", "Da Funk", "Daft Punk", None);
            for r in &rows {
                let stray = r.replace("\x1b[", "");
                assert!(!stray.contains("[38") && !stray.contains("\x1b"), "frame {n}: {r:?}");
            }
        }
    }
}
