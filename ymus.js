#!/usr/bin/env node
// ymus: search YouTube Music and play it inside the terminal.
// mpv runs headless and is driven over its JSON IPC socket. No account: everything is anonymous.
// Likes, play history, search history, settings and the last session live in a local SQLite file (see db.js).
import { Innertube } from 'youtubei.js';
import { spawn } from 'node:child_process';
import net from 'node:net';
import fs from 'node:fs';
import readline from 'node:readline';
import * as cover from './cover.js';
import { RED, GRAY, TXT, GREEN, RIB, RESET, bgc, fgc, cut, cell, tabBar, searchBar, frame, hints, spectrum, LIKE, UNLIKED, DISLIKE } from './zj.js';
import * as viz from './viz.js';
import * as trip from './trip.js';
import * as lyrics from './lyrics.js';
import { buildMix, findStyles } from './mixes.js';
import { createListenBrainz } from './listenbrainz.js';

const TOP = 4; // first screen row inside the body frame (tab bar, search bar, frame border)

const db = await import('./db.js');
const ytp = Innertube.create(); // connects while the UI starts; awaited by the first search
ytp.catch(() => {}); // a failure surfaces in the red message of that search, not as a crash
// shortcut: macOS caps unix socket paths at ~104 chars, so /tmp, not os.tmpdir()
const SOCK = `/tmp/ymus-${process.pid}.sock`;
const out = process.stdout;

function loadSession() { // the queue and position of the previous run, if it did not finish
  try {
    const s = JSON.parse(db.getSetting('session') || 'null');
    return s && Array.isArray(s.queue) && s.queue[s.pos] ? s : null;
  } catch { return null; }
}

const st = {
  tab: 'home', prevTab: 'home', typing: false, buf: '', hIdx: -1, hList: [], draft: '',
  msg: '', note: '', busy: '', help: false, full: false, menu: null, lyrics: false, lyricScroll: 0, lyricLine: -1,
  // results view: current list and the stack of lists to go back to. A 'mixed' list has a type on every item, plus headers
  items: [], itemKind: 'mixed', title: '', numbered: false, back: [], query: '',
  // home tab: two lists, one focused at a time (st.sel belongs to the focused one)
  homeSongs: [], homeArtists: [], homeFocus: 'songs', homeSel: { songs: 0, artists: 0 },
  liked: [], sel: 0, searchSel: 0,
  // stats tab
  stats: { rows: [], totals: [0, 0, 0] }, statCat: 0, period: 0, heard: 0, rawPos: null, statsAt: 0, playDur: 0,
  queue: [], pos: -1, paused: false, loading: false, time: 0, dur: 0, vol: Number(db.getSetting('volume') ?? 100), playId: null,
  direct: new Set(), session: loadSession(),
  wave: db.getSetting('wave') !== 'off', apos: null, bands: Array(viz.BANDS).fill(0), energy: 0,
  trip: false, tripT: 0, tripAt: 0, // trip: the psychedelic visualizer inside the full player; tripT its own clock
  shown: '', mq: { key: '', at: 0, step: 0, on: false }, want: '', wantAt: 0, geo: {}, drag: false, lastClick: null,
};
let likes = db.likedIds(), dislikes = new Set(db.disliked().map(t => t.id));

// ---------- mpv ----------
const mpvArgs = ['--idle=yes', '--no-video', '--no-terminal', '--ytdl-format=bestaudio/best', '--input-ipc-server=' + SOCK,
  '--prefetch-playlist=yes', '--demuxer-readahead-secs=30']; // open the next entry early and buffer its first seconds
mpvArgs.push(`--volume=${st.vol}`); // as you left it
if (process.env.YM_MPV_ARGS) mpvArgs.push(...process.env.YM_MPV_ARGS.split(' ')); // e.g. YM_MPV_ARGS=--ao=null for silent runs
const mpv = spawn('mpv', mpvArgs, { stdio: ['ignore', 'ignore', 'ignore', 'pipe'] }); // fd 3: the visualizer's measurements
const feed = viz.createFeed();
mpv.stdio[3]?.on('data', d => feed.push(d)); // must always be drained, or the audio filter would block
mpv.on('error', e => quit(e.code === 'ENOENT' ? 'mpv not found: brew install mpv yt-dlp' : String(e)));
mpv.on('exit', () => quit());

let sock;
const cmd = (...command) => sock?.write(JSON.stringify({ command }) + '\n');
const trackUrl = t => 'https://music.youtube.com/watch?v=' + t.id;

async function connect() {
  for (let i = 0; i < 100 && !fs.existsSync(SOCK); i++) await new Promise(r => setTimeout(r, 50));
  sock = net.connect(SOCK);
  let rest = '';
  sock.on('data', d => {
    const lines = (rest + d).split('\n');
    rest = lines.pop();
    lines.forEach(l => onEvent(JSON.parse(l)));
  });
  ['time-pos', 'duration', 'pause', 'volume', 'playlist-pos', 'audio-pts'].forEach((p, i) => cmd('observe_property', i + 1, p));
  if (st.wave) applyWave();
}

const saveHeard = skipped => { if (st.playId) db.endPlay(st.playId, Math.round(st.heard), st.playDur ? Math.round(st.playDur) : null, skipped); };
// A skip: you moved on (mpv reports "stop") before 30 s of a song that is longer than that.
function logEnd(reason) {
  saveHeard(reason === 'stop' && st.heard < 30 && !(st.playDur && st.playDur < 40));
  st.playId = null;
}

const friendly = err => (err === 'loading failed'
  ? 'Could not load this track (yt-dlp failed). It may be restricted or unavailable.' : 'Playback error: ' + (err ?? 'unknown'));

function onEvent(e) {
  if (e.event === 'property-change') {
    const v = e.data;
    if (e.name === 'audio-pts') { st.apos = v == null ? null : { pos: v, at: performance.now() }; return; } // the position being heard
    if (e.name === 'time-pos') { // seconds actually heard: small steps forward count, seeks (big jumps) do not
      const step = v - st.rawPos;
      if (st.rawPos != null && v != null && step > 0 && step < 2) {
        const before = Math.floor(st.heard / 5);
        st.heard += step;
        if (Math.floor(st.heard / 5) !== before) saveHeard(); // every 5 s, so stats are live and a crash loses little
      }
      st.rawPos = v ?? null;
    }
    if (e.name === 'time-pos') { const t = Math.floor(v ?? 0); if (t === st.time) return; st.time = t; }
    else if (e.name === 'duration') { st.dur = v ?? 0; if (v > 0) st.playDur = v; }
    else if (e.name === 'pause') st.paused = !!v;
    else if (e.name === 'volume' && v != null) { st.vol = Math.round(v); db.setSetting('volume', String(st.vol)); }
    else if (e.name === 'playlist-pos') { st.pos = v ?? -1; st.time = 0; st.dur = 0; }
  } else if (e.event === 'start-file') { st.loading = true; st.msg = ''; st.playDur = 0; }
  else if (e.event === 'file-loaded') {
    st.loading = false;
    st.heard = 0; st.rawPos = null;
    if (st.queue[st.pos]) st.playId = db.startPlay(st.queue[st.pos]);
    preload();
  } else if (e.event === 'end-file') {
    st.loading = false;
    logEnd(e.reason);
    if (e.reason === 'error') st.msg = friendly(e.file_error);
    if (e.reason !== 'redirect' && st.pos >= st.queue.length - 1 && e.reason !== 'error') st.pos = -1;
  }
  draw();
}

function applyWave() {
  cmd('af', st.wave ? 'add' : 'remove', st.wave ? viz.FILTER : '@viz');
}

// Once something plays, the last session's row leaves Home.
function dropResumeRow() {
  st.session = null;
  if (st.homeSongs[0]?.type !== 'resume') return;
  st.homeSongs.shift();
  if (st.tab === 'home' && st.homeFocus === 'songs') st.sel = Math.max(0, st.sel - 1);
  else st.homeSel.songs = Math.max(0, st.homeSel.songs - 1);
}

function play(tracks) {
  dropResumeRow();
  st.queue = tracks;
  st.direct = new Set();
  st.pos = 0; st.loading = true; st.paused = false;
  tracks.forEach((t, i) => cmd('loadfile', trackUrl(t), i ? 'append' : 'replace'));
  cmd('set_property', 'pause', false);
}

