// Own local log: every track seen, likes, and play history. Same file and schema as the Node version.
use crate::item::{Item, Kind};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub fn default_path() -> PathBuf {
    std::env::var_os("YM_DB").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".ymus").join("ymus.db")
    })
}

pub struct Play {
    pub track: Item,
    pub at: i64,
    pub seconds: i64,
    pub duration: Option<i64>,
    pub skipped: bool,
}

pub struct Signals {
    pub plays: Vec<Play>,
    pub liked: Vec<(Item, i64)>,
    pub disliked: Vec<Item>,
}

pub struct Recap {
    pub songs: Vec<Item>,
    pub artists: Vec<Item>,
    pub albums: Vec<Item>,
}

pub struct Db {
    c: Connection,
    last_search: Cell<i64>,
}

fn track(r: &Row) -> rusqlite::Result<Item> {
    let text = |name| r.get::<_, Option<String>>(name).map(Option::unwrap_or_default);
    Ok(Item {
        kind: Kind::Track,
        id: r.get("id")?,
        title: text("title")?,
        artist: text("artist")?,
        album: text("album")?,
        cover: text("cover")?,
        ..Default::default()
    })
}

impl Db {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Db> {
        if let Some(dir) = path.parent() {
            use std::os::unix::fs::DirBuilderExt;
            let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir);
        }
        let c = Connection::open(path)?;
        c.busy_timeout(Duration::from_secs(5))?; // worker threads open their own connection
        c.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS tracks(id TEXT PRIMARY KEY, title TEXT, artist TEXT, album TEXT, cover TEXT);
             CREATE TABLE IF NOT EXISTS likes(track_id TEXT PRIMARY KEY REFERENCES tracks(id), at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS dislikes(track_id TEXT PRIMARY KEY REFERENCES tracks(id), at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS plays(id INTEGER PRIMARY KEY, track_id TEXT NOT NULL REFERENCES tracks(id),
               at INTEGER NOT NULL, seconds INTEGER NOT NULL DEFAULT 0);
             CREATE INDEX IF NOT EXISTS plays_at ON plays(at DESC);
             CREATE TABLE IF NOT EXISTS searches(q TEXT PRIMARY KEY, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT);
             CREATE TABLE IF NOT EXISTS cache(k TEXT PRIMARY KEY, v TEXT, at INTEGER NOT NULL);",
        )?;
        // columns added later; older rows keep NULL / 0
        let columns: HashSet<String> = c.prepare("PRAGMA table_info(plays)")?.query_map([], |r| r.get(1))?.collect::<Result<_, _>>()?;
        if !columns.contains("duration") {
            c.execute_batch("ALTER TABLE plays ADD COLUMN duration INTEGER")?;
        }
        if !columns.contains("skipped") {
            c.execute_batch("ALTER TABLE plays ADD COLUMN skipped INTEGER NOT NULL DEFAULT 0")?;
        }
        Ok(Db { c, last_search: Cell::new(0) })
    }

    // shortcut: a failed local read or write yields nothing instead of ending the session; surface it if it ever happens
    fn rows(&self, sql: &str, p: impl rusqlite::Params, f: impl Fn(&Row) -> rusqlite::Result<Item>) -> Vec<Item> {
        self.c
            .prepare_cached(sql)
            .and_then(|mut s| s.query_map(p, |r| f(r))?.collect())
            .unwrap_or_default()
    }

    fn exec(&self, sql: &str, p: impl rusqlite::Params) -> usize {
        self.c.prepare_cached(sql).and_then(|mut s| s.execute(p)).unwrap_or(0)
    }

    fn save_track(&self, t: &Item) {
        let cover = (!t.cover.is_empty()).then_some(&t.cover);
        self.exec(
            "INSERT INTO tracks VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title, artist=excluded.artist,
             album=COALESCE(NULLIF(excluded.album, ''), album), cover=COALESCE(excluded.cover, cover)",
            params![t.id, t.title, t.artist, t.album, cover],
        );
    }

    fn ids(&self, table: &str) -> HashSet<String> {
        self.c
            .prepare(&format!("SELECT track_id FROM {table}"))
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
            .unwrap_or_default()
    }
    pub fn liked_ids(&self) -> HashSet<String> {
        self.ids("likes")
    }
    pub fn disliked_ids(&self) -> HashSet<String> {
        self.ids("dislikes")
    }

