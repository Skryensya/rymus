// Gathers candidates (YouTube Music radios, and artists ListenBrainz finds similar) and hands them to the ranking,
// plus the two outside services involved: similar artists and genre tags from ListenBrainz, artist ids from
// MusicBrainz (same project). No account; only artist names and their public ids are sent, and answers are cached.
use crate::i18n::tr;
use crate::db::{now_ms, Db};
use crate::item::{clean, Item, Spec};
use crate::recommend::{self as rec, Source};
use crate::yt::Yt;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const SIMILAR: &str = "https://labs.api.listenbrainz.org/similar-artists/json?algorithm=session_based_days_7500_session_300_contribution_5_threshold_10_limit_100_filter_True_skip_30&artist_mbids=";
const DAY: u64 = 86_400;

fn get_json(url: &str) -> Result<Value, String> {
    let agent = crate::yt::agent();
    let mut attempt = 1;
    loop {
        match agent.get(url).set("User-Agent", "rymus/2.0").call() {
            Ok(res) => return res.into_json().map_err(|e| e.to_string()),
            // MusicBrainz is often "busy"
            Err(ureq::Error::Status(503 | 429, _)) if attempt < 3 => thread::sleep(Duration::from_millis(1500 * attempt)),
            Err(e) => return Err(e.to_string()),
        }
        attempt += 1;
    }
}

// MusicBrainz allows one request per second.
fn musicbrainz(url: &str) -> Result<Value, String> {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        thread::sleep(Duration::from_millis(1100).saturating_sub(t.elapsed()));
    }
    let result = get_json(url);
    *last = Some(Instant::now());
    result
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn artist_id(db: &Db, name: &str) -> Result<Option<String>, String> {
    let key = format!("mbid:{}", name.to_lowercase());
    if let Some(v) = db.cache_get(&key, None) {
        return Ok(v.as_str().map(String::from));
    }
    let query = encode(&format!("artist:\"{}\"", name.replace(['"', '\\'], " ")));
    let found = musicbrainz(&format!("https://musicbrainz.org/ws/2/artist?fmt=json&limit=3&query={query}"))?;
    let id = found["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["score"].as_i64().unwrap_or(0) >= 90 && a["name"].as_str().is_some_and(|n| n.to_lowercase() == name.to_lowercase()))
        .and_then(|a| a["id"].as_str().map(String::from)); // exact names only
    db.cache_set(&key, &json!(id));
    Ok(id)
}

// (name, closeness score), closest first.
fn similar(db: &Db, mbid: &str) -> Result<Vec<(String, f64)>, String> {
    let key = format!("similar:{mbid}");
    let list = match db.cache_get(&key, Some(Duration::from_secs(30 * DAY))) {
        Some(v) => v,
        None => {
            let raw = get_json(&format!("{SIMILAR}{mbid}"))?;
            let list: Vec<Value> = raw
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| a["name"].is_string() && a["artist_mbid"].as_str() != Some(mbid))
                .take(30)
                .map(|a| json!({"name": a["name"], "score": a["score"]}))
                .collect();
            let list = Value::Array(list);
            db.cache_set(&key, &list);
            list
        }
    };
    Ok(list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((clean(a["name"].as_str()?), a["score"].as_f64().unwrap_or(0.0))))
        .collect())
}

// Genre tags per id, most used first.
fn genres(db: &Db, ids: &[String]) -> Result<Vec<Vec<String>>, String> {
    let age = Some(Duration::from_secs(90 * DAY));
    let missing: Vec<&str> = ids.iter().filter(|id| db.cache_get(&format!("tags:{id}"), age).is_none()).map(String::as_str).collect();
    if !missing.is_empty() {
        let answer = get_json(&format!("https://api.listenbrainz.org/1/metadata/artist/?inc=tag&artist_mbids={}", missing.join(",")))?;
        for a in answer.as_array().into_iter().flatten() {
            let mut tags: Vec<&Value> = a["tag"]["artist"].as_array().into_iter().flatten().filter(|t| t["genre_mbid"].is_string()).collect();
            tags.sort_by_key(|t| -t["count"].as_i64().unwrap_or(0));
            let tags: Vec<&Value> = tags.into_iter().take(5).map(|t| &t["tag"]).collect();
            if let Some(id) = a["artist_mbid"].as_str().or(a["mbid"].as_str()) {
                db.cache_set(&format!("tags:{id}"), &json!(tags));
            }
        }
    }
    Ok(ids
        .iter()
        .map(|id| {
            db.cache_get(&format!("tags:{id}"), age)
                .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(clean)).collect()))
                .unwrap_or_default()
        })
        .collect())
}

fn songs_by(yt: &Yt, name: &str) -> Result<Vec<Item>, String> {
    let lower = name.to_lowercase();
    Ok(yt.songs(name)?.into_iter().filter(|t| rec::main_artist(t) == lower).collect())
}

