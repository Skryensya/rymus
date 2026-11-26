// "the old smoke", one of the scenes of the full screen visualizer (src/scenes.rs): psychedelic, in ASCII, after Windows Media Player's "Battery" and MilkDrop. One ethereal field,
// like smoke, seen as an endless tunnel: shapes are born in the middle and drift outwards, so it always feels like
// flying forward through it, at the song's pace. Each song gets its own look from its id (symmetry, spiral, swirl size,
// noise seed), and the music keeps shaping it: the bass bends and wobbles the smoke, the spectrum lights it by angle,
// the tempo and loudness set the speed, each beat pushes forward and sends a soft ripple out, and the dynamic range sets
// how strongly the symmetry shows. Everything that comes from the sound is smoothed, so nothing flickers. Colors come
// from the cover's palette, carried by the flow. The background is never black.
use crate::scenes::Cell;
use crate::ui::Rgb;
use std::f64::consts::PI;

const RAMP: &[u8] = b" .`':-~=+*";

fn avg(a: &[f64]) -> f64 {
    a.iter().sum::<f64>() / a.len() as f64
}
fn sat(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}
fn fract(x: f64) -> f64 {
    x - x.floor()
}
fn hash(s: &str) -> u32 {
    s.chars().fold(7u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32))
}

// The palette (dark to light) as a gradient that goes there and back: any g lands on it, 0 dark, 1 light, 2 dark again.
fn along(pal: &[Rgb], g: f64) -> [f64; 3] {
    let u = 1.0 - (((g % 2.0) + 2.0) % 2.0 - 1.0).abs();
    let x = u * (pal.len() - 1) as f64;
    let i = x.floor() as usize;
    let j = (i + 1).min(pal.len() - 1);
    std::array::from_fn(|k| pal[i][k] as f64 + (pal[j][k] as f64 - pal[i][k] as f64) * (x - i as f64))
}

// Smooth 3D value noise, 0..1, and its sum over octaves. 3D: two dimensions wrap around the tunnel, one goes along it.
fn rnd(i: f64, j: f64, k: f64, seed: f64) -> f64 {
    fract((i * 127.1 + j * 311.7 + k * 74.7 + seed * 13.3).sin() * 43758.5453)
}
fn smoothstep(f: f64) -> f64 {
    f * f * (3.0 - 2.0 * f)
}
fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + (b - a) * f
}
fn noise(x: f64, y: f64, z: f64, seed: f64) -> f64 {
    let (i, j, k) = (x.floor(), y.floor(), z.floor());
    let (u, v, w) = (smoothstep(x - i), smoothstep(y - j), smoothstep(z - k));
    let plane = |kk| {
        lerp(
            lerp(rnd(i, j, kk, seed), rnd(i + 1.0, j, kk, seed), u),
            lerp(rnd(i, j + 1.0, kk, seed), rnd(i + 1.0, j + 1.0, kk, seed), u),
            v,
        )
    };
    lerp(plane(k), plane(k + 1.0), w)
}
fn fbm(mut x: f64, mut y: f64, mut z: f64, seed: f64, octaves: usize) -> f64 {
    let (mut s, mut amp, mut norm) = (0.0, 0.5, 0.0);
    for _ in 0..octaves {
        s += amp * noise(x, y, z, seed);
        norm += amp;
        (x, y, z) = (x * 2.03 + 1.7, y * 2.03 + 9.2, z * 2.03 + 4.1);
        amp *= 0.5;
    }
    s / norm
}