    // A track is liked, disliked or neither. True if it now has the rating asked for.
    fn toggle(&self, table: &str, other: &str, t: &Item) -> bool {
        self.save_track(t);
        if self.exec(&format!("DELETE FROM {table} WHERE track_id=?"), [&t.id]) > 0 {
            return false;
        }
        self.exec(&format!("DELETE FROM {other} WHERE track_id=?"), [&t.id]);
        self.exec(&format!("INSERT INTO {table} VALUES(?,?)"), params![t.id, now_ms()]);
        true
    }
    pub fn toggle_like(&self, t: &Item) -> bool {
        self.toggle("likes", "dislikes", t)
    }
    pub fn toggle_dislike(&self, t: &Item) -> bool {
        self.toggle("dislikes", "likes", t)
    }

    pub fn liked(&self) -> Vec<Item> {
        self.rows("SELECT t.* FROM likes l JOIN tracks t ON t.id=l.track_id ORDER BY l.at DESC, l.rowid DESC", [], track)
    }
    pub fn disliked(&self) -> Vec<Item> {
        self.rows("SELECT t.* FROM dislikes d JOIN tracks t ON t.id=d.track_id", [], track)
    }

    pub fn start_play(&self, t: &Item, at: i64) -> i64 {
        self.save_track(t);
        self.exec("INSERT INTO plays(track_id, at) VALUES(?,?)", params![t.id, at]);
        self.c.last_insert_rowid()
    }
    pub fn end_play(&self, id: i64, seconds: i64, duration: Option<i64>, skipped: bool) {
        self.exec(
            "UPDATE plays SET seconds=?, duration=COALESCE(?, duration), skipped=? WHERE id=?",
            params![seconds, duration, skipped as i64, id],
        );
    }

