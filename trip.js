// Psychedelic full-screen visualizer in ASCII, after Windows Media Player's "Battery" and MilkDrop. One ethereal field,
// like smoke, seen as an endless tunnel: shapes are born in the middle and drift outwards, so it always feels like
// flying forward through it, at the song's pace. Each song gets its own look from its id (symmetry, spiral, swirl size,
// noise seed), and the music keeps shaping it: the bass bends and wobbles the smoke, the spectrum lights it by angle, the tempo and loudness set the speed, each beat pushes forward and sends a soft
// ripple out, and the dynamic range sets how strongly the symmetry shows. Everything that comes from the sound is
// smoothed, so nothing flickers. Colors come from the cover's palette, carried by the flow. The background is never black.
const RAMP = " .`':-~=+*";

const avg = (a, i, j) => a.slice(i, j).reduce((s, v) => s + v, 0) / (j - i);
const sat = v => Math.max(0, Math.min(1, v));
const fract = x => x - Math.floor(x);
const hash = s => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);

// The palette (dark to light) as a gradient that goes there and back: any g lands on it, 0 dark, 1 light, 2 dark again.
function along(pal, g) {
  const u = 1 - Math.abs((((g % 2) + 2) % 2) - 1), x = u * (pal.length - 1), i = Math.floor(x), j = Math.min(pal.length - 1, i + 1);
  return pal[i].map((c, k) => c + (pal[j][k] - c) * (x - i));
}
const mix = (c, to, f) => c.map((v, k) => v + (to[k] - v) * f);

// Smooth 3D value noise, 0..1, and its sum over octaves (fbm). 3D: two dimensions wrap around the tunnel, one goes along it.
const rnd = (i, j, k, seed) => fract(Math.sin(i * 127.1 + j * 311.7 + k * 74.7 + seed * 13.3) * 43758.5453);
const ease = f => f * f * (3 - 2 * f);
function noise(x, y, z, seed) {
  const i = Math.floor(x), j = Math.floor(y), k = Math.floor(z), u = ease(x - i), v = ease(y - j), w = ease(z - k);
  const lerp = (a, b, f) => a + (b - a) * f;
  const plane = kk => lerp(lerp(rnd(i, j, kk, seed), rnd(i + 1, j, kk, seed), u), lerp(rnd(i, j + 1, kk, seed), rnd(i + 1, j + 1, kk, seed), u), v);
  return lerp(plane(k), plane(k + 1), w);
}
function fbm(x, y, z, seed, oct) {
  let s = 0, amp = 0.5, norm = 0;
  for (let o = 0; o < oct; o++) { s += amp * noise(x, y, z, seed); norm += amp; x = x * 2.03 + 1.7; y = y * 2.03 + 9.2; z = z * 2.03 + 4.1; amp *= 0.5; }
  return s / norm;
}

// A song's own look, the same every time it plays.
export function look(id = '') {
  const h = hash(id), pick = (n, k) => Math.floor(h / k) % n;
  return {
    seed: h % 1000,
    fold: [1, 2, 3, 4, 5, 6, 8][pick(7, 1)], // 1: no symmetry
    twist: (pick(9, 7) - 4) * 0.15,           // how much the tunnel spirals
    scale: 3 + pick(10, 2521) / 4,             // 3..5.25: how many swirls fit around it
    turn: pick(2, 31) ? 1 : -1,
    hue: pick(100, 997) / 100,                 // where on the palette it starts
  };
}

// What the visualizer has heard of the song so far.
let song, L = look(), lastT = 0, ph = 0, travel = 0, push = 0, avgBass = 0, cool = 0, pulse = 0, drift = 0;
let smooth = null, sEnergy = 0; // the bands and loudness, eased so the picture never jumps
let beats = [], loudHist = [], histAt = 0, feel = [0.5, 0.5, 0.3];
export const heard = () => feel; // [tempo, loudness, dynamic range], each 0..1

const median = a => [...a].sort((x, y) => x - y)[a.length >> 1];
// [tempo (0 = 60 BPM, 1 = 180), loudness, dynamic range] from the beats and loudness of the last seconds
function listen(t, energy) {
  const gaps = beats.slice(1).map((b, i) => b - beats[i]).filter(g => g > 0.25 && g < 1.5); // 40..240 BPM
  const bpm = gaps.length >= 3 ? 60 / median(gaps) : 120;
  if (t - histAt > 0.1) { histAt = t; loudHist.push(energy); if (loudHist.length > 150) loudHist.shift(); } // 15 s
  const sorted = [...loudHist].sort((x, y) => x - y), at = f => sorted[Math.floor(f * (sorted.length - 1))] ?? 0;
  return [sat((bpm - 60) / 120), at(0.5), sat((at(0.9) - at(0.1)) * 2)];
}

