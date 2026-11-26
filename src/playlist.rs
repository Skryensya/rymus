// Importing a playlist from a pasted link, without any account. YouTube Music playlists are read as they are;
// Spotify's public embed page gives names only, so each song is looked up on YouTube Music.
use crate::item::{clean, Item};
use crate::yt::{agent, Yt};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, PartialEq)]
pub enum Link {
    Youtube(String),
    Spotify(String),
}

impl Link {
    // The same playlist pasted in any form gives the same address, which is how a second paste is recognized.
    pub fn url(&self) -> String {
        match self {
            Link::Youtube(id) => format!("https://music.youtube.com/playlist?list={id}"),
            Link::Spotify(id) => format!("https://open.spotify.com/playlist/{id}"),
        }
    }
}

fn id_at(s: &str) -> Option<String> {
    let id: String = s.chars().take_while(|c| c.is_ascii_alphanumeric() || "-_".contains(*c)).collect();
    (!id.is_empty()).then_some(id)
}

pub fn link(text: &str) -> Option<Link> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("spotify:playlist:") {
        return id_at(rest).map(Link::Spotify);
    }
    let address = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")).unwrap_or(text);
    let (host, path) = address.split_once('/')?;
    match host {
        "open.spotify.com" => id_at(path.split_once("playlist/")?.1).map(Link::Spotify),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" => {
            let query = path.split_once('?')?.1;
            id_at(query.split('&').find_map(|p| p.strip_prefix("list="))?).map(Link::Youtube)
        }
        _ => None,
    }
}

pub struct Imported {
    pub name: String,
    pub tracks: Vec<Item>,
    pub missing: usize,
    pub partial: bool, // the source had (or may have had) more than we could read
}

struct Wanted {
    title: String,
    artists: Vec<String>,
}

// Spotify's embed page carries the playlist as JSON: its name and up to 100 songs.
const SPOTIFY_MOST: usize = 100;
fn spotify_page(html: &str) -> Option<(String, Vec<Wanted>)> {
    let json = html.split_once("id=\"__NEXT_DATA__\"")?.1.split_once('>')?.1.split_once("</script>")?.0;
    let data: Value = serde_json::from_str(json).ok()?;
    let entity = &data["props"]["pageProps"]["state"]["data"]["entity"];
    let text = |v: &Value| clean(v.as_str().unwrap_or(""));
    let songs = entity["trackList"]
        .as_array()?
        .iter()
        .map(|t| Wanted {
            title: text(&t["title"]),
            artists: text(&t["subtitle"]).split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
        })
        .filter(|w| !w.title.is_empty())
        .collect();
    Some((text(&entity["name"]), songs))
}

// "Song (feat. X) - Remastered 2011" -> "song"
pub(crate) fn plain(s: &str) -> String {
    let mut depth = 0;
    let base = s.split(" - ").next().unwrap_or(s);
    let kept: String = base
        .chars()
        .filter(|&c| {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                _ => return depth <= 0 && c.is_alphanumeric(),
            }
            false
        })
        .flat_map(char::to_lowercase)
        .collect();
    if kept.is_empty() { s.to_lowercase() } else { kept }
}

// The search result that is the wanted song, if any: same artist and same title. Better nothing than another song.
fn best(wanted_title: &str, artists: &[String], found: Vec<Item>) -> Option<Item> {
    let title = plain(wanted_title);
    let artists: Vec<String> = artists.iter().map(|a| plain(a)).collect();
    let score = |t: &Item| {
        let by = plain_each(&t.artist);
        if !by.iter().any(|b| artists.iter().any(|a| a.contains(b.as_str()) || b.contains(a.as_str()))) {
            return None;
        }
        let other = plain(&t.title);
        if t.title.eq_ignore_ascii_case(wanted_title) {
            Some(2)
        } else if other == title {
            Some(1)
        } else {
            (other.contains(&title) || title.contains(&other)).then_some(0)
        }
    };
    let mut pick: Option<(i32, Item)> = None;
    for t in found {
        if let Some(s) = score(&t) {
            if pick.as_ref().is_none_or(|p| s > p.0) {
                pick = Some((s, t));
            }
        }
    }
    pick.map(|p| p.1)
}
fn plain_each(credit: &str) -> Vec<String> {
    credit.split(", ").filter(|a| !a.is_empty()).map(plain).collect()
}

