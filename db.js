// Own local log: every track seen, likes, and play history. node:sqlite is built in, no dependency.
import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const file = process.env.YM_DB ?? path.join(os.homedir(), '.ymus', 'ymus.db');
fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
const db = new DatabaseSync(file);
db.exec(`
  PRAGMA journal_mode = WAL;
  CREATE TABLE IF NOT EXISTS tracks(id TEXT PRIMARY KEY, title TEXT, artist TEXT, album TEXT, cover TEXT);
  CREATE TABLE IF NOT EXISTS likes(track_id TEXT PRIMARY KEY REFERENCES tracks(id), at INTEGER NOT NULL);
  CREATE TABLE IF NOT EXISTS dislikes(track_id TEXT PRIMARY KEY REFERENCES tracks(id), at INTEGER NOT NULL);
  CREATE TABLE IF NOT EXISTS plays(id INTEGER PRIMARY KEY, track_id TEXT NOT NULL REFERENCES tracks(id),
    at INTEGER NOT NULL, seconds INTEGER NOT NULL DEFAULT 0);
  CREATE INDEX IF NOT EXISTS plays_at ON plays(at DESC);
  CREATE TABLE IF NOT EXISTS searches(q TEXT PRIMARY KEY, at INTEGER NOT NULL);
  CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT);
  CREATE TABLE IF NOT EXISTS cache(k TEXT PRIMARY KEY, v TEXT, at INTEGER NOT NULL);
`);
// columns added later; older rows keep NULL / 0
const playColumns = db.prepare('PRAGMA table_info(plays)').all().map(c => c.name);
if (!playColumns.includes('duration')) db.exec('ALTER TABLE plays ADD COLUMN duration INTEGER');
if (!playColumns.includes('skipped')) db.exec('ALTER TABLE plays ADD COLUMN skipped INTEGER NOT NULL DEFAULT 0');

const q = sql => db.prepare(sql);
const upsert = q('INSERT INTO tracks VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title, artist=excluded.artist, album=COALESCE(NULLIF(excluded.album, \'\'), album), cover=COALESCE(excluded.cover, cover)');
const saveTrack = t => upsert.run(t.id, t.title, t.artist ?? '', t.album ?? '', t.cover ?? null);

export const likedIds = () => new Set(q('SELECT track_id FROM likes').all().map(r => r.track_id));

// A track is liked, disliked or neither. Returns true if it now has the rating asked for.
function toggle(table, other, t) {
  saveTrack(t);
  if (q(`DELETE FROM ${table} WHERE track_id=?`).run(t.id).changes) return false;
  q(`DELETE FROM ${other} WHERE track_id=?`).run(t.id);
  q(`INSERT INTO ${table} VALUES(?,?)`).run(t.id, Date.now());
  return true;
}
export const toggleLike = t => toggle('likes', 'dislikes', t);
export const toggleDislike = t => toggle('dislikes', 'likes', t);
export const disliked = () => q('SELECT t.* FROM dislikes d JOIN tracks t ON t.id=d.track_id').all();

export const liked = () => q('SELECT t.* FROM likes l JOIN tracks t ON t.id=l.track_id ORDER BY l.at DESC, l.rowid DESC').all();

export const history = (limit = 200) =>
  q('SELECT t.*, p.at AS played_at FROM plays p JOIN tracks t ON t.id=p.track_id ORDER BY p.at DESC, p.id DESC LIMIT ?').all(limit);

export function startPlay(t, at = Date.now()) {
  saveTrack(t);
  return Number(q('INSERT INTO plays(track_id, at) VALUES(?,?)').run(t.id, at).lastInsertRowid);
}

export const endPlay = (playId, seconds, duration = null, skipped = false) =>
  q('UPDATE plays SET seconds=?, duration=COALESCE(?, duration), skipped=? WHERE id=?').run(seconds, duration, skipped ? 1 : 0, playId);

// For the recommendation engine: every play with its track, what was rated, and everything already heard.
export const signals = () => ({
  plays: q('SELECT t.*, p.at, p.seconds, p.duration, p.skipped FROM plays p JOIN tracks t ON t.id = p.track_id').all(),
  liked: q('SELECT t.*, l.at FROM likes l JOIN tracks t ON t.id = l.track_id').all(),
  disliked: disliked(),
});
export const heard = () => q(`SELECT * FROM tracks WHERE id IN
  (SELECT track_id FROM plays UNION SELECT track_id FROM likes UNION SELECT track_id FROM dislikes)`).all();

