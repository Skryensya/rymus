// Gathers candidates (YouTube Music radios, and artists ListenBrainz finds similar) and hands them to the ranking.
import * as rec from './recommend.js';

const clean = s => s.replace(/[\x00-\x1f\x7f-\x9f]/g, ''); // names from outside end up on the terminal
const done = results => results.filter(r => r.status === 'fulfilled').map(r => r.value);
const model = db => rec.taste(db.signals());

async function radio(yt, track) {
  return (await yt.music.getUpNext(track.id, true)).contents.map(rec.toTrack).filter(Boolean);
}

async function songsBy(yt, name) {
  const found = (await yt.music.search(name, { type: 'song' })).songs?.contents ?? [];
  return found.filter(x => x.id).map(x => ({
    id: x.id, title: x.title ?? '', artist: x.artists?.map(a => a.name).join(', ') ?? '', album: x.album?.name ?? '',
    cover: x.thumbnail?.contents?.reduce((best, t) => (t.width > (best?.width ?? 0) ? t : best), null)?.url,
  })).filter(t => rec.mainArtist(t) === name.toLowerCase());
}

// Songs by artists similar to the given ones, preferring artists you do not know yet.
async function neighbors(yt, lb, names, taste, perName) {
  const picked = new Map(); // similar artist -> closeness 0..1
  const results = await Promise.allSettled(names.map(async name => {
    const id = await lb.artistId(name);
    if (!id) return;
    const list = await lb.similar(id);
    const top = list[0]?.score || 1;
    const unknown = list.slice(0, 25).filter(a => !taste.artists.has(a.name.toLowerCase()));
    for (const a of rec.sample(unknown, a => a.score / top).slice(0, perName)) {
      picked.set(a.name, Math.max(picked.get(a.name) ?? 0, a.score / top));
    }
  }));
  const lists = done(await Promise.allSettled([...picked].map(async ([name, closeness]) =>
    ({ via: 'like:' + name, weight: 0.5 + 0.5 * closeness, tracks: (await songsBy(yt, name)).slice(0, 4) }))));
  return { lists, failed: results.length > 0 && results.every(r => r.status === 'rejected') };
}

// spec: { kind: 'all' } | { kind: 'style', artists } | { kind: 'radio', track } | { kind: 'radio', artist }
export async function buildMix({ yt, lb, db }, spec) {
  const taste = model(db);
  const heard = db.heard();
  let seeds, names;
  if (spec.track) {
    seeds = [spec.track];
    names = [spec.track.artist.split(', ')[0]];
  } else if (spec.artist) {
    seeds = (await songsBy(yt, spec.artist)).slice(0, 3);
    names = [spec.artist];
  } else {
    const mine = spec.artists && new Set(spec.artists.map(a => a.toLowerCase()));
    seeds = rec.pickSeeds(mine ? taste.tracks.filter(t => mine.has(rec.mainArtist(t))) : taste.tracks);
    names = seeds.map(t => t.artist.split(', ')[0]);
  }
  if (!seeds.length) throw new Error('nothing to start from yet: play or like a few songs');
  const [radios, near] = await Promise.all([
    Promise.allSettled(seeds.map(async s => ({ via: 'radio:' + s.id, weight: 1, tracks: await radio(yt, s) }))),
    neighbors(yt, lb, names, taste, names.length > 1 ? 2 : 8),
  ]);
  const lists = [...done(radios), ...near.lists];
  if (!lists.some(l => l.tracks.length)) throw new Error(radios.find(r => r.reason)?.reason.message ?? 'no recommendations');
  // a radio from one song starts with that song
  const start = spec.track ? [spec.track] : [];
  const tracks = [...start, ...rec.rank(lists, { model: taste, heard: [...heard, ...start], size: 30 - start.length })];
  return { tracks, withoutListenBrainz: near.failed };
}

// Your tastes split by style, from your 12 favorite artists.
export async function findStyles({ lb, db }) {
  const top = [...model(db).artists.values()].filter(a => a.score > 0).sort((a, b) => b.score - a.score).slice(0, 12);
  const known = done(await Promise.allSettled(top.map(async a => {
    const id = await lb.artistId(a.name);
    return id ? { ...a, id, similar: (await lb.similar(id)).map(s => s.name.toLowerCase()) } : null;
  }))).filter(Boolean);
  if (top.length && !known.length) throw new Error('ListenBrainz did not recognize your artists or is unreachable');
  const tags = await lb.genres(known.map(a => a.id));
  return rec.styles(known.map((a, i) => ({ ...a, tags: tags[i] }))).map(s => ({ name: clean(s.name), artists: s.artists }));
}
