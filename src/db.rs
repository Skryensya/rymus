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

// The tables that are your data, in the order they are exported and read back (songs before what points at them).
const EXPORTED: [&str; 10] = ["tracks", "likes", "dislikes", "plays", "playlists", "playlist_tracks", "searches", "settings", "saved_albums", "follows"];
// Lyrics kept on disk, all together: about 3 000 songs. Past it the ones used longest ago are dropped.
pub const LYRICS_CAP: i64 = 10 * 1024 * 1024;

pub fn default_path() -> PathBuf {
    std::env::var_os("YM_DB").map(PathBuf::from).unwrap_or_else(|| {
        let home = crate::platform::home_dir();
        let dir = home.join(".rymus");
        moved(&home.join(".ymus"), &dir);
        dir.join("rymus.db")
    })
}

// Before the rename the data lived in ~/.ymus/ymus.db. It moves once to ~/.rymus/rymus.db, and links are left at the
// old names so the JavaScript version, and anything else that knows the old place, still finds the same data.
fn moved(old: &std::path::Path, new: &std::path::Path) {
    let old_is_dir = std::fs::symlink_metadata(old).is_ok_and(|m| m.is_dir()); // a link means it already moved
    if new.exists() || !old_is_dir || std::fs::rename(old, new).is_err() {
        return;
    }
    for end in ["", "-wal", "-shm"] {
        // the journal files go with the database, or the last changes are lost
        let _ = std::fs::rename(new.join(format!("ymus.db{end}")), new.join(format!("rymus.db{end}")));
    }
    let _ = std::os::unix::fs::symlink("rymus.db", new.join("ymus.db"));
    let _ = std::os::unix::fs::symlink(new, old);
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
    // A connection to nothing, held for the moment the real database is closed and replaced.
    pub fn memory() -> Db {
        Db { c: Connection::open_in_memory().expect("an in-memory database"), last_search: Cell::new(0) }
    }
    // Your data as CSV files, one per table, in a new folder (an existing one is never overwritten). The caches are
    // left out: they are rebuilt on their own.
    pub fn export_csv(&self, dir: &std::path::Path) -> Result<(), String> {
        if dir.exists() {
            return Err(format!("{} already exists", dir.display()));
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for table in EXPORTED {
            let mut q = self.c.prepare(&format!("SELECT * FROM {table}")).map_err(|e| e.to_string())?;
            let names: Vec<String> = q.column_names().iter().map(|n| n.to_string()).collect();
            let mut out = crate::csv::line(&names);
            let mut rows = q.query([]).map_err(|e| e.to_string())?;
            while let Some(r) = rows.next().map_err(|e| e.to_string())? {
                let fields: Vec<String> = (0..names.len())
                    .map(|i| match r.get_ref(i) {
                        Ok(rusqlite::types::ValueRef::Integer(n)) => n.to_string(),
                        Ok(rusqlite::types::ValueRef::Real(x)) => x.to_string(),
                        Ok(rusqlite::types::ValueRef::Text(t)) => String::from_utf8_lossy(t).into_owned(),
                        _ => String::new(), // NULL
                    })
                    .collect();
                out += &crate::csv::line(&fields);
            }
            std::fs::write(dir.join(format!("{table}.csv")), out).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    // A new database at `into` holding what an exported folder has. Columns are matched by name, so a folder from an
    // older or newer version still reads; a column this version does not know is refused, never put in the SQL.
    pub fn import_csv(dir: &std::path::Path, into: &std::path::Path) -> Result<(), String> {
        if let Some(t) = ["tracks", "likes", "plays"].iter().find(|t| !dir.join(format!("{t}.csv")).is_file()) {
            return Err(format!("{} has no {t}.csv", dir.display()));
        }
        let _ = std::fs::remove_file(into);
        let db = Db::open(into).map_err(|e| e.to_string())?;
        db.c.execute_batch("BEGIN").map_err(|e| e.to_string())?;
        for table in EXPORTED {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("{table}.csv"))) else { continue };
            let mut rows = crate::csv::parse(&text).into_iter();
            let Some(head) = rows.next() else { continue };
            let known: HashSet<String> = db.c.prepare(&format!("PRAGMA table_info({table})")).and_then(|mut q| q.query_map([], |r| r.get(1))?.collect()).map_err(|e| e.to_string())?;
            if let Some(bad) = head.iter().find(|h| !known.contains(*h)) {
                return Err(format!("{table}.csv has a column rymus does not know: {bad}"));
            }
            let sql = format!("INSERT OR REPLACE INTO {table}({}) VALUES({})", head.iter().map(|h| format!("\"{h}\"")).collect::<Vec<_>>().join(","), vec!["?"; head.len()].join(","));
            let mut q = db.c.prepare(&sql).map_err(|e| e.to_string())?;
            for row in rows.filter(|r| r.len() == head.len()) {
                // an empty field was NULL: text columns read it back as empty
                let values: Vec<Option<&str>> = row.iter().map(|f| (!f.is_empty()).then_some(f.as_str())).collect();
                q.execute(rusqlite::params_from_iter(values)).map_err(|e| format!("{table}.csv: {e}"))?;
            }
        }
        db.c.execute_batch("COMMIT").map_err(|e| e.to_string())
    }

    // Lyrics already fetched, by song id; a hit counts as a use, so what you play stays and the rest goes first.
    pub fn lyrics_get(&self, id: &str) -> Option<String> {
        let v: String = self.c.query_row("SELECT v FROM lyrics WHERE id=?", [id], |r| r.get(0)).ok()?;
        // "none" keeps the time it was asked, so it can be asked again (see media::stale)
        self.exec("UPDATE lyrics SET at=? WHERE id=? AND v != '\"none\"'", params![now_ms(), id]);
        Some(v)
    }
    pub fn lyrics_age(&self, id: &str) -> Option<Duration> {
        let at: i64 = self.c.query_row("SELECT at FROM lyrics WHERE id=?", [id], |r| r.get(0)).ok()?;
        Some(Duration::from_millis((now_ms() - at).max(0) as u64))
    }
    // Keeps them, and keeps all of them together under LYRICS_CAP by dropping the ones used longest ago.
    pub fn lyrics_set(&self, id: &str, v: &str) {
        self.exec("INSERT INTO lyrics VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET v=excluded.v, at=excluded.at", params![id, v, now_ms()]);
        let size = || self.c.query_row("SELECT COALESCE(SUM(length(v)), 0) FROM lyrics", [], |r| r.get::<_, i64>(0)).unwrap_or(0);
        while size() > LYRICS_CAP {
            if self.exec("DELETE FROM lyrics WHERE id IN (SELECT id FROM lyrics ORDER BY at LIMIT 1)", []) == 0 {
                break;
            }
        }
    }

    pub fn open(path: &std::path::Path) -> rusqlite::Result<Db> {
        if let Some(dir) = path.parent() {
            crate::platform::private_dir(dir);
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
             CREATE TABLE IF NOT EXISTS playlists(id INTEGER PRIMARY KEY, name TEXT NOT NULL, source TEXT, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS playlist_tracks(id INTEGER PRIMARY KEY, playlist_id INTEGER NOT NULL, track_id TEXT NOT NULL REFERENCES tracks(id));
             CREATE TABLE IF NOT EXISTS searches(q TEXT PRIMARY KEY, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT);
             CREATE TABLE IF NOT EXISTS cache(k TEXT PRIMARY KEY, v TEXT, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS lyrics(id TEXT PRIMARY KEY, v TEXT NOT NULL, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS saved_albums(id TEXT PRIMARY KEY, title TEXT, artist TEXT, year TEXT, cover TEXT, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS follows(id TEXT PRIMARY KEY, name TEXT, cover TEXT, known TEXT, fresh TEXT, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS downloads(id TEXT PRIMARY KEY, path TEXT NOT NULL, at INTEGER NOT NULL);",
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
        self.rows("SELECT t.* FROM dislikes d JOIN tracks t ON t.id=d.track_id ORDER BY d.at DESC, d.rowid DESC", [], track)
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

    // (when, seconds heard) of every play since a moment, for the charts.
    pub fn listening(&self, since: i64) -> Vec<(i64, i64)> {
        let Ok(mut q) = self.c.prepare("SELECT at, seconds FROM plays WHERE at >= ? AND seconds > 0") else { return vec![] };
        q.query_map([since], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    // Top songs, artists and albums by time listened since a moment.
    pub fn recap(&self, since: i64) -> Recap {
        let rows = self.rows(
            "SELECT t.*, SUM(p.seconds) AS seconds FROM plays p JOIN tracks t ON t.id = p.track_id
             WHERE p.at >= ? AND p.seconds > 0 GROUP BY t.id ORDER BY seconds DESC, t.title",
            [since],
            |r| Ok(Item { seconds: r.get("seconds")?, ..track(r)? }),
        );
        let rank = |mut list: Vec<Item>| {
            list.sort_by(|a, b| {
                b.seconds.cmp(&a.seconds).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())) // time only, never play counts
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

    // The last songs played, each once, newest first.
    // Every listen, newest first, with when it started.
    pub fn history(&self, limit: i64) -> Vec<(Item, i64)> {
        self.c
            .prepare("SELECT t.*, p.at FROM plays p JOIN tracks t ON t.id = p.track_id WHERE p.seconds > 0 ORDER BY p.at DESC, p.id DESC LIMIT ?")
            .and_then(|mut s| s.query_map([limit], |r| Ok((track(r)?, r.get("at")?)))?.collect())
            .unwrap_or_default()
    }
    pub fn recent(&self, limit: i64) -> Vec<Item> {
        self.rows(
            "SELECT t.* FROM tracks t JOIN (SELECT track_id, MAX(at) AS last_at, MAX(id) AS last FROM plays GROUP BY track_id) p
             ON p.track_id = t.id ORDER BY p.last_at DESC, p.last DESC LIMIT ?",
            [limit],
            track,
        )
    }

    // ---------- playlists ----------
    pub fn playlists(&self) -> Vec<Item> {
        self.rows(
            "SELECT p.id, p.name, p.source, COUNT(e.id) AS songs,
               (SELECT t.cover FROM playlist_tracks f JOIN tracks t ON t.id = f.track_id WHERE f.playlist_id = p.id AND t.cover IS NOT NULL ORDER BY f.id LIMIT 1) AS cover
             FROM playlists p LEFT JOIN playlist_tracks e ON e.playlist_id = p.id GROUP BY p.id ORDER BY p.at DESC, p.id DESC",
            [],
            |r| {
                let source: Option<String> = r.get("source")?;
                let songs: i64 = r.get("songs")?;
                let from = match source.as_deref() {
                    Some(s) if s.contains("spotify") => " · from Spotify",
                    Some(_) => " · from YouTube Music",
                    None => "",
                };
                Ok(Item {
                    kind: Kind::Playlist,
                    id: r.get::<_, i64>("id")?.to_string(),
                    title: r.get("name")?,
                    cover: r.get::<_, Option<String>>("cover")?.unwrap_or_default(),
                    detail: Some(format!("{songs} song{}{from}", if songs == 1 { "" } else { "s" })),
                    ..Default::default()
                })
            },
        )
    }
    // A playlist's songs in order; `uid` is the entry, since a song can be in it twice.
    pub fn playlist_tracks(&self, playlist: i64) -> Vec<Item> {
        self.rows(
            "SELECT t.*, e.id AS entry FROM playlist_tracks e JOIN tracks t ON t.id = e.track_id WHERE e.playlist_id = ? ORDER BY e.id",
            [playlist],
            |r| Ok(Item { uid: r.get::<_, i64>("entry")? as u64, ..track(r)? }),
        )
    }
    pub fn playlist_from(&self, source: &str) -> Option<(i64, String)> {
        self.c.query_row("SELECT id, name FROM playlists WHERE source=?", [source], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok().flatten()
    }
    // All or nothing: a playlist is never left half written.
    pub fn create_playlist(&self, name: &str, source: Option<&str>, tracks: &[Item]) -> Option<i64> {
        let all = self.c.unchecked_transaction().ok()?;
        all.execute("INSERT INTO playlists(name, source, at) VALUES(?,?,?)", params![name, source, now_ms()]).ok()?;
        let id = all.last_insert_rowid();
        for t in tracks {
            self.save_track(t);
            all.execute("INSERT INTO playlist_tracks(playlist_id, track_id) VALUES(?,?)", params![id, t.id]).ok()?;
        }
        all.commit().ok()?;
        Some(id)
    }
    // False if the song was already there.
    pub fn add_to_playlist(&self, playlist: i64, t: &Item) -> bool {
        self.save_track(t);
        self.exec(
            "INSERT INTO playlist_tracks(playlist_id, track_id) SELECT ?1, ?2
             WHERE NOT EXISTS (SELECT 1 FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2)",
            params![playlist, t.id],
        ) > 0
    }
    pub fn remove_entry(&self, entry: u64) {
        self.exec("DELETE FROM playlist_tracks WHERE id=?", [entry as i64]);
    }
    pub fn rename_playlist(&self, playlist: i64, name: &str) {
        self.exec("UPDATE playlists SET name=? WHERE id=?", params![name, playlist]);
    }
    // Albums you saved (by liking them), newest first.
    pub fn save_album(&self, a: &Item) {
        self.exec("INSERT OR IGNORE INTO saved_albums VALUES(?,?,?,?,?,?)", params![a.id, a.title, a.author, a.year, a.cover, now_ms()]);
    }
    pub fn drop_album(&self, id: &str) {
        self.exec("DELETE FROM saved_albums WHERE id=?", [id]);
    }
    pub fn saved_albums(&self) -> Vec<Item> {
        self.rows("SELECT * FROM saved_albums ORDER BY at DESC, rowid DESC", [], |r| {
            let text = |n| r.get::<_, Option<String>>(n).map(Option::unwrap_or_default);
            Ok(Item { kind: crate::item::Kind::Album, id: r.get("id")?, title: text("title")?, author: text("artist")?, year: text("year")?, cover: text("cover")?, ..Default::default() })
        })
    }
    // Followed artists. `known` are the ids of the albums seen so far; `fresh` the new ones not opened yet.
    pub fn follow(&self, id: &str, name: &str, cover: &str, known: &Value) {
        self.exec("INSERT OR REPLACE INTO follows VALUES(?,?,?,?,'[]',?)", params![id, name, cover, known.to_string(), now_ms()]);
    }
    pub fn unfollow(&self, id: &str) {
        self.exec("DELETE FROM follows WHERE id=?", [id]);
    }
    pub fn follows(&self) -> Vec<(Item, Value, Value)> {
        let read = |r: &Row| {
            let text = |n| r.get::<_, Option<String>>(n).map(Option::unwrap_or_default);
            let json = |n| text(n).map(|t| serde_json::from_str::<Value>(&t).unwrap_or(Value::Null));
            let artist = Item { kind: crate::item::Kind::Artist, id: r.get("id")?, title: text("name")?, cover: text("cover")?, author: "Artist".into(), ..Default::default() };
            Ok((artist, json("known")?, json("fresh")?))
        };
        self.c.prepare("SELECT * FROM follows ORDER BY name COLLATE NOCASE").and_then(|mut s| s.query_map([], read)?.collect()).unwrap_or_default()
    }
    pub fn set_releases(&self, id: &str, known: &Value, fresh: &Value) {
        self.exec("UPDATE follows SET known=?, fresh=? WHERE id=?", params![known.to_string(), fresh.to_string(), id]);
    }

    // Songs kept as files.
    pub fn add_download(&self, t: &Item, file: &std::path::Path) {
        self.save_track(t);
        self.exec("INSERT OR REPLACE INTO downloads VALUES(?,?,?)", params![t.id, file.to_string_lossy(), now_ms()]);
    }
    // The downloads (rymus's own, not your music) whose song is known, for filling in what they lack.
    pub fn download_files(&self) -> Vec<(Item, std::path::PathBuf)> {
        self.c
            .prepare("SELECT t.*, d.path FROM downloads d JOIN tracks t ON t.id = d.id")
            .and_then(|mut s| s.query_map([], |r| Ok((track(r)?, std::path::PathBuf::from(r.get::<_, String>("path")?))))?.collect())
            .unwrap_or_default()
    }
    pub fn downloads(&self) -> Vec<(String, std::path::PathBuf)> {
        self.c
            .prepare("SELECT id, path FROM downloads")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, std::path::PathBuf::from(r.get::<_, String>(1)?))))?.collect())
            .unwrap_or_default()
    }
    pub fn drop_download(&self, id: &str) {
        self.exec("DELETE FROM downloads WHERE id=?", [id]);
    }
    pub fn downloaded_tracks(&self) -> Vec<Item> {
        self.rows("SELECT t.* FROM downloads d JOIN tracks t ON t.id=d.id ORDER BY d.at DESC", [], track)
    }
    // Each download with how many times it was heard, the least heard (then the oldest) first.
    pub fn download_plays(&self) -> Vec<(String, std::path::PathBuf, i64)> {
        self.c
            .prepare(
                "SELECT d.id, d.path, (SELECT COUNT(*) FROM plays p WHERE p.track_id=d.id AND p.seconds > 0) AS n
                 FROM downloads d ORDER BY n, d.at",
            )
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, std::path::PathBuf::from(r.get::<_, String>(1)?), r.get(2)?)))?.collect())
            .unwrap_or_default()
    }

    pub fn playlist_source(&self, playlist: i64) -> Option<String> {
        self.c.query_row("SELECT source FROM playlists WHERE id=?", [playlist], |r| r.get(0)).ok().flatten()
    }
    pub fn delete_playlist(&self, playlist: i64) {
        self.exec("DELETE FROM playlist_tracks WHERE playlist_id=?", [playlist]);
        self.exec("DELETE FROM playlists WHERE id=?", [playlist]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyrics_stay_under_the_cap_dropping_the_ones_used_longest_ago() {
        let dir = std::env::temp_dir().join(format!("rymus-lyrics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Db::open(&dir.join("t.db")).unwrap();
        let big = "x".repeat((LYRICS_CAP / 4) as usize);
        for id in ["a", "b", "c"] {
            db.lyrics_set(id, &big);
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(db.lyrics_get("a").is_some()); // played again: now the most recent
        std::thread::sleep(Duration::from_millis(3));
        db.lyrics_set("d", &big);
        db.lyrics_set("e", &big); // over the cap: b goes, the oldest use
        assert!(db.lyrics_get("b").is_none());
        assert!(db.lyrics_get("a").is_some() && db.lyrics_get("e").is_some());
        db.lyrics_set("n", "\"none\"");
        assert!(db.lyrics_age("n").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn csv_export_and_import_keep_everything() {
        let dir = std::env::temp_dir().join(format!("rymus-csv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Db::open(&dir.join("t.db")).unwrap();
        let t = Item { id: "v1".into(), title: "Hey, \"you\"".into(), artist: "Band".into(), ..Default::default() };
        db.toggle_like(&t);
        db.set_setting("accent", "blue");
        db.export_csv(&dir.join("out")).unwrap();
        assert!(db.export_csv(&dir.join("out")).is_err(), "an existing folder is never overwritten");
        Db::import_csv(&dir.join("out"), &dir.join("back.db")).unwrap();
        let back = Db::open(&dir.join("back.db")).unwrap();
        assert!(back.liked_ids().contains("v1"));
        assert_eq!(back.get_setting("accent").as_deref(), Some("blue"));
        // a column it does not know is refused, never put in the SQL
        std::fs::write(dir.join("out/settings.csv"), "k,v;DROP TABLE likes\r\na,b\r\n").unwrap();
        assert!(Db::import_csv(&dir.join("out"), &dir.join("bad.db")).unwrap_err().contains("does not know"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_old_data_folder_moves_once_and_the_old_names_still_work() {
        let home = std::env::temp_dir().join(format!("rymus-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let (old, new) = (home.join(".ymus"), home.join(".rymus"));
        std::fs::create_dir_all(old.join("hooks")).unwrap();
        std::fs::write(old.join("ymus.db"), "data").unwrap();
        std::fs::write(old.join("ymus.db-wal"), "journal").unwrap();
        moved(&old, &new);
        assert_eq!(std::fs::read_to_string(new.join("rymus.db")).unwrap(), "data");
        assert_eq!(std::fs::read_to_string(new.join("rymus.db-wal")).unwrap(), "journal");
        assert!(new.join("hooks").is_dir(), "everything in the folder comes along");
        assert_eq!(std::fs::read_to_string(old.join("ymus.db")).unwrap(), "data", "the old path reaches the same file");
        moved(&old, &new); // the second run finds it moved and changes nothing
        assert_eq!(std::fs::read_to_string(new.join("rymus.db")).unwrap(), "data");
        let _ = std::fs::remove_dir_all(&home);
    }

    pub fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("rymus-{name}-{}", std::process::id()));
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
        assert_eq!(ids(&db.recent(50)), ["bbb", "aaa"]);
        db.save_search("daft punk");
        db.save_search("boards of canada");
        db.save_search("daft punk");
        assert_eq!(db.recent_searches(), ["daft punk", "boards of canada"]);
        db.set_setting("k", "1");
        db.set_setting("k", "2");
        assert_eq!(db.get_setting("k").as_deref(), Some("2"));
        assert!(db.toggle_dislike(&b));
        assert!(!db.liked_ids().contains("bbb"));
    }

    #[test]
    fn playlists() {
        let db = temp_db("playlists");
        let (a, b) = (Item { cover: "http://c/a".into(), ..song("a", "X", "") }, song("b", "Y", ""));
        let id = db.create_playlist("Mine", Some("https://open.spotify.com/playlist/1"), &[a.clone(), b.clone(), a.clone()]).unwrap();
        assert_eq!(ids(&db.playlist_tracks(id)), ["a", "b", "a"]);
        assert_eq!(db.playlist_from("https://open.spotify.com/playlist/1"), Some((id, "Mine".to_string())));
        db.remove_entry(db.playlist_tracks(id)[0].uid); // only that one of the two
        assert_eq!(ids(&db.playlist_tracks(id)), ["b", "a"]);
        assert!(!db.add_to_playlist(id, &b));
        assert!(db.add_to_playlist(id, &song("c", "Z", "")));
        db.rename_playlist(id, "Renamed");
        let all = db.playlists();
        assert_eq!((all[0].title.as_str(), all[0].cover.as_str(), all[0].detail.as_deref()), ("Renamed", "http://c/a", Some("3 songs · from Spotify")));
        db.delete_playlist(id);
        assert!(db.playlists().is_empty() && db.playlist_tracks(id).is_empty());
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
        assert_eq!(month.songs[1].seconds, 200); // two plays of 100 s
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