// Songs by artists similar to the given ones, preferring artists you do not know yet.
fn neighbors(yt: &Yt, db: &Db, names: &[String], model: &rec::Model, per_name: usize) -> (Vec<Source>, bool) {
    let mut picked: Vec<(String, f64)> = Vec::new(); // similar artist, closeness 0..1
    let mut failed = 0;
    for name in names {
        let list = artist_id(db, name).and_then(|id| match id {
            Some(id) => similar(db, &id),
            None => Ok(Vec::new()),
        });
        let Ok(list) = list else {
            failed += 1;
            continue;
        };
        let top = list.first().map_or(1.0, |a| a.1.max(1.0));
        let unknown: Vec<(String, f64)> = list.into_iter().take(25).filter(|a| model.artist(&a.0.to_lowercase()).is_none()).collect();
        for (n, score) in rec::sample(unknown, |a| a.1 / top, &mut rec::random).into_iter().take(per_name) {
            match picked.iter_mut().find(|p| p.0 == n) {
                Some(p) => p.1 = p.1.max(score / top),
                None => picked.push((n, score / top)),
            }
        }
    }
    let lists = thread::scope(|s| {
        let jobs: Vec<_> = picked.iter().map(|(name, closeness)| s.spawn(move || (name, closeness, songs_by(yt, name)))).collect();
        jobs.into_iter()
            .filter_map(|j| j.join().ok())
            .filter_map(|(name, closeness, songs)| {
                let tracks = songs.ok()?.into_iter().take(4).collect();
                Some(Source { via: format!("like:{name}"), weight: 0.5 + 0.5 * closeness, tracks })
            })
            .collect()
    });
    (lists, !names.is_empty() && failed == names.len())
}

pub struct Mix {
    pub tracks: Vec<Item>,
    pub without_listenbrainz: bool,
}

pub fn build_mix(yt: &Yt, db_path: &Path, spec: &Spec) -> Result<Mix, String> {
    let db = Db::open(db_path).map_err(|e| e.to_string())?;
    let model = rec::taste(&db.signals(), now_ms());
    let heard = db.heard();
    let (seeds, names): (Vec<Item>, Vec<String>) = match spec {
        Spec::RadioTrack(t) => (vec![(**t).clone()], vec![t.main_artist().to_string()]),
        Spec::RadioArtist(a) => (songs_by(yt, a)?.into_iter().take(3).collect(), vec![a.clone()]),
        _ => {
            let pool: Vec<rec::Taste> = match spec {
                Spec::Style(artists) => {
                    let mine: Vec<String> = artists.iter().map(|a| a.to_lowercase()).collect();
                    model.tracks.iter().filter(|t| mine.contains(&rec::main_artist(&t.track))).cloned().collect()
                }
                _ => model.tracks.clone(),
            };
            let seeds = rec::pick_seeds(&pool, 5, &mut rec::random);
            let names = seeds.iter().map(|t| t.main_artist().to_string()).collect();
            (seeds, names)
        }
    };
    if seeds.is_empty() {
        return Err(tr!("nothing to start from yet: play or like a few songs").into());
    }
    let mut error = String::new();
    let (mut lists, failed) = thread::scope(|s| {
        let radios: Vec<_> = seeds.iter().map(|t| s.spawn(move || (t.id.clone(), yt.radio(&t.id)))).collect();
        let (near, failed) = neighbors(yt, &db, &names, &model, if names.len() > 1 { 2 } else { 8 });
        let mut lists = Vec::new();
        for (id, result) in radios.into_iter().filter_map(|j| j.join().ok()) {
            match result {
                Ok(tracks) => lists.push(Source { via: format!("radio:{id}"), weight: 1.0, tracks }),
                Err(e) => error = e,
            }
        }
        lists.extend(near);
        (lists, failed)
    });
    if lists.iter().all(|l| l.tracks.is_empty()) {
        return Err(if error.is_empty() { tr!("no recommendations").into() } else { error });
    }
    lists.retain(|l| !l.tracks.is_empty());
    // a radio from one song starts with that song
    let mut tracks: Vec<Item> = match spec {
        Spec::RadioTrack(t) => vec![(**t).clone()],
        _ => vec![],
    };
    let mut known = heard;
    known.extend(tracks.iter().cloned());
    let size = 30 - tracks.len();
    tracks.extend(rec::rank(&lists, &model, &known, size, &mut rec::random));
    Ok(Mix { tracks, without_listenbrainz: failed })
}

// Your tastes split by style, from your 12 favorite artists.
pub fn find_styles(db_path: &Path) -> Result<Vec<rec::Style>, String> {
    let db = Db::open(db_path).map_err(|e| e.to_string())?;
    let model = rec::taste(&db.signals(), now_ms());
    let mut top: Vec<&rec::ArtistTaste> = model.artists.iter().filter(|a| a.score > 0.0).collect();
    top.sort_by(|a, b| b.score.total_cmp(&a.score));
    top.truncate(12);
    let mut known = Vec::new();
    for a in &top {
        let Ok(Some(id)) = artist_id(&db, &a.name) else { continue };
        let Ok(list) = similar(&db, &id) else { continue };
        known.push((id, a.name.clone(), a.score, list.into_iter().map(|s| s.0.to_lowercase()).collect::<Vec<_>>()));
    }
    if !top.is_empty() && known.is_empty() {
        return Err(tr!("ListenBrainz did not recognize your artists or is unreachable").into());
    }
    let tags = genres(&db, &known.iter().map(|k| k.0.clone()).collect::<Vec<_>>())?;
    let artists = known
        .into_iter()
        .zip(tags)
        .map(|((_, name, score, similar), tags)| rec::Artist { name, score, tags, similar })
        .collect();
    Ok(rec::styles(artists))
}
