// YouTube Music without an account: the few InnerTube calls ymus needs, parsed from their JSON.
use crate::item::{clean, Item, Kind};
use serde_json::{json, Value};
use std::time::Duration;

const API: &str = "https://music.youtube.com/youtubei/v1/";
const SONGS: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
const ALBUMS: &str = "EgWKAQIYAWoKEAkQBRAKEAMQBA==";
const ARTISTS: &str = "EgWKAQIgAWoKEAkQBRAKEAMQBA==";

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()
}

#[derive(Clone)]
pub struct Yt {
    agent: ureq::Agent,
}

// Every value stored under `key`, at any depth, in document order.
fn find<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                if k == key {
                    out.push(v);
                } else {
                    find(v, key, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|v| find(v, key, out)),
        _ => {}
    }
}
fn all<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    find(v, key, &mut out);
    out
}
fn first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    all(v, key).into_iter().next()
}
fn string(v: Option<&Value>) -> String {
    clean(v.and_then(Value::as_str).unwrap_or(""))
}

struct Run {
    text: String,
    page: String,
}
fn runs(v: &Value) -> Vec<Run> {
    v["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| Run { text: string(r.get("text")), page: string(first(r, "pageType")) })
        .collect()
}
fn text(v: &Value) -> String {
    runs(v).into_iter().map(|r| r.text).collect()
}
// "Artist & Other • Album • 3:45" -> groups split at the bullets.
fn groups(rs: Vec<Run>) -> Vec<Vec<Run>> {
    let mut out = vec![Vec::new()];
    for r in rs {
        if r.text.trim() == "•" {
            out.push(Vec::new());
        } else {
            out.last_mut().unwrap().push(r);
        }
    }
    out
}
fn names(rs: &[Run]) -> String {
    rs.iter()
        .map(|r| r.text.trim())
        .filter(|t| !matches!(*t, "" | "," | "&" | "and"))
        .collect::<Vec<_>>()
        .join(", ")
}
fn album_of(gs: &[Vec<Run>]) -> String {
    gs.iter().flatten().find(|r| r.page.ends_with("ALBUM")).map_or(String::new(), |r| r.text.clone())
}
fn best_thumb(v: &Value) -> String {
    all(v, "thumbnails")
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .max_by_key(|t| t["width"].as_i64().unwrap_or(0))
        .map_or(String::new(), |t| string(t.get("url")))
}
fn column(item: &Value, i: usize) -> &Value {
    &item["flexColumns"][i]["musicResponsiveListItemFlexColumnRenderer"]["text"]
}
fn video_id(item: &Value) -> String {
    let direct = string(item["playlistItemData"].get("videoId"));
    if direct.is_empty() {
        string(first(column(item, 0), "videoId"))
    } else {
        direct
    }
}
fn list_items(v: &Value) -> Vec<&Value> {
    all(v, "musicResponsiveListItemRenderer")
}

pub fn parse_songs(v: &Value) -> Vec<Item> {
    list_items(v)
        .into_iter()
        .map(|x| {
            let gs = groups(runs(column(x, 1)));
            Item {
                kind: Kind::Track,
                id: video_id(x),
                title: text(column(x, 0)),
                artist: gs.first().map_or(String::new(), |g| names(g)),
                album: album_of(&gs),
                cover: best_thumb(&x["thumbnail"]),
                ..Default::default()
            }
        })
        .filter(|t| !t.id.is_empty())
        .collect()
}

pub fn parse_albums(v: &Value) -> Vec<Item> {
    list_items(v)
        .into_iter()
        .map(|x| {
            let gs = groups(runs(column(x, 1))); // "Album • Artist • 2013"
            Item {
                kind: Kind::Album,
                id: string(x["navigationEndpoint"]["browseEndpoint"].get("browseId")),
                title: text(column(x, 0)),
                author: gs.get(1).map_or(String::new(), |g| names(g)),
                year: gs.get(2).map_or(String::new(), |g| names(g)),
                cover: best_thumb(&x["thumbnail"]),
                ..Default::default()
            }
        })
        .filter(|t| !t.id.is_empty())
        .collect()
}

pub fn parse_artists(v: &Value) -> Vec<Item> {
    list_items(v)
        .into_iter()
        .map(|x| Item {
            kind: Kind::Artist,
            id: string(x["navigationEndpoint"]["browseEndpoint"].get("browseId")),
            title: text(column(x, 0)),
            author: "Artist".into(),
            cover: best_thumb(&x["thumbnail"]),
            ..Default::default()
        })
        .filter(|t| !t.id.is_empty())
        .collect()
}

// Tracks of an album page; the album's own title, artist and cover fill what a row does not say.
pub fn parse_album(v: &Value, album: &Item) -> Vec<Item> {
    list_items(v)
        .into_iter()
        .map(|x| {
            let artist = names(&runs(column(x, 1)));
            Item {
                kind: Kind::Track,
                id: video_id(x),
                title: text(column(x, 0)),
                artist: if artist.is_empty() { album.author.clone() } else { artist },
                album: album.title.clone(),
                cover: album.cover.clone(),
                ..Default::default()
            }
        })
        .filter(|t| !t.id.is_empty())
        .collect()
}

// An artist page: (popular songs, albums).
pub fn parse_artist(v: &Value, name: &str) -> (Vec<Item>, Vec<Item>) {
    // the shelf's own title: keys come out sorted, so a deep search would find a nested one first
    let titled = |title: &Value| text(title).to_lowercase();
    let songs = all(v, "musicShelfRenderer")
        .into_iter()
        .find(|s| matches!(titled(&s["title"]).as_str(), "top songs" | "songs"))
        .map(|s| {
            list_items(s)
                .into_iter()
                .map(|x| {
                    let artist = names(&runs(column(x, 1)));
                    // columns: title, artists, plays, album
                    Item {
                        kind: Kind::Track,
                        id: video_id(x),
                        title: text(column(x, 0)),
                        artist: if artist.is_empty() { name.into() } else { artist },
                        album: text(column(x, 3)),
                        cover: best_thumb(&x["thumbnail"]),
                        ..Default::default()
                    }
                })
                .filter(|t| !t.id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let albums = all(v, "musicCarouselShelfRenderer")
        .into_iter()
        .find(|s| first(&s["header"], "title").is_some_and(|t| titled(t) == "albums"))
        .map(|s| {
            all(s, "musicTwoRowItemRenderer")
                .into_iter()
                .map(|x| {
                    let year = runs(&x["subtitle"]).pop().map_or(String::new(), |r| r.text);
                    Item {
                        kind: Kind::Album,
                        id: string(x["navigationEndpoint"]["browseEndpoint"].get("browseId")),
                        title: text(&x["title"]),
                        author: name.into(),
                        year,
                        cover: best_thumb(&x["thumbnailRenderer"]),
                        ..Default::default()
                    }
                })
                .filter(|t| !t.id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    (songs, albums)
}

pub fn parse_radio(v: &Value) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let Some(panel) = first(v, "playlistPanelRenderer") else { return out };
    for entry in panel["contents"].as_array().into_iter().flatten() {
        // a "wrapper" entry carries the song and its video version: take the primary one
        let wrapper = &entry["playlistPanelVideoWrapperRenderer"]["primaryRenderer"];
        let Some(x) = first(if wrapper.is_null() { entry } else { wrapper }, "playlistPanelVideoRenderer") else { continue };
        let gs = groups(runs(&x["longBylineText"]));
        let t = Item {
            kind: Kind::Track,
            id: string(x.get("videoId")),
            title: text(&x["title"]),
            artist: gs.first().map_or(String::new(), |g| names(g)),
            album: album_of(&gs),
            cover: best_thumb(&x["thumbnail"]),
            ..Default::default()
        };
        if !t.id.is_empty() {
            out.push(t);
        }
    }
    out
}

impl Yt {
    pub fn new() -> Yt {
        Yt { agent: agent() }
    }

    fn call(&self, endpoint: &str, mut body: Value) -> Result<Value, String> {
        body["context"] = json!({"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20250101.01.00", "hl": "en", "gl": "US"}});
        self.agent
            .post(&format!("{API}{endpoint}?prettyPrint=false"))
            .set("User-Agent", "Mozilla/5.0")
            .set("Origin", "https://music.youtube.com")
            .send_json(body)
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())
    }
    fn search(&self, query: &str, params: &str) -> Result<Value, String> {
        self.call("search", json!({"query": query, "params": params}))
    }
    pub fn songs(&self, query: &str) -> Result<Vec<Item>, String> {
        Ok(parse_songs(&self.search(query, SONGS)?))
    }
    pub fn albums(&self, query: &str) -> Result<Vec<Item>, String> {
        Ok(parse_albums(&self.search(query, ALBUMS)?))
    }
    pub fn artists(&self, query: &str) -> Result<Vec<Item>, String> {
        Ok(parse_artists(&self.search(query, ARTISTS)?))
    }
    pub fn album(&self, album: &Item) -> Result<Vec<Item>, String> {
        Ok(parse_album(&self.call("browse", json!({"browseId": album.id}))?, album))
    }
    pub fn artist(&self, id: &str, name: &str) -> Result<(Vec<Item>, Vec<Item>), String> {
        Ok(parse_artist(&self.call("browse", json!({"browseId": id}))?, name))
    }
    pub fn radio(&self, video: &str) -> Result<Vec<Item>, String> {
        let body = json!({"videoId": video, "playlistId": format!("RDAMVM{video}"), "isAudioOnly": true});
        Ok(parse_radio(&self.call("next", body)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_titles_and_radio_wrappers() {
        let song = |id: &str| json!({"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": id}}});
        // "contents" sorts before "title", and holds a title of its own
        let page = json!({"musicShelfRenderer": {"contents": [song("s"), {"menu": {"title": {"runs": [{"text": "Share"}]}}}],
            "title": {"runs": [{"text": "Top songs"}]}}});
        assert_eq!(parse_artist(&page, "A").0.len(), 1);
        let video = |id: &str| json!({"playlistPanelVideoRenderer": {"videoId": id}});
        let radio = json!({"playlistPanelRenderer": {"contents": [{"playlistPanelVideoWrapperRenderer": {
            "counterpart": [{"counterpartRenderer": video("video")}], "primaryRenderer": video("song")}}, video("plain")]}});
        assert_eq!(parse_radio(&radio).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["song", "plain"]);
    }

    #[test]
    fn parses_rows() {
        let run = |t: &str, page: &str| match page {
            "" => json!({"text": t}),
            p => json!({"text": t, "navigationEndpoint": {"browseEndpoint": {"x": {"pageType": p}}}}),
        };
        let col = |runs: Vec<Value>| json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}});
        let v = json!({"c": [{"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": "vid"},
            "thumbnail": {"thumbnails": [{"url": "small", "width": 60}, {"url": "big\u{1b}", "width": 120}]},
            "flexColumns": [col(vec![run("Song", "")]), col(vec![run("A", "MUSIC_PAGE_TYPE_ARTIST"), run(" & ", ""), run("B", ""), run(" • ", ""),
                run("Disc", "MUSIC_PAGE_TYPE_ALBUM"), run(" • ", ""), run("3:00", "")])]
        }}]});
        let t = &parse_songs(&v)[0];
        assert_eq!((t.id.as_str(), t.title.as_str(), t.artist.as_str(), t.album.as_str(), t.cover.as_str()), ("vid", "Song", "A, B", "Disc", "big"));
    }
}
