// Spectrum analyzer inside mpv's own audio filter. The audio is split into band-pass copies, merged next to the untouched
// stereo pair as extra channels, measured per channel by astats, and then everything but the original pair is dropped.
// mpv filters audio well before you hear it, so every measurement is printed with its timestamp (to fd 3, a pipe back to
// ymus) and drawn only when playback reaches that moment.
export const BANDS = 32;
const LOW = 50, HIGH = 14000; // Hz, bands are spaced logarithmically (by pitch)

const freqs = Array.from({ length: BANDS }, (_, i) => Math.round(LOW * (HIGH / LOW) ** (i / (BANDS - 1))));
export const GRAPH = [
  'aformat=sample_fmts=fltp:channel_layouts=stereo,asetnsamples=n=1024:p=0,asplit=2[orig][mix]', // one measurement every ~21 ms
  `[mix]aformat=channel_layouts=mono,asplit=${BANDS}${freqs.map((_, i) => `[s${i}]`).join('')}`,
  // two band-pass stages in a row: steeper slopes, so a loud bass does not light up its neighbors
  ...freqs.map((f, i) => `[s${i}]${`bandpass=f=${f}:width_type=o:w=0.35,`.repeat(2)}aformat=channel_layouts=mono[b${i}]`),
  `[orig]${freqs.map((_, i) => `[b${i}]`).join('')}amerge=inputs=${BANDS + 1},` +
    'astats=metadata=1:reset=1:measure_perchannel=RMS_level:measure_overall=none,' +
    "ametadata=mode=print:file='pipe\\:3':direct=1,pan=stereo|c0=c0|c1=c1",
].join(';');
export const FILTER = `@viz:lavfi=graph=%${GRAPH.length}%${GRAPH}`; // %n% = mpv's quoting for a value with [ ] ; in it

// Parses what ametadata prints ("pts_time:" line, then one line per channel) and hands it out in time with playback.
export function createFeed() {
  let rest = '', queue = [];
  return {
    push(chunk) {
      const lines = (rest + chunk).split('\n');
      rest = lines.pop();
      for (const line of lines) {
        let m;
        if ((m = line.match(/pts_time:(-?[\d.]+)/))) {
          const t = +m[1];
          // shortcut: a jump back (seek, next track) drops what was queued, so a gapless track change can blank the
          // last second of the previous one; keep one queue per file if that ever shows
          if (queue.length && t < queue.at(-1).t - 0.5) queue = [];
          queue.push({ t, db: Array(BANDS).fill(-120) });
          if (queue.length > 3000) queue.shift();
        } else if ((m = line.match(/astats\.(\d+)\.RMS_level=(-?[\d.]+)/))) {
          const band = m[1] - 3; // channels 1 and 2 are the original audio; "-inf" (silence) does not match
          if (band >= 0 && queue.length) queue.at(-1).db[band] = +m[2];
        }
      }
    },
    // The loudest value of each band among the measurements playback has just passed; null if there is nothing new.
    at(pos) {
      let n = 0;
      while (n < queue.length && queue[n].t <= pos) n++;
      if (!n) return null;
      return queue.splice(0, n).reduce((max, f) => max.map((v, i) => Math.max(v, f.db[i])), Array(BANDS).fill(-120));
    },
  };
}

// dB -> bar heights 0..1. Each band is scaled against its own recent peak (treble is far quieter than bass, so a
// shared scale would leave the right half flat). Bars jump up at once and fall gradually.
const RANGE = 20, QUIETEST_PEAK = -54;
const peaks = Array(BANDS).fill(QUIETEST_PEAK);
export function levels(db, prev) {
  return db.map((v, i) => {
    peaks[i] = Math.max(v, peaks[i] - 0.1, QUIETEST_PEAK);
    const now = Math.max(0, Math.min(1, (v - peaks[i] + RANGE) / RANGE)) ** 2.2;
    return Math.max(now, (prev[i] ?? 0) * 0.74);
  });
}