// Continue the previous run: from the saved track and second onward.
function resume() {
  const s = st.session;
  if (!s) return;
  dropResumeRow();
  st.queue = s.queue.slice(s.pos);
  st.direct = new Set();
  st.pos = 0; st.loading = true; st.paused = false;
  st.queue.forEach((t, i) => (i ? cmd('loadfile', trackUrl(t), 'append') : cmd('loadfile', trackUrl(t), 'replace', -1, `start=${s.time}`)));
  cmd('set_property', 'pause', false);
}

function saveSession() {
  if (!st.queue.length) return; // never played this run: keep whatever was saved
  const keep = st.pos >= 0;
  db.setSetting('session', keep ? JSON.stringify({
    queue: st.queue.slice(0, 500).map(({ id, title, artist, album, cover: c }) => ({ id, title, artist, album, cover: c })),
    pos: st.pos, time: st.time,
  }) : ''); // finished playing: nothing to resume
}

// Resolve the stream URL of every upcoming track with yt-dlp (two at a time) and swap it into mpv's playlist.
// Without this mpv only resolves a track when it becomes current (~3s of silence). With real URLs in place, mpv's
// prefetch opens the next entry early and buffers its first seconds, and jumping to any queued track starts in under a second.
const resolving = new Set();
function preload() {
  for (let i = st.pos + 1; i < st.queue.length && resolving.size < 2; i++) {
    const t = st.queue[i];
    if (!st.direct.has(t.id) && !resolving.has(t.id)) resolve(t);
  }
}
function resolve(t) {
  resolving.add(t.id);
  const args = ['-g', '-f', 'bestaudio/best', '--no-playlist', trackUrl(t)];
  const y = spawn('yt-dlp', args, { stdio: ['ignore', 'pipe', 'ignore'] });
  let o = '';
  y.stdout.on('data', d => (o += d));
  y.on('error', () => resolving.delete(t.id));
  y.on('close', code => {
    resolving.delete(t.id);
    const url = o.split('\n')[0].trim();
    const idx = st.queue.indexOf(t); // the queue may have changed while yt-dlp ran
    if (code === 0 && url && idx > st.pos) {
      st.direct.add(t.id);
      cmd('playlist-remove', idx);
      cmd('loadfile', url, 'insert-at', idx);
    } else if (code !== 0) st.direct.add(t.id); // do not retry a failing track forever
    preload();
  });
}

// ---------- queue editing (st.queue mirrors mpv's playlist, entry by entry) ----------
function queueTrack(t, next = false) {
  if (next && st.pos >= 0) { st.queue.splice(st.pos + 1, 0, t); cmd('loadfile', trackUrl(t), 'insert-next'); }
  else { st.queue.push(t); cmd('loadfile', trackUrl(t), 'append-play'); } // plays right away if nothing is playing
  preload();
}

function removeFromQueue(t) {
  const idx = st.queue.indexOf(t);
  if (idx < 0) return;
  st.queue.splice(idx, 1);
  if (idx < st.pos) st.pos--;
  cmd('playlist-remove', idx);
  st.sel = Math.min(st.sel, Math.max(0, st.queue.length - 1));
  preload();
}

function moveInQueue(t, dir) {
  const idx = st.queue.indexOf(t), j = idx + dir, now = st.queue[st.pos];
  if (idx < 0 || j < 0 || j >= st.queue.length) return;
  [st.queue[idx], st.queue[j]] = [st.queue[j], st.queue[idx]];
  cmd('playlist-move', idx, dir > 0 ? idx + 2 : j); // mpv moves the entry to *before* the target index
  if (now) st.pos = st.queue.indexOf(now);
  st.sel = j;
}

// ---------- data ----------
const list = () => (st.tab === 'home' ? (st.homeFocus === 'songs' ? { kind: 'tracks', data: st.homeSongs } : { kind: 'artistStats', data: st.homeArtists })
  : st.tab === 'liked' ? { kind: 'tracks', data: st.liked }
  : st.tab === 'queue' ? { kind: 'tracks', data: st.queue }
  : st.tab === 'stats' ? { kind: 'tracks', data: st.stats.rows }
  : { kind: st.itemKind, data: st.items });
const bestThumb = ts => ts?.reduce((m, t) => (t.width > (m?.width ?? 0) ? t : m), null)?.url;
const names = xs => xs?.map(x => x.name).join(', ') ?? '';

const DISCOVER = { type: 'discover', id: 'discover', title: '✦ Discover', detail: 'new songs · mixes by style' };
function resumeRow() {
  const t = st.session?.queue[st.session.pos];
  return t ? [{ type: 'resume', id: 'resume', title: `⏯ Resume: ${t.title}`, detail: `${t.artist} · at ${fmt(st.session.time)}` }] : [];
}
const loadHome = () => { st.homeSongs = [...resumeRow(), DISCOVER, ...db.topTracks()]; st.homeArtists = db.topArtists(); };

const CATS = ['songs', 'artists', 'albums'];
const PERIODS = [['week', 7], ['month', 30], ['year', 365]]; // rolling windows ending now
const minutes = s => (s > 0 && s < 60 ? '<1 min' : `${Math.round(s / 60)} min`);
function loadStats() {
  const since = days => Date.now() - days * 86400000;
  const cat = CATS[st.statCat];
  const type = { songs: 'tracks', artists: 'artistStats', albums: 'albumStats' }[cat];
  st.stats = {
    totals: PERIODS.map(([, days]) => db.listened(since(days))),
    rows: db.recap(since(PERIODS[st.period][1]))[cat].map((r, i) => ({
      ...r, type, rank: i + 1,
      detail: `${minutes(r.seconds)} · ${r.plays}×${cat === 'artists' ? '' : ' · ' + r.artist}`,
    })),
  };
}

const lb = createListenBrainz({ get: db.cacheGet, set: db.cacheSet });
const sources = async () => ({ yt: await ytp, lb, db });
const mixRow = (id, title, detail, spec) => ({ type: 'mix', id, title, detail, spec });

// The Discover view: one mix of everything at once, and a mix per style as soon as they are worked out.
function openDiscover() {
  if (!db.heard().length) { st.msg = 'Play or like a few songs first'; return; }
  openResults();
  st.back = [];
  const all = mixRow('all', '✦ Everything new', 'from all your taste', { kind: 'all' });
  showList('Discover · only songs you have not heard', [all, { type: 'header', title: 'Reading your styles…' }]);
  st.sel = 0;
  const items = st.items;
  const put = rows => { items.splice(1, items.length - 1, ...rows); draw(); };
  sources().then(findStyles).then(found => put(found.length
    ? [{ type: 'header', title: 'By style' }, ...found.map(s => mixRow(s.name, `✦ ${s.name}`, s.artists.slice(0, 3).join(', '), { kind: 'style', artists: s.artists }))]
    : [{ type: 'header', title: 'No styles yet: listen to more artists' }]))
    .catch(e => put([{ type: 'header', title: `Styles unavailable: ${e.message}` }]));
}

function playMix(spec, label) {
  if (st.busy) return;
  busy(`Building ${label}…`, async () => {
    const { tracks, withoutListenBrainz } = await buildMix(await sources(), spec);
    if (!tracks.length) { st.msg = 'Nothing new found, try another mix'; return; }
    play(tracks);
    setTab('queue');
    note(`${label}: ${tracks.filter(t => t.fresh).length} new songs${withoutListenBrainz ? ' · without ListenBrainz' : ''}`);
  });
}

// A mix of new songs around whatever a row is (or the playing song).
function startRadio(it, type) {
  if (!it || !['tracks', 'artists', 'artistStats', 'albums', 'albumStats'].includes(type)) [it, type] = [st.queue[st.pos], 'tracks'];
  if (!it) return note('Nothing to start a radio from');
  const artist = type === 'albums' ? it.author : type === 'albumStats' ? it.artist : it.title;
  if (type === 'tracks') playMix({ kind: 'radio', track: { id: it.id, title: it.title, artist: it.artist, album: it.album, cover: it.cover } }, `Radio · ${it.title}`);
  else playMix({ kind: 'radio', artist }, `Radio · ${artist}`);
}

function focusHome(pane) {
  if (pane === st.homeFocus) return;
  st.homeSel[st.homeFocus] = st.sel;
  st.homeFocus = pane;
  st.sel = st.homeSel[pane];
}

