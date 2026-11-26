// The recommendation engine: a taste model built from your own log, and the ranking of candidate songs.
// Pure functions only; fetching candidates is mixes.rs.
use crate::db::{Play, Signals};
use crate::item::Item;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

const DAY: f64 = 86_400_000.0;

// shortcut: xorshift seeded from the clock; fine for shuffling a mix, not for anything secret
pub fn random() -> f64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

pub fn main_artist(t: &Item) -> String {
    t.main_artist().to_lowercase()
}

// The same song under another id: "(Official Video)", "[Remastered]", "- Radio Edit", "ft. X" do not make it new.
pub fn song_key(t: &Item) -> String {
    let lower = t.title.to_lowercase();
    let mut s = String::new();
    let mut depth = 0usize;
    for c in lower.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    for cut in [" - ", " ft ", " ft. ", " feat ", " feat. "] {
        if let Some(i) = s.find(cut) {
            s.truncate(i);
        }
    }
    format!("{}|{}", s.trim(), main_artist(t))
}

// ---------- taste ----------
fn local(at_ms: i64) -> (i32, i32) {
    let tm = crate::platform::local_time(at_ms / 1000);
    (tm.tm_hour, tm.tm_wday)
}
fn part(h: i32) -> u8 {
    match h {
        0..=4 => 0,
        5..=11 => 1,
        12..=17 => 2,
        _ => 3,
    }
}
// How much a play from `at` says about what you want to hear `now`: recent ones count more (half every 30 days),
// and so do ones from the same time of day and the same kind of day (weekday or weekend).
pub fn relevance(at: i64, now: i64) -> f64 {
    let ((ha, da), (hb, db)) = (local(at), local(now));
    let weekend = |d: i32| d % 6 == 0;
    0.5f64.powf((now - at) as f64 / (30.0 * DAY))
        * if part(ha) == part(hb) { 1.3 } else { 1.0 }
        * if weekend(da) == weekend(db) { 1.2 } else { 1.0 }
}

// What one play says: skipped early is a "no", heard to (nearly) the end a "yes".
pub fn play_value(p: &Play) -> f64 {
    if p.skipped {
        return -1.0;
    }
    let whole = match p.duration {
        Some(d) if d > 0 => p.seconds as f64 >= (0.8 * d as f64).min(240.0),
        _ => p.seconds >= 120,
    };
    if whole {
        1.0
    } else if p.seconds >= 30 {
        0.5
    } else {
        0.0
    }
}

#[derive(Clone)]
pub struct Taste {
    pub track: Item,
    pub score: f64,
}
#[derive(Clone)]
pub struct ArtistTaste {
    pub name: String,
    pub score: f64,
}
pub struct Model {
    pub tracks: Vec<Taste>,
    pub artists: Vec<ArtistTaste>,
    by_name: HashMap<String, usize>,
}
impl Model {
    pub fn artist(&self, lower: &str) -> Option<f64> {
        self.by_name.get(lower).map(|&i| self.artists[i].score)
    }
}

pub fn taste(s: &Signals, now: i64) -> Model {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut tracks: Vec<Taste> = Vec::new();
    let mut add = |t: &Item, score: f64| {
        let i = *at.entry(t.id.clone()).or_insert_with(|| {
            tracks.push(Taste { track: t.clone(), score: 0.0 });
            tracks.len() - 1
        });
        tracks[i].score += score;
    };
    for p in &s.plays {
        add(&p.track, play_value(p) * relevance(p.at, now));
    }
    for (t, when) in &s.liked {
        add(t, 1.5 + 1.5 * 0.5f64.powf((now - when) as f64 / (180.0 * DAY))); // an old like fades to half
    }
    for t in &s.disliked {
        add(t, -4.0);
    }
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut artists: Vec<ArtistTaste> = Vec::new();
    for t in &tracks {
        let i = *by_name.entry(main_artist(&t.track)).or_insert_with(|| {
            artists.push(ArtistTaste { name: t.track.main_artist().to_string(), score: 0.0 });
            artists.len() - 1
        });
        artists[i].score += t.score;
    }
    tracks.sort_by(|a, b| b.score.total_cmp(&a.score));
    Model { tracks, artists, by_name }
}

// Random order in which heavier items tend to come first (weighted sampling without replacement).
pub fn sample<T>(items: Vec<T>, weight: impl Fn(&T) -> f64, rand: &mut dyn FnMut() -> f64) -> Vec<T> {
    let mut keyed: Vec<(f64, T)> = items.into_iter().map(|x| (rand().powf(1.0 / weight(&x).max(0.05)), x)).collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    keyed.into_iter().map(|x| x.1).collect()
}

