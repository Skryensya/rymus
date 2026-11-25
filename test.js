// Run: node test.js  (uses a throwaway database)
import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import fs from 'node:fs';

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ymus-test-'));
process.env.YM_DB = path.join(tmp, 't.db');
const db = await import('./db.js');

const a = { id: 'aaa', title: 'One', artist: 'X', album: 'Al', cover: 'http://c/1' };
const b = { id: 'bbb', title: 'Two', artist: 'Y', album: 'Al', cover: null };

assert.equal(db.toggleLike(a), true);
assert.equal(db.toggleLike(b), true);
assert.deepEqual([...db.likedIds()].sort(), ['aaa', 'bbb']);
assert.deepEqual(db.liked().map(t => t.id), ['bbb', 'aaa']); // newest like first
assert.equal(db.toggleLike(a), false);
assert.deepEqual([...db.likedIds()], ['bbb']);

const p1 = db.startPlay(a); db.endPlay(p1, 42);
db.startPlay(b);
const h = db.history();
assert.deepEqual(h.map(r => r.id), ['bbb', 'aaa']);
assert.equal(h[1].title, 'One');

const c = { id: 'ccc', title: 'Three', artist: 'Y, Z', album: 'Al2', cover: null };
db.toggleLike(c);
// scores: bbb = 1 play + like = 4, ccc = like = 3, aaa = 1 play (unliked)
assert.deepEqual(db.topTracks().map(r => r.id), ['bbb', 'ccc', 'aaa']);
const ta = db.topArtists(); // Y = 1 play + 2 likes, Z = 1 like, X = 1 play (credits are split on ", ")
assert.deepEqual(ta.map(x => x.title), ['Y', 'Z', 'X']);
assert.deepEqual([ta[0].plays, ta[0].likes], [1, 2]);
assert.equal(ta.find(x => x.title === 'X').cover, 'http://c/1'); // X's only track has a cover
assert.equal(ta[0].cover, null); // Y's tracks have no cover

db.saveSearch('daft punk'); db.saveSearch('boards of canada'); db.saveSearch('daft punk');
assert.deepEqual(db.recentSearches(), ['daft punk', 'boards of canada']); // re-searching moves it to the top, no duplicate
assert.equal(db.getSetting('theme'), undefined);
db.setSetting('theme', 'blue'); db.setSetting('theme', 'green');
assert.equal(db.getSetting('theme'), 'green');

fs.rmSync(tmp, { recursive: true });
console.log('db ok');

const viz = await import('./viz.js');
const feed = viz.createFeed();
const frame = (t, db) => `frame:0 pts:0 pts_time:${t}\nlavfi.astats.1.RMS_level=-3\nlavfi.astats.3.RMS_level=${db}\nlavfi.astats.4.RMS_level=-inf\n`;
feed.push(frame(1, -30) + frame(1.02, -10).slice(0, 20)); // lines may arrive cut in half
feed.push(frame(1.02, -10).slice(20) + frame(2, -5));
assert.equal(feed.at(0.5), null); // measured ahead of time: not shown yet
const heard = feed.at(1.5);
assert.deepEqual(heard.slice(0, 2), [-10, -120]); // channel 3 is the first band; loudest of what was passed
assert.equal(feed.at(1.5), null);
feed.push(frame(0.2, -20)); // seek back: the queued 2 s measurement is dropped
assert.equal(feed.at(5)[0], -20);
const loud = viz.levels(heard, Array(viz.BANDS).fill(0));
assert.deepEqual([loud[0], loud[1]], [1, 0]);
const after = viz.levels(Array(viz.BANDS).fill(-120), loud);
assert.ok(after[0] > 0 && after[0] < 1); // falls gradually
console.log('viz ok');

const ly = await import('./lyrics.js');
assert.deepEqual(ly.parseLrc('[ar:X]\n[00:12.50] second\n[00:01.00][00:20.00] chorus'),
  [{ t: 1, text: 'chorus' }, { t: 12.5, text: 'second' }, { t: 20, text: 'chorus' }]);