// A song's own look, the same every time it plays.
#[derive(Debug, PartialEq)]
pub struct Look {
    seed: f64,
    fold: u32,  // 1: no symmetry
    twist: f64, // how much the tunnel spirals
    scale: f64, // how many swirls fit around it
    turn: f64,
    hue: f64, // where on the palette it starts
}
pub fn look(id: &str) -> Look {
    let h = hash(id);
    let pick = |n: u32, k: u32| (h / k) % n;
    Look {
        seed: (h % 1000) as f64,
        fold: [1, 2, 3, 4, 5, 6, 8][pick(7, 1) as usize],
        twist: (pick(9, 7) as f64 - 4.0) * 0.15,
        scale: 3.0 + pick(10, 2521) as f64 / 4.0,
        turn: if pick(2, 31) == 1 { 1.0 } else { -1.0 },
        hue: pick(100, 997) as f64 / 100.0,
    }
}

// What the visualizer has heard of the song so far.
pub struct Trip {
    song: String,
    look: Look,
    last_t: f64,
    phase: f64,
    travel: f64,
    push: f64,
    avg_bass: f64,
    cool: f64,
    pulse: f64,
    drift: f64,
    smooth: Option<Vec<f64>>, // the bands and loudness, eased so the picture never jumps
    energy: f64,
    beats: Vec<f64>,
    loud: Vec<f64>,
    heard_at: f64,
    pub feel: [f64; 3], // [tempo (0 = 60 BPM, 1 = 180), loudness, dynamic range], each 0..1
}

impl Trip {
    pub fn new() -> Trip {
        Trip {
            song: String::new(), look: look(""), last_t: 0.0, phase: 0.0, travel: 0.0, push: 0.0, avg_bass: 0.0, cool: 0.0,
            pulse: 0.0, drift: 0.0, smooth: None, energy: 0.0, beats: vec![], loud: vec![], heard_at: 0.0, feel: [0.5, 0.5, 0.3],
        }
    }

    // The song's tempo, loudness and dynamic range from the beats and loudness of the last seconds.
    fn listen(&mut self, t: f64, energy: f64) -> [f64; 3] {
        let mut gaps: Vec<f64> = self.beats.windows(2).map(|b| b[1] - b[0]).filter(|&g| g > 0.25 && g < 1.5).collect(); // 40..240 BPM
        gaps.sort_by(f64::total_cmp);
        let bpm = if gaps.len() >= 3 { 60.0 / gaps[gaps.len() / 2] } else { 120.0 };
        if t - self.heard_at > 0.1 {
            self.heard_at = t;
            self.loud.push(energy);
            if self.loud.len() > 150 {
                self.loud.remove(0); // 15 s
            }
        }
        let mut sorted = self.loud.clone();
        sorted.sort_by(f64::total_cmp);
        let at = |f: f64| sorted.get((f * (sorted.len().max(1) - 1) as f64).floor() as usize).copied().unwrap_or(0.0);
        [sat((bpm - 60.0) / 120.0), at(0.5), sat((at(0.9) - at(0.1)) * 2.0)]
    }