const pushBack = () => st.back.push({ items: st.items, itemKind: st.itemKind, title: st.title, numbered: st.numbered, sel: st.sel });
function goBack() {
  const b = st.back.pop();
  if (b) Object.assign(st, { items: b.items, itemKind: b.itemKind, title: b.title, numbered: b.numbered, sel: b.sel });
}

// Runs a slow API call with the spinner on; errors land in the red message.
async function busy(label, fn) {
  st.busy = label; st.msg = ''; draw();
  try { await fn(); } catch (e) { st.msg = `${label.replace('…', '')} failed: ${e.message}`; }
  st.busy = '';
  draw();
}

// Search results replace the body of whatever tab you were on; Esc (or any tab) brings it back.
function openResults() {
  if (st.tab === 'search') return;
  if (st.tab === 'home') st.homeSel[st.homeFocus] = st.sel;
  st.prevTab = st.tab; st.tab = 'search'; st.menu = null; st.lyrics = false;
}
const closeResults = () => { if (st.tab === 'search') setTab(st.prevTab); };
function reopenResults() {
  if (!st.items.length || st.tab === 'search') return;
  openResults();
  st.sel = Math.min(st.searchSel, st.items.length - 1);
}

// One list made of titled groups; empty groups disappear.
const grouped = groups => groups.flatMap(([title, items]) => (items.length ? [{ type: 'header', title }, ...items] : []));
function showList(title, items) {
  Object.assign(st, { items, itemKind: 'mixed', title, numbered: false, sel: items.length ? 1 : 0 }); // row 0 is a header
}

const runSearch = text => busy('Searching…', async () => {
  db.saveSearch(text);
  const yt = await ytp;
  const find = async (type, max, toItem) =>
    ((await yt.music.search(text, { type }))[type + 's']?.contents ?? []).filter(x => x.id).slice(0, max).map(toItem);
  const [artists, albums, songs] = await Promise.all([
    find('artist', 3, x => ({ type: 'artists', id: x.id, title: x.name ?? '', author: 'Artist', cover: bestThumb(x.thumbnail?.contents) })),
    find('album', 6, x => ({ type: 'albums', id: x.id, title: x.title ?? '', author: x.author?.name ?? '', year: x.year, cover: bestThumb(x.thumbnail?.contents) })),
    find('song', 20, x => ({ type: 'tracks', id: x.id, title: x.title ?? '', artist: names(x.artists), album: x.album?.name ?? '', cover: bestThumb(x.thumbnail?.contents) })),
  ]);
  st.query = text;
  openResults();
  st.back = [];
  showList(`Results for "${text}"`, grouped([['Artists', artists], ['Albums', albums], ['Songs', songs]]));
  if (!st.items.length) st.msg = 'Nothing found';
});

async function showAlbum(a) {
  const al = await (await ytp).music.getAlbum(a.id);
  const tracks = al.contents.filter(t => t.id).map(t => ({
    id: t.id, title: t.title ?? '', artist: names(t.artists) || a.author, album: a.title, cover: a.cover,
  }));
  if (st.tab === 'search') pushBack();
  else { openResults(); st.back = []; }
  Object.assign(st, { items: tracks, itemKind: 'tracks', title: `${a.title} · ${a.author}`, numbered: true, sel: 0 });
}
const openAlbum = a => busy('Loading album…', () => showAlbum(a));
// An album known only by name and artist (Stats).
const goAlbum = ({ title, artist }) => busy('Loading album…', async () => {
  const found = (await (await ytp).music.search(`${title} ${artist}`, { type: 'album' })).albums?.contents ?? [];
  const a = found.find(x => x.title?.toLowerCase() === title.toLowerCase()) ?? found[0];
  if (!a) { st.msg = `No album found: ${title}`; return; }
  await showAlbum({ id: a.id, title: a.title, author: a.author?.name ?? artist, cover: bestThumb(a.thumbnail?.contents) });
});

// The artist's page: popular songs first, then albums.
async function showArtist(a) {
  const page = await (await ytp).music.getArtist(a.id);
  const section = re => page.sections?.find(x => re.test(String(x.title ?? x.header?.title ?? '')))?.contents?.filter(x => x.id) ?? [];
  const songs = section(/^(top )?songs$/i).map(x => ({
    type: 'tracks', id: x.id, title: String(x.title ?? ''), artist: names(x.artists) || a.title,
    album: String(x.flex_columns?.[3]?.title ?? ''), cover: bestThumb(x.thumbnail?.contents),
  }));
  const albums = section(/^albums$/i).map(x => ({
    type: 'albums', id: x.id, title: String(x.title ?? ''), author: a.title, year: x.year, cover: bestThumb(x.thumbnail),
  }));
  if (!songs.length && !albums.length) { st.msg = 'Nothing found for this artist'; return; }
  if (st.tab === 'search') pushBack(); // Esc returns to where you were
  else { openResults(); st.back = []; }
  showList(a.title, grouped([['Popular songs', songs], ['Albums', albums]]));
}
const openArtist = a => busy('Loading artist…', () => showArtist(a));

// Open an artist known only by name (Home, and "Go to artist" on a song).
function goArtist(credit) {
  const name = String(credit).split(', ')[0];
  st.typing = false;
  busy('Loading artist…', async () => {
    const found = (await (await ytp).music.search(name, { type: 'artist' })).artists?.contents ?? [];
    const a = found.find(x => x.name?.toLowerCase() === name.toLowerCase()) ?? found[0];
    if (!a) { st.msg = `No artist found: ${name}`; return; }
    await showArtist({ id: a.id, title: a.name });
  });
}

function startTyping() {
  st.buf = ''; st.hIdx = -1; st.typing = true;
}

const TABS = ['home', 'liked', 'queue', 'stats'];
const cycleTab = dir => {
  const base = st.tab === 'search' ? st.prevTab : st.tab;
  setTab(TABS[(TABS.indexOf(base) + dir + TABS.length) % TABS.length]);
};
function setTab(tab) {
  if (st.tab === 'home') st.homeSel[st.homeFocus] = st.sel;
  if (st.tab === 'search') st.searchSel = st.sel;
  st.tab = tab; st.typing = false; st.menu = null; st.lyrics = false;
  if (tab === 'liked') st.liked = db.liked();
  if (tab === 'home') loadHome();
  if (tab === 'stats') loadStats();
  st.sel = tab === 'search' ? st.searchSel : tab === 'home' ? st.homeSel[st.homeFocus] : tab === 'queue' ? Math.max(0, st.pos) : 0;
  st.sel = Math.min(st.sel, Math.max(0, list().data.length - 1));
}

let noteTimer;
function note(text) {
  st.note = text; clearTimeout(noteTimer);
  noteTimer = setTimeout(() => { st.note = ''; draw(); }, 3000);
}

// Likes or dislikes t; without t, the highlighted track, or the playing one when the highlight is not a track
// or the list is hidden (full player, lyrics).
function rate(kind, t = null) {
  t ??= !st.full && !st.lyrics && typeAt() === 'tracks' ? list().data[st.sel] : st.queue[st.pos];
  if (!t) return note(`Nothing to ${kind}`);
  const on = kind === 'like' ? db.toggleLike(t) : db.toggleDislike(t);
  likes = db.likedIds();
  dislikes = new Set(db.disliked().map(x => x.id));
  if (st.tab === 'liked') st.liked = db.liked();
  if (st.tab === 'home') loadHome();
  if (st.tab === 'stats') loadStats();
  st.sel = Math.min(st.sel, Math.max(0, list().data.length - 1));
  note(`${!on ? `Removed ${kind}` : kind === 'like' ? `${LIKE} Liked` : `${DISLIKE} Disliked`}: ${t.title}`);
  // disliking what is playing moves on to the next song
  if (on && kind === 'dislike' && st.queue[st.pos]?.id === t.id && st.pos < st.queue.length - 1) cmd('playlist-next');
}

// Is this row the track that is playing? In the queue the same song can appear twice, so compare the entry itself.
const isCurrent = it => { const now = st.queue[st.pos]; return !!now && (st.tab === 'queue' ? it === now : it.id === now.id); };