// A few songs you currently enjoy to start from, one per artist.
pub fn pick_seeds(tracks: &[Taste], n: usize, rand: &mut dyn FnMut() -> f64) -> Vec<Item> {
    let mut seen = HashSet::new();
    sample(tracks.iter().filter(|t| t.score > 0.0).collect(), |t| t.score, rand)
        .into_iter()
        .filter(|t| seen.insert(main_artist(&t.track)))
        .take(n)
        .map(|t| t.track.clone())
        .collect()
}

// ---------- candidates ----------
pub struct Source {
    pub via: String,
    pub weight: f64,
    pub tracks: Vec<Item>,
}

// Order for listening: best first, but never the same artist twice in a row (when it can be avoided) and
// changing source as it goes, so it drifts instead of staying in one corner.
fn flow(mut left: Vec<(String, Item)>) -> Vec<Item> {
    let mut out: Vec<(String, Item)> = Vec::new();
    while !left.is_empty() {
        let n = out.len();
        let last = (n > 0).then(|| (out[n - 1].0.clone(), main_artist(&out[n - 1].1)));
        let before = (n > 1).then(|| main_artist(&out[n - 2].1));
        let other = |x: &(String, Item)| last.as_ref().is_none_or(|l| main_artist(&x.1) != l.1);
        let i = left
            .iter()
            .position(|x| other(x) && before.as_ref().is_none_or(|b| &main_artist(&x.1) != b) && last.as_ref().is_none_or(|l| x.0 != l.0))
            .filter(|&i| i <= 6) // do not reach far down the ranking just to change source
            .or_else(|| left.iter().position(other))
            .unwrap_or(0);
        out.push(left.remove(i));
    }
    out.into_iter().map(|x| x.1).collect()
}

