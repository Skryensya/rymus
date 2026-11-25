// Zellij-like look: powerline ribbons for the tab bar, rounded panes with the title in the border,
// a status bar of key hints. Pure string builders, no state. 24-bit color.
export const RED = [232, 36, 36], GRAY = [90, 90, 90], BAR = [28, 28, 28];
export const RIB = [48, 48, 52], TXT = [208, 208, 208], BLACK = [0, 0, 0], GREEN = [166, 209, 137];
export const SEP = process.env.YM_PLAIN ? ' ' : ''; // powerline arrow; YM_PLAIN=1 if your font lacks it
// Nerd Font glyphs (Ghostty ships them); plain symbols with YM_PLAIN=1.
export const [LIKE, UNLIKED, DISLIKE] = process.env.YM_PLAIN ? ['♥', '♡', '✗'] : ['\uf004', '\uf08a', '\uf165'];

export const bgc = a => `\x1b[48;2;${a.join(';')}m`;
export const fgc = a => `\x1b[38;2;${a.join(';')}m`;
export const RESET = '\x1b[0m';
export const cut = (s, n) => (s.length > n ? s.slice(0, Math.max(0, n - 1)) + '…' : s);
// text padded or cut to exactly w visible columns, styled by f
export const cell = (txt, f, w) => { const t = cut(txt, w); return f(t) + ' '.repeat(Math.max(0, w - t.length)); };

// Tab bar: [ymus][tab][tab]... as ribbons. Returns the line and the column span of each tab (for the mouse).
export function tabBar(W, tabs, active, accent) {
  const build = short => {
    const items = [{ t: ' ♫ ymus ', bg: BAR, fg: accent, bold: true },
      ...tabs.map(t => (t.k === active ? { t: ` ${short ? t.short : t.full} `, bg: accent, fg: BLACK, bold: true } : { t: ` ${short ? t.short : t.full} `, bg: RIB, fg: TXT }))];
    let s = '', col = 1, last = null;
    const spans = [];
    items.forEach((r, i) => {
      if (last) { s += `${fgc(last)}${bgc(r.bg)}${SEP}`; col++; } else s += bgc(r.bg);
      s += `${fgc(r.fg)}${r.bold ? '\x1b[1m' : ''}${r.t}\x1b[22m`;
      if (i) spans.push({ k: tabs[i - 1].k, x0: col - 1 || 1, x1: col + r.t.length - 1 });
      col += r.t.length; last = r.bg;
    });
    s += `${bgc(BAR)}${fgc(last)}${SEP}`;
    return { s, len: col, spans };
  };
  let b = build(false);
  if (b.len > W) b = build(true);
  const room = W - b.len;
  return { line: b.s + bgc(BAR) + ' '.repeat(Math.max(0, room)) + RESET, spans: b.spans };
}

// Pane frame (inner rows must already be exactly w-2 visible columns). Title sits in the top border.
export function frame(title, tcol, inner, w, color) {
  const t = cut(title, Math.max(0, w - 5));
  const b = s => fgc(color) + s + '\x1b[39m';
  const top = b('╭─ ') + fgc(tcol) + '\x1b[1m' + t + '\x1b[22m\x1b[39m' + b(' ' + '─'.repeat(Math.max(0, w - 5 - t.length)) + '╮');
  return [top, ...inner.map(l => b('│') + l + b('│')), b('╰' + '─'.repeat(w - 2) + '╯')];
}

// Bottom bar: key + label; as many as fit.
export function hints(list, W, accent) {
  let s = ' ', len = 1;
  for (const [k, l] of list) {
    const n = k.length + l.length + 4;
    if (len + n > W) break;
    s += `${fgc(accent)}\x1b[1m${k}\x1b[22m ${fgc([130, 130, 130])}${l}   `;
    len += n;
  }
  return s + ' '.repeat(Math.max(0, W - len)) + RESET;
}

// Spectrum in braille, bass on the left: h rows by exactly w columns, one thin line per column (4 dots of height per
// row). bands are 0..1; columns in between are interpolated.
const DOTS = [0, 0x40, 0x44, 0x46, 0x47]; // 0 to 4 dots of the left braille column, filled from the bottom
export function spectrum(bands, w, h, accent) {
  const heights = Array.from({ length: w }, (_, i) => {
    const x = w > 1 ? i * (bands.length - 1) / (w - 1) : 0, a = Math.floor(x), b = Math.min(bands.length - 1, a + 1);
    return Math.round((bands[a] + (bands[b] - bands[a]) * (x - a)) * 4 * h) || 1; // a baseline stays visible
  });
  return Array.from({ length: h }, (_, row) => {
    const floor = (h - 1 - row) * 4;
    const s = heights.map(e => String.fromCharCode(0x2800 + DOTS[Math.max(0, Math.min(4, e - floor))])).join('');
    const light = h > 1 ? 0.6 * (1 - row / (h - 1)) : 0; // top rows are paler
    return fgc(accent.map(c => Math.round(c + (255 - c) * light))) + s + '\x1b[39m';
  });
}

// Always-visible search field under the tab bar: [ / Search ] text
// Returns the line (exactly W wide) plus the mouse span of the field (1-indexed columns).
export function searchBar(W, { typing, text, placeholder, accent }) {
  const label = ' / Search ';
  const fieldW = Math.max(0, W - label.length - 1);
  const body = typing ? `${text}█` : text || placeholder;
  const shown = typing ? body.slice(-fieldW) : cut(body, fieldW);
  const field = (typing ? shown : `\x1b[2m${shown}\x1b[22m`) + ' '.repeat(Math.max(0, fieldW - shown.length));
  const lab = typing ? `${bgc(accent)}${fgc(BLACK)}\x1b[1m${label}\x1b[22m${RESET}` : `${bgc(RIB)}${fgc(TXT)}${label}${RESET}`;
  return { line: `${lab} ${field}`, field: { x0: 1, x1: W } };
}