// What the highlighted row is: mixed lists say it per item.
const typeAt = () => { const { kind, data } = list(); return data[st.sel]?.type ?? kind; };
// The songs from row i on (a mixed list also holds artists, albums and headers).
const tracksFrom = i => { const { kind, data } = list(); return data.slice(i).filter(t => (t.type ?? kind) === 'tracks'); };
// Headers cannot be selected: step past them, turning back at either end of the list.
function moveSel(to) {
  const { data } = list(), dir = to < st.sel ? -1 : 1;
  const at = Math.max(0, Math.min(data.length - 1, to));
  let i = at;
  while (data[i]?.type === 'header') i += dir;
  if (!data[i]) for (i = at; data[i]?.type === 'header';) i -= dir;
  st.sel = Math.max(0, i);
}

function activate() {
  const it = list().data[st.sel], kind = typeAt();
  if (!it) return;
  if (kind === 'albums') openAlbum(it);
  else if (kind === 'artists') openArtist(it);
  else if (kind === 'artistStats') goArtist(it.title);
  else if (kind === 'albumStats') goAlbum(it);
  else if (kind === 'discover') openDiscover();
  else if (kind === 'mix') playMix(it.spec, it.title.replace('✦ ', ''));
  else if (kind === 'resume') resume();
  else if (isCurrent(it)) cmd('cycle', 'pause'); // never restarts what is already playing
  else if (st.tab === 'queue') cmd('playlist-play-index', st.queue.indexOf(it));
  else play(tracksFrom(st.sel));
}

// Enter and space are the same key: act on the highlighted row; with no row in sight, play or pause.
function press() {
  if (!st.full && !st.lyrics && list().data[st.sel]) activate();
  else if (st.queue[st.pos]) cmd('cycle', 'pause');
  else resume();
}

function back() {
  if (st.menu) st.menu = null;
  else if (st.full) st.full = false;
  else if (st.lyrics) st.lyrics = false;
  else if (st.tab === 'search') { if (st.back.length) goBack(); else closeResults(); }
}

// Actions on the highlighted row (key m, or right-click on a row).
function openMenu() {
  const idx = st.sel, it = list().data[idx], kind = typeAt();
  if (!it) return;
  let items;
  if (kind === 'albums') items = [['Open album', () => openAlbum(it)]];
  else if (kind === 'artists') items = [['Open artist', () => openArtist(it)]];
  else if (kind === 'artistStats') items = [['Open artist', () => goArtist(it.title)]];
  else if (kind === 'albumStats') items = [['Open album', () => goAlbum(it)]];
  else if (kind === 'discover') items = [['Open Discover', openDiscover]];
  else if (kind === 'mix') items = [['Play mix', () => playMix(it.spec, it.title.replace('✦ ', ''))]];
  else if (kind === 'resume') items = [['Resume', resume]];
  else {
    items = [
      st.tab === 'queue' ? ['Jump to this track', () => cmd('playlist-play-index', st.queue.indexOf(it))] : ['Play from here', () => play(tracksFrom(idx))],
      ['Play next', () => { queueTrack(it, true); note(`Next: ${it.title}`); }],
      ['Add to queue', () => { queueTrack(it); note(`Queued: ${it.title}`); }],
      [likes.has(it.id) ? 'Remove like' : 'Like', () => rate('like', it)],
      [dislikes.has(it.id) ? 'Remove dislike' : 'Dislike', () => rate('dislike', it)],
      ['Go to artist', () => goArtist(it.artist || it.title)],
      ...(st.tab === 'queue' ? [['Remove from queue', () => removeFromQueue(it)]] : []),
    ];
  }
  if (['tracks', 'artists', 'artistStats', 'albums', 'albumStats'].includes(kind)) items.push(['Start radio', () => startRadio(it, kind)]);
  st.menu = { title: it.title, items, sel: 0 };
}

function runMenu(i) {
  const m = st.menu;
  st.menu = null;
  m?.items[i]?.[1]();
}

function toggleLyrics() {
  st.lyrics = !st.lyrics;
  st.full = false; st.lyricScroll = 0;
  if (st.lyrics) lyrics.forgetErrors();
}

// Where playback is right now, finer than the whole seconds of st.time.
const position = () => (st.apos ? st.apos.pos + (st.paused || st.loading ? 0 : Math.min(0.3, (performance.now() - st.apos.at) / 1000)) : 0);
const lineAt = lines => lines.findLastIndex(l => l.t <= position() + 0.15); // a hair early reads as on time
const lyricsNow = () => { const now = st.queue[st.pos]; return now && st.dur ? lyrics.get(now, st.dur, draw) : undefined; };

// The L inner rows of the lyrics view. Timed lyrics follow the song; untimed ones are scrolled by hand.
function lyricRows(w, L, A) {
  const center = (text, style = x => x) => {
    const n = lyrics.width(text), p = Math.max(0, (w - n) >> 1);
    return ' '.repeat(p) + style(text) + ' '.repeat(Math.max(0, w - p - n));
  };
  const r = lyricsNow();
  const message = !st.queue[st.pos] ? 'Nothing playing' : !r ? `${spin()} Looking for lyrics…`
    : r.error ? 'Could not reach LRCLIB. Press y twice to retry.' : r.instrumental ? '♪ Instrumental ♪'
    : !r.synced?.length && !r.plain ? 'No lyrics found on LRCLIB.' : '';
  if (message) return Array.from({ length: L }, (_, i) => (i === L >> 1 ? center(cut(message, w), dim) : ' '.repeat(w)));
  const current = r.synced ? lineAt(r.synced) : -1;
  st.lyricLine = current;
  const rows = (r.synced?.map(l => l.text || '♪') ?? r.plain).flatMap((text, line) => lyrics.wrap(text, w - 4).map(part => ({ part, line })));
  let top;
  if (r.synced) top = Math.max(0, rows.findIndex(x => x.line === Math.max(0, current)) - (L >> 1));
  else top = st.lyricScroll = Math.max(0, Math.min(st.lyricScroll, rows.length - L));
  return Array.from({ length: L }, (_, i) => {
    const x = rows[top + i];
    if (!x) return ' '.repeat(w);
    return center(x.part, !r.synced ? id : x.line === current ? t => fg(A, bold(t)) : dim);
  });
}

function toggleWave() {
  st.wave = !st.wave;
  db.setSetting('wave', st.wave ? 'on' : 'off');
  applyWave();
  note(`Visualizer ${st.wave ? 'on' : 'off'}`);
}

// ---------- ui ----------
const fmt = s => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
const dim = s => `\x1b[2m${s}\x1b[0m`, bold = s => `\x1b[1m${s}\x1b[0m`, red = s => `\x1b[31m${s}\x1b[0m`, id = s => s;
const fg = (a, s) => `\x1b[38;2;${a.join(';')}m${s}\x1b[39m`;
// Selected row: accent bar + soft background. s is plain text of the row minus its first column.
const sel = (a, s) => `${bgc(RIB)}${fgc(a)}▌${fgc([255, 255, 255])}\x1b[1m${s}${RESET}`;
const pink = s => fg([244, 114, 182], s), blue = s => fg([125, 160, 220], s);
// [glyph, color] of a track's rating, or null.
const ratingOf = trackId => (likes.has(trackId) ? [LIKE, pink] : dislikes.has(trackId) ? [DISLIKE, blue] : null);
const tag = trackId => (ratingOf(trackId) ? `${ratingOf(trackId)[0]} ` : '');
const paint = (trackId, text) => { const r = ratingOf(trackId); return r ? text.replace(r[0], r[1](r[0])) : text; };
const SPIN = '⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏';
const spin = () => SPIN[Math.floor(Date.now() / 100) % SPIN.length];
// A cover color brightened for a dark background (and so black text on it stays readable).
const lift = c => c.map(v => Math.round(v * (230 / Math.max(1, ...c)) * 0.8 + 50));
// Accent = the most vivid color of the current cover; green until there is one, or if it has no color.
function accent(got) {
  const c = got?.rgb;
  return !c || Math.max(...c) - Math.min(...c) < 40 ? GREEN : lift(c);
}
// Text that does not fit scrolls sideways (only used on the focused row): it holds for a moment, then loops.
const MQ_STEP = 200, MQ_HOLD = 2;
const mqStep = () => Math.max(0, Math.floor((Date.now() - st.mq.at) / MQ_STEP) - MQ_HOLD);
function marquee(text, w) {
  if (text.length <= w) return text.padEnd(w);
  st.mq.on = true;
  const loop = text + '   ·   ';
  st.mq.step = mqStep();
  const from = st.mq.step % loop.length;
  return (loop + loop).slice(from, from + w);
}