assert.equal(ly.parseLrc('[00:01.00] a\x1b[2J\x07b')[0].text, 'a[2Jb'); // no escape sequences reach the terminal
assert.deepEqual(ly.wrap('one two three', 7), ['one two', 'three']);
assert.deepEqual(ly.wrap('日本語の歌', 4), ['日本', '語の', '歌']);
const rec = (duration, synced, plain) => ({ duration, syncedLyrics: synced, plainLyrics: plain });
assert.equal(ly.pick([rec(300, '[00:01.00] far'), rec(201, '[00:01.00] near')], 200).synced[0].text, 'near');
assert.deepEqual(ly.pick([rec(300, '[00:01.00] far', 'words')], 200), { plain: ['words'] }); // other length: timing not trusted
assert.deepEqual(ly.pick([], 200), {});
console.log('lyrics ok');

// recap: real time listened, per window
const day = 86400000, now = Date.now();
process.env.YM_DB = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'ymus-recap-')), 't.db');
const db2 = await import('./db.js?recap');
const song = (id, artist, album) => ({ id, title: id, artist, album, cover: null });
const add = (t, ago, seconds) => db2.endPlay(db2.startPlay(t, now - ago * day), seconds);
add(song('s1', 'A, B', 'One'), 1, 100); add(song('s1', 'A, B', 'One'), 2, 100);
add(song('s2', 'B', 'Two'), 20, 500); add(song('s3', 'C', ''), 200, 50); add(song('s4', 'C', ''), 1, 0);
assert.deepEqual([7, 30, 365].map(d => db2.listened(now - d * day)), [200, 700, 750]);
const month = db2.recap(now - 30 * day);
assert.deepEqual(month.songs.map(r => [r.id, r.seconds, r.plays]), [['s2', 500, 1], ['s1', 200, 2]]);
assert.deepEqual(month.artists.map(r => [r.title, r.seconds]), [['B', 700], ['A', 200]]);
assert.deepEqual(month.albums.map(r => [r.title, r.artist]), [['Two', 'B'], ['One', 'A']]);
db2.toggleLike(song('s1', 'A, B', '')); // a later save without an album keeps the known one
assert.equal(db2.liked()[0].album, 'One');
console.log('recap ok');

const reco = await import('./recommend.js');
const r = (id, title, artist) => ({ id, title, artist });
const at = new Date(2026, 5, 10, 9).getTime(); // a Wednesday morning
assert.ok(reco.relevance(at, at) > reco.relevance(at - 30 * day, at)); // recent counts more
assert.ok(reco.relevance(at - 7 * day, at) > reco.relevance(at - 7 * day + 11 * 3600000, at)); // same time of day counts more
assert.deepEqual([{ skipped: 1, seconds: 10 }, { seconds: 190, duration: 200 }, { seconds: 60, duration: 200 }, { seconds: 5 }].map(reco.playValue), [-1, 1, 0.5, 0]);
const model = reco.taste({
  plays: [{ ...r('m1', 'Mine', 'A'), at, seconds: 200, duration: 200 }, { ...r('m2', 'Meh', 'S'), at, seconds: 5, skipped: 1 },
    { ...r('m3', 'Meh2', 'S'), at, seconds: 5, skipped: 1 }, { ...r('m4', 'Meh3', 'S'), at, seconds: 5, skipped: 1 }],
  liked: [{ ...r('m1', 'Mine', 'A'), at }], disliked: [r('d', 'Hate', 'H')],
}, at);
assert.equal(model.tracks[0].id, 'm1');
assert.ok(model.artists.get('s').score < -3 && model.artists.get('h').score < -3);
assert.deepEqual(reco.pickSeeds(model.tracks).map(t => t.id), ['m1']); // only what you enjoy
const known = [r('m1', 'Mine', 'A'), r('d', 'Hate', 'H')];
const lists = [{ via: 'a', weight: 1, tracks: [r('x', 'Mine (Official Video)', 'A'), r('s1', 'By skipped', 'S'), r('n1', 'New', 'Z'), r('n2', 'Two', 'Z'), r('n3', 'Three', 'Z'), r('n5', 'Five', 'W')] },
  { via: 'b', weight: 1, tracks: [r('n1', 'New', 'Z'), r('n4', 'Four', 'Y')] }];
