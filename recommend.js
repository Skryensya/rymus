// The recommendation engine: a taste model built from your own log, and the ranking of candidate songs.
// Pure functions only; fetching candidates is mixes.js.
const DAY = 86400000;
export const mainArtist = t => (t.artist ?? '').split(', ')[0].toLowerCase();
// The same song under another id: "(Official Video)", "[Remastered]", "- Radio Edit", "ft. X" do not make it new.
export const songKey = t => `${t.title.toLowerCase().replace(/\s*[([].*?[)\]]|\s+-\s+.*$|\s+(ft|feat)\.? .*$/g, '').trim()}|${mainArtist(t)}`;

// ---------- taste ----------
const part = h => (h < 5 ? 'night' : h < 12 ? 'morning' : h < 18 ? 'afternoon' : 'evening');
// How much a play from `at` says about what you want to hear `now`: recent ones count more (half every 30 days),
// and so do ones from the same time of day and the same kind of day (weekday or weekend).
export function relevance(at, now) {
  const a = new Date(at), b = new Date(now), weekend = d => d.getDay() % 6 === 0;
  return 0.5 ** ((now - at) / (30 * DAY)) * (part(a.getHours()) === part(b.getHours()) ? 1.3 : 1) * (weekend(a) === weekend(b) ? 1.2 : 1);
}

// What one play says: skipped early is a "no", heard to (nearly) the end a "yes".
export function playValue(p) {
  if (p.skipped) return -1;
  const whole = p.duration ? p.seconds >= Math.min(0.8 * p.duration, 240) : p.seconds >= 120;
  return whole ? 1 : p.seconds >= 30 ? 0.5 : 0;
}

// plays: db.signals(); liked and disliked: tracks (liked ones carry `at`). Scores per track and per artist.
export function taste({ plays, liked, disliked }, now = Date.now()) {
  const tracks = new Map();
  const add = (t, score) => {
    const e = tracks.get(t.id) ?? { id: t.id, title: t.title, artist: t.artist, album: t.album, cover: t.cover, score: 0 };
    e.score += score;
    tracks.set(t.id, e);
  };
  for (const p of plays) add(p, playValue(p) * relevance(p.at, now));
  for (const t of liked) add(t, 1.5 + 1.5 * 0.5 ** ((now - (t.at ?? now)) / (180 * DAY))); // an old like fades to half
  for (const t of disliked) add(t, -4);
  const artists = new Map();
  for (const t of tracks.values()) {
    const key = mainArtist(t);
    const a = artists.get(key) ?? { name: t.artist.split(', ')[0], score: 0 };
    a.score += t.score;
    artists.set(key, a);
  }
  return { tracks: [...tracks.values()].sort((a, b) => b.score - a.score), artists };
}

// Random order in which heavier items tend to come first (weighted sampling without replacement).
export const sample = (items, weight, rand = Math.random) =>
  items.map(x => [rand() ** (1 / Math.max(0.05, weight(x))), x]).sort((a, b) => b[0] - a[0]).map(p => p[1]);

// A few songs you currently enjoy to start from, one per artist.
export function pickSeeds(tracks, n = 5, rand = Math.random) {
  const seen = new Set();
  return sample(tracks.filter(t => t.score > 0), t => t.score, rand)
    .filter(t => !seen.has(mainArtist(t)) && seen.add(mainArtist(t))).slice(0, n);
}

// ---------- candidates ----------
// One entry of a YouTube "up next" list -> a track, or null.
export function toTrack(x) {
  const v = x?.primary ?? x;
  if (!v?.video_id) return null;
  const thumbs = v.thumbnail ?? [];
  return {
    id: v.video_id, title: String(v.title ?? ''), artist: v.artists?.map(a => a.name).join(', ') || v.author || '',
    album: v.album?.name ?? '', cover: thumbs.reduce((best, t) => (t.width > (best?.width ?? 0) ? t : best), null)?.url,
  };
}

// Order for listening: best first, but never the same artist twice in a row (when it can be avoided) and
// changing source as it goes, so it drifts instead of staying in one corner.
export function flow(items) {
  const left = [...items], out = [];
  while (left.length) {
    const last = out.at(-1), before = out.at(-2);
    const other = x => !last || mainArtist(x) !== mainArtist(last);
    let i = left.findIndex(x => other(x) && (!before || mainArtist(x) !== mainArtist(before)) && x.via !== last?.via);
    if (i < 0 || i > 6) i = left.findIndex(other); // do not reach far down the ranking just to change source
    out.push(left.splice(Math.max(0, i), 1)[0]);
  }
  return out;
}

// Only songs you have not heard. lists: [{ via, weight, tracks }] with tracks in the source's own order.
// A song scores more when several sources lead to it. Your taste then moves it: artists you skip or dislike go
// down (or out), and artists you do not know get a small push. At most two songs per artist.
export function rank(lists, { model, heard = [], size = 30, rand = Math.random }) {
  const ids = new Set(heard.map(t => t.id)), songs = new Set(heard.map(songKey));
  const found = new Map();
  for (const { via, weight, tracks } of lists) {
    tracks.forEach((t, i) => {
      const key = songKey(t);
      if (ids.has(t.id) || songs.has(key)) return;
      const hit = found.get(key) ?? { track: t, score: 0, via };
      hit.score += weight * (1 - 0.5 * i / tracks.length);
      found.set(key, hit);
    });
  }
  const scored = [];
  for (const { track, score, via } of found.values()) {
    const known = model.artists.get(mainArtist(track))?.score;
    if (known <= -3) continue;
    const mine = known === undefined ? 0.3 : known < 0 ? known : Math.min(1, known / 5) * 0.4;
    scored.push({ ...track, via, fresh: true, score: score + mine + rand() * 0.4 });
  }
  const perArtist = new Map(), picked = [];
  for (const t of scored.sort((a, b) => b.score - a.score)) {
    const n = perArtist.get(mainArtist(t)) ?? 0;
    if (n >= 2) continue;
    perArtist.set(mainArtist(t), n + 1);
    picked.push(t);
    if (picked.length >= size) break;
  }
  return flow(picked).map(({ via, score, ...t }) => t);
}

// ---------- styles ----------
// Groups your artists by shared genre tags and by who ListenBrainz says they are similar to.
// artists: [{ name, score, tags: [], similar: [lowercase names] }] -> [{ name: 'house · electronic', artists: [] }]
export function styles(artists, max = 4) {
  const link = (a, b) => {
    const shared = a.tags.filter(t => b.tags.includes(t)).length;
    const union = new Set([...a.tags, ...b.tags]).size;
    const near = a.similar.includes(b.name.toLowerCase()) || b.similar.includes(a.name.toLowerCase());
    return (union ? shared / union : 0) + (near ? 0.4 : 0);
  };
  const groups = [];
  for (const a of [...artists].sort((x, y) => y.score - x.score)) {
    const best = groups.map(g => [link(g[0], a), g]).sort((x, y) => y[0] - x[0])[0];
    if (best && best[0] >= 0.25) best[1].push(a); else groups.push([a]);
  }
  const total = g => g.reduce((n, a) => n + a.score, 0);
  return groups.filter(g => g[0].tags.length || g.length > 1).sort((x, y) => total(y) - total(x)).slice(0, max).map(g => {
    const tags = new Map();
    for (const a of g) a.tags.forEach((t, i) => tags.set(t, (tags.get(t) ?? 0) + a.score / (i + 1)));
    const top = [...tags].sort((x, y) => y[1] - x[1]).slice(0, 2).map(x => x[0]);
    return { name: top.join(' · ') || g[0].name, artists: g.map(a => a.name) };
  });
}