// The L inner rows of a list pane, each exactly w columns: marks, title column, detail column.
// hl: draw the selection (only the focused pane).
function listRows(kind, data, selIdx, top, w, L, hl, A, now, empty) {
  return Array.from({ length: L }, (_, i) => {
    const idx = top + i, it = data[idx];
    if (!it) return i === 1 && !data.length ? dim(cell('   ' + empty, x => x, w)) : ' '.repeat(w);
    const type = it.type ?? kind;
    if (type === 'header') return ' ' + fg(A, bold(cell(it.title, x => x, w - 1)));
    const playing = type === 'tracks' && now && it.id === now.id, rating = type === 'tracks' ? ratingOf(it.id) : null;
    const num = it.rank ? String(it.rank).padStart(3) + '  ' : st.tab === 'search' && st.numbered ? String(idx + 1).padStart(2) + '  ' : '';
    const detail = it.detail ?? (type === 'albums' || type === 'artists' ? `${it.author}${it.year ? ' · ' + it.year : ''}`
      : type === 'artistStats' ? `${it.plays} plays${it.likes ? ` · ${LIKE} ${it.likes}` : ''}` : it.artist);
    const room = w - 5 - num.length; // bar, two marks, space, right margin
    const tw = Math.ceil(room * 0.6);
    const rw = Math.max(0, room - tw - 1), focused = hl && idx === selIdx;
    if (focused) { // a new row starts from the beginning
      const key = `${st.tab}|${idx}|${it.id}`;
      if (key !== st.mq.key) Object.assign(st.mq, { key, at: Date.now() });
    }
    const title = focused ? marquee(it.title, tw) : cell(it.title, x => x, tw);
    const rest = focused && rw ? marquee(detail, rw) : cell(detail, x => x, rw);
    const mark = playing ? '♪' : it.fresh ? '✦' : ' ';
    const marks = mark + (rating?.[0] ?? ' ') + ' ';
    if (focused) return sel(A, `${marks}${num}${title} ${rest} `);
    const styled = (mark === ' ' ? ' ' : fg(A, mark)) + (rating ? rating[1](rating[0]) : ' ') + ' ';
    return ` ${styled}${dim(num)}${playing ? fg(A, title) : title} ${dim(rest)} `;
  });
}

const HELP = [
  ['/', 'search · Enter shows artists, albums and songs · ↑ ↓ past searches'],
  ['↑ ↓', 'move · Esc goes back'],
  ['⏎ space', 'open or play the highlighted row · on the playing song, pause or resume'],
  ['Tab', 'switch between Home, Liked and Queue · ← → switches panel on Home'],
  ['n p', 'next / previous · Shift ← → seek 10s · + - volume'],
  ['l x a m', 'like · dislike (skips it, trains Discover) · add to queue · more actions'],
  ['d J K', 'in Queue: remove · move down / up'],
  ['v y w q', 'full player · lyrics · visualizer on / off · quit'],
  ['V', 'trip: psychedelic visualizer that follows the song (needs w on)'],
  ['D R', 'D discover new songs · R radio from the highlighted song, artist or album'],
  ['t', 'in Stats: week / month / year · ← → songs / artists / albums'],
  ['mouse', 'optional: click · double-click plays · right-click menu · drag the bar to seek'],
];
const EMPTY = {
  search: 'No results.', liked: 'No likes yet. Press l on any track.',
  queue: 'The queue is empty. Press Enter on a track, or a to add one.',
  stats: 'Nothing listened in this period yet.',
  homeSongs: 'Search with / and play something.', homeArtists: 'No artists yet.',
};

let pending = false;
function draw() {
  if (pending) return;
  pending = true;
  setTimeout(() => { pending = false; render(); }, 30);
}
setInterval(() => {
  let need = st.busy || st.loading;
  if (st.wave) { // move the bars at a steady pace; they fall to the baseline when nothing plays
    const playing = st.pos >= 0 && !st.paused && !st.loading && !!st.apos;
    // where playback will be when this frame reaches the screen (draw() waits 30 ms)
    const db = playing ? feed.at(st.apos.pos + Math.min(0.3, (performance.now() - st.apos.at) / 1000) + 0.03) : null;
    const next = db ? viz.levels(db, st.bands) : st.bands.map(v => (v > 0.02 ? v * 0.74 : 0));
    need ||= next.some((v, i) => v !== st.bands[i]); // no redraw once everything is still
    st.bands = next;
    st.energy += ((db ? viz.energy(db) : 0) - st.energy) * 0.15; // smoothed over about a quarter second
  }
  if (st.full && st.trip) { // its clock is playback time: it stops with the music
    const t = performance.now(), playing = st.pos >= 0 && !st.paused && !st.loading;
    if (playing) { st.tripT += Math.min(0.1, (t - st.tripAt) / 1000) ; need = true; }
    st.tripAt = t;
  }
  if (st.mq.on && mqStep() !== st.mq.step) need = true;
  if (st.tab === 'stats' && st.pos >= 0 && !st.paused && Date.now() - st.statsAt > 2000) { st.statsAt = Date.now(); loadStats(); need = true; }
  if (st.lyrics) { const r = lyricsNow(); need ||= !r ? !!st.queue[st.pos] : !!r.synced && lineAt(r.synced) !== st.lyricLine; }
  if (need) draw();
}, 40);

function infoOf(f) {
  if (!f) return [];
  const heart = likes.has(f.id) ? [`${LIKE}  Liked · l to remove`, pink] : dislikes.has(f.id) ? [`${DISLIKE}  Disliked · x to remove`, blue]
    : [`${UNLIKED}  l like  ·  ${DISLIKE}  x dislike`, dim];
  if (f.plays !== undefined && f.artist === undefined) return [[f.title, bold], [`${f.plays} plays${f.likes ? ` · ${LIKE} ${f.likes}` : ''}`, id]]; // artist stats
  if (f.author) return [[f.title, bold], [f.author + (f.year ? ' · ' + f.year : ''), id]]; // album / artist result
  return [[f.title, bold], [f.artist ?? '', id], [f.album ?? '', dim], ['', id], heart];
}

// Big cover + info for the full player. Real image where the terminal can draw one (kitty protocol, sent after the frame),
// colored half blocks everywhere else.
function fullPane(iw, L, focus, got) {
  let C = iw - 4;
  if (C / 2 + 6 > L) C = 2 * (L - 6); // leave room for the info lines
  const show = focus?.cover && C >= 12;
  const R = show ? Math.ceil(C / 2) : 0;
  const padL = (iw - C) >> 1;
  const geo = show && cover.supported ? { row: TOP, col: 2 + padL, cols: C } : null;
  const art = show && !cover.supported && got ? cover.cells(focus.cover, C, R, draw) : null;
  const info = infoOf(focus);
  const inner = Array.from({ length: L }, (_, i) => {
    if (i < R) return art?.[i] ? ' '.repeat(padL) + art[i] + ' '.repeat(iw - C - padL) : ' '.repeat(iw);
    const it = info[i - (R ? R + 1 : 0)];
    if (!it) return ' '.repeat(iw);
    const t = cut(it[0], iw - 2), p = (iw - t.length) >> 1;
    return ' '.repeat(p) + it[1](t) + ' '.repeat(iw - p - t.length);
  });
  return { inner, geo };
}