    pub fn signals(&self) -> Signals {
        let plays = self
            .c
            .prepare("SELECT t.*, p.at, p.seconds, p.duration, p.skipped FROM plays p JOIN tracks t ON t.id = p.track_id")
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok(Play {
                        track: track(r)?,
                        at: r.get("at")?,
                        seconds: r.get("seconds")?,
                        duration: r.get("duration")?,
                        skipped: r.get::<_, i64>("skipped")? != 0,
                    })
                })?
                .collect()
            })
            .unwrap_or_default();
        let liked = self
            .c
            .prepare("SELECT t.*, l.at FROM likes l JOIN tracks t ON t.id = l.track_id")
            .and_then(|mut s| s.query_map([], |r| Ok((track(r)?, r.get("at")?)))?.collect())
            .unwrap_or_default();
        Signals { plays, liked, disliked: self.disliked() }
    }

    pub fn heard(&self) -> Vec<Item> {
        self.rows(
            "SELECT * FROM tracks WHERE id IN
             (SELECT track_id FROM plays UNION SELECT track_id FROM likes UNION SELECT track_id FROM dislikes)",
            [],
            track,
        )
    }

    // Answers from outside services, kept between sessions. None when missing or too old.
    pub fn cache_get(&self, k: &str, max_age: Option<Duration>) -> Option<Value> {
        let (v, at): (String, i64) = self
            .c
            .query_row("SELECT v, at FROM cache WHERE k=?", [k], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .ok()??;
        if max_age.is_some_and(|m| now_ms() - at > m.as_millis() as i64) {
            return None;
        }
        serde_json::from_str(&v).ok()
    }
    pub fn cache_set(&self, k: &str, v: &Value) {
        self.exec(
            "INSERT INTO cache VALUES(?,?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v, at=excluded.at",
            params![k, v.to_string(), now_ms()],
        );
    }

    pub fn save_search(&self, text: &str) {
        // strictly increasing, so "most recent first" never ties
        let at = now_ms().max(self.last_search.get() + 1);
        self.last_search.set(at);
        self.exec("INSERT INTO searches VALUES(?,?) ON CONFLICT(q) DO UPDATE SET at=excluded.at", params![text, at]);
    }
    pub fn recent_searches(&self) -> Vec<String> {
        self.c
            .prepare("SELECT q FROM searches ORDER BY at DESC LIMIT 50")
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
            .unwrap_or_default()
    }

    pub fn get_setting(&self, k: &str) -> Option<String> {
        self.c.query_row("SELECT v FROM settings WHERE k=?", [k], |r| r.get(0)).optional().ok().flatten()
    }
    pub fn set_setting(&self, k: &str, v: &str) {
        self.exec("INSERT INTO settings VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", params![k, v]);
    }

    pub fn listened(&self, since: i64) -> i64 {
        self.c
            .query_row("SELECT COALESCE(SUM(seconds), 0) FROM plays WHERE at >= ?", [since], |r| r.get(0))
            .unwrap_or(0)
    }

    // Top songs, artists and albums by time listened since a moment.
    pub fn recap(&self, since: i64) -> Recap {
        let rows = self.rows(
            "SELECT t.*, COUNT(*) AS plays, SUM(p.seconds) AS seconds FROM plays p JOIN tracks t ON t.id = p.track_id
             WHERE p.at >= ? AND p.seconds > 0 GROUP BY t.id ORDER BY seconds DESC, plays DESC, t.title",
            [since],
            |r| Ok(Item { plays: r.get("plays")?, seconds: r.get("seconds")?, ..track(r)? }),
        );
        let rank = |mut list: Vec<Item>| {
            list.sort_by(|a, b| {
                b.seconds.cmp(&a.seconds).then(b.plays.cmp(&a.plays)).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            });
            list.truncate(100);
            list
        };
        let group = |entries: &dyn Fn(&Item) -> Vec<(String, Item)>| {
            let mut at: HashMap<String, usize> = HashMap::new();
            let mut out: Vec<Item> = Vec::new();
            for r in &rows {
                for (key, base) in entries(r) {
                    let i = *at.entry(key.clone()).or_insert_with(|| {
                        out.push(Item { id: key, ..base });
                        out.len() - 1
                    });
                    let g = &mut out[i];
                    g.plays += r.plays;
                    g.seconds += r.seconds;
                    if g.cover.is_empty() {
                        g.cover = r.cover.clone(); // rows come longest first
                    }
                }
            }
            rank(out)
        };
        let artists = group(&|r| {
            r.artist
                .split(", ")
                .filter(|n| !n.is_empty())
                .map(|n| (n.to_string(), Item { title: n.to_string(), ..Default::default() }))
                .collect()
        });
        let albums = group(&|r| {
            if r.album.is_empty() {
                return vec![];
            }
            let first = r.main_artist().to_string();
            vec![(format!("{}\n{first}", r.album), Item { title: r.album.clone(), artist: first, ..Default::default() })]
        });
        Recap { songs: rank(rows.clone()), artists, albums }
    }

    // Favorites = what you play plus what you like (a like weighs as much as 3 plays).
    pub fn top_tracks(&self, limit: i64) -> Vec<Item> {
        self.rows(
            "SELECT * FROM (
               SELECT t.*, (SELECT COUNT(*) FROM plays p WHERE p.track_id = t.id) AS plays,
                      (SELECT MAX(at) FROM plays p WHERE p.track_id = t.id) AS last_at,
                      EXISTS(SELECT 1 FROM likes l WHERE l.track_id = t.id) AS liked
               FROM tracks t WHERE t.id NOT IN (SELECT track_id FROM dislikes))
             WHERE plays > 0 OR liked
             ORDER BY plays + 3 * liked DESC, last_at DESC, id LIMIT ?",
            [limit],
            |r| Ok(Item { plays: r.get("plays")?, ..track(r)? }),
        )
    }

    // shortcut: artists are split from the stored "A, B" credit string, so a name containing ", " counts as two
    pub fn top_artists(&self) -> Vec<Item> {
        let rows = self.rows(
            "SELECT t.id, t.title, t.artist, t.album, t.cover,
               (SELECT COUNT(*) FROM plays p WHERE p.track_id = t.id) AS plays,
               (SELECT COUNT(*) FROM likes l WHERE l.track_id = t.id) AS likes FROM tracks t
             WHERE t.id NOT IN (SELECT track_id FROM dislikes)",
            [],
            |r| Ok(Item { plays: r.get("plays")?, likes: r.get("likes")?, ..track(r)? }),
        );
        let mut at: HashMap<String, usize> = HashMap::new();
        let mut out: Vec<(Item, i64)> = Vec::new(); // artist, score of the track its cover came from
        for r in &rows {
            for name in r.artist.split(", ").filter(|n| !n.is_empty()) {
                let i = *at.entry(name.to_string()).or_insert_with(|| {
                    out.push((Item { kind: Kind::ArtistStat, id: name.into(), title: name.into(), ..Default::default() }, -1));
                    out.len() - 1
                });
                let (a, best) = &mut out[i];
                a.plays += r.plays;
                a.likes += r.likes;
                let score = r.plays + 3 * r.likes;
                if !r.cover.is_empty() && score > *best {
                    a.cover = r.cover.clone(); // artist art = cover of their favorite track
                    *best = score;
                }
            }
        }
        let score = |a: &Item| a.plays + 3 * a.likes;
        let mut list: Vec<Item> = out.into_iter().map(|x| x.0).filter(|a| score(a) > 0).collect();
        list.sort_by(|x, y| score(y).cmp(&score(x)).then_with(|| x.title.to_lowercase().cmp(&y.title.to_lowercase())));
        list.truncate(30);
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("ymus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Db::open(&dir.join("t.db")).unwrap()
    }
    fn song(id: &str, artist: &str, album: &str) -> Item {
        Item { id: id.into(), title: id.into(), artist: artist.into(), album: album.into(), ..Default::default() }
    }
    fn ids(v: &[Item]) -> Vec<&str> {
        v.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn likes_favorites_and_settings() {
        let db = temp_db("likes");
        let a = Item { cover: "http://c/1".into(), ..song("aaa", "X", "Al") };
        let b = song("bbb", "Y", "Al");
        assert!(db.toggle_like(&a));
        assert!(db.toggle_like(&b));
        assert_eq!(ids(&db.liked()), ["bbb", "aaa"]);
        assert!(!db.toggle_like(&a));
        let p = db.start_play(&a, now_ms());
        db.end_play(p, 42, None, false);
        db.start_play(&b, now_ms());
        db.toggle_like(&song("ccc", "Y, Z", "Al2"));
        assert_eq!(ids(&db.top_tracks(50)), ["bbb", "ccc", "aaa"]);
        let artists = db.top_artists();
        assert_eq!(artists.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Y", "Z", "X"]);
        assert_eq!((artists[0].plays, artists[0].likes, artists[2].cover.as_str()), (1, 2, "http://c/1"));
        db.save_search("daft punk");
        db.save_search("boards of canada");
        db.save_search("daft punk");
        assert_eq!(db.recent_searches(), ["daft punk", "boards of canada"]);
        db.set_setting("k", "1");
        db.set_setting("k", "2");
        assert_eq!(db.get_setting("k").as_deref(), Some("2"));
        assert!(db.toggle_dislike(&b));
        assert!(!db.liked_ids().contains("bbb") && !ids(&db.top_tracks(50)).contains(&"bbb"));
    }

    #[test]
    fn recap_signals_and_cache() {
        let db = temp_db("recap");
        let day = 86_400_000;
        let add = |t: Item, ago: i64, s: i64| db.end_play(db.start_play(&t, now_ms() - ago * day), s, None, false);
        add(song("s1", "A, B", "One"), 1, 100);
        add(song("s1", "A, B", "One"), 2, 100);
        add(song("s2", "B", "Two"), 20, 500);
        add(song("s3", "C", ""), 200, 50);
        add(song("s4", "C", ""), 1, 0);
        assert_eq!([7, 30, 365].map(|d| db.listened(now_ms() - d * day)), [200, 700, 750]);
        let month = db.recap(now_ms() - 30 * day);
        assert_eq!(ids(&month.songs), ["s2", "s1"]);
        assert_eq!((month.songs[1].seconds, month.songs[1].plays), (200, 2));
        assert_eq!(month.artists.iter().map(|a| (a.title.as_str(), a.seconds)).collect::<Vec<_>>(), [("B", 700), ("A", 200)]);
        assert_eq!(month.albums.iter().map(|a| (a.title.as_str(), a.artist.as_str())).collect::<Vec<_>>(), [("Two", "B"), ("One", "A")]);
        db.toggle_like(&song("s1", "A, B", "")); // a later save without an album keeps the known one
        assert_eq!(db.liked()[0].album, "One");
        let p = db.start_play(&song("k1", "K", "Al"), now_ms());
        db.end_play(p, 12, Some(200), true);
        let s = db.signals();
        let k = s.plays.iter().find(|p| p.track.id == "k1").unwrap();
        assert_eq!((k.seconds, k.duration, k.skipped), (12, Some(200), true));
        db.cache_set("c", &serde_json::json!({"a": [1]}));
        assert!(db.cache_get("c", None).is_some());
        std::thread::sleep(Duration::from_millis(5));
        assert!(db.cache_get("c", Some(Duration::from_millis(1))).is_none());
        db.cache_set("none", &Value::Null);
        assert_eq!(db.cache_get("none", None), Some(Value::Null));
    }
}