// Answers from outside services, kept between sessions. get returns undefined when missing or too old.
export function cacheGet(k, maxAge = Infinity) {
  const r = q('SELECT v, at FROM cache WHERE k=?').get(k);
  return r && Date.now() - r.at <= maxAge ? JSON.parse(r.v) : undefined;
}
export const cacheSet = (k, v) => q('INSERT INTO cache VALUES(?,?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v, at=excluded.at').run(k, JSON.stringify(v), Date.now());

// Strictly increasing timestamps, so "most recent first" never ties.
let last = 0;
const tick = () => (last = Math.max(Date.now(), last + 1));

export const saveSearch = text =>
  q('INSERT INTO searches VALUES(?,?) ON CONFLICT(q) DO UPDATE SET at=excluded.at').run(text, tick());
export const recentSearches = (n = 50) => q('SELECT q FROM searches ORDER BY at DESC LIMIT ?').all(n).map(r => r.q);

export const getSetting = k => q('SELECT v FROM settings WHERE k=?').get(k)?.v;
export const setSetting = (k, v) => q('INSERT INTO settings VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v').run(k, v);

export const listened = since => q('SELECT COALESCE(SUM(seconds), 0) AS s FROM plays WHERE at >= ?').get(since).s;

// Top songs, artists and albums by time listened since a moment.
export function recap(since, limit = 100) {
  const rows = q(`SELECT t.*, COUNT(*) AS plays, SUM(p.seconds) AS seconds FROM plays p JOIN tracks t ON t.id = p.track_id
    WHERE p.at >= ? AND p.seconds > 0 GROUP BY t.id ORDER BY seconds DESC, plays DESC, t.title`).all(since);
  const rank = list => list.sort((a, b) => b.seconds - a.seconds || b.plays - a.plays || a.title.localeCompare(b.title)).slice(0, limit);
  const group = entries => {
    const m = new Map();
    for (const r of rows) {
      for (const [key, base] of entries(r)) {
        const g = m.get(key) ?? { ...base, id: key, plays: 0, seconds: 0, cover: r.cover }; // rows come longest first
        g.plays += r.plays; g.seconds += r.seconds; g.cover ??= r.cover;
        m.set(key, g);
      }
    }
    return rank([...m.values()]);
  };
  const first = r => r.artist.split(', ')[0];
  return {
    songs: rank(rows),
    artists: group(r => r.artist.split(', ').filter(Boolean).map(name => [name, { title: name }])),
    albums: group(r => (r.album ? [[`${r.album}\n${first(r)}`, { title: r.album, artist: first(r) }]] : [])),
  };
}

// Favorites = what you play plus what you like (a like weighs as much as 3 plays).
export const topTracks = (limit = 50) => q(`
  SELECT * FROM (
    SELECT t.*, (SELECT COUNT(*) FROM plays p WHERE p.track_id = t.id) AS plays,
           (SELECT MAX(at) FROM plays p WHERE p.track_id = t.id) AS last_at,
           EXISTS(SELECT 1 FROM likes l WHERE l.track_id = t.id) AS liked
    FROM tracks t WHERE t.id NOT IN (SELECT track_id FROM dislikes))
  WHERE plays > 0 OR liked
  ORDER BY plays + 3 * liked DESC, last_at DESC, id LIMIT ?`).all(limit);

// shortcut: artists are split from the stored "A, B" credit string, so a name that contains ", " is counted as two
export function topArtists(limit = 30) {
  const m = new Map();
  const rows = q(`SELECT t.artist, t.cover,
    (SELECT COUNT(*) FROM plays p WHERE p.track_id = t.id) AS plays,
    (SELECT COUNT(*) FROM likes l WHERE l.track_id = t.id) AS likes FROM tracks t
    WHERE t.id NOT IN (SELECT track_id FROM dislikes)`).all();
  for (const r of rows) {
    for (const name of r.artist.split(', ').filter(Boolean)) {
      const a = m.get(name) ?? { id: name, title: name, plays: 0, likes: 0, cover: null, best: -1 };
      a.plays += r.plays; a.likes += r.likes;
      const sc = r.plays + 3 * r.likes;
      if (r.cover && sc > a.best) { a.cover = r.cover; a.best = sc; } // artist art = cover of their favorite track
      m.set(name, a);
    }
  }
  const score = a => a.plays + 3 * a.likes;
  return [...m.values()].filter(a => score(a)).sort((x, y) => score(y) - score(x) || x.title.localeCompare(y.title)).slice(0, limit);
}