// The player dock: cover, title, artist, wave and progress in R content rows.
function dock(W, L, R, focus, got, A, now) {
  const iw = W - 2;
  const thumb = !!focus?.cover && !st.full; // the full player already shows the big cover
  const C = 2 * R, tw = thumb ? C + 2 : 0, tx = Math.max(10, iw - 2 - tw);
  const art = thumb && !cover.supported && got ? cover.cells(focus.cover, C, R, draw) : null;
  const geo = thumb && cover.supported ? { row: L + 6, col: 3, cols: C } : null;
  const blank = ' '.repeat(tx);

  const status = st.loading ? spin() : st.paused ? '⏸' : '▶';
  const count = now ? ` ${st.pos + 1}/${st.queue.length}` : '';
  let r0, r1;
  if (now) {
    const t = cut(`${status} ${tag(now.id)}${now.title}`, tx - count.length);
    r0 = fg(A, t[0]) + paint(now.id, bold(t.slice(1))) + ' '.repeat(Math.max(0, tx - t.length - count.length)) + dim(count);
    r1 = cell(`  ${now.artist}${now.album ? ' · ' + now.album : ''}`, dim, tx);
  } else if (st.session) {
    const q = st.session.queue[st.session.pos];
    r0 = fg(A, cell(`⏯ ${q.title} — ${q.artist}`, id, tx));
    r1 = cell(`  stopped at ${fmt(st.session.time)} · resume it from Home`, dim, tx);
  } else {
    r0 = focus ? cell(focus.title, bold, tx) : cell('Nothing playing', dim, tx);
    r1 = focus ? cell(focus.artist ?? focus.author ?? '', dim, tx) : blank;
  }
  const bl = `${fmt(now ? st.time : 0)} `;
  const right = cut(` ${now && st.dur ? fmt(st.dur) : '--:--'}   vol ${st.vol}`, Math.max(0, tx - bl.length));
  const bw = Math.max(0, tx - bl.length - right.length);
  const filled = st.dur ? Math.round((bw - 1) * Math.min(1, st.time / st.dur)) : 0;
  const prog = now && bw ? dim(bl) + fg(A, '━'.repeat(filled) + '●') + dim('─'.repeat(bw - 1 - filled) + right) + ' '.repeat(Math.max(0, tx - bl.length - bw - right.length)) : blank;
  Object.assign(st.geo, {
    barRow: L + 5 + R, barX0: 2 + tw + bl.length, barW: bw, nowRow: L + 6,
    thumb: thumb ? { x0: 3, x1: 2 + C, y0: L + 6, y1: L + 5 + R } : null,
  });

  const middle = st.wave && now ? spectrum(st.bands, tx, R - 3, A) : Array(R - 3).fill(blank);
  const text = [r0, r1, ...middle, prog];
  const inner = text.map((tr, i) => ' ' + (thumb ? (art?.[i] ?? ' '.repeat(C)) + '  ' : '') + tr + ' ');
  return { lines: frame(now ? 'Now playing' : st.session ? 'Last session' : focus ? 'Preview' : 'Now playing', TXT, inner, W, GRAY), geo };
}

function hintList(kind, now) {
  if (st.menu) return [['↑↓', 'move'], ['⏎', 'run'], ['esc', 'close']];
  if (st.lyrics && !st.typing) return [['y', 'close lyrics'], ['l', 'like'], ['x', 'dislike'], ['⏎/space', st.paused ? 'resume' : 'pause'], ['n p', 'next/prev'], ...(lyricsNow()?.plain ? [['↑↓', 'scroll']] : []), ['⇧←→', 'seek'], ['+ -', 'volume'], ['?', 'keys']];
  if (st.typing) return [['⏎', 'search'], ['↑↓', 'history'], ['esc', 'cancel']];
  if (st.full && st.trip) return [['V', 'close'], ['v', 'cover'], ['w', `react to music ${st.wave ? 'on' : 'off'}`], ['⏎/space', st.paused ? 'resume' : 'pause'], ['n p', 'next/prev'], ['+ -', 'volume'], ['?', 'keys']];
  if (st.full) return [['l', 'like'], ['x', 'dislike'], ['v', 'close'], ['y', 'lyrics'], ['⏎/space', st.paused ? 'resume' : 'pause'], ['n p', 'next/prev'], ['+ -', 'volume'], ['?', 'keys']];
  const cur = list().data[st.sel];
  kind = typeAt();
  const enter = kind === 'resume' ? 'resume' : kind === 'mix' ? 'play mix' : kind !== 'tracks' ? 'open' : cur && isCurrent(cur) ? (st.paused ? 'resume' : 'pause') : st.tab === 'queue' ? 'jump' : 'play';
  const k = [['⏎/space', enter], ['/', 'search'], ['?', 'keys'], ['tab', 'sections']];
  if (now) k.push(['y', 'lyrics']);
  if (st.tab === 'home') k.push(['D', 'discover'], ['←→', 'songs/artists']);
  if (st.tab === 'stats') k.push(['←→', 'songs/artists/albums'], ['t', 'week/month/year']);
  if (st.tab === 'search') k.push(['esc', 'back']);
  if (st.tab === 'queue') k.push(['d', 'remove'], ['J K', 'move']);
  else if (kind === 'tracks') k.push(['l', 'like'], ['x', 'dislike'], ['a', 'queue'], ['R', 'radio']);
  k.push(['+ -', 'volume'], ['⇧←→', 'seek']);
  if (kind === 'tracks') k.push(['m', 'menu']);
  k.push(['v', 'player'], ['q', 'quit']);
  return k;
}