const got = reco.rank(lists, { model, heard: known, rand: () => 0.5 });
assert.equal(got[0].id, 'n1'); // two sources lead to it
assert.ok(got.every(t => t.fresh && !['x', 's1', 'm1', 'd'].includes(t.id))); // only new, no artists you keep skipping
assert.equal(got.filter(t => t.artist === 'Z').length, 2);
assert.ok(got.every((t, i) => !i || t.artist !== got[i - 1].artist)); // never the same artist twice in a row
const art = (name, score, tags, similar = []) => ({ name, score, tags, similar });
assert.deepEqual(reco.styles([art('Daft', 5, ['house', 'electronic']), art('Justice', 3, ['electronic', 'house']), art('Phoebe', 4, ['indie', 'folk']), art('Julien', 1, [], ['phoebe']), art('Nobody', 1, [])]),
  [{ name: 'house · electronic', artists: ['Daft', 'Justice'] }, { name: 'indie · folk', artists: ['Phoebe', 'Julien'] }]);
assert.equal(reco.toTrack({ primary: { video_id: 'v', title: 'T', artists: [{ name: 'A' }], thumbnail: [{ width: 1, url: 'u' }] } }).cover, 'u');
console.log('recommend ok');

// like and dislike exclude each other; disliked songs leave the favorites and the recommendations
const bad = song('d1', 'Q', 'Al');
db2.toggleLike(bad); assert.equal(db2.toggleDislike(bad), true);
assert.ok(!db2.likedIds().has('d1') && db2.disliked().some(t => t.id === 'd1') && !db2.topTracks().some(t => t.id === 'd1') && !db2.topArtists().some(a => a.title === 'Q'));
db2.toggleLike(bad); assert.equal(db2.disliked().length, 0);
console.log('dislike ok');

// the engine's view of the log, and the cache
const id1 = db2.startPlay(song('k1', 'K', 'Al')); db2.endPlay(id1, 12, 200, true);
const row = db2.signals().plays.find(p => p.id === 'k1');
assert.deepEqual([row.seconds, row.duration, row.skipped], [12, 200, 1]);
assert.ok(db2.heard().some(t => t.id === 'k1'));
db2.cacheSet('k', { a: [1] }); assert.deepEqual(db2.cacheGet('k'), { a: [1] }); assert.equal(db2.cacheGet('k', -1), undefined);
db2.cacheSet('none', null); assert.equal(db2.cacheGet('none'), null);
console.log('signals ok');


// cover color: a mostly muted-green cover with one pure red pixel stays green; a gray cover stays gray
const { vivid } = await import('./cover.js');
const px = (...cs) => Buffer.from(cs.flat());
assert.deepEqual(vivid(px(...Array(20).fill([90, 150, 70]), [250, 20, 20])), [90, 150, 70]);
assert.deepEqual(vivid(px([128, 128, 128], [128, 128, 128])), [128, 128, 128]);
console.log('cover ok');

// trip: every row is exactly w visible characters, for every effect, silent or loud
const trip = await import('./trip.js');
const seen = new Set();
let tt = 0;
for (const _ of Object.keys(trip.MODES)) for (const b of [0, 1]) {
  const rows = trip.frame(37, 9, tt += 0.05, Array(32).fill(b), [200, 40, 40], 'song-a');
  assert.equal(rows.length, 9);
  for (const r of rows) assert.equal(r.replace(/\x1b\[[\d;]*m/g, '').length, 37);
  seen.add(trip.mode()); if (b) trip.shift(1);
}
assert.equal(seen.size, Object.keys(trip.MODES).length); // ] walks through every mode
trip.frame(37, 9, tt += 0.05, Array(32).fill(0.5), [200, 40, 40], 'song-b'); const mb = trip.mode();
trip.frame(37, 9, tt += 0.05, Array(32).fill(0.5), [200, 40, 40], 'song-a'); trip.shift(0);
trip.frame(37, 9, tt += 0.05, Array(32).fill(0.5), [200, 40, 40], 'song-b');
assert.equal(trip.mode(), mb); // a song always opens with the same mode
console.log('trip ok');
