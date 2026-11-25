// Spectrum analyzer inside mpv's own audio filter. The audio is split into band-pass copies, merged next to the untouched
// stereo pair as extra channels, measured per channel by astats, and then everything but the original pair is dropped.
// mpv filters audio well before you hear it, so every measurement is printed with its timestamp (to fd 3, a pipe back to
// ymus) and drawn only when playback reaches that moment.
use std::collections::VecDeque;

pub const BANDS: usize = 32;
const LOW: f64 = 50.0;
const HIGH: f64 = 14000.0; // Hz, bands are spaced logarithmically (by pitch)

pub fn graph() -> String {
    let freqs: Vec<i64> = (0..BANDS).map(|i| (LOW * (HIGH / LOW).powf(i as f64 / (BANDS - 1) as f64)).round() as i64).collect();
    let labels = |p: &str| (0..BANDS).map(|i| format!("[{p}{i}]")).collect::<String>();
    let mut parts = vec![
        // one measurement every ~21 ms
        "aformat=sample_fmts=fltp:channel_layouts=stereo,asetnsamples=n=1024:p=0,asplit=2[orig][mix]".to_string(),
        format!("[mix]aformat=channel_layouts=mono,asplit={BANDS}{}", labels("s")),
    ];
    // two band-pass stages in a row: steeper slopes, so a loud bass does not light up its neighbors
    for (i, f) in freqs.iter().enumerate() {
        let pass = format!("bandpass=f={f}:width_type=o:w=0.35,").repeat(2);
        parts.push(format!("[s{i}]{pass}aformat=channel_layouts=mono[b{i}]"));
    }
    parts.push(format!(
        "[orig]{}amerge=inputs={},astats=metadata=1:reset=1:measure_perchannel=RMS_level:measure_overall=none,\
         ametadata=mode=print:file='pipe\\:3':direct=1,pan=stereo|c0=c0|c1=c1",
        labels("b"),
        BANDS + 1
    ));
    parts.join(";")
}

pub fn filter() -> String {
    let g = graph();
    format!("@viz:lavfi=graph=%{}%{g}", g.len()) // %n% = mpv's quoting for a value with [ ] ; in it
}

pub type Db = [f64; BANDS];

// Parses what ametadata prints ("pts_time:" line, then one line per channel) and hands it out in time with playback.
#[derive(Default)]
pub struct Feed {
    rest: Vec<u8>,
    queue: VecDeque<(f64, Db)>,
}

impl Feed {
    pub fn push(&mut self, chunk: &[u8]) {
        self.rest.extend_from_slice(chunk);
        let Some(end) = self.rest.iter().rposition(|&b| b == b'\n') else { return };
        let whole: Vec<u8> = self.rest.drain(..=end).collect();
        for line in String::from_utf8_lossy(&whole).lines() {
            if let Some((_, t)) = line.split_once("pts_time:") {
                let Ok(t) = t.trim().parse::<f64>() else { continue };
                // shortcut: a jump back (seek, next track) drops what was queued, so a gapless track change can blank
                // the last second of the previous one; keep one queue per file if that ever shows
                if self.queue.back().is_some_and(|last| t < last.0 - 0.5) {
                    self.queue.clear();
                }
                self.queue.push_back((t, [-120.0; BANDS]));
                if self.queue.len() > 3000 {
                    self.queue.pop_front();
                }
            } else if let Some((channel, value)) = line.split_once("astats.").and_then(|(_, s)| s.split_once(".RMS_level=")) {
                // channels 1 and 2 are the original audio; "-inf" (silence) is left at the floor
                let (Ok(channel), Ok(value)) = (channel.parse::<usize>(), value.trim().parse::<f64>()) else { continue };
                if let Some(band) = channel.checked_sub(3).filter(|&b| b < BANDS) {
                    if let Some(last) = self.queue.back_mut() {
                        if value.is_finite() {
                            last.1[band] = value;
                        }
                    }
                }
            }
        }
    }

    // The loudest value of each band among the measurements playback has just passed; None if there is nothing new.
    pub fn at(&mut self, pos: f64) -> Option<Db> {
        let mut out: Option<Db> = None;
        while self.queue.front().is_some_and(|f| f.0 <= pos) {
            let (_, db) = self.queue.pop_front()?;
            let max = out.get_or_insert([-120.0; BANDS]);
            for i in 0..BANDS {
                max[i] = max[i].max(db[i]);
            }
        }
        out
    }
}

// dB -> bar heights 0..1. Each band is scaled against its own recent peak (treble is far quieter than bass, so a
// shared scale would leave the right half flat). Bars jump up at once and fall gradually.
const RANGE: f64 = 20.0;
const QUIETEST_PEAK: f64 = -54.0;

pub struct Levels {
    peaks: Db,
}
impl Default for Levels {
    fn default() -> Self {
        Levels { peaks: [QUIETEST_PEAK; BANDS] }
    }
}
impl Levels {
    pub fn next(&mut self, db: &Db, prev: &Db) -> Db {
        std::array::from_fn(|i| {
            self.peaks[i] = db[i].max(self.peaks[i] - 0.1).max(QUIETEST_PEAK);
            let now = ((db[i] - self.peaks[i] + RANGE) / RANGE).clamp(0.0, 1.0).powf(2.2);
            now.max(prev[i] * 0.74)
        })
    }
}

// How loud the music really is, 0..1: Levels scales every band to its own recent peak, so a soft song ends up looking
// as loud as a loud one. This sums the bands' raw power instead.
// shortcut: QUIET and LOUD were picked by reasoning (mastered pop sits near -12 dB), not measured; tune them by ear
const QUIET: f64 = -45.0; // dB
const LOUD: f64 = -12.0;
pub fn energy(db: &[f64]) -> f64 {
    let power: f64 = db.iter().map(|v| 10f64.powf(v / 10.0)).sum();
    let total = 10.0 * if power > 0.0 { power } else { 1e-12 }.log10();
    ((total - QUIET) / (LOUD - QUIET)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_and_levels() {
        let frame = |t: f64, db: i32| {
            format!("frame:0 pts:0 pts_time:{t}\nlavfi.astats.1.RMS_level=-3\nlavfi.astats.3.RMS_level={db}\nlavfi.astats.4.RMS_level=-inf\n")
        };
        let mut feed = Feed::default();
        let second = frame(1.02, -10);
        feed.push(format!("{}{}", frame(1.0, -30), &second[..20]).as_bytes()); // lines may arrive cut in half
        feed.push(format!("{}{}", &second[20..], frame(2.0, -5)).as_bytes());
        assert!(feed.at(0.5).is_none()); // measured ahead of time: not shown yet
        let heard = feed.at(1.5).unwrap();
        assert_eq!((heard[0], heard[1]), (-10.0, -120.0));
        assert!(feed.at(1.5).is_none());
        feed.push(frame(0.2, -20).as_bytes()); // seek back
        assert_eq!(feed.at(5.0).unwrap()[0], -20.0);
        assert_eq!((energy(&[-120.0; BANDS]), energy(&[-10.0; BANDS])), (0.0, 1.0));
        assert!(energy(&[-45.0; BANDS]) < 0.5);
        let mut levels = Levels::default();
        let loud = levels.next(&heard, &[0.0; BANDS]);
        assert_eq!((loud[0], loud[1]), (1.0, 0.0));
        let after = levels.next(&[-120.0; BANDS], &loud);
        assert!(after[0] > 0.0 && after[0] < 1.0);
    }
}