function render() {
  const W = out.columns, H = out.rows;
  const compact = H < 22; // too short for the framed dock: two plain lines instead
  const R = H >= 40 && W >= 80 ? 8 : H >= 30 && W >= 70 ? 6 : 4; // bigger cover and wave when there is room
  const L = Math.max(1, H - 7 - (compact ? (st.wave ? 1 : 0) : R)); // rows inside the body frame
  st.mq.on = false; // set again by the focused row if its text overflows
  const { kind, data } = list();
  const now = st.queue[st.pos];
  const focus = now ?? data[st.sel]; // the cover is about what plays, else what is highlighted

  // load the cover once the highlight settles (no fetch storm while scrolling)
  let got;
  if (focus?.cover) {
    if (focus.cover !== st.want) { st.want = focus.cover; st.wantAt = Date.now(); setTimeout(draw, 220); }
    if (now || Date.now() - st.wantAt > 200) got = cover.get(focus.cover, draw);
  }
  const A = accent(got);

  const tb = tabBar(W, [
    { k: 'home', full: 'Home', short: 'H' },
    { k: 'liked', full: `${LIKE} Liked${likes.size ? ' ' + likes.size : ''}`, short: LIKE },
    { k: 'queue', full: `Queue${st.queue.length ? ' ' + st.queue.length : ''}`, short: 'Q' },
    { k: 'stats', full: 'Stats', short: 'S' },
  ], st.tab === 'search' ? st.prevTab : st.tab, A);
  st.geo.tabs = tb.spans;
  const sb = searchBar(W, { typing: st.typing, text: st.typing ? st.buf : st.query, accent: A, placeholder: 'artists, albums, songs…' });
  st.geo.field = sb.field;

  // pane title: typing / busy / error / note take over the title of the focused pane
  const override = st.busy || st.msg || st.note;
  const ctx = st.busy ? [`${spin()} ${st.busy}`, A] : st.msg ? [st.msg, RED] : [st.note, TXT];
  const titleFor = def => (override ? ctx : [def, A]);
  const topOf = (s, n) => Math.min(Math.max(0, s - (L >> 1)), Math.max(0, n - L));

  let body, geo = null;
  const panes = [];
  if (st.help) {
    const iw = W - 2;
    const rows = HELP.map(([h, t]) => { const p = cut(h.padEnd(8) + t, iw - 2); return ' ' + fg(A, bold(p.slice(0, 8))) + p.slice(8) + ' '.repeat(iw - 1 - p.length); });
    while (rows.length < L) rows.push(' '.repeat(iw));
    body = frame('Keys · press any key to close', A, rows.slice(0, L), W, A);
  } else if (st.menu) {
    const iw = W - 2, m = st.menu;
    const rows = m.items.map(([label], i) => ' ' + (i === m.sel ? sel(A, cell(` ${i + 1}  ${label}`, id, iw - 3)) : cell(`  ${i + 1}  ${label}`, id, iw - 2)) + ' ');
    while (rows.length < L) rows.push(' '.repeat(iw));
    body = frame(`Actions · ${m.title}`, A, rows.slice(0, L), W, A);
  } else if (st.lyrics) {
    const [t, c] = titleFor(`Lyrics${now ? ` · ${now.title} — ${now.artist}` : ''}${lyricsNow()?.plain ? ' · not synced' : ''}`);
    body = frame(t, c, lyricRows(W - 2, L, A), W, A);
    panes.push({ id: 'cover', x0: 1, x1: W });
  } else if (st.full && st.trip) {
    const [t, c] = titleFor(`Trip${now ? ` · ${now.title} — ${now.artist}` : ''}${st.wave ? '' : ' · w to react to the music'}`);
    body = frame(t, c, trip.frame(W - 2, L, st.tripT, st.bands, got?.palette ?? [A], now?.id, st.energy), W, A);
    panes.push({ id: 'cover', x0: 1, x1: W });
  } else if (st.full) {
    const fp = fullPane(W - 2, L, focus, got);
    geo = fp.geo;
    const [t, c] = titleFor('Now playing');
    body = frame(t, c, fp.inner, W, A);
    panes.push({ id: 'cover', x0: 1, x1: W });
  } else if (st.tab === 'home') {
    const Ws = Math.ceil(W * 0.58), Wa = W - Ws;
    const sFocus = st.homeFocus === 'songs';
    const selS = sFocus ? st.sel : st.homeSel.songs, selA = sFocus ? st.homeSel.artists : st.sel;
    const topS = topOf(selS, st.homeSongs.length), topA = topOf(selA, st.homeArtists.length);
    st.geo.tops = { songs: topS, artists: topA };
    const [tS, cS] = sFocus ? titleFor('Top songs') : ['Top songs', TXT];
    const [tA, cA] = sFocus ? ['Your artists', TXT] : titleFor('Your artists');
    const fS = frame(tS, cS, listRows('tracks', st.homeSongs, selS, topS, Ws - 2, L, sFocus, A, now, EMPTY.homeSongs), Ws, sFocus ? A : GRAY);
    const fA = frame(tA, cA, listRows('artistStats', st.homeArtists, selA, topA, Wa - 2, L, !sFocus, A, now, EMPTY.homeArtists), Wa, sFocus ? GRAY : A);
    body = fS.map((l, i) => l + fA[i]);
    panes.push({ id: 'songs', x0: 1, x1: Ws }, { id: 'artists', x0: Ws + 1, x1: W });
  } else if (st.tab === 'stats') {
    const iw = W - 2, n = Math.max(1, L - 2), top = Math.min(Math.max(0, st.sel - (n >> 1)), Math.max(0, data.length - n));
    st.geo.tops = { list: top - 2 }; // two summary rows sit above the list
    const total = cell(` ${iw < 70 ? 'Listened' : 'Minutes listened'} · ${PERIODS.map(([p], i) => `${p} ${minutes(st.stats.totals[i])}`).join(' · ')}`, id, iw);
    const pick = (names, on) => names.map((name, i) => { const t = name[0].toUpperCase() + name.slice(1); return i === on ? `[${t}]` : ` ${t} `; }).join(' ');
    // the keys sit next to what they change
    const left = ` ${pick(CATS, st.statCat)}  ←→`, right = `t  ${pick(PERIODS.map(p => p[0]), st.period)} `;
    const selector = cell(left + ' '.repeat(Math.max(2, iw - left.length - right.length)) + right, id, iw).replace(/\[\w+\]/g, m => fg(A, bold(m)));
    const [t, c] = titleFor(`Stats · top 100 ${CATS[st.statCat]} · last ${PERIODS[st.period][1]} days`);
    body = frame(t, c, [bold(total), selector, ...listRows(kind, data, st.sel, top, iw, n, true, A, now, EMPTY.stats)].slice(0, L), W, A);
    panes.push({ id: 'list', x0: 1, x1: W, head: 2 });
  } else {
    const top = topOf(st.sel, data.length);
    st.geo.tops = { list: top };
    const [t, c] = titleFor({ search: st.title || 'Results', liked: 'Liked', queue: 'Queue' }[st.tab]);
    body = frame(t, c, listRows(kind, data, st.sel, top, W - 2, L, true, A, now, EMPTY[st.tab]), W, A);
    panes.push({ id: 'list', x0: 1, x1: W });
  }
  st.geo.panes = panes; st.geo.L = L;

  const lines = [tb.line, sb.line, ...body];
  if (compact) { // two plain lines (three with the wave), no cover
    const status = st.loading ? spin() : st.paused ? '⏸' : '▶';
    const count = now ? ` ${st.pos + 1}/${st.queue.length}` : '';
    if (now) {
      const t = cut(`${status} ${tag(now.id)}${now.title} — ${now.artist}`, W - 1 - count.length);
      lines.push(fg(A, t[0]) + paint(now.id, t.slice(1)) + dim(count) + ' '.repeat(Math.max(0, W - t.length - count.length)));
    } else lines.push(st.session ? fg(A, cell(`⏯ Resume: ${st.session.queue[st.session.pos].title} (Home)`, id, W)) : dim('Nothing playing') + ' '.repeat(Math.max(0, W - 15)));
    if (st.wave) lines.push(now ? spectrum(st.bands, W, 1, A)[0] : ' '.repeat(W));
    const bl = `${fmt(st.time)} `;
    const right = ` ${st.dur ? fmt(st.dur) : '--:--'}  vol ${st.vol}`;
    const bw = Math.max(0, W - bl.length - right.length - 1);
    const filled = st.dur ? Math.round((bw - 1) * Math.min(1, st.time / st.dur)) : 0;
    Object.assign(st.geo, { barRow: H - 1, barX0: bl.length, barW: bw, nowRow: H - (st.wave ? 3 : 2), thumb: null });
    lines.push(now && bw ? bl + fg(A, '━'.repeat(filled) + '●') + dim('─'.repeat(bw - 1 - filled)) + right + ' ' : ' '.repeat(W));
  } else {
    const d = dock(W, L, R, focus, got, A, now);
    lines.push(...d.lines);
    geo = geo ?? d.geo;
  }
  lines.push(hints(hintList(kind, now), W, A));

  // real cover image: only re-sent when it changes; the old one stays until the new one has loaded
  let img = '';
  if (cover.supported) {
    const key = geo && focus?.cover ? [focus.cover, geo.cols, geo.row, geo.col].join('|') : '';
    if ((!key || got) && key !== st.shown) {
      img = (st.shown ? cover.clear : '') + (key ? cover.place(got.b64, geo.row, geo.col, geo.cols) : '');
      st.shown = key;
    }
  }
  // every line is exactly W wide, so no erase-to-end-of-line is needed (it would clip the last column)
  out.write('\x1b[H' + lines.join('\n') + img);
}

// ---------- input ----------
function quit(err) {
  try { saveSession(); logEnd(); cmd('quit'); mpv.kill(); } catch {}
  fs.rmSync(SOCK, { force: true });
  out.write(cover.clear + '\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[?7h\x1b[?25h\x1b[?1049l');
  if (err) console.error(err);
  process.exit(err ? 1 : 0);
}

let lastSeek = 0;
function seekAt(x) {
  const g = st.geo;
  if (!st.dur || !g.barW) return;
  const pct = Math.max(0, Math.min(100, (x - g.barX0 - 0.5) / g.barW * 100));
  cmd('seek', Math.round(pct), 'absolute-percent');
}

const paneAt = x => st.geo.panes?.find(p => x >= p.x0 && x <= p.x1);

// The list row under the pointer, if any.
function rowAt(x, y) {
  const g = st.geo, pane = paneAt(x);
  if (!pane || pane.id === 'cover' || y < TOP + (pane.head ?? 0) || y >= TOP + g.L) return null;
  return { pane: pane.id, i: g.tops[pane.id] + y - TOP };
}

function bodyClick(x, y) {
  const r = rowAt(x, y);
  if (!r) return;
  if (st.tab === 'home') focusHome(r.pane);
  if (!list().data[r.i] || list().data[r.i].type === 'header') return;
  const dbl = st.lastClick && st.lastClick.i === r.i && st.lastClick.pane === r.pane && Date.now() - st.lastClick.t < 450;
  st.lastClick = { i: r.i, pane: r.pane, t: Date.now() };
  st.sel = r.i;
  if (dbl) activate();
}