// h rows of exactly w visible characters. t is playback time in seconds (it stops while paused), bands the 0..1 levels,
// pal the cover's palette (dark to light, at least one color), id the song, energy how loud it really is (0..1): a soft
// song stays dim and slow even though its bands are scaled up to their own peaks.
export function frame(w, h, t, bands, pal, id, energy = 1) {
  if (id && id !== song) { song = id; L = look(id); beats = []; loudHist = []; } // a new song is listened to from scratch
  const dt = Math.max(0, Math.min(0.1, t - lastT));
  lastT = t;
  // ease towards the sound: up in about 0.15 s, down in about 0.6 s (the raw levels jump every frame and made it flicker)
  const ease = (from, to) => from + (to - from) * Math.min(1, dt / (to > from ? 0.15 : 0.6));
  smooth = smooth ? smooth.map((v, i) => ease(v, bands[i] * (0.25 + 0.75 * energy))) : bands.map(() => 0);
  sEnergy = ease(sEnergy, energy);
  const B = smooth, band = u => { const x = u * (B.length - 1), i = Math.floor(x), j = Math.min(B.length - 1, i + 1); return B[i] + (B[j] - B[i]) * (x - i); };
  const bass = avg(B, 0, 6), mid = avg(B, 6, 19);
  if (dt) {
    const raw = avg(bands, 0, 6) * (0.25 + 0.75 * energy); // beats need the unsmoothed bass
    avgBass += (raw - avgBass) * Math.min(1, dt * 2);
    if ((cool -= dt) <= 0 && raw > avgBass * 1.3 + 0.08) { // a beat: the bass jumps above its recent average
      cool = 0.25; pulse = energy; push = energy; drift += 0.04 * energy;
      beats.push(t); if (beats.length > 16) beats.shift();
    }
    pulse *= Math.exp(-dt * 1.5); push *= Math.exp(-dt * 3);
    const target = listen(t, energy);
    feel = feel.map((f, k) => f + (target[k] - f) * Math.min(1, dt / 3)); // the song's feel, over a few seconds
    const pace = (0.3 + feel[0] * 0.9) * (0.25 + 0.75 * sEnergy); // faster with the tempo, slower when soft
    travel += dt * (pace + push * 1.5); // forward through the tunnel, with a surge on each beat
    ph += dt * pace * 0.3;
  }
  const warp = 0.6 + bass * 1.2 + mid * 0.5, sym = sat(0.25 + feel[2] * 1.2), ring = (1 - pulse) * 1.4;
  const q = c => c.map(n => Math.min(255, Math.round(n)) & 0xf0).join(';'); // shortcut: 16 steps per channel so neighbors share one escape code
  const rows = [];
  for (let j = 0; j < h; j++) {
    let line = '', last = '';
    const y0 = (j - h / 2 + 0.5) / (h / 2);
    // [brightness, flow] of column i
    const smoke = i => {
      const x0 = (i - w / 2 + 0.5) / h; // a cell is about twice as tall as wide
      // the whole picture wobbles on a slow flow of its own (harder with the bass), so it is never a perfect tunnel
      const wob = 0.5 + bass * 0.6;
      const X = x0 + (Math.sin(y0 * 2.3 + ph * 0.9 + L.seed) + Math.sin((x0 + y0) * 1.7 - ph * 0.6)) * 0.12 * wob;
      const Y = y0 + (Math.sin(x0 * 2.1 - ph * 0.7 + L.seed * 0.7) + Math.sin((x0 - y0) * 1.9 + ph * 0.5)) * 0.12 * wob;
      const r = Math.hypot(X, Y);
      const depth = -Math.log(Math.max(r, 0.12)) * 1.1; // far in the middle, near at the edges; the middle is a far wall
      const a = Math.atan2(Y, X) + ph * 0.15 * L.turn + depth * L.twist;
      // a point on the tunnel wall: around it (a circle, so there is no seam) and along it (depth, moving towards you)
      let cx = Math.cos(a), cy = Math.sin(a);
      if (L.fold > 1) { // mirror into fold slices, as strongly as the song's dynamics ask (blended on the circle: blending
        const sl = Math.PI * 2 / L.fold, f = Math.abs((((a % sl) + sl) % sl) - sl / 2); // angles breaks where they wrap)
        cx += (Math.cos(f) - cx) * sym; cy += (Math.sin(f) - cy) * sym;
      }
      cx *= L.scale; cy *= L.scale;
      const z = depth + travel;
      const qx = fbm(cx, cy, z * 0.7 + ph, L.seed, 2); // smoke bent by smoke
      let v = fbm(cx + warp * qx, cy - warp * qx, z + warp * qx * 0.6, L.seed, 3);
      v = sat((v - 0.32) * 2.4) ** 1.3; // keep the wisps, drop the haze
      v *= 0.6 + band(Math.abs(Math.atan2(cy, cx)) / Math.PI) * 0.7; // the spectrum lights it by angle, mirrored like the folds
      v += 0.3 * pulse * Math.exp(-(((r - ring) * 5) ** 2)); // the beat's ripple
      return [sat(v * (0.45 + 0.55 * sEnergy)), qx];
    };
    // the smoke is smooth, so only every other column is worked out and the ones between are averaged: half the work
    const S = [], at = i => (S[i] ??= smoke(i));
    for (let i = 0; i < w; i++) {
      const [v, qx] = i % 2 === 0 || i + 1 >= w ? at(i - (i % 2)) : at(i - 1).map((n, k) => (n + at(i + 1)[k]) / 2);
      const c = along(pal, L.hue + v * 0.9 + qx * 0.8 + drift);
      // never black: the background is the palette color dimmed where the smoke is thin, lifted if the cover is very dark
      let bg = c.map(n => n * (0.4 + v * 0.6));
      const top = Math.max(...bg);
      if (top < 70) bg = top ? bg.map(n => n * 70 / top) : [70, 70, 70];
      const col = `38;2;${q(mix(c, [255, 255, 255], 0.3 + v * 0.5))};48;2;${q(bg)}`;
      if (col !== last) { line += `\x1b[${col}m`; last = col; }
      line += RAMP[Math.min(RAMP.length - 1, Math.floor(v * RAMP.length))];
    }
    rows.push(line + '\x1b[39;49m');
  }
  return rows;
}