// Only songs you have not heard. A song scores more when several sources lead to it. Your taste then moves it:
// artists you skip or dislike go down (or out), unknown artists get a small push. At most two songs per artist.
pub fn rank(lists: &[Source], model: &Model, heard: &[Item], size: usize, rand: &mut dyn FnMut() -> f64) -> Vec<Item> {
    let ids: HashSet<&str> = heard.iter().map(|t| t.id.as_str()).collect();
    let songs: HashSet<String> = heard.iter().map(song_key).collect();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut found: Vec<(Item, f64, String)> = Vec::new();
    for l in lists {
        for (i, t) in l.tracks.iter().enumerate() {
            let key = song_key(t);
            if ids.contains(t.id.as_str()) || songs.contains(&key) {
                continue;
            }
            let j = *at.entry(key).or_insert_with(|| {
                found.push((t.clone(), 0.0, l.via.clone()));
                found.len() - 1
            });
            found[j].1 += l.weight * (1.0 - 0.5 * i as f64 / l.tracks.len() as f64);
        }
    }
    let mut scored: Vec<(f64, String, Item)> = Vec::new();
    for (track, score, via) in found {
        let known = model.artist(&main_artist(&track));
        let mine = match known {
            Some(k) if k <= -3.0 => continue,
            None => 0.3,
            Some(k) if k < 0.0 => k,
            Some(k) => (k / 5.0).min(1.0) * 0.4,
        };
        scored.push((score + mine + rand() * 0.4, via, Item { fresh: true, ..track }));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut per: HashMap<String, usize> = HashMap::new();
    let mut picked = Vec::new();
    for (_, via, t) in scored {
        let n = per.entry(main_artist(&t)).or_insert(0);
        if *n >= 2 {
            continue;
        }
        *n += 1;
        picked.push((via, t));
        if picked.len() >= size {
            break;
        }
    }
    flow(picked)
}

// ---------- styles ----------
pub struct Artist {
    pub name: String,
    pub score: f64,
    pub tags: Vec<String>,
    pub similar: Vec<String>, // lowercase names
}
pub struct Style {
    pub name: String,
    pub artists: Vec<String>,
}

// Groups your artists by shared genre tags and by who ListenBrainz says they are similar to.
pub fn styles(mut artists: Vec<Artist>) -> Vec<Style> {
    let link = |a: &Artist, b: &Artist| {
        let shared = a.tags.iter().filter(|t| b.tags.contains(t)).count();
        let union: HashSet<&String> = a.tags.iter().chain(&b.tags).collect();
        let near = a.similar.contains(&b.name.to_lowercase()) || b.similar.contains(&a.name.to_lowercase());
        (if union.is_empty() { 0.0 } else { shared as f64 / union.len() as f64 }) + if near { 0.4 } else { 0.0 }
    };
    artists.sort_by(|x, y| y.score.total_cmp(&x.score));
    let mut groups: Vec<Vec<Artist>> = Vec::new();
    for a in artists {
        let mut best: Option<(f64, usize)> = None;
        for (i, g) in groups.iter().enumerate() {
            let l = link(&g[0], &a);
            if best.is_none_or(|b| l > b.0) {
                best = Some((l, i));
            }
        }
        match best {
            Some((l, i)) if l >= 0.25 => groups[i].push(a),
            _ => groups.push(vec![a]),
        }
    }
    let total = |g: &Vec<Artist>| g.iter().map(|a| a.score).sum::<f64>();
    groups.retain(|g| !g[0].tags.is_empty() || g.len() > 1);
    groups.sort_by(|x, y| total(y).total_cmp(&total(x)));
    groups
        .into_iter()
        .take(4)
        .map(|g| {
            let mut tags: Vec<(String, f64)> = Vec::new();
            for a in &g {
                for (i, t) in a.tags.iter().enumerate() {
                    let add = a.score / (i + 1) as f64;
                    match tags.iter_mut().find(|x| &x.0 == t) {
                        Some(x) => x.1 += add,
                        None => tags.push((t.clone(), add)),
                    }
                }
            }
            tags.sort_by(|x, y| y.1.total_cmp(&x.1));
            let name = tags.iter().take(2).map(|x| x.0.as_str()).collect::<Vec<_>>().join(" · ");
            Style { name: if name.is_empty() { g[0].name.clone() } else { name }, artists: g.into_iter().map(|a| a.name).collect() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, title: &str, artist: &str) -> Item {
        Item { id: id.into(), title: title.into(), artist: artist.into(), ..Default::default() }
    }
    fn play(t: Item, at: i64, seconds: i64, duration: Option<i64>, skipped: bool) -> Play {
        Play { track: t, at, seconds, duration, skipped }
    }

    #[test]
    fn taste_and_ranking() {
        let at = 1_781_100_000_000;
        let (day, hour) = (86_400_000, 3_600_000);
        assert!(relevance(at, at) > relevance(at - 30 * day, at));
        assert!(relevance(at - 7 * day, at) > relevance(at - 7 * day + 11 * hour, at));
        let values: Vec<f64> = [(10, None, true), (190, Some(200), false), (60, Some(200), false), (5, None, false)]
            .iter()
            .map(|&(s, d, k)| play_value(&play(r("x", "x", "x"), at, s, d, k)))
            .collect();
        assert_eq!(values, [-1.0, 1.0, 0.5, 0.0]);
        let signals = Signals {
            plays: vec![
                play(r("m1", "Mine", "A"), at, 200, Some(200), false),
                play(r("m2", "Meh", "S"), at, 5, None, true),
                play(r("m3", "Meh2", "S"), at, 5, None, true),
                play(r("m4", "Meh3", "S"), at, 5, None, true),
            ],
            liked: vec![(r("m1", "Mine", "A"), at)],
            disliked: vec![r("d", "Hate", "H")],
        };
        let model = taste(&signals, at);
        assert_eq!(model.tracks[0].track.id, "m1");
        assert!(model.artist("s").unwrap() < -3.0 && model.artist("h").unwrap() < -3.0);
        let mut half = || 0.5;
        assert_eq!(pick_seeds(&model.tracks, 5, &mut half).iter().map(|t| t.id.clone()).collect::<Vec<_>>(), ["m1"]);
        let heard = [r("m1", "Mine", "A"), r("d", "Hate", "H")];
        let lists = [
            Source {
                via: "a".into(),
                weight: 1.0,
                tracks: vec![r("x", "Mine (Official Video)", "A"), r("s1", "By skipped", "S"), r("n1", "New", "Z"), r("n2", "Two", "Z"), r("n3", "Three", "Z"), r("n5", "Five", "W")],
            },
            Source { via: "b".into(), weight: 1.0, tracks: vec![r("n1", "New", "Z"), r("n4", "Four", "Y")] },
        ];
        let got = rank(&lists, &model, &heard, 30, &mut half);
        assert_eq!(got[0].id, "n1");
        assert!(got.iter().all(|t| t.fresh && !["x", "s1", "m1", "d"].contains(&t.id.as_str())));
        assert_eq!(got.iter().filter(|t| t.artist == "Z").count(), 2);
        assert!(got.windows(2).all(|w| w[0].artist != w[1].artist));
    }

    #[test]
    fn groups_styles() {
        let art = |name: &str, score, tags: &[&str], similar: &[&str]| Artist {
            name: name.into(),
            score,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            similar: similar.iter().map(|s| s.to_string()).collect(),
        };
        let got = styles(vec![
            art("Daft", 5.0, &["house", "electronic"], &[]),
            art("Justice", 3.0, &["electronic", "house"], &[]),
            art("Phoebe", 4.0, &["indie", "folk"], &[]),
            art("Julien", 1.0, &[], &["phoebe"]),
            art("Nobody", 1.0, &[], &[]),
        ]);
        let got: Vec<(String, Vec<String>)> = got.into_iter().map(|s| (s.name, s.artists)).collect();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].0.as_str(), got[0].1.join(",")), ("house · electronic", "Daft,Justice".to_string()));
        assert_eq!((got[1].0.as_str(), got[1].1.join(",")), ("indie · folk", "Phoebe,Julien".to_string()));
    }
}