// Trackpads and wheels report in bursts: take at most one step per WHEEL_MS, so one notch is one row.
const WHEEL_MS = 40;
let lastWheel = 0;
// SGR mouse: b = button + 4 shift + 8 alt + 16 ctrl + 32 motion + 64 wheel; press=false is a release.
function onMouse(b, x, y, press) {
  const g = st.geo, motion = !!(b & 32), wheel = !!(b & 64), btn = b & 3;
  if (!press) { st.drag = false; return; }
  if (st.help) { if (!motion && !wheel) { st.help = false; draw(); } return; }
  if (wheel) {
    if (btn > 1 || Date.now() - lastWheel < WHEEL_MS) return; // 2 and 3 are sideways swipes
    lastWheel = Date.now();
  }
  if (st.menu) {
    if (wheel) st.menu.sel = Math.max(0, Math.min(st.menu.items.length - 1, st.menu.sel + (btn === 0 ? -1 : 1)));
    else if (!motion) { const i = y - TOP; if (btn === 0 && st.menu.items[i]) runMenu(i); else st.menu = null; }
    draw();
    return;
  }
  if (btn === 0 && !motion && !wheel) { // tabs and the search field never get trapped by the search box
    const t = y === 1 && g.tabs?.find(s => x >= s.x0 && x <= s.x1);
    if (t) { st.typing = false; setTab(t.k); draw(); return; }
    if (y === 2 && g.field && x >= g.field.x0 && x <= g.field.x1) { startTyping(); draw(); return; }
  }
  if (st.typing) return;
  if (st.drag && motion) { // dragging along the progress bar scrubs
    if (Date.now() - lastSeek > 60) { lastSeek = Date.now(); seekAt(x); }
    return;
  }
  if (wheel && st.lyrics) st.lyricScroll += btn === 0 ? -1 : 1;
  else if (wheel) {
    const pane = paneAt(x);
    if (st.tab === 'home' && pane && pane.id !== 'cover') focusHome(pane.id);
    moveSel(st.sel + (btn === 0 ? -1 : 1));
  } else if (btn === 2 && !motion) {
    const r = !st.full && !st.lyrics && rowAt(x, y);
    if (r && list().data[r.i] && list().data[r.i].type !== 'header') { if (st.tab === 'home') focusHome(r.pane); st.sel = r.i; openMenu(); } else back();
  } else if (btn === 0 && !motion) {
    const th = g.thumb;
    if (y === g.barRow) { if (st.dur && x > g.barX0 && x <= g.barX0 + g.barW) { st.drag = true; seekAt(x); } }
    else if (th && x >= th.x0 && x <= th.x1 && y >= th.y0 && y <= th.y1) { st.full = !st.full; st.trip = false; }
    else if (y === g.nowRow) { if (st.queue[st.pos]) cmd('cycle', 'pause'); else if (st.session) resume(); }
    else if (st.full) st.full = false;
    else if (y >= TOP && y < TOP + g.L) bodyClick(x, y);
  }
  draw();
}

function onKey(s, k = {}) {
  if (k.ctrl && k.name === 'c') return quit();
  if (st.help) { st.help = false; return draw(); }
  if (st.menu) {
    const m = st.menu, n = m.items.length;
    if (k.name === 'up' || k.name === 'k') m.sel = Math.max(0, m.sel - 1);
    else if (k.name === 'down' || k.name === 'j') m.sel = Math.min(n - 1, m.sel + 1);
    else if (k.name === 'return' || k.name === 'space') runMenu(m.sel);
    else if (k.name === 'escape' || k.name === 'q' || k.name === 'm') st.menu = null;
    else if (/^[1-9]$/.test(s ?? '') && m.items[+s - 1]) runMenu(+s - 1);
    return draw();
  }
  if (st.typing) {
    if (k.name === 'return') { st.typing = false; if (st.buf.trim()) runSearch(st.buf.trim()); else reopenResults(); }
    else if (k.name === 'escape') st.typing = false;
    else if (k.name === 'tab') { st.typing = false; cycleTab(k.shift ? -1 : 1); }
    else if (k.name === 'up' || k.name === 'down') {
      if (st.hIdx === -1) { st.hList = db.recentSearches(); st.draft = st.buf; }
      st.hIdx = Math.max(-1, Math.min(st.hList.length - 1, st.hIdx + (k.name === 'up' ? 1 : -1)));
      st.buf = st.hIdx === -1 ? st.draft : st.hList[st.hIdx];
    }
    else if (k.name === 'backspace') st.buf = st.buf.slice(0, -1);
    else if (s && s >= ' ' && !k.ctrl && !k.meta) st.buf += s;
    return draw();
  }
  const { kind, data } = list();
  const n = data.length, cur = data[st.sel];
  const inQueue = st.tab === 'queue';
  if (st.lyrics || st.full) { // the list is hidden: movement scrolls the lyrics and list actions do nothing
    const step = { up: -1, k: -1, down: 1, j: 1, pageup: -10, pagedown: 10, home: -1e6, g: -1e6, end: 1e6 }[k.name];
    if (step) st.lyricScroll = Math.max(0, st.lyricScroll + step);
    if (step || ['a', 'd', 'm'].includes(k.name)) return draw();
  }
  // capitals first: readline reports J as name "j" with shift
  if (s === 'D') { openDiscover(); return draw(); }
  if (s === 'V') { st.trip = !(st.full && st.trip); st.full = st.trip; st.lyrics = false; st.tripAt = performance.now(); return draw(); }
  if (s === 'R') { // radio from the highlighted row, or from what plays when the list is hidden
    const hidden = st.full || st.lyrics;
    startRadio(hidden ? null : list().data[st.sel], hidden ? null : typeAt());
    return draw();
  }
  if (st.tab === 'stats' && !st.lyrics && (s === 't' || (!k.shift && (k.name === 'left' || k.name === 'right')))) {
    if (s === 't') st.period = (st.period + 1) % PERIODS.length;
    else st.statCat = (st.statCat + (k.name === 'right' ? 1 : CATS.length - 1)) % CATS.length;
    st.sel = 0; loadStats();
    return draw();
  }
  if ((s === 'J' || s === 'K') && inQueue) {
    if (cur) moveInQueue(cur, s === 'J' ? 1 : -1);
    return draw();
  }
  switch (k.name) {
    case 'up': case 'k': moveSel(st.sel - 1); break;
    case 'down': case 'j': moveSel(st.sel + 1); break;
    case 'pageup': moveSel(st.sel - 10); break;
    case 'pagedown': moveSel(st.sel + 10); break;
    case 'home': case 'g': moveSel(0); break;
    case 'end': moveSel(n - 1); break;
    case 'return': case 'space': press(); break;
    case 'tab': cycleTab(k.shift ? -1 : 1); break;
    case 'escape': case 'backspace': case 'h': back(); break;
    case 'n': cmd('playlist-next'); break;
    case 'p': cmd('playlist-prev'); break;
    case 'left': if (k.shift) cmd('seek', -10); else if (st.tab === 'home') focusHome('songs'); break; // plain arrows never move the song
    case 'right': if (k.shift) cmd('seek', 10); else if (st.tab === 'home') focusHome('artists'); break;
    case 'l': rate('like'); break;
    case 'x': rate('dislike'); break;
    case 'a': if (typeAt() === 'tracks' && cur) { queueTrack(cur); note(`Queued: ${cur.title}`); } else note('Select a track to queue'); break;
    case 'd': if (inQueue && cur) removeFromQueue(cur); break;
    case 'm': openMenu(); break;
    case 'v': st.full = !st.full || st.trip; st.trip = false; st.lyrics = false; break;
    case 'y': toggleLyrics(); break;
    case 'w': toggleWave(); break;
    case 'q': return quit();
    default:
      if (s === '/') startTyping(); // the only key that searches
      else if (s === '?') st.help = true;
      else if (s === '+' || s === '=') cmd('add', 'volume', 5);
      else if (s === '-') cmd('add', 'volume', -5);
  }
  draw();
}

// ---------- main ----------
await connect();
loadHome();
// alt screen, hide cursor, autowrap off (a wide glyph then clips instead of wrapping), mouse: press/drag + SGR coordinates
out.write('\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J\x1b[?1000h\x1b[?1002h\x1b[?1006h');
readline.emitKeypressEvents(process.stdin);
process.stdin.setRawMode(true);
process.stdin.resume();
// shortcut: mouse reports are stripped from the raw stream before readline sees them (it would turn them into typed characters)
const MOUSE = /\x1b\[<(\d+);(\d+);(\d+)([Mm])/g;
const emit = process.stdin.emit.bind(process.stdin);
process.stdin.emit = (ev, d, ...rest) => {
  if (ev === 'data') {
    const str = d.toString();
    const clean = str.replace(MOUSE, (_, b, x, y, t) => { onMouse(+b, +x, +y, t === 'M'); return ''; });
    if (!clean) return true;
    if (clean !== str) d = Buffer.from(clean);
  }
  return emit(ev, d, ...rest);
};
process.stdin.on('keypress', onKey);
out.on('resize', () => { out.write('\x1b[2J'); st.shown = ''; draw(); });
['SIGTERM', 'SIGHUP', 'SIGINT'].forEach(sig => process.on(sig, () => quit()));
process.on('uncaughtException', e => quit(e.stack));
process.on('exit', () => mpv.kill());
render();
