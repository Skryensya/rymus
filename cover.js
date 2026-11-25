// Cover art via the kitty graphics protocol (Ghostty, kitty, WezTerm). ffmpeg converts any image to PNG.
import { spawn } from 'node:child_process';

export const supported = !!process.env.KITTY_WINDOW_ID || /ghostty|wezterm/i.test(process.env.TERM_PROGRAM ?? '') || /kitty|ghostty/.test(process.env.TERM ?? '');

const ready = new Map(); // url -> { b64: base64 PNG, rgb: its most vivid color }
const busy = new Set();  // loading or failed: never retried this session
const originals = new Map(); // url -> original image bytes
const rasters = new Map();   // url|w|h -> rows of colored cells
const rbusy = new Set();

const ffmpeg = (img, args) => new Promise((res, rej) => {
  const f = spawn('ffmpeg', ['-v', 'error', '-i', 'pipe:0', ...args, 'pipe:1']);
  const chunks = [];
  f.stdout.on('data', d => chunks.push(d));
  f.on('error', rej);
  f.on('close', code => (code === 0 ? res(Buffer.concat(chunks)) : rej(new Error('ffmpeg ' + code))));
  f.stdin.on('error', () => {});
  f.stdin.end(img);
});

// The cover's dominant color: pixels are grouped into 12 hue buckets weighted by saturation × brightness,
// and the most vivid pixel of the heaviest bucket wins. An average turns most covers into gray, and the single
// most vivid pixel lets a small red detail beat a cover that is mostly green.
export function vivid(px) {
  const total = new Array(12).fill(0), best = new Array(12).fill(null), top = new Array(12).fill(-1);
  for (let i = 0; i < px.length; i += 3) {
    const [r, g, b] = [px[i], px[i + 1], px[i + 2]], max = Math.max(r, g, b), d = max - Math.min(r, g, b);
    if (!d) continue;
    const h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4; // 0..6
    const k = Math.floor(h * 2) % 12, s = d * max;
    total[k] += s;
    if (s > top[k]) { top[k] = s; best[k] = [r, g, b]; }
  }
  const k = total.indexOf(Math.max(...total));
  return best[k] ?? [px[0] ?? 0, px[1] ?? 0, px[2] ?? 0];
}

async function load(url) {
  const img = Buffer.from(await (await fetch(url)).arrayBuffer());
  originals.set(url, img);
  const [png, px] = await Promise.all([
    ffmpeg(img, ['-vf', 'scale=400:-1', '-f', 'image2pipe', '-vcodec', 'png']),
    ffmpeg(img, ['-vf', 'scale=16:16:flags=area', '-f', 'rawvideo', '-pix_fmt', 'rgb24']),
  ]);
  return { b64: png.toString('base64'), rgb: vivid(px) };
}

// Returns { b64, rgb } if cached; otherwise starts loading and calls onReady when done.
export function get(url, onReady) {
  if (ready.has(url)) return ready.get(url);
  if (!busy.has(url)) {
    busy.add(url);
    load(url).then(b => { ready.set(url, b); onReady(); }).catch(() => {});
  }
}

export const clear = '\x1b_Ga=d,d=A,q=2\x1b\\';

// Draw at 1-indexed row/col, `cols` cells wide (height follows the image aspect). q=2: terminal sends no replies.
export function place(b64, row, col, cols) {
  let s = `\x1b[${row};${col}H`;
  for (let i = 0; i < b64.length; i += 4096) {
    const m = i + 4096 < b64.length ? 1 : 0;
    s += `\x1b_G${i ? '' : `a=T,f=100,c=${cols},C=1,`}q=2,m=${m};${b64.slice(i, i + 4096)}\x1b\\`;
  }
  return s;
}

// Colored text version of the cover for terminals without an image protocol: h rows of w half blocks,
// foreground = top pixel, background = bottom pixel. Returns the rows if cached; otherwise computes them and calls onReady.
// The original must be loaded first (see get).
export function cells(url, w, h, onReady) {
  const key = `${url}|${w}|${h}`;
  if (rasters.has(key)) return rasters.get(key);
  const img = originals.get(url);
  if (!img || rbusy.has(key)) return;
  rbusy.add(key);
  ffmpeg(img, ['-vf', `scale=${w}:${h * 2}:flags=area`, '-f', 'rawvideo', '-pix_fmt', 'rgb24']).then(px => {
    const rgb = (x, y) => { const i = (y * w + x) * 3; return `${px[i]};${px[i + 1]};${px[i + 2]}`; };
    rasters.set(key, Array.from({ length: h }, (_, y) =>
      Array.from({ length: w }, (_, x) => `\x1b[38;2;${rgb(x, 2 * y)};48;2;${rgb(x, 2 * y + 1)}m▀`).join('') + '\x1b[0m'));
    onReady();
  }).catch(() => {});
}