// Looks every song up, several at a time, keeping the playlist's order. `progress` gets how many are done.
fn find_all(yt: &Yt, wanted: &[Wanted], progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<Item>, String> {
    let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let found: Mutex<Vec<Option<Item>>> = Mutex::new(vec![None; wanted.len()]);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(w) = wanted.get(i) else { break };
                let query = format!("{} {}", w.title, w.artists.first().map_or("", String::as_str));
                match yt.songs(&query).or_else(|_| yt.songs(&query)) {
                    Ok(rows) => found.lock().unwrap_or_else(|e| e.into_inner())[i] = best(&w.title, &w.artists, rows),
                    Err(e) => *failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(e),
                }
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, wanted.len());
            });
        }
    });
    // a lost connection must not leave a half imported playlist that looks complete
    if let Some(e) = failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e);
    }
    Ok(found.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().flatten().collect())
}

pub fn fetch(yt: &Yt, link: &Link, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Imported, String> {
    let imported = match link {
        Link::Youtube(id) => {
            let (name, tracks, partial) = yt.playlist(id)?;
            Imported { name, tracks, missing: 0, partial }
        }
        Link::Spotify(id) => {
            let html = agent()
                .get(&format!("https://open.spotify.com/embed/playlist/{id}"))
                .set("User-Agent", "Mozilla/5.0")
                .call()
                .map_err(|e| e.to_string())?
                .into_string()
                .map_err(|e| e.to_string())?;
            let (name, wanted) = spotify_page(&html).ok_or("Spotify did not show this playlist (is it public?)")?;
            let tracks = find_all(yt, &wanted, progress)?;
            Imported { name, missing: wanted.len() - tracks.len(), partial: wanted.len() >= SPOTIFY_MOST, tracks }
        }
    };
    if imported.tracks.is_empty() {
        return Err("no songs found (is the playlist public?)".into());
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Kind;

    #[test]
    fn links() {
        let yt = Some(Link::Youtube("PL4fG-a_1".into()));
        assert_eq!(link("https://music.youtube.com/playlist?list=PL4fG-a_1&si=x"), yt);
        assert_eq!(link(" youtube.com/watch?v=abc&list=PL4fG-a_1 "), yt);
        let sp = Some(Link::Spotify("37i9dQ".into()));
        assert_eq!(link("https://open.spotify.com/playlist/37i9dQ?si=abc"), sp);
        assert_eq!(link("https://open.spotify.com/intl-es/playlist/37i9dQ"), sp);
        assert_eq!(link("spotify:playlist:37i9dQ"), sp);
        for other in ["daft punk", "AC/DC", "https://youtu.be/abc", "https://evil.com/playlist?list=PL1", "https://open.spotify.com/album/1"] {
            assert_eq!(link(other), None, "{other}");
        }
    }

    #[test]
    fn spotify_songs_are_matched_by_title_and_artist() {
        let html = r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":
            {"name":"Mix","trackList":[{"title":"One More Time","subtitle":"Daft Punk, Romanthony"}]}}}}}}</script>"#;
        let (name, wanted) = spotify_page(html).unwrap();
        assert_eq!((name.as_str(), wanted[0].artists.len()), ("Mix", 2));
        let song = |id: &str, title: &str, artist: &str| Item { kind: Kind::Track, id: id.into(), title: title.into(), artist: artist.into(), ..Default::default() };
        let pick = |title: &str, rows: Vec<Item>| best(title, &["Daft Punk".to_string()], rows).map(|t| t.id);
        let rows = vec![song("cover", "One More Time", "Someone"), song("live", "One More Time (Live)", "Daft Punk"), song("yes", "One More Time", "Daft Punk")];
        assert_eq!(pick("One More Time", rows.clone()), Some("yes".into()));
        assert_eq!(pick("One More Time - Radio Edit", rows), Some("live".into()));
        assert_eq!(pick("Aerodynamic", vec![song("no", "One More Time", "Daft Punk")]), None);
    }
}
