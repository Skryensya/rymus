// YouTube Music without an account: the few InnerTube calls rymus needs, parsed from their JSON.
use crate::item::{clean, Item, Kind};
use serde_json::{json, Value};
use std::time::Duration;

#[cfg_attr(test, allow(dead_code))] // tests answer from made up data
const API: &str = "https://music.youtube.com/youtubei/v1/";
const SONGS: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
const ALBUMS: &str = "EgWKAQIYAWoKEAkQBRAKEAMQBA==";
const ARTISTS: &str = "EgWKAQIgAWoKEAkQBRAKEAMQBA==";

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()
}

#[derive(Clone)]
pub struct Yt {
    #[cfg_attr(test, allow(dead_code))]
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

fn song(x: &Value) -> Item {
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
}
#[cfg(test)]
pub fn parse_songs(v: &Value) -> Vec<Item> {
    list_items(v).into_iter().map(song).filter(|t| !t.id.is_empty()).collect()
}

// The words of a name, lower case and without accents: "Beyoncé - Halo" -> ["beyonce", "halo"].
pub(crate) fn words(s: &str) -> Vec<String> {
    let folded: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .filter(|&c| !matches!(c, '\'' | '’'))
        .collect();
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}
// Versions you did not ask for go after the song itself.
const VARIANTS: [&str; 7] = ["remix", "live", "cover", "karaoke", "instrumental", "sped", "slowed"];

// How faithful a title by someone is to what was typed, lower first: (words of the query found nowhere, how close the
// title is to what is left of the query once the artist's name is taken out, an unasked version). Popularity does not
// count: between equals, YouTube's own order stays.
fn relevance(query: &str, title: &str, by: &str) -> (usize, u8, u8) {
    let (q, full, who) = (words(query), words(title), words(by));
    // "Song (feat. X) - Remastered" -> "Song"
    let bare = title.split(['(', '[']).next().unwrap_or(title);
    let base = words(bare.split(" - ").next().unwrap_or(bare));
    let within = |set: &[String], w: &String| set.iter().any(|x| x.starts_with(w.as_str()));
    let missing = q.iter().filter(|w| !within(&full, w) && !within(&who, w)).count();
    let rest: Vec<String> = q.iter().filter(|w| !within(&who, w)).cloned().collect();
    let closeness = if rest.is_empty() || base == q || base == rest {
        0
    } else if base.join(" ").starts_with(&rest.join(" ")) {
        1
    } else {
        2
    };
    let variant = full.iter().any(|w| VARIANTS.contains(&w.as_str()) && !q.contains(w));
    (missing, closeness, u8::from(variant))
}

// The artist named as typed, then names that start with it, then names that hold its words ("the" aside).
pub fn ranked_artists(v: &Value, query: &str) -> Vec<Item> {
    let name = |s: &str| {
        let mut w = words(s);
        if w.len() > 1 && w[0] == "the" {
            w.remove(0);
        }
        w
    };
    let q = name(query);
    let mut rows: Vec<Item> = list_items(v).into_iter().map(parse_artist_row).filter(|a| !a.id.is_empty()).collect();
    rows.sort_by_cached_key(|a| {
        let n = name(&a.title);
        if n == q {
            0
        } else if n.join(" ").starts_with(&q.join(" ")) {
            1
        } else if q.iter().all(|w| n.iter().any(|x| x.starts_with(w.as_str()))) {
            2
        } else {
            3
        }
    });
    rows
}

pub fn ranked_songs(v: &Value, query: &str) -> Vec<Item> {
    let mut rows: Vec<Item> = list_items(v).into_iter().map(song).filter(|t| !t.id.is_empty()).collect();
    rows.sort_by_cached_key(|t| relevance(query, &t.title, &t.artist));
    rows
}

pub fn ranked_albums(v: &Value, query: &str) -> Vec<Item> {
    let mut rows = parse_albums(v);
    rows.sort_by_cached_key(|a| relevance(query, &a.title, &a.author));
    rows
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

fn parse_artist_row(x: &Value) -> Item {
    Item {
        kind: Kind::Artist,
        id: string(x["navigationEndpoint"]["browseEndpoint"].get("browseId")),
        title: text(column(x, 0)),
        author: "Artist".into(),
        cover: best_thumb(&x["thumbnail"]),
        ..Default::default()
    }
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

// Rows of a playlist page or of one of its continuations: title, artists, album.
fn parse_playlist_rows(v: &Value) -> Vec<Item> {
    list_items(v)
        .into_iter()
        .map(|x| Item {
            kind: Kind::Track,
            id: video_id(x),
            title: text(column(x, 0)),
            artist: groups(runs(column(x, 1))).first().map_or(String::new(), |g| names(g)),
            album: text(column(x, 2)),
            cover: best_thumb(&x["thumbnail"]),
            ..Default::default()
        })
        .filter(|t| !t.id.is_empty()) // removed or unavailable videos have no id
        .collect()
}
fn next_page(v: &Value) -> Option<String> {
    let token = first(first(v, "continuationItemRenderer")?, "continuationCommand")?["token"].as_str()?;
    Some(token.to_string())
}

// A music video or somebody's upload, as opposed to the song itself ("ATV"). A row that does not say counts as a song.
fn is_video(entry: &Value) -> bool {
    let kind = first(&entry["navigationEndpoint"], "musicVideoType").and_then(Value::as_str).unwrap_or("");
    matches!(kind, "MUSIC_VIDEO_TYPE_OMV" | "MUSIC_VIDEO_TYPE_UGC")
}

// The songs of a radio, which feeds Discover and the mixes: videos are left out, there are plenty of songs.
// The artist's picture, square (the page links to a wide banner of it), and the list of all their songs by popularity.
pub fn parse_artist_head(v: &Value) -> (String, String) {
    let banner = best_thumb(&v["header"]);
    let picture = match banner.rsplit_once("=w") {
        Some((base, _)) => format!("{base}=w544-h544-p-l90-rj"),
        None => banner,
    };
    // the "Top songs" shelf's title and footer lead to that list
    let songs = all(v, "musicShelfRenderer")
        .into_iter()
        .flat_map(|s| [&s["title"], &s["bottomEndpoint"]])
        .filter_map(|link| first(link, "browseId").and_then(Value::as_str))
        .find(|id| id.starts_with("VL"))
        .map_or(String::new(), clean);
    (picture, songs)
}

pub struct Artist {
    pub songs: Vec<Item>,
    pub albums: Vec<Item>,
    pub picture: String,
    pub all_songs: String, // browse id, empty if the page has none
}

pub fn parse_radio(v: &Value) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let Some(panel) = first(v, "playlistPanelRenderer") else { return out };
    for entry in panel["contents"].as_array().into_iter().flatten() {
        // a "wrapper" entry carries the song and its video version: take the primary one
        let wrapper = &entry["playlistPanelVideoWrapperRenderer"]["primaryRenderer"];
        let Some(x) = first(if wrapper.is_null() { entry } else { wrapper }, "playlistPanelVideoRenderer") else { continue };
        if is_video(x) {
            continue;
        }
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

    #[cfg(test)]
    fn call(&self, endpoint: &str, body: Value) -> Result<Value, String> {
        Ok(fake::answer(endpoint, &body))
    }
    #[cfg(not(test))]
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
    // A page of search results, 20 or so, of songs (0), albums (1) or artists (2), and the token of the next page.
    pub fn page(&self, kind: usize, query: &str, token: Option<&str>) -> Result<(Vec<Item>, Option<String>), String> {
        let page = match token {
            Some(token) => self.call("search", json!({"continuation": token}))?,
            None => self.search(query, [SONGS, ALBUMS, ARTISTS][kind])?,
        };
        let rows = [ranked_songs, ranked_albums, ranked_artists][kind](&page, query);
        let next = first(&page, "nextContinuationData").and_then(|n| n["continuation"].as_str()).map(String::from);
        Ok((rows, next))
    }
    pub fn songs(&self, query: &str) -> Result<Vec<Item>, String> {
        Ok(ranked_songs(&self.search(query, SONGS)?, query))
    }
    pub fn albums(&self, query: &str) -> Result<Vec<Item>, String> {
        Ok(ranked_albums(&self.search(query, ALBUMS)?, query))
    }
    pub fn artists(&self, query: &str) -> Result<Vec<Item>, String> {
        Ok(ranked_artists(&self.search(query, ARTISTS)?, query))
    }
    pub fn album(&self, album: &Item) -> Result<Vec<Item>, String> {
        Ok(parse_album(&self.call("browse", json!({"browseId": album.id}))?, album))
    }
    pub fn artist(&self, id: &str, name: &str) -> Result<Artist, String> {
        let page = self.call("browse", json!({"browseId": id}))?;
        let (songs, albums) = parse_artist(&page, name);
        let (picture, all_songs) = parse_artist_head(&page);
        Ok(Artist { songs, albums, picture, all_songs })
    }
    // The first page (up to 100) of an artist's songs, most played first.
    pub fn artist_songs(&self, list: &str) -> Result<Vec<Item>, String> {
        let page = self.call("browse", json!({"browseId": list}))?;
        Ok(first(&page, "musicPlaylistShelfRenderer").map_or(Vec::new(), parse_playlist_rows))
    }
    // (name, songs, whether there were more than we read)
    pub fn playlist(&self, id: &str) -> Result<(String, Vec<Item>, bool), String> {
        const MOST: usize = 2000;
        let page = self.call("browse", json!({"browseId": format!("VL{id}")}))?;
        let name = ["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer"]
            .iter()
            .find_map(|k| first(&page, k))
            .map_or(String::new(), |h| text(&h["title"]));
        let shelf = first(&page, "musicPlaylistShelfRenderer").ok_or("this playlist cannot be read (is it public?)")?;
        let mut tracks = parse_playlist_rows(shelf);
        let mut token = next_page(shelf);
        while let Some(t) = token.take().filter(|_| tracks.len() < MOST) {
            let more = self.call("browse", json!({"continuation": t}))?;
            let rows = parse_playlist_rows(&more);
            if !rows.is_empty() {
                token = next_page(&more);
                tracks.extend(rows);
            }
        }
        let partial = tracks.len() >= MOST;
        Ok((name, tracks, partial))
    }
    pub fn radio(&self, video: &str) -> Result<Vec<Item>, String> {
        let body = json!({"videoId": video, "playlistId": format!("RDAMVM{video}"), "isAudioOnly": true});
        Ok(parse_radio(&self.call("next", body)?))
    }
}

// The made up YouTube Music the tests run against: three results for any search, a radio and a playlist.
#[cfg(test)]
mod fake {
    use super::{ALBUMS, ARTISTS};
    use serde_json::{json, Value};

    fn row(id: &str, title: &str, artist: &str) -> Value {
        let column = |t: &str| json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": t}]}}});
        json!({"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": id}, "flexColumns": [column(title), column(artist)]}})
    }
    fn radio_entry(id: &str, title: &str, kind: &str) -> Value {
        json!({"playlistPanelVideoRenderer": {"videoId": id, "title": {"runs": [{"text": title}]}, "longBylineText": {"runs": [{"text": "Radio Band"}]},
            "navigationEndpoint": {"watchEndpoint": {"musicVideoType": format!("MUSIC_VIDEO_TYPE_{kind}")}}}})
    }

    pub fn answer(endpoint: &str, body: &Value) -> Value {
        match endpoint {
            "search" if body["params"] == ARTISTS && body["query"] == "Band" => json!({"rows": [{"musicResponsiveListItemRenderer": {
                "navigationEndpoint": {"browseEndpoint": {"browseId": "UCband"}},
                "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Band"}]}}}]}}]}),
            // what a followed artist has put out since: one album more
            "browse" if body["browseId"] == "UCnews" => json!({"musicCarouselShelfRenderer": {"header": {"title": {"runs": [{"text": "Albums"}]}},
                "contents": [{"musicTwoRowItemRenderer": {"navigationEndpoint": {"browseEndpoint": {"browseId": "MPREnew"}}, "title": {"runs": [{"text": "Fresh Record"}]},
                    "subtitle": {"runs": [{"text": "2026"}]}}}]}}),
            "browse" if body["browseId"] == "UCband" => json!({"musicShelfRenderer": {"title": {"runs": [{"text": "Top songs",
                "navigationEndpoint": {"browseEndpoint": {"browseId": "VLall"}}}]}, "contents": [row("t1", "Hit", "Band")]}}),
            "browse" if body["browseId"] == "VLall" => json!({"musicPlaylistShelfRenderer": {"contents": [row("t1", "Hit", "Band"),
                row("t2", "Hit (Live)", "Band"), row("t3", "Deep Cut", "Band"), row("g", "Guest Spot", "Someone Else"), row("t4", "Closer", "Band")]}}),
            // an album: "Long Road" by Band, 2019, three songs
            "search" if body["params"] == ALBUMS && (body["query"] == "record" || body["query"] == "Long Road Band") => json!({"rows": [{"musicResponsiveListItemRenderer": {
                "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREroad"}},
                "thumbnail": {"thumbnails": [{"url": "test://road", "width": 120}]},
                "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Long Road"}]}}},
                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Album"}, {"text": " • "}, {"text": "Band"}, {"text": " • "}, {"text": "2019"}]}}}]}}]}),
            "browse" if body["browseId"] == "MPREroad" => json!({"musicShelfRenderer": {"contents": [row("a1", "Opening", ""), row("a2", "Middle", ""), row("a3", "Ending", "")]}}),
            "search" if body["params"] == ALBUMS || body["params"] == ARTISTS => json!({}),
            // a second page, reached by its token
            "search" if body["continuation"] == "more" => json!({"rows": [row("m1", "Later One", "Band"), row("m2", "Later Two", "Band")]}),
            "search" if body["query"] == "many" && body["params"] == super::SONGS => json!({"rows": (1..=8).map(|i| row(&format!("k{i}"), &format!("Many {i}"), "Band")).collect::<Vec<_>>(),
                "continuations": [{"nextContinuationData": {"continuation": "more"}}]}),
            "search" => json!({"rows": [row("s1", "Result One", "Band"), row("s2", "Result Two", "Band"), row("s3", "Result Three", "Band")]}),
            "next" => {
                let seed = body["videoId"].as_str().unwrap_or("");
                json!({"playlistPanelRenderer": {"contents": [radio_entry(seed, "Result One", "ATV"), radio_entry("r1", "Similar A", "ATV"),
                    radio_entry("clip", "Official Video", "OMV"), radio_entry("r2", "Similar B", "ATV"), radio_entry("r3", "Similar C", "ATV"),
                    radio_entry("r4", "Similar D", "ATV"), radio_entry("r5", "Similar E", "ATV")]}})
            }
            _ => json!({"musicResponsiveHeaderRenderer": {"title": {"runs": [{"text": "Road Songs"}]}},
                "musicPlaylistShelfRenderer": {"contents": [row("p1", "Track One", "Driver"), row("p2", "Track Two", "Driver")]}}),
        }
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
        let kind = |id: &str, kind: &str| json!({"playlistPanelVideoRenderer": {"videoId": id,
            "navigationEndpoint": {"watchEndpoint": {"x": {"musicVideoType": format!("MUSIC_VIDEO_TYPE_{kind}")}}}}});
        let radio = json!({"playlistPanelRenderer": {"contents": [kind("clip", "OMV"), kind("upload", "UGC"), kind("audio", "ATV")]}});
        assert_eq!(parse_radio(&radio).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["audio"]);
    }

    #[test]
    fn ranking() {
        let row = |id: &str, title: &str, by: &str| {
            let col = |t: &str| json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": t}]}}});
            json!({"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": id}, "flexColumns": [col(title), col(by)]}})
        };
        let ids = |rows: Vec<Value>, q: &str| ranked_songs(&json!({"rows": rows}), q).into_iter().map(|t| t.id).collect::<Vec<_>>();
        // by the artist, in YouTube's order; then the ones that only mention them; then what does not answer
        let page = vec![row("remix", "Chord Memory (Daft Punk Remix)", "Ian Pooley"), row("score", "The Son of Flynn", "Daft Punk"),
            row("feat", "Starboy (feat. Daft Punk)", "The Weeknd"), row("hit", "Harder, Better, Faster, Stronger", "Daft Punk"), row("other", "Blinding Lights", "The Weeknd")];
        assert_eq!(ids(page, "Daft Punk"), ["score", "hit", "feat", "remix", "other"]);
        // the title as typed beats a longer one and a version, whatever YouTube put first
        let page = vec![row("live", "Halo (Live)", "Beyoncé"), row("longer", "Halo of Light", "Beyoncé"), row("other", "Halo", "Someone"), row("it", "Halo", "Beyoncé")];
        assert_eq!(ids(page.clone(), "halo beyonce"), ["it", "live", "longer", "other"]);
        assert_eq!(ids(page, "halo live")[0], "live", "a version you ask for comes first");
        let artist = |id: &str, name: &str| {
            let col = |t: &str| json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": t}]}}});
            json!({"musicResponsiveListItemRenderer": {"navigationEndpoint": {"browseEndpoint": {"browseId": id}}, "flexColumns": [col(name)]}})
        };
        let v = json!({"rows": [artist("big", "Queens of the Stone Age"), artist("weeknd", "The Weeknd"), artist("queen", "Queen")]});
        let first = |q: &str| ranked_artists(&v, q)[0].id.clone();
        assert_eq!((first("queen"), first("weeknd")), ("queen".to_string(), "weeknd".to_string()));
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
