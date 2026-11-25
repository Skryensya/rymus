// Psychedelic full-screen visualizer in ASCII, after Windows Media Player's "Battery" and MilkDrop. Like them it is a
// feedback loop: every frame starts from the previous one, zoomed, rotated and faded, so whatever is drawn leaves
// trails. On top of that each frame paints the current mode's pattern and a ring shaped by the spectrum.
// It follows the song: loudness sets how much gets painted (silence fades to black), the bass drives zoom and beats
// (flash, kick, a turn the other way), the mids the rotation, the treble the length of the trails. A new song picks
// its own mode and turn; a big drop changes mode. Colors start from the cover's main color.
const RAMP = ' .:-=+*#%@';
const MIN_MODE = 8, MAX_MODE = 30; // seconds a mode lasts: a drop can change it after MIN, it changes anyway after MAX

const avg = (a, i, j) => a.slice(i, j).reduce((s, v) => s + v, 0) / (j - i);
const clamp = v => Math.max(-1, Math.min(1, v));

function hsv(h, s, v) {
  h = ((h % 1) + 1) % 1;
  const f = n => { const k = (n + h * 6) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); };
  return [f(5), f(3), f(1)].map(c => Math.round(c * 255));
}
function hue([r, g, b]) {
  const max = Math.max(r, g, b), d = max - Math.min(r, g, b);
  if (!d) return 0;
  return ((max === r ? (g - b) / d + 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4) % 6) / 6;
}

// Each mode: p paints (a point x, y roughly in -1..1, time, bass -> -1..1, only the bright part shows), z and r scale
// how the feedback zooms (negative: shrinks towards the center) and turns.
const polar = (x, y) => [Math.hypot(x, y), Math.atan2(y, x)];
export const MODES = {
  plasma: { z: 1, r: 1, p: (x, y, t, b) => { const z = 3 + b * 4; return (Math.sin(x * z + t) + Math.sin(y * z * 1.3 - t * 0.7) + Math.sin((x + y) * z + t * 1.1) + Math.sin(Math.hypot(x, y) * z * 2 - t * 2)) / 4; } },
  tunnel: { z: 1.5, r: 0.5, p: (x, y, t, b) => { const [r, a] = polar(x, y); return Math.sin(1.5 / (r + 0.05) + t * 3 + b * 3) * Math.cos(a * 4 + t + 0.5 / (r + 0.05)); } },
  kaleido: { z: 1, r: 1, p: (x, y, t, b) => {
    const [r, a0] = polar(x, y), slice = Math.PI / 3, a = Math.abs((((a0 + t * 0.3) % slice) + slice) % slice - slice / 2); // 6-fold mirror
    return (Math.sin(r * 10 - t * 4 + b * 6) + Math.sin(a * 12 + r * 4 - t)) / 2;
  } },
  spiral: { z: 0.8, r: 2, p: (x, y, t, b) => { const [r, a] = polar(x, y); return Math.sin(a * 3 + r * (8 + b * 6) - t * 3); } },
  ripple: { z: 0.5, r: 0.3, p: (x, y, t) => Math.sin(Math.hypot(x, y) * 14 - t * 5) * Math.cos(Math.hypot(x - Math.sin(t * 0.7) * 0.5, y - Math.cos(t * 0.5) * 0.5) * 10 + t * 3) },
  rays: { z: 2, r: -1, p: (x, y, t, b) => { const [r, a] = polar(x, y); return clamp(Math.sin(a * (8 + Math.round(b * 8)) + t * 2) * 2 * (1 - Math.min(1, r)) - 0.2); } },
  checker: { z: 0.6, r: 1.5, p: (x, y, t, b) => {
    const [r, a0] = polar(x, y), a = a0 + t * 0.4 + r * 2, s = 4 + b * 4;
    return clamp(Math.sin(Math.cos(a) * r * s * 3) * Math.sin(Math.sin(a) * r * s * 3) * 2);
  } },
  waves: { z: -0.6, r: 0, p: (x, y, t, b) => clamp(1 - Math.min(...[0, 1, 2].map(k => Math.abs(y - Math.sin(x * (3 + k) + t * (2 + k) + k * 2) * (0.25 + b * 0.6)))) * 5) },
  blobs: { z: 0.4, r: 0.8, p: (x, y, t, b) => {
    let s = 0;
    for (let k = 0; k < 4; k++) { const cx = Math.sin(t * (0.6 + k * 0.23) + k) * 0.8, cy = Math.cos(t * (0.5 + k * 0.17) + k * 2) * 0.7; s += (0.02 + b * 0.03) / ((x - cx) ** 2 + (y - cy) ** 2 + 0.001); }
    return clamp(s - 1);
  } },
  flower: { z: 1, r: -1.5, p: (x, y, t, b) => { const [r, a] = polar(x, y); return clamp(1 - Math.abs(r - 0.45 * (0.6 + 0.4 * Math.cos(a * 6 + t)) * (1 + b)) * 6); } },
  grid: { z: -0.4, r: 0.6, p: (x, y, t, b) => Math.max(Math.cos(x * 12 + Math.sin(y * 3 + t) * (2 + b * 3)), Math.cos(y * 12 + Math.sin(x * 3 - t) * (2 + b * 3))) * 2 - 1 },
  mandala: { z: 0.7, r: 1, p: (x, y, t, b) => {
    const [r, a0] = polar(x, y), slice = Math.PI / 4, a = Math.abs((((a0 - t * 0.2) % slice) + slice) % slice - slice / 2); // 8-fold mirror
    const u = Math.cos(a) * r * 6, v = Math.sin(a) * r * 6;
    return (Math.sin(u + t) + Math.sin(v * 2 - t * 1.3) + Math.sin((u + v) * (1 + b) + t * 0.7)) / 3;
  } },
};
const ORDER = Object.keys(MODES);
const hash = s => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);