    // h rows of exactly w visible characters. t is playback time in seconds (it stops while paused), bands the 0..1
    // levels, pal the cover's palette (dark to light, at least one color), id the song, energy how loud it really is
    // (0..1): a soft song stays dim and slow even though its bands are scaled up to their own peaks.
    #[allow(clippy::too_many_arguments)]
    #[cfg(test)] // the app draws it through the scenes; its own test still looks at it as rows
    pub fn frame(&mut self, w: usize, h: usize, t: f64, bands: &[f64], pal: &[Rgb], id: &str, energy: f64) -> Vec<String> {
        let cells = self.cells(w, h, t, bands, pal, id, energy);
        crate::scenes::to_rows(&cells, w, h)
    }
    // The same picture as cells, for the scenes (src/scenes.rs), which this is one of.
    #[allow(clippy::too_many_arguments)]
    pub fn cells(&mut self, w: usize, h: usize, t: f64, bands: &[f64], pal: &[Rgb], id: &str, energy: f64) -> Vec<Cell> {
        if !id.is_empty() && id != self.song {
            self.song = id.to_string(); // a new song is listened to from scratch
            self.look = look(id);
            self.beats.clear();
            self.loud.clear();
        }
        let dt = (t - self.last_t).clamp(0.0, 0.1);
        self.last_t = t;
        // ease towards the sound: up in about 0.15 s, down in about 0.6 s (the raw levels jump and made it flicker)
        let ease = |from: f64, to: f64| from + (to - from) * (dt / if to > from { 0.15 } else { 0.6 }).min(1.0);
        let gain = 0.25 + 0.75 * energy;
        let smooth: Vec<f64> = match &self.smooth {
            Some(old) => old.iter().zip(bands).map(|(&o, &b)| ease(o, b * gain)).collect(),
            None => vec![0.0; bands.len()],
        };
        self.energy = ease(self.energy, energy);
        let (bass, mid) = (avg(&smooth[0..6]), avg(&smooth[6..19]));
        if dt > 0.0 {
            let raw = avg(&bands[0..6]) * gain; // beats need the unsmoothed bass
            self.avg_bass += (raw - self.avg_bass) * (dt * 2.0).min(1.0);
            self.cool -= dt;
            if self.cool <= 0.0 && raw > self.avg_bass * 1.3 + 0.08 {
                // a beat: the bass jumps above its recent average
                self.cool = 0.25;
                self.pulse = energy;
                self.push = energy;
                self.drift += 0.04 * energy;
                self.beats.push(t);
                if self.beats.len() > 16 {
                    self.beats.remove(0);
                }
            }
            self.pulse *= (-dt * 1.5).exp();
            self.push *= (-dt * 3.0).exp();
            let target = self.listen(t, energy);
            for (f, to) in self.feel.iter_mut().zip(target) {
                *f += (to - *f) * (dt / 3.0).min(1.0); // the song's feel, over a few seconds
            }
            // faster with the tempo, slower when soft
            let pace = (0.3 + self.feel[0] * 0.9) * (0.25 + 0.75 * self.energy);
            self.travel += dt * (pace + self.push * 1.5); // forward through the tunnel, with a surge on each beat
            self.phase += dt * pace * 0.3;
        }
        let band = |u: f64| {
            let x = u * (smooth.len() - 1) as f64;
            let i = (x.floor() as usize).min(smooth.len() - 1);
            let j = (i + 1).min(smooth.len() - 1);
            smooth[i] + (smooth[j] - smooth[i]) * (x - i as f64)
        };
        let warp = 0.6 + bass * 1.2 + mid * 0.5;
        let sym = sat(0.25 + self.feel[2] * 1.2);
        let ring = (1.0 - self.pulse) * 1.4;
        let (ph, travel, pulse, drift, loudness, l) = (self.phase, self.travel, self.pulse, self.drift, self.energy, &self.look);

        let mut out = Vec::with_capacity(w * h);
        for j in 0..h {
            let y0 = (j as f64 - h as f64 / 2.0 + 0.5) / (h as f64 / 2.0);
            // (brightness, flow) of column i
            let smoke = |i: usize| -> (f64, f64) {
                let x0 = (i as f64 - w as f64 / 2.0 + 0.5) / h as f64; // a cell is about twice as tall as wide
                // the whole picture wobbles on a slow flow of its own (harder with the bass): never a perfect tunnel
                let wob = 0.5 + bass * 0.6;
                let x = x0 + ((y0 * 2.3 + ph * 0.9 + l.seed).sin() + ((x0 + y0) * 1.7 - ph * 0.6).sin()) * 0.12 * wob;
                let y = y0 + ((x0 * 2.1 - ph * 0.7 + l.seed * 0.7).sin() + ((x0 - y0) * 1.9 + ph * 0.5).sin()) * 0.12 * wob;
                let r = x.hypot(y);
                let depth = -r.max(0.12).ln() * 1.1; // far in the middle, near at the edges
                let a = y.atan2(x) + ph * 0.15 * l.turn + depth * l.twist;
                // a point on the tunnel wall: around it (a circle, so there is no seam) and along it (depth)
                let (mut cx, mut cy) = (a.cos(), a.sin());
                if l.fold > 1 {
                    // mirror into fold slices, as strongly as the song's dynamics ask (blended on the circle)
                    let slice = PI * 2.0 / l.fold as f64;
                    let f = (((a % slice) + slice) % slice - slice / 2.0).abs();
                    cx += (f.cos() - cx) * sym;
                    cy += (f.sin() - cy) * sym;
                }
                cx *= l.scale;
                cy *= l.scale;
                let z = depth + travel;
                let qx = fbm(cx, cy, z * 0.7 + ph, l.seed, 2); // smoke bent by smoke
                let mut v = fbm(cx + warp * qx, cy - warp * qx, z + warp * qx * 0.6, l.seed, 3);
                v = sat((v - 0.32) * 2.4).powf(1.3); // keep the wisps, drop the haze
                v *= 0.6 + band(cy.atan2(cx).abs() / PI) * 0.7; // the spectrum lights it by angle, mirrored
                v += 0.3 * pulse * (-((r - ring) * 5.0).powi(2)).exp(); // the beat's ripple
                (sat(v * (0.45 + 0.55 * loudness)), qx)
            };
            // the smoke is smooth: only every other column is worked out and the ones between are averaged
            let mut done: Vec<Option<(f64, f64)>> = vec![None; w];
            let mut at = |i: usize| *done[i].get_or_insert_with(|| smoke(i));
            for i in 0..w {
                let (v, qx) = if i % 2 == 0 || i + 1 >= w {
                    at(i - i % 2)
                } else {
                    let (a, b) = (at(i - 1), at(i + 1));
                    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
                };
                let c = along(pal, l.hue + v * 0.9 + qx * 0.8 + drift);
                // never black: the palette color dimmed where the smoke is thin, lifted if the cover is very dark
                let mut back = c.map(|n| n * (0.4 + v * 0.6));
                let top = back.iter().cloned().fold(0.0, f64::max);
                if top < 70.0 {
                    back = if top > 0.0 { back.map(|n| n * 70.0 / top) } else { [70.0; 3] };
                }
                let fg = c.map(|n| ((n + (255.0 - n) * (0.3 + v * 0.5)) / 255.0) as f32);
                let bg = back.map(|n| (n / 255.0).clamp(0.0, 1.0) as f32);
                out.push(Cell { ch: RAMP[((v * RAMP.len() as f64) as usize).min(RAMP.len() - 1)] as char, fg, bg });
            }
        }
        self.smooth = Some(smooth);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::visible;

    const PAL: [Rgb; 2] = [[30, 60, 20], [200, 40, 40]];

    #[test]
    fn rows_are_exact_and_the_tempo_is_heard() {
        let mut trip = Trip::new();
        let mut t = 0.0;
        for b in [0.0, 1.0] {
            t += 0.05;
            let rows = trip.frame(37, 9, t, &[b; 32], &PAL, "song-a", 1.0);
            assert_eq!(rows.len(), 9);
            assert!(rows.iter().all(|r| visible(r).chars().count() == 37));
        }
        // listen for 40 s at 25 fps with a bass hit every `gap` seconds
        let mut listen = |id: &str, gap: f64, energy: f64| {
            for _ in 0..1000 {
                t += 0.04;
                let hit = t % gap < 0.05;
                let bands: Vec<f64> = (0..32).map(|k| if k < 6 { if hit { 1.0 } else { 0.2 } } else { 0.3 }).collect();
                trip.frame(20, 6, t, &bands, &PAL, id, energy);
            }
            trip.feel[0]
        };
        assert!(listen("slow", 1.0, 0.3) < 0.1); // 60 BPM
        assert!((listen("fast", 0.4, 0.9) - 0.75).abs() < 0.1); // 150 BPM
        assert_eq!(look("a"), look("a"));
        assert_ne!(look("a"), look("b"));
    }
}
