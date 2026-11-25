// Similar artists and genre tags from ListenBrainz; artist ids from MusicBrainz (same project). No account.
// Only artist names and their public ids are sent. Everything is cached in the local database.
const HEADERS = { 'User-Agent': 'ymus/1.0' };
const SIMILAR = 'https://labs.api.listenbrainz.org/similar-artists/json?algorithm=session_based_days_7500_session_300_contribution_5_threshold_10_limit_100_filter_True_skip_30&artist_mbids=';
const DAY = 86400000;
const sleep = ms => new Promise(r => setTimeout(r, ms));

async function getJson(url) {
  for (let attempt = 1; ; attempt++) {
    const res = await fetch(url, { headers: HEADERS, signal: AbortSignal.timeout(15000) });
    if (res.ok) return res.json();
    if (attempt >= 3 || (res.status !== 503 && res.status !== 429)) throw new Error(`${new URL(url).host} ${res.status}`);
    await sleep(1500 * attempt); // MusicBrainz is often "busy"
  }
}

// MusicBrainz allows one request per second.
let turn = Promise.resolve();
const musicbrainz = url => {
  const result = turn.then(() => getJson(url));
  turn = result.then(() => sleep(1100), () => sleep(1100));
  return result;
};

// cache: { get(key, maxAgeMs), set(key, value) }
export function createListenBrainz(cache) {
  async function artistId(name) {
    const key = 'mbid:' + name.toLowerCase();
    const hit = cache.get(key);
    if (hit !== undefined) return hit;
    const q = encodeURIComponent(`artist:"${name.replace(/["\\]/g, ' ')}"`);
    const found = (await musicbrainz(`https://musicbrainz.org/ws/2/artist?fmt=json&limit=3&query=${q}`)).artists ?? [];
    const id = found.find(a => a.score >= 90 && a.name.toLowerCase() === name.toLowerCase())?.id ?? null; // exact names only
    cache.set(key, id);
    return id;
  }

  async function similar(mbid) {
    const key = 'similar:' + mbid;
    const hit = cache.get(key, 30 * DAY);
    if (hit) return hit;
    const list = (await getJson(SIMILAR + mbid)).filter(a => a.name && a.artist_mbid !== mbid).slice(0, 30)
      .map(a => ({ name: a.name, score: a.score }));
    cache.set(key, list);
    return list;
  }

  // Genre tags per id, most used first.
  async function genres(mbids) {
    const missing = mbids.filter(id => !cache.get('tags:' + id, 90 * DAY));
    if (missing.length) {
      for (const a of await getJson('https://api.listenbrainz.org/1/metadata/artist/?inc=tag&artist_mbids=' + missing.join(','))) {
        const tags = (a.tag?.artist ?? []).filter(t => t.genre_mbid).sort((x, y) => y.count - x.count).slice(0, 5).map(t => String(t.tag));
        cache.set('tags:' + (a.artist_mbid ?? a.mbid), tags);
      }
    }
    return mbids.map(id => cache.get('tags:' + id, 90 * DAY) ?? []);
  }

  return { artistId, similar, genres };
}