// The feedback state: brightness and hue per cell, double-buffered.
let W = 0, H = 0, I, Hu, I2, Hu2, lastT = 0, song;
let avgBass = 0, cool = 0, kick = 0, flash = 0, turn = 1, mi = 0, since = 0, loud = 0;
export const mode = () => ORDER[mi];
const pick = n => { mi = ((n % ORDER.length) + ORDER.length) % ORDER.length; since = 0; flash = 1; };
export const shift = d => pick(mi + d); // by hand: ] next mode, [ previous

function step(dt, t, bands, base) {
  const bass = avg(bands, 0, 6), mid = avg(bands, 6, 19), treb = avg(bands, 19, bands.length);
  loud += (avg(bands, 0, bands.length) - loud) * Math.min(1, dt * 3);
  avgBass += (bass - avgBass) * Math.min(1, dt * 2);
  since += dt;
  if ((cool -= dt) <= 0 && bass > avgBass * 1.3 + 0.08) { // a beat
    cool = 0.25; kick = 1; flash = Math.max(flash, 0.6);
    if (Math.random() < 0.3) turn = -turn;
    if (since > MIN_MODE && bass > avgBass * 1.8 + 0.15) pick(mi + 1 + Math.floor(Math.random() * (ORDER.length - 1))); // a drop
  }
  if (since > MAX_MODE) pick(mi + 1);
  kick *= Math.exp(-dt * 4); flash *= Math.exp(-dt * 6);

  const m = MODES[ORDER[mi]];
  const zoom = 1 + dt * m.z * (0.3 + bass * 1.2 + kick * 4), rot = dt * turn * m.r * (0.15 + mid * 2);
  const decay = Math.exp(-dt * (1.6 - treb * 1.0)), gain = 0.1 + loud * 1.4;
  for (let j = 0; j < H; j++) {
    const y = (j - H / 2) / (H / 2);
    for (let i = 0; i < W; i++) {
      const x = (i - W / 2) / H, r = Math.hypot(x, y), a = Math.atan2(y, x); // a cell is about twice as tall as wide
      // where this cell's content was last frame: closer to the center and turned back (more twist near the middle)
      const ra = a - rot * (1 + Math.max(0, 1 - r)), rr = r / zoom;
      // bilinear: rounding to the nearest cell would swallow the tiny per-frame zoom and turn, and freeze the picture
      const px = Math.cos(ra) * rr * H + W / 2 - 0.5, py = Math.sin(ra) * rr * H / 2 + H / 2 - 0.5;
      const i0 = Math.floor(px), j0 = Math.floor(py), wx = px - i0, wy = py - j0;
      let v = 0, h = 0, top = -1;
      for (const [di, dj, wgt] of [[0, 0, (1 - wx) * (1 - wy)], [1, 0, wx * (1 - wy)], [0, 1, (1 - wx) * wy], [1, 1, wx * wy]]) {
        const si = i0 + di, sj = j0 + dj;
        if (si < 0 || si >= W || sj < 0 || sj >= H) continue;
        const c = I[sj * W + si] * wgt;
        v += c;
        if (c > top) { top = c; h = Hu[sj * W + si]; } // hues do not average (red + violet is not green): take the strongest
      }
      v *= decay; h += dt * 0.08;
      // the pattern only paints its peaks, so the trails stay visible between them
      const p = m.p(x, y, t, bass), pi = Math.min(1, Math.max(0, p) ** 2 * gain);
      if (pi > v) { v = pi; h = base + p * 0.5 + r * 0.3 + t * 0.03; }
      // the spectrum ring: its radius at each angle is the level of one band, mirrored left/right
      const k = Math.round(Math.abs(a) / Math.PI * (bands.length - 1));
      const ring = Math.max(0, 1 - Math.abs(r - (0.15 + bands[k] * 0.7 + kick * 0.1)) * 14) * Math.min(1, bass * 1.5);
      if (ring > v) { v = ring; h = base + Math.abs(a) / Math.PI * 0.6 + t * 0.05; }
      I2[j * W + i] = v; Hu2[j * W + i] = h;
    }
  }
  [I, I2, Hu, Hu2] = [I2, I, Hu2, Hu];
}

