// Lyrics from lrclib.net (free, no account). Only the title, artist, album and length of the song are sent.
use crate::item::{clean, Item};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const API: &str = "https://lrclib.net/api/";

#[derive(Debug, PartialEq)]
pub enum Lyrics {
    Synced(Vec<(f64, String)>),
    Plain(Vec<String>),
    Instrumental,
    None,
    Error,
}

// "[mm:ss.xx] line" (a line may carry several stamps) -> (seconds, text) in time order.
pub fn parse_lrc(lrc: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for line in lrc.lines() {
        let (mut stamps, mut text, mut rest) = (Vec::new(), String::new(), line);
        while let Some(open) = rest.find('[') {
            let Some(close) = rest[open..].find(']').map(|c| open + c) else { break };
            text.push_str(&rest[..open]);
            if let Some((m, s)) = rest[open + 1..close].split_once(':') {
                if let (Ok(m), Ok(s)) = (m.parse::<u32>(), s.parse::<f64>()) {
                    stamps.push(m as f64 * 60.0 + s);
                }
            }
            rest = &rest[close + 1..];
        }
        text.push_str(rest);
        let text = clean(&text).trim().to_string();
        out.extend(stamps.into_iter().map(|t| (t, text.clone())));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

// Word wrap to w terminal columns; a word longer than a row is split.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in text.split(' ') {
        if !row.is_empty() && row.width() + 1 + word.width() <= w {
            row.push(' ');
            row.push_str(word);
            continue;
        }
        if !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        for c in word.chars() {
            if row.width() + c.width().unwrap_or(0) > w && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
            }
            row.push(c);
        }
    }
    rows.push(row);
    rows
}

// Picks from lrclib records. Timed lyrics are only trusted from a recording of (almost) the same length.
pub fn pick(records: &[Value], duration: f64) -> Lyrics {
    let off = |r: &Value| (r["duration"].as_f64().unwrap_or(0.0) - duration).abs();
    let field = |r: &&Value, k: &str| r[k].as_str().is_some_and(|s| !s.is_empty());
    let timed = records.iter().filter(|r| field(r, "syncedLyrics") && off(r) <= 3.0).min_by(|a, b| off(a).total_cmp(&off(b)));
    if let Some(r) = timed {
        return Lyrics::Synced(parse_lrc(r["syncedLyrics"].as_str().unwrap_or("")));
    }
    let plain = records
        .iter()
        .find(|r| field(r, "plainLyrics") && off(r) <= 3.0)
        .or_else(|| records.iter().find(|r| field(r, "plainLyrics")));
    if let Some(r) = plain {
        return Lyrics::Plain(r["plainLyrics"].as_str().unwrap_or("").split('\n').map(|l| clean(l).trim().to_string()).collect());
    }
    if records.iter().any(|r| r["instrumental"].as_bool() == Some(true) && off(r) <= 3.0) {
        Lyrics::Instrumental
    } else {
        Lyrics::None
    }
}

fn ask(path: &str, params: &[(&str, &str)]) -> Result<Option<Value>, ()> {
    let mut req = crate::yt::agent().get(&format!("{API}{path}")).set("User-Agent", "ymus");
    for (k, v) in params {
        req = req.query(k, v);
    }
    match req.call() {
        Ok(res) => res.into_json().map(Some).map_err(|_| ()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(_) => Err(()),
    }
}

// "(feat. …)" is often left out by other databases.
fn without_feat(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    for open in ["(feat", "[feat", "(ft", "[ft"] {
        if let Some(i) = lower.find(open) {
            if let Some(j) = title[i..].find([')', ']']) {
                return format!("{}{}", title[..i].trim_end(), &title[i + j + 1..]);
            }
        }
    }
    title.to_string()
}

pub fn fetch(track: &Item, duration: f64) -> Lyrics {
    let artist = track.main_artist();
    let run = || -> Result<Lyrics, ()> {
        let length = format!("{}", duration.round());
        let exact = ask("get", &[("track_name", &track.title), ("artist_name", artist), ("album_name", &track.album), ("duration", &length)])?;
        if let Some(r) = exact.as_ref().filter(|r| r["syncedLyrics"].as_str().is_some_and(|s| !s.is_empty())) {
            return Ok(pick(std::slice::from_ref(r), duration));
        }
        let found = ask("search", &[("track_name", &without_feat(&track.title)), ("artist_name", artist)])?;
        let mut records: Vec<Value> = exact.into_iter().collect();
        records.extend(found.and_then(|v| v.as_array().cloned()).unwrap_or_default());
        Ok(pick(&records, duration))
    };
    run().unwrap_or(Lyrics::Error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parsing_wrapping_and_picking() {
        let got = parse_lrc("[ar:X]\n[00:12.50] second\n[00:01.00][00:20.00] chorus");
        let expect: Vec<(f64, String)> = [(1.0, "chorus"), (12.5, "second"), (20.0, "chorus")].map(|(t, s)| (t, s.to_string())).to_vec();
        assert_eq!(got, expect);
        assert_eq!(parse_lrc("[00:01.00] a\u{1b}[2J\u{7}b")[0].1, "a[2Jb"); // no escape sequences reach the terminal
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("日本語の歌", 4), ["日本", "語の", "歌"]);
        let rec = |d: i32, synced: &str, plain: &str| json!({"duration": d, "syncedLyrics": synced, "plainLyrics": plain});
        let near = pick(&[rec(300, "[00:01.00] far", ""), rec(201, "[00:01.00] near", "")], 200.0);
        assert_eq!(near, Lyrics::Synced(vec![(1.0, "near".into())]));
        assert_eq!(pick(&[rec(300, "[00:01.00] far", "words")], 200.0), Lyrics::Plain(vec!["words".into()]));
        assert_eq!(pick(&[], 200.0), Lyrics::None);
        assert_eq!(without_feat("Get Lucky (feat. X)"), "Get Lucky");
    }
}
