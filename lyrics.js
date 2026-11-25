// Lyrics from lrclib.net (free, no account). Only the title, artist, album and length of the song are sent.
const API = 'https://lrclib.net/api/';
const cache = new Map(); // track id -> result

// Text from outside goes straight to the terminal: drop control characters so it cannot send escape sequences.
const clean = s => s.replace(/[\x00-\x1f\x7f-\x9f]/g, '').trim();

// "[mm:ss.xx] line" (a line may carry several stamps) -> [{ t: seconds, text }] in time order.
export function parseLrc(lrc) {
  const out = [];
  for (const line of lrc.split('\n')) {
    const stamps = [...line.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)];
    const text = clean(line.replace(/\[[^\]]*\]/g, ''));
    for (const m of stamps) out.push({ t: +m[1] * 60 + +m[2], text });
  }
  return out.sort((a, b) => a.t - b.t);
}

// Terminal columns of one character: CJK and emoji take two, combining marks none.
const charWidth = c => (/\p{M}/u.test(c) ? 0
  : /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹯＀-｠￠-￦]|\p{Extended_Pictographic}/u.test(c) ? 2 : 1);
export const width = s => [...s].reduce((n, c) => n + charWidth(c), 0);

// Word wrap to w columns; a word longer than a row is split.
export function wrap(text, w) {
  const rows = [];
  let row = '';
  for (const word of text.split(' ')) {
    if (row && width(row) + 1 + width(word) <= w) { row += ' ' + word; continue; }
    if (row) rows.push(row);
    row = '';
    for (const c of word) {
      if (width(row) + charWidth(c) > w) { rows.push(row); row = ''; }
      row += c;
    }
  }
  rows.push(row);
  return rows;
}

// Picks from lrclib records. Timed lyrics are only trusted from a recording of (almost) the same length.
export function pick(records, duration) {
  const close = r => Math.abs(r.duration - duration) <= 3;
  const timed = records.filter(r => r.syncedLyrics && close(r))
    .sort((a, b) => Math.abs(a.duration - duration) - Math.abs(b.duration - duration))[0];
  if (timed) return { synced: parseLrc(timed.syncedLyrics) };
  const plain = records.find(r => r.plainLyrics && close(r)) ?? records.find(r => r.plainLyrics);
  if (plain) return { plain: plain.plainLyrics.split('\n').map(clean) };
  return records.some(r => r.instrumental && close(r)) ? { instrumental: true } : {};
}

async function ask(path, params) {
  const res = await fetch(`${API}${path}?${new URLSearchParams(params)}`, { headers: { 'User-Agent': 'ymus' }, signal: AbortSignal.timeout(10000) });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error('LRCLIB ' + res.status);
  return res.json();
}

// The cached result for a track, or undefined after starting to fetch it; onReady is called when it arrives.
export function get(track, duration, onReady) {
  const hit = cache.get(track.id);
  if (hit) return hit.loading ? undefined : hit;
  cache.set(track.id, { loading: true });
  const artist = track.artist.split(', ')[0];
  (async () => {
    // exact match first, then a search without the "(feat. …)" part that other databases often leave out
    const exact = await ask('get', { track_name: track.title, artist_name: artist, album_name: track.album ?? '', duration: Math.round(duration) });
    if (exact?.syncedLyrics) return pick([exact], duration);
    const found = await ask('search', { track_name: track.title.replace(/\s*[([](feat|ft)\b[^)\]]*[)\]]/i, ''), artist_name: artist });
    return pick([...(exact ? [exact] : []), ...(Array.isArray(found) ? found : [])], duration);
  })().then(r => cache.set(track.id, r), () => cache.set(track.id, { error: true })).then(onReady);
}

// A failed request is worth another try the next time the view is opened.
export const forgetErrors = () => { for (const [k, v] of cache) if (v.error) cache.delete(k); };