// h rows of exactly w visible characters. t is the visualizer's own clock (seconds), bands the 0..1 levels, id the
// song (a new one picks its mode). The same t twice (paused) redraws the last frame without moving it.
export function frame(w, h, t, bands, accent, id) {
  if (w !== W || h !== H) { W = w; H = h; [I, Hu, I2, Hu2] = [0, 0, 0, 0].map(() => new Float32Array(w * h)); }
  if (id && id !== song) { song = id; pick(hash(id)); turn = hash(id) & 1 ? 1 : -1; }
  const dt = Math.max(0, Math.min(0.1, t - lastT));
  lastT = t;
  if (dt) step(dt, t, bands, hue(accent));
  const rows = [];
  for (let j = 0; j < h; j++) {
    let s = '', last = '';
    for (let i = 0; i < w; i++) {
      const v = Math.min(1, I[j * w + i] + flash * 0.3 * loud), hh = Hu[j * w + i];
      // shortcut: colors are rounded to 16 steps so neighbors share one escape code; finer if it ever looks banded
      const q = c => c.map(n => n & 0xf0).join(';');
      const col = `38;2;${q(hsv(hh + 0.1, 0.6, Math.min(1, 0.3 + v)))};48;2;${q(hsv(hh, 1, v * 0.6))}`;
      if (col !== last) { s += `\x1b[${col}m`; last = col; }
      s += RAMP[Math.min(RAMP.length - 1, Math.floor(v * RAMP.length))];
    }
    rows.push(s + '\x1b[39;49m');
  }
  return rows;
}
