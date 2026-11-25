// Application state and behavior: playback, lists, actions and input. Drawing is in render.rs.
// One thread owns the state; everything slow runs on its own thread and answers with a message.
use crate::cover::{self, Got};
use crate::db::{now_ms, Db};
use crate::item::{Item, Kind, Spec};
use crate::lyrics::{self, Lyrics};
use crate::mixes;
use crate::mpv::Mpv;
use crate::trip::Trip;
use crate::ui::glyphs;
use crate::viz::{self, BANDS};
use crate::yt::Yt;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TOP: usize = 4; // first screen row inside the body frame (tab bar, search bar, frame border)
pub const CATS: [&str; 3] = ["songs", "artists", "albums"];
pub const PERIODS: [(&str, i64); 3] = [("week", 7), ("month", 30), ("year", 365)]; // rolling windows ending now
pub const TABS: [Tab; 4] = [Tab::Home, Tab::Liked, Tab::Queue, Tab::Stats];
const WHEEL: Duration = Duration::from_millis(40);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Home,
    Liked,
    Queue,
    Stats,
    Search,
}

pub type Apply = Box<dyn FnOnce(&mut App) + Send>;
pub enum Msg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
    Mpv(Value),
    Quit,
    Resolved { id: String, uid: u64, url: Option<String> },
    Apply(Apply),
}

#[derive(Clone)]
pub enum Action {
    OpenAlbum(Item),
    OpenArtist(Item),
    GoArtist(String),
    GoAlbum(Item),
    Discover,
    PlayMix(Spec, String),
    Resume,
    Jump(u64),
    PlayFrom(usize),
    PlayNext(Item),
    Enqueue(Item),
    Rate(bool, Item),
    Remove(u64),
    Radio(Item),
}

pub struct Menu {
    pub title: String,
    pub items: Vec<(String, Action)>,
    pub sel: usize,
}

// A results list you can come back to with Esc.
pub struct Saved {
    pub items: Vec<Item>,
    pub title: String,
    pub numbered: bool,
    pub sel: usize,
    pub list_id: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PaneId {
    Songs,
    Artists,
    List,
    Cover,
}
pub struct Pane {
    pub id: PaneId,
    pub x0: usize,
    pub x1: usize,
    pub top: i64,  // list index shown on the first row of the body
    pub head: usize, // fixed rows above the list
}
// Where things are on screen, for the mouse (1-indexed).
#[derive(Default)]
pub struct Geo {
    pub tabs: Vec<(usize, usize, usize)>,
    pub field_x1: usize,
    pub bar_row: usize,
    pub bar_x0: usize,
    pub bar_w: usize,
    pub now_row: usize,
    pub thumb: Option<(usize, usize, usize, usize)>,
    pub panes: Vec<Pane>,
    pub body: usize,
}

#[derive(Default)]
pub struct Marquee {
    pub key: String,
    pub at: Option<Instant>,
    pub step: usize,
    pub on: bool,
}

pub struct Session {
    pub queue: Vec<Item>,
    pub time: i64,
}

fn load_session(db: &Db) -> Option<Session> {
    let v: Value = serde_json::from_str(&db.get_setting("session")?).ok()?;
    let pos = v["pos"].as_u64()? as usize;
    let text = |t: &Value, k: &str| t[k].as_str().unwrap_or("").to_string();
    let queue: Vec<Item> = v["queue"]
        .as_array()?
        .iter()
        .skip(pos)
        .map(|t| Item { id: text(t, "id"), title: text(t, "title"), artist: text(t, "artist"), album: text(t, "album"), cover: text(t, "cover"), ..Default::default() })
        .collect();
    (!queue.is_empty()).then(|| Session { queue, time: v["time"].as_i64().unwrap_or(0) })
}

pub fn fmt(seconds: f64) -> String {
    let s = seconds.max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}
pub fn minutes(s: i64) -> String {
    if s > 0 && s < 60 {
        "<1 min".into()
    } else {
        format!("{} min", (s as f64 / 60.0).round())
    }
}

fn grouped(groups: Vec<(&str, Vec<Item>)>) -> Vec<Item> {
    groups
        .into_iter()
        .filter(|g| !g.1.is_empty())
        .flat_map(|(title, items)| std::iter::once(Item::header(title)).chain(items))
        .collect()
}

fn track_url(t: &Item) -> String {
    format!("https://music.youtube.com/watch?v={}", t.id)
}

fn mix_row(id: &str, title: String, detail: String, spec: Spec) -> Item {
    Item { kind: Kind::Mix, id: id.into(), title, detail: Some(detail), spec: Some(spec), ..Default::default() }
}

pub struct App {
    pub db: Db,
    pub db_path: PathBuf,
    pub yt: Yt,
    pub tx: Sender<Msg>,
    pub mpv: Mpv,
    pub feed: Arc<Mutex<viz::Feed>>,
    pub quit: bool,
    pub clear_screen: bool,

    pub tab: Tab,
    pub prev_tab: Tab,
    pub typing: bool,
    pub buf: String,
    history_at: Option<usize>,
    history: Vec<String>,
    draft: String,
    pub msg: String,
    pub note: String,
    note_until: Option<Instant>,
    pub busy: String,
    pub help: bool,
    pub full: bool,
    pub menu: Option<Menu>,
    pub lyrics: bool,
    pub lyric_scroll: usize,
    pub lyric_line: Option<usize>,

    // results view: current list and the stack of lists to go back to
    pub items: Vec<Item>,
    pub title: String,
    pub numbered: bool,
    pub back: Vec<Saved>,
    pub query: String,
    list_id: u64,
    next_id: u64,

    // home tab: two lists, one focused at a time (sel belongs to the focused one)
    pub home_songs: Vec<Item>,
    pub home_artists: Vec<Item>,
    pub home_focus: PaneId,
    pub home_sel: [usize; 2],

    pub liked: Vec<Item>,
    pub sel: usize,
    search_sel: usize,

    pub stat_rows: Vec<Item>,
    pub totals: [i64; 3],
    pub stat_cat: usize,
    pub period: usize,
    stats_at: Instant,
    heard: f64,
    raw_pos: Option<f64>,
    play_dur: f64,

    pub queue: Vec<Item>,
    pub pos: i64,
    pub paused: bool,
    pub loading: bool,
    pub time: i64,
    pub dur: f64,
    pub vol: i64,
    play_id: Option<i64>,
    direct: HashSet<String>,
    resolving: HashSet<String>,
    pub session: Option<Session>,
    pub likes: HashSet<String>,
    pub dislikes: HashSet<String>,

    pub wave: bool,
    apos: Option<(f64, Instant)>,
    levels: viz::Levels,
    pub bands: [f64; BANDS],
    pub energy: f64, // how loud it really is, for the trip visualizer
    pub trip_on: bool,
    pub trip: Trip,
    pub trip_t: f64,
    trip_at: Instant,

    pub covers: HashMap<String, Option<Arc<Got>>>, // None: loading, or failed (never retried this session)
    pub rasters: HashMap<(String, usize, usize), Option<Vec<String>>>,
    pub kitty: bool,
    pub shown: String,
    pub want: String,
    pub want_at: Instant,
    pub redraw_at: Option<Instant>,
    pub lyric_cache: HashMap<String, Option<Lyrics>>,

    pub mq: Marquee,
    pub geo: Geo,
    drag: bool,
    last_click: Option<(usize, usize, Instant)>,
    last_seek: Instant,
    last_wheel: Instant,
}

impl App {
    pub fn new(db: Db, db_path: PathBuf, tx: Sender<Msg>, mpv: Mpv, feed: Arc<Mutex<viz::Feed>>, vol: i64) -> App {
        let now = Instant::now();
        let session = load_session(&db);
        let likes = db.liked_ids();
        let dislikes = db.disliked_ids();
        let wave = db.get_setting("wave").as_deref() != Some("off");
        App {
            db, db_path, yt: Yt::new(), tx, mpv, feed, quit: false, clear_screen: false,
            tab: Tab::Home, prev_tab: Tab::Home, typing: false, buf: String::new(), history_at: None, history: vec![], draft: String::new(),
            msg: String::new(), note: String::new(), note_until: None, busy: String::new(), help: false, full: false, menu: None,
            lyrics: false, lyric_scroll: 0, lyric_line: None,
            items: vec![], title: String::new(), numbered: false, back: vec![], query: String::new(), list_id: 0, next_id: 1,
            home_songs: vec![], home_artists: vec![], home_focus: PaneId::Songs, home_sel: [0, 0],
            liked: vec![], sel: 0, search_sel: 0,
            stat_rows: vec![], totals: [0; 3], stat_cat: 0, period: 0, stats_at: now, heard: 0.0, raw_pos: None, play_dur: 0.0,
            queue: vec![], pos: -1, paused: false, loading: false, time: 0, dur: 0.0, vol, play_id: None,
            direct: HashSet::new(), resolving: HashSet::new(), session, likes, dislikes,
            wave, apos: None, levels: Default::default(), bands: [0.0; BANDS], energy: 0.0, trip_on: false, trip: Trip::new(), trip_t: 0.0, trip_at: now,
            covers: HashMap::new(), rasters: HashMap::new(), kitty: cover::supported(), shown: String::new(), want: String::new(), want_at: now,
            redraw_at: None, lyric_cache: HashMap::new(),
            mq: Default::default(), geo: Default::default(), drag: false, last_click: None, last_seek: now, last_wheel: now,
        }
    }

    pub fn start(&mut self) {
        for (i, p) in ["time-pos", "duration", "pause", "volume", "playlist-pos", "audio-pts"].iter().enumerate() {
            self.mpv.cmd(json!(["observe_property", i + 1, p]));
        }
        if self.wave {
            self.apply_wave();
        }
        self.load_home();
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn now(&self) -> Option<&Item> {
        usize::try_from(self.pos).ok().and_then(|p| self.queue.get(p))
    }

    // ---------- lists ----------
    pub fn data(&self) -> &[Item] {
        match self.tab {
            Tab::Home if self.home_focus == PaneId::Songs => &self.home_songs,
            Tab::Home => &self.home_artists,
            Tab::Liked => &self.liked,
            Tab::Queue => &self.queue,
            Tab::Stats => &self.stat_rows,
            Tab::Search => &self.items,
        }
    }
    pub fn cur(&self) -> Option<&Item> {
        self.data().get(self.sel)
    }
    pub fn kind_at(&self) -> Kind {
        self.cur().map_or(Kind::Track, |t| t.kind)
    }
    // The songs from row i on (a mixed list also holds artists, albums and headers).
    fn tracks_from(&self, i: usize) -> Vec<Item> {
        self.data().iter().skip(i).filter(|t| t.kind == Kind::Track).cloned().collect()
    }
    // Headers cannot be selected: step past them, turning back at either end of the list.
    fn move_sel(&mut self, to: i64) {
        let data = self.data();
        if data.is_empty() {
            self.sel = 0;
            return;
        }
        let dir: i64 = if to < self.sel as i64 { -1 } else { 1 };
        let at = to.clamp(0, data.len() as i64 - 1);
        let header = |i: i64| i >= 0 && data.get(i as usize).is_some_and(|t| t.kind == Kind::Header);
        let mut i = at;
        while header(i) {
            i += dir;
        }
        if i < 0 || i >= data.len() as i64 {
            i = at;
            while header(i) {
                i -= dir;
            }
        }
        self.sel = i.max(0) as usize;
    }
    // Is this row the track that is playing? In the queue the same song can appear twice, so compare the entry itself.
    pub fn is_current(&self, it: &Item) -> bool {
        self.now().is_some_and(|n| if self.tab == Tab::Queue { it.uid == n.uid } else { it.id == n.id })
    }

    fn resume_row(&self) -> Option<Item> {
        let s = self.session.as_ref()?;
        let t = s.queue.first()?;
        Some(Item {
            kind: Kind::Resume,
            id: "resume".into(),
            title: format!("⏯ Resume: {}", t.title),
            detail: Some(format!("{} · at {}", t.artist, fmt(s.time as f64))),
            ..Default::default()
        })
    }
    fn load_home(&mut self) {
        let discover = Item { kind: Kind::Discover, id: "discover".into(), title: "✦ Discover".into(), detail: Some("new songs · mixes by style".into()), ..Default::default() };
        self.home_songs = self.resume_row().into_iter().chain(std::iter::once(discover)).chain(self.db.top_tracks(50)).collect();
        self.home_artists = self.db.top_artists();
    }
    fn load_stats(&mut self) {
        let since = |days: i64| now_ms() - days * 86_400_000;
        self.totals = std::array::from_fn(|i| self.db.listened(since(PERIODS[i].1)));
        let recap = self.db.recap(since(PERIODS[self.period].1));
        let (rows, kind) = match self.stat_cat {
            0 => (recap.songs, Kind::Track),
            1 => (recap.artists, Kind::ArtistStat),
            _ => (recap.albums, Kind::AlbumStat),
        };
        let artists = self.stat_cat == 1;
        self.stat_rows = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let by = if artists { String::new() } else { format!(" · {}", r.artist) };
                Item { kind, rank: i + 1, detail: Some(format!("{} · {}×{by}", minutes(r.seconds), r.plays)), ..r }
            })
            .collect();
    }
    fn clamp_sel(&mut self) {
        self.sel = self.sel.min(self.data().len().saturating_sub(1));
    }

    // ---------- mpv ----------
    fn cmd(&mut self, command: Value) {
        self.mpv.cmd(command);
    }
    fn apply_wave(&mut self) {
        let command = if self.wave { json!(["af", "add", viz::filter()]) } else { json!(["af", "remove", "@viz"]) };
        self.cmd(command);
    }
    fn save_heard(&self, skipped: bool) {
        if let Some(id) = self.play_id {
            let duration = (self.play_dur > 0.0).then(|| self.play_dur.round() as i64);
            self.db.end_play(id, self.heard.round() as i64, duration, skipped);
        }
    }
    // A skip: you moved on (mpv reports "stop") before 30 s of a song that is longer than that.
    fn log_end(&mut self, reason: &str) {
        self.save_heard(reason == "stop" && self.heard < 30.0 && !(self.play_dur > 0.0 && self.play_dur < 40.0));
        self.play_id = None;
    }

    // False when nothing on screen changed.
    fn on_mpv(&mut self, e: &Value) -> bool {
        match e["event"].as_str().unwrap_or("") {
            "property-change" => {
                let v = &e["data"];
                match e["name"].as_str().unwrap_or("") {
                    "audio-pts" => {
                        self.apos = v.as_f64().map(|p| (p, Instant::now())); // the position being heard
                        return false;
                    }
                    "time-pos" => {
                        // seconds actually heard: small steps forward count, seeks (big jumps) do not
                        if let (Some(raw), Some(p)) = (self.raw_pos, v.as_f64()) {
                            let step = p - raw;
                            if step > 0.0 && step < 2.0 {
                                let before = (self.heard / 5.0).floor();
                                self.heard += step;
                                if (self.heard / 5.0).floor() != before {
                                    self.save_heard(false); // every 5 s, so stats are live
                                }
                            }
                        }
                        self.raw_pos = v.as_f64();
                        let t = v.as_f64().unwrap_or(0.0).floor() as i64;
                        if t == self.time {
                            return false;
                        }
                        self.time = t;
                    }
                    "duration" => {
                        self.dur = v.as_f64().unwrap_or(0.0);
                        if self.dur > 0.0 {
                            self.play_dur = self.dur;
                        }
                    }
                    "pause" => self.paused = v.as_bool().unwrap_or(false),
                    "volume" => {
                        if let Some(v) = v.as_f64() {
                            self.vol = v.round() as i64;
                            self.db.set_setting("volume", &self.vol.to_string());
                        }
                    }
                    "playlist-pos" => {
                        self.pos = v.as_i64().unwrap_or(-1);
                        self.time = 0;
                        self.dur = 0.0;
                    }
                    _ => return false,
                }
            }
            "start-file" => {
                self.loading = true;
                self.msg.clear();
                self.play_dur = 0.0;
            }
            "file-loaded" => {
                self.loading = false;
                self.heard = 0.0;
                self.raw_pos = None;
                if let Some(t) = self.now() {
                    self.play_id = Some(self.db.start_play(t, now_ms()));
                }
                self.preload();
            }
            "end-file" => {
                self.loading = false;
                let reason = e["reason"].as_str().unwrap_or("");
                self.log_end(reason);
                if reason == "error" {
                    self.msg = match e["file_error"].as_str() {
                        Some("loading failed") => "Could not load this track (yt-dlp failed). It may be restricted or unavailable.".into(),
                        other => format!("Playback error: {}", other.unwrap_or("unknown")),
                    };
                }
                if reason != "redirect" && reason != "error" && self.pos >= self.queue.len() as i64 - 1 {
                    self.pos = -1;
                }
            }
            _ => return false,
        }
        true
    }

    // Once something plays, the last session's row leaves Home.
    fn drop_resume_row(&mut self) {
        self.session = None;
        if self.home_songs.first().is_none_or(|t| t.kind != Kind::Resume) {
            return;
        }
        self.home_songs.remove(0);
        if self.tab == Tab::Home && self.home_focus == PaneId::Songs {
            self.sel = self.sel.saturating_sub(1);
        } else {
            self.home_sel[0] = self.home_sel[0].saturating_sub(1);
        }
    }

    fn set_queue(&mut self, mut tracks: Vec<Item>) {
        for t in &mut tracks {
            t.uid = self.id();
        }
        self.queue = tracks;
        self.direct.clear();
        self.pos = 0;
        self.loading = true;
        self.paused = false;
    }
    fn play(&mut self, tracks: Vec<Item>) {
        self.drop_resume_row();
        self.set_queue(tracks);
        let urls: Vec<String> = self.queue.iter().map(track_url).collect();
        for (i, url) in urls.iter().enumerate() {
            self.cmd(json!(["loadfile", url, if i > 0 { "append" } else { "replace" }]));
        }
        self.cmd(json!(["set_property", "pause", false]));
    }
    // Continue the previous run: from the saved track and second onward.
    fn resume(&mut self) {
        let Some(s) = self.session.take() else { return };
        self.drop_resume_row();
        self.set_queue(s.queue);
        let urls: Vec<String> = self.queue.iter().map(track_url).collect();
        for (i, url) in urls.iter().enumerate() {
            let command = if i > 0 { json!(["loadfile", url, "append"]) } else { json!(["loadfile", url, "replace", -1, format!("start={}", s.time)]) };
            self.cmd(command);
        }
        self.cmd(json!(["set_property", "pause", false]));
    }

    fn save_session(&self) {
        if self.queue.is_empty() {
            return; // never played this run: keep whatever was saved
        }
        let value = if self.pos >= 0 {
            let queue: Vec<Value> = self
                .queue
                .iter()
                .take(500)
                .map(|t| json!({"id": t.id, "title": t.title, "artist": t.artist, "album": t.album, "cover": (!t.cover.is_empty()).then_some(&t.cover)}))
                .collect();
            json!({"queue": queue, "pos": self.pos, "time": self.time}).to_string()
        } else {
            String::new() // finished playing: nothing to resume
        };
        self.db.set_setting("session", &value);
    }

    pub fn shutdown(&mut self) {
        self.save_session();
        self.log_end("quit");
        self.mpv.stop();
    }

    // Resolve the stream URL of every upcoming track with yt-dlp (two at a time) and swap it into mpv's playlist, so
    // mpv can prefetch it and jumping to a queued track starts in under a second.
    fn preload(&mut self) {
        let mut i = (self.pos + 1).max(0) as usize;
        while i < self.queue.len() && self.resolving.len() < 2 {
            let t = &self.queue[i];
            if !self.direct.contains(&t.id) && !self.resolving.contains(&t.id) {
                self.resolving.insert(t.id.clone());
                let (id, uid, url, tx) = (t.id.clone(), t.uid, track_url(t), self.tx.clone());
                std::thread::spawn(move || {
                    let out = std::process::Command::new("yt-dlp")
                        .args(["-g", "-f", "bestaudio/best", "--no-playlist", &url])
                        .stdin(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output();
                    let url = out.ok().filter(|o| o.status.success()).and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.trim().to_string()));
                    let _ = tx.send(Msg::Resolved { id, uid, url });
                });
            }
            i += 1;
        }
    }
    fn resolved(&mut self, id: String, uid: u64, url: Option<String>) {
        self.resolving.remove(&id);
        match url.filter(|u| !u.is_empty()) {
            Some(url) => {
                // the queue may have changed while yt-dlp ran
                if let Some(idx) = self.queue.iter().position(|t| t.uid == uid).filter(|&i| i as i64 > self.pos) {
                    self.direct.insert(id);
                    self.cmd(json!(["playlist-remove", idx]));
                    self.cmd(json!(["loadfile", url, "insert-at", idx]));
                }
            }
            None => {
                self.direct.insert(id); // do not retry a failing track forever
            }
        }
        self.preload();
    }

    // ---------- queue editing (queue mirrors mpv's playlist, entry by entry) ----------
    fn queue_track(&mut self, mut t: Item, next: bool) {
        t.uid = self.id();
        let url = track_url(&t);
        if next && self.pos >= 0 {
            let at = (self.pos as usize + 1).min(self.queue.len());
            self.queue.insert(at, t);
            self.cmd(json!(["loadfile", url, "insert-next"]));
        } else {
            self.queue.push(t);
            self.cmd(json!(["loadfile", url, "append-play"])); // plays right away if nothing is playing
        }
        self.preload();
    }
    fn remove_from_queue(&mut self, uid: u64) {
        let Some(idx) = self.queue.iter().position(|t| t.uid == uid) else { return };
        self.queue.remove(idx);
        if (idx as i64) < self.pos {
            self.pos -= 1;
        }
        self.cmd(json!(["playlist-remove", idx]));
        self.sel = self.sel.min(self.queue.len().saturating_sub(1));
        self.preload();
    }
    fn move_in_queue(&mut self, uid: u64, dir: i64) {
        let Some(idx) = self.queue.iter().position(|t| t.uid == uid) else { return };
        let j = idx as i64 + dir;
        if j < 0 || j >= self.queue.len() as i64 {
            return;
        }
        let now = self.now().map(|t| t.uid);
        self.queue.swap(idx, j as usize);
        // mpv moves the entry to *before* the target index
        let target = if dir > 0 { idx as i64 + 2 } else { j };
        self.cmd(json!(["playlist-move", idx, target]));
        if let Some(p) = now.and_then(|u| self.queue.iter().position(|t| t.uid == u)) {
            self.pos = p as i64;
        }
        self.sel = j as usize;
    }

    // ---------- slow work ----------
    // Runs on its own thread with the spinner on; errors land in the red message.
    fn run_busy(&mut self, label: String, work: impl FnOnce() -> Result<Apply, String> + Send + 'static) {
        self.busy = label.clone();
        self.msg.clear();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = work();
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                match result {
                    Ok(apply) => apply(app),
                    Err(e) => app.msg = format!("{} failed: {e}", label.replace('…', "")),
                }
                app.busy.clear();
            })));
        });
    }

    // Search results replace the body of whatever tab you were on; Esc (or any tab) brings it back.
    fn open_results(&mut self) {
        if self.tab == Tab::Search {
            return;
        }
        if self.tab == Tab::Home {
            self.remember_home_sel();
        }
        self.prev_tab = self.tab;
        self.tab = Tab::Search;
        self.menu = None;
        self.lyrics = false;
    }
    fn reopen_results(&mut self) {
        if self.items.is_empty() || self.tab == Tab::Search {
            return;
        }
        self.open_results();
        self.sel = self.search_sel.min(self.items.len() - 1);
    }
    fn set_list(&mut self, title: String, items: Vec<Item>, numbered: bool, sel: usize) {
        self.title = title;
        self.items = items;
        self.numbered = numbered;
        self.sel = sel;
        self.list_id = self.id();
    }
    fn push_back(&mut self) {
        self.back.push(Saved { items: std::mem::take(&mut self.items), title: std::mem::take(&mut self.title), numbered: self.numbered, sel: self.sel, list_id: self.list_id });
    }
    // Show a list on top of the current tab; from inside the results, Esc returns to where you were.
    fn enter_list(&mut self, title: String, items: Vec<Item>, numbered: bool, sel: usize) {
        if self.tab == Tab::Search {
            self.push_back();
        } else {
            self.open_results();
            self.back.clear();
        }
        self.set_list(title, items, numbered, sel);
    }
    fn go_back(&mut self) {
        if let Some(s) = self.back.pop() {
            self.items = s.items;
            self.title = s.title;
            self.numbered = s.numbered;
            self.sel = s.sel;
            self.list_id = s.list_id;
        }
    }

    fn run_search(&mut self, text: String) {
        self.db.save_search(&text);
        let yt = self.yt.clone();
        self.run_busy("Searching…".into(), move || {
            let (artists, albums, songs) = std::thread::scope(|s| {
                let a = s.spawn(|| yt.artists(&text));
                let b = s.spawn(|| yt.albums(&text));
                let c = yt.songs(&text);
                let join = |r: std::thread::Result<Result<Vec<Item>, String>>| r.unwrap_or_else(|_| Err("search thread failed".into()));
                (join(a.join()), join(b.join()), c)
            });
            let head = |mut v: Vec<Item>, n: usize| {
                v.truncate(n);
                v
            };
            let rows = grouped(vec![("Artists", head(artists?, 3)), ("Albums", head(albums?, 6)), ("Songs", head(songs?, 20))]);
            Ok(Box::new(move |app: &mut App| {
                app.query = text.clone();
                app.open_results();
                app.back.clear();
                if rows.is_empty() {
                    app.msg = "Nothing found".into();
                }
                let sel = usize::from(!rows.is_empty()); // row 0 is a header
                app.set_list(format!("Results for \"{text}\""), rows, false, sel);
            }) as Apply)
        });
    }

    fn album_page(yt: &Yt, album: Item) -> Result<Apply, String> {
        let tracks = yt.album(&album)?;
        Ok(Box::new(move |app: &mut App| app.enter_list(format!("{} · {}", album.title, album.author), tracks, true, 0)))
    }
    fn open_album(&mut self, album: Item) {
        let yt = self.yt.clone();
        self.run_busy("Loading album…".into(), move || App::album_page(&yt, album));
    }
    // An album known only by name and artist (Stats).
    fn go_album(&mut self, it: Item) {
        let yt = self.yt.clone();
        self.run_busy("Loading album…".into(), move || {
            let found = yt.albums(&format!("{} {}", it.title, it.artist))?;
            let wanted = it.title.to_lowercase();
            let a = found.iter().find(|a| a.title.to_lowercase() == wanted).or(found.first()).cloned();
            match a {
                Some(mut a) => {
                    if a.author.is_empty() {
                        a.author = it.artist.clone();
                    }
                    App::album_page(&yt, a)
                }
                None => Ok(Box::new(move |app: &mut App| app.msg = format!("No album found: {}", it.title)) as Apply),
            }
        });
    }
    // The artist's page: popular songs first, then albums.
    fn artist_page(yt: &Yt, id: &str, name: String) -> Result<Apply, String> {
        let (songs, albums) = yt.artist(id, &name)?;
        Ok(Box::new(move |app: &mut App| {
            if songs.is_empty() && albums.is_empty() {
                app.msg = "Nothing found for this artist".into();
                return;
            }
            let rows = grouped(vec![("Popular songs", songs), ("Albums", albums)]);
            app.enter_list(name, rows, false, 1);
        }))
    }
    fn open_artist(&mut self, artist: Item) {
        let yt = self.yt.clone();
        self.run_busy("Loading artist…".into(), move || App::artist_page(&yt, &artist.id, artist.title));
    }
    // Open an artist known only by name (Home, and "Go to artist" on a song).
    fn go_artist(&mut self, credit: &str) {
        let name = credit.split(", ").next().unwrap_or("").to_string();
        self.typing = false;
        let yt = self.yt.clone();
        self.run_busy("Loading artist…".into(), move || {
            let found = yt.artists(&name)?;
            let wanted = name.to_lowercase();
            match found.iter().find(|a| a.title.to_lowercase() == wanted).or(found.first()) {
                Some(a) => App::artist_page(&yt, &a.id, a.title.clone()),
                None => Ok(Box::new(move |app: &mut App| app.msg = format!("No artist found: {name}")) as Apply),
            }
        });
    }

    // The Discover view: one mix of everything at once, and a mix per style as soon as they are worked out.
    fn open_discover(&mut self) {
        if self.db.heard().is_empty() {
            self.msg = "Play or like a few songs first".into();
            return;
        }
        self.open_results();
        self.back.clear();
        let all = mix_row("all", "✦ Everything new".into(), "from all your taste".into(), Spec::All);
        self.set_list("Discover · only songs you have not heard".into(), vec![all, Item::header("Reading your styles…")], false, 0);
        let (list, path, tx) = (self.list_id, self.db_path.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let rows = match mixes::find_styles(&path) {
                Ok(found) if found.is_empty() => vec![Item::header("No styles yet: listen to more artists")],
                Ok(found) => std::iter::once(Item::header("By style"))
                    .chain(found.into_iter().map(|s| {
                        let who = s.artists.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                        mix_row(&s.name, format!("✦ {}", s.name), who, Spec::Style(s.artists))
                    }))
                    .collect(),
                Err(e) => vec![Item::header(&format!("Styles unavailable: {e}"))],
            };
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                // the view may be open, or waiting behind Esc
                let items = if app.list_id == list { Some(&mut app.items) } else { app.back.iter_mut().find(|s| s.list_id == list).map(|s| &mut s.items) };
                if let Some(items) = items {
                    items.truncate(1);
                    items.extend(rows);
                }
            })));
        });
    }

    fn play_mix(&mut self, spec: Spec, label: String) {
        if !self.busy.is_empty() {
            return;
        }
        let (yt, path) = (self.yt.clone(), self.db_path.clone());
        self.run_busy(format!("Building {label}…"), move || {
            let mix = mixes::build_mix(&yt, &path, &spec)?;
            Ok(Box::new(move |app: &mut App| {
                if mix.tracks.is_empty() {
                    app.msg = "Nothing new found, try another mix".into();
                    return;
                }
                let fresh = mix.tracks.iter().filter(|t| t.fresh).count();
                let without = if mix.without_listenbrainz { " · without ListenBrainz" } else { "" };
                app.play(mix.tracks);
                app.set_tab(Tab::Queue);
                app.set_note(format!("{label}: {fresh} new songs{without}"));
            }) as Apply)
        });
    }
    // A mix of new songs around whatever a row is (or the playing song).
    fn start_radio(&mut self, it: Option<Item>) {
        use Kind::*;
        let it = it
            .filter(|t| matches!(t.kind, Track | Artist | ArtistStat | Album | AlbumStat))
            .or_else(|| self.now().cloned().map(|t| Item { kind: Track, ..t }));
        let Some(it) = it else { return self.set_note("Nothing to start a radio from".into()) };
        if it.kind == Track {
            let label = format!("Radio · {}", it.title);
            let seed = Item { id: it.id, title: it.title, artist: it.artist, album: it.album, cover: it.cover, ..Default::default() };
            return self.play_mix(Spec::RadioTrack(Box::new(seed)), label);
        }
        let artist = match it.kind {
            Album => it.author,
            AlbumStat => it.artist,
            _ => it.title,
        };
        let label = format!("Radio · {artist}");
        self.play_mix(Spec::RadioArtist(artist), label);
    }

    // ---------- navigation ----------
    fn remember_home_sel(&mut self) {
        self.home_sel[usize::from(self.home_focus == PaneId::Artists)] = self.sel;
    }
    fn focus_home(&mut self, pane: PaneId) {
        if pane == self.home_focus || !matches!(pane, PaneId::Songs | PaneId::Artists) {
            return;
        }
        self.remember_home_sel();
        self.home_focus = pane;
        self.sel = self.home_sel[usize::from(pane == PaneId::Artists)];
    }
    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab == Tab::Home {
            self.remember_home_sel();
        }
        if self.tab == Tab::Search {
            self.search_sel = self.sel;
        }
        self.tab = tab;
        self.typing = false;
        self.menu = None;
        self.lyrics = false;
        match tab {
            Tab::Liked => self.liked = self.db.liked(),
            Tab::Home => self.load_home(),
            Tab::Stats => self.load_stats(),
            _ => {}
        }
        self.sel = match tab {
            Tab::Search => self.search_sel,
            Tab::Home => self.home_sel[usize::from(self.home_focus == PaneId::Artists)],
            Tab::Queue => self.pos.max(0) as usize,
            _ => 0,
        };
        self.clamp_sel();
    }
    fn cycle_tab(&mut self, dir: i64) {
        let base = if self.tab == Tab::Search { self.prev_tab } else { self.tab };
        let i = TABS.iter().position(|&t| t == base).unwrap_or(0) as i64;
        self.set_tab(TABS[(i + dir).rem_euclid(TABS.len() as i64) as usize]);
    }

    pub fn set_note(&mut self, text: String) {
        self.note = text;
        self.note_until = Some(Instant::now() + Duration::from_secs(3));
    }

    // Likes or dislikes t; without t, the highlighted track, or the playing one when the highlight is not a track
    // or the list is hidden (full player, lyrics).
    fn rate(&mut self, like: bool, t: Option<Item>) {
        let t = t.or_else(|| {
            if !self.full && !self.lyrics && self.kind_at() == Kind::Track { self.cur().cloned() } else { self.now().cloned() }
        });
        let verb = if like { "like" } else { "dislike" };
        let Some(t) = t else { return self.set_note(format!("Nothing to {verb}")) };
        let on = if like { self.db.toggle_like(&t) } else { self.db.toggle_dislike(&t) };
        self.likes = self.db.liked_ids();
        self.dislikes = self.db.disliked_ids();
        match self.tab {
            Tab::Liked => self.liked = self.db.liked(),
            Tab::Home => self.load_home(),
            Tab::Stats => self.load_stats(),
            _ => {}
        }
        self.clamp_sel();
        let what = match (on, like) {
            (false, _) => format!("Removed {verb}"),
            (true, true) => format!("{} Liked", glyphs().like),
            (true, false) => format!("{} Disliked", glyphs().dislike),
        };
        self.set_note(format!("{what}: {}", t.title));
        // disliking what is playing moves on to the next song
        if on && !like && self.now().is_some_and(|n| n.id == t.id) && self.pos < self.queue.len() as i64 - 1 {
            self.cmd(json!(["playlist-next"]));
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::OpenAlbum(a) => self.open_album(a),
            Action::OpenArtist(a) => self.open_artist(a),
            Action::GoArtist(name) => self.go_artist(&name),
            Action::GoAlbum(a) => self.go_album(a),
            Action::Discover => self.open_discover(),
            Action::PlayMix(spec, label) => self.play_mix(spec, label),
            Action::Resume => self.resume(),
            Action::Jump(uid) => {
                if let Some(i) = self.queue.iter().position(|t| t.uid == uid) {
                    self.cmd(json!(["playlist-play-index", i]));
                }
            }
            Action::PlayFrom(i) => {
                let tracks = self.tracks_from(i);
                self.play(tracks);
            }
            Action::PlayNext(t) => {
                self.set_note(format!("Next: {}", t.title));
                self.queue_track(t, true);
            }
            Action::Enqueue(t) => {
                self.set_note(format!("Queued: {}", t.title));
                self.queue_track(t, false);
            }
            Action::Rate(like, t) => self.rate(like, Some(t)),
            Action::Remove(uid) => self.remove_from_queue(uid),
            Action::Radio(t) => self.start_radio(Some(t)),
        }
    }

    // What Enter does on a row that is not a plain track.
    fn open_action(it: &Item) -> Option<Action> {
        Some(match it.kind {
            Kind::Album => Action::OpenAlbum(it.clone()),
            Kind::Artist => Action::OpenArtist(it.clone()),
            Kind::ArtistStat => Action::GoArtist(it.title.clone()),
            Kind::AlbumStat => Action::GoAlbum(it.clone()),
            Kind::Discover => Action::Discover,
            Kind::Mix => Action::PlayMix(it.spec.clone()?, it.title.replace("✦ ", "")),
            Kind::Resume => Action::Resume,
            Kind::Track | Kind::Header => return None,
        })
    }

    fn activate(&mut self) {
        let Some(it) = self.cur().cloned() else { return };
        if let Some(action) = App::open_action(&it) {
            self.run(action);
        } else if it.kind == Kind::Header {
        } else if self.is_current(&it) {
            self.cmd(json!(["cycle", "pause"])); // never restarts what is already playing
        } else if self.tab == Tab::Queue {
            self.run(Action::Jump(it.uid));
        } else {
            self.run(Action::PlayFrom(self.sel));
        }
    }
    // Enter and space are the same key: act on the highlighted row; with no row in sight, play or pause.
    fn press(&mut self) {
        if !self.full && !self.lyrics && self.cur().is_some() {
            self.activate();
        } else {
            self.toggle_pause();
        }
    }
    fn toggle_pause(&mut self) {
        if self.now().is_some() {
            self.cmd(json!(["cycle", "pause"]));
        } else {
            self.resume();
        }
    }
    fn escape(&mut self) {
        if self.menu.is_some() {
            self.menu = None;
        } else if self.full {
            self.full = false;
        } else if self.lyrics {
            self.lyrics = false;
        } else if self.tab == Tab::Search {
            if self.back.is_empty() {
                self.set_tab(self.prev_tab);
            } else {
                self.go_back();
            }
        }
    }

    // Actions on the highlighted row (key m, or right-click on a row).
    fn open_menu(&mut self) {
        let Some(it) = self.cur().cloned() else { return };
        if it.kind == Kind::Header {
            return;
        }
        let item = |label: &str, a: Action| (label.to_string(), a);
        let mut items = match App::open_action(&it) {
            Some(a) => {
                let label = match it.kind {
                    Kind::Album | Kind::AlbumStat => "Open album",
                    Kind::Artist | Kind::ArtistStat => "Open artist",
                    Kind::Discover => "Open Discover",
                    Kind::Mix => "Play mix",
                    _ => "Resume",
                };
                vec![item(label, a)]
            }
            None => {
                let queue = self.tab == Tab::Queue;
                let credit = if it.artist.is_empty() { &it.title } else { &it.artist };
                let mut v = vec![
                    if queue { item("Jump to this track", Action::Jump(it.uid)) } else { item("Play from here", Action::PlayFrom(self.sel)) },
                    item("Play next", Action::PlayNext(it.clone())),
                    item("Add to queue", Action::Enqueue(it.clone())),
                    item(if self.likes.contains(&it.id) { "Remove like" } else { "Like" }, Action::Rate(true, it.clone())),
                    item(if self.dislikes.contains(&it.id) { "Remove dislike" } else { "Dislike" }, Action::Rate(false, it.clone())),
                    item("Go to artist", Action::GoArtist(credit.clone())),
                ];
                if queue {
                    v.push(item("Remove from queue", Action::Remove(it.uid)));
                }
                v
            }
        };
        if matches!(it.kind, Kind::Track | Kind::Artist | Kind::ArtistStat | Kind::Album | Kind::AlbumStat) {
            items.push(item("Start radio", Action::Radio(it.clone())));
        }
        self.menu = Some(Menu { title: it.title, items, sel: 0 });
    }
    fn run_menu(&mut self, i: usize) {
        if let Some(action) = self.menu.take().and_then(|m| m.items.get(i).map(|x| x.1.clone())) {
            self.run(action);
        }
    }

    // ---------- lyrics and covers ----------
    // Where playback is right now, finer than the whole seconds of `time`.
    fn position(&self) -> f64 {
        self.apos.map_or(0.0, |(p, at)| p + if self.paused || self.loading { 0.0 } else { at.elapsed().as_secs_f64().min(0.3) })
    }
    pub fn line_at(&self, lines: &[(f64, String)]) -> Option<usize> {
        let pos = self.position() + 0.15; // a hair early reads as on time
        lines.iter().rposition(|l| l.0 <= pos)
    }
    // Makes sure the lyrics of the playing track are on their way; true once they are here.
    pub fn want_lyrics(&mut self) -> Option<String> {
        let t = self.now()?.clone();
        if self.dur <= 0.0 {
            return None;
        }
        if !self.lyric_cache.contains_key(&t.id) {
            self.lyric_cache.insert(t.id.clone(), None);
            let (tx, dur, id) = (self.tx.clone(), self.dur, t.id.clone());
            std::thread::spawn(move || {
                let result = lyrics::fetch(&t, dur);
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    app.lyric_cache.insert(id, Some(result));
                })));
            });
            return None;
        }
        let id = t.id;
        self.lyric_cache.get(&id).is_some_and(Option::is_some).then_some(id)
    }
    pub fn lyrics_ready(&self) -> Option<&Lyrics> {
        self.lyric_cache.get(&self.now()?.id)?.as_ref()
    }
    fn toggle_lyrics(&mut self) {
        self.lyrics = !self.lyrics;
        self.full = false;
        self.lyric_scroll = 0;
        if self.lyrics {
            self.lyric_cache.retain(|_, v| *v != Some(Lyrics::Error)); // a failed request is worth another try
        }
    }
    pub fn cover(&mut self, url: &str) -> Option<Arc<Got>> {
        if let Some(hit) = self.covers.get(url) {
            return hit.clone();
        }
        self.covers.insert(url.to_string(), None);
        let (url, tx) = (url.to_string(), self.tx.clone());
        std::thread::spawn(move || {
            if let Some(got) = cover::load(&url) {
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    app.covers.insert(url, Some(Arc::new(got)));
                })));
            }
        });
        None
    }
    // Half-block rows of a loaded cover, computed off the main thread.
    pub fn cells(&mut self, url: &str, got: &Arc<Got>, w: usize, h: usize) -> Option<Vec<String>> {
        let key = (url.to_string(), w, h);
        if let Some(hit) = self.rasters.get(&key) {
            return hit.clone();
        }
        self.rasters.insert(key.clone(), None);
        let (got, tx) = (got.clone(), self.tx.clone());
        std::thread::spawn(move || {
            if let Some(rows) = cover::cells(&got.image, w, h) {
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    app.rasters.insert(key, Some(rows));
                })));
            }
        });
        None
    }

    // ---------- time ----------
    // Called every 40 ms; true when the screen has to be drawn again.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut need = !self.busy.is_empty() || self.loading;
        let playing = self.pos >= 0 && !self.paused && !self.loading;
        if self.note_until.is_some_and(|t| now >= t) {
            self.note_until = None;
            self.note.clear();
            need = true;
        }
        if self.redraw_at.is_some_and(|t| now >= t) {
            self.redraw_at = None;
            need = true;
        }
        if self.wave {
            // move the bars at a steady pace; they fall to the baseline when nothing plays
            let db = self.apos.filter(|_| playing).and_then(|(p, at)| {
                // where playback will be when this frame reaches the screen
                let pos = p + at.elapsed().as_secs_f64().min(0.3) + 0.03;
                self.feed.lock().unwrap_or_else(|e| e.into_inner()).at(pos)
            });
            let loud = db.map_or(0.0, |db| viz::energy(&db));
            self.energy += (loud - self.energy) * 0.15; // smoothed over about a quarter second
            let next = match db {
                Some(db) => self.levels.next(&db, &self.bands),
                None => self.bands.map(|v| if v > 0.02 { v * 0.74 } else { 0.0 }),
            };
            need |= next != self.bands; // no redraw once everything is still
            self.bands = next;
        }
        if self.full && self.trip_on {
            // its clock is playback time: it stops with the music
            if playing {
                self.trip_t += now.duration_since(self.trip_at).as_secs_f64().min(0.1);
                need = true;
            }
            self.trip_at = now;
        }
        if self.mq.on && crate::render::marquee_step(&self.mq) != self.mq.step {
            need = true;
        }
        if self.tab == Tab::Stats && self.pos >= 0 && !self.paused && now.duration_since(self.stats_at) > Duration::from_secs(2) {
            self.stats_at = now;
            self.load_stats();
            need = true;
        }
        if self.lyrics {
            match self.want_lyrics() {
                None => need |= self.now().is_some(),
                Some(_) => {
                    if let Some(Lyrics::Synced(lines)) = self.lyrics_ready() {
                        need |= self.line_at(lines) != self.lyric_line;
                    }
                }
            }
        }
        need
    }

    // ---------- input ----------
    // True when the screen has to be drawn again.
    pub fn handle(&mut self, m: Msg) -> bool {
        match m {
            Msg::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Msg::Key(_) => return false,
            Msg::Mouse(e) => return self.on_mouse(e),
            Msg::Resize => {
                self.clear_screen = true;
                self.shown.clear();
            }
            Msg::Mpv(e) => return self.on_mpv(&e),
            Msg::Quit => self.quit = true,
            Msg::Resolved { id, uid, url } => {
                self.resolved(id, uid, url);
                return false;
            }
            Msg::Apply(f) => f(self),
        }
        true
    }

    fn seek_at(&mut self, x: usize) {
        if self.dur <= 0.0 || self.geo.bar_w == 0 {
            return;
        }
        let pct = ((x as f64 - self.geo.bar_x0 as f64 - 0.5) / self.geo.bar_w as f64 * 100.0).clamp(0.0, 100.0);
        self.cmd(json!(["seek", pct.round(), "absolute-percent"]));
    }
    // (pane index, list index) of the row under the pointer.
    fn row_at(&self, x: usize, y: usize) -> Option<(usize, usize)> {
        let (p, pane) = self.geo.panes.iter().enumerate().find(|(_, p)| x >= p.x0 && x <= p.x1)?;
        if pane.id == PaneId::Cover || y < TOP + pane.head || y >= TOP + self.geo.body {
            return None;
        }
        usize::try_from(pane.top + (y - TOP) as i64).ok().map(|i| (p, i))
    }
    fn selectable(&self, i: usize) -> bool {
        self.data().get(i).is_some_and(|t| t.kind != Kind::Header)
    }
    fn body_click(&mut self, x: usize, y: usize) {
        let Some((pane, i)) = self.row_at(x, y) else { return };
        if self.tab == Tab::Home {
            self.focus_home(self.geo.panes[pane].id);
        }
        if !self.selectable(i) {
            return;
        }
        let double = self.last_click.is_some_and(|(p, j, at)| p == pane && j == i && at.elapsed() < Duration::from_millis(450));
        self.last_click = Some((pane, i, Instant::now()));
        self.sel = i;
        if double {
            self.activate();
        }
    }

    fn on_mouse(&mut self, e: MouseEvent) -> bool {
        use MouseEventKind as M;
        let (x, y) = (e.column as usize + 1, e.row as usize + 1);
        let wheel: i64 = match e.kind {
            M::ScrollUp => -1,
            M::ScrollDown => 1,
            _ => 0,
        };
        match e.kind {
            M::Up(_) => {
                self.drag = false;
                return false;
            }
            M::Moved | M::ScrollLeft | M::ScrollRight => return false,
            _ => {}
        }
        if self.help {
            if matches!(e.kind, M::Down(_)) {
                self.help = false;
                return true;
            }
            return false;
        }
        if wheel != 0 {
            if self.last_wheel.elapsed() < WHEEL {
                return false; // trackpads report in bursts: one notch is one row
            }
            self.last_wheel = Instant::now();
        }
        if self.menu.is_some() {
            if let Some(m) = &mut self.menu {
                if wheel != 0 {
                    m.sel = (m.sel as i64 + wheel).clamp(0, m.items.len() as i64 - 1) as usize;
                    return true;
                }
            }
            if let M::Down(button) = e.kind {
                let i = y.checked_sub(TOP);
                match (button, i) {
                    (MouseButton::Left, Some(i)) if self.menu.as_ref().is_some_and(|m| i < m.items.len()) => self.run_menu(i),
                    _ => self.menu = None,
                }
            }
            return true;
        }
        if e.kind == M::Down(MouseButton::Left) {
            // tabs and the search field never get trapped by the search box
            if y == 1 {
                if let Some(&(i, _, _)) = self.geo.tabs.iter().find(|s| x >= s.1 && x <= s.2) {
                    self.typing = false;
                    self.set_tab(TABS[i]);
                    return true;
                }
            }
            if y == 2 && x <= self.geo.field_x1 {
                self.start_typing();
                return true;
            }
        }
        if self.typing {
            return false;
        }
        match e.kind {
            M::Drag(_) => {
                // dragging along the progress bar scrubs
                if self.drag && self.last_seek.elapsed() > Duration::from_millis(60) {
                    self.last_seek = Instant::now();
                    self.seek_at(x);
                }
                return false;
            }
            M::ScrollUp | M::ScrollDown if self.lyrics => self.lyric_scroll = (self.lyric_scroll as i64 + wheel).max(0) as usize,
            M::ScrollUp | M::ScrollDown => {
                if self.tab == Tab::Home {
                    if let Some(id) = self.geo.panes.iter().find(|p| x >= p.x0 && x <= p.x1).map(|p| p.id) {
                        self.focus_home(id);
                    }
                }
                self.move_sel(self.sel as i64 + wheel);
            }
            M::Down(MouseButton::Right) => {
                let row = if self.full || self.lyrics { None } else { self.row_at(x, y) };
                if let Some((pane, i)) = row {
                    if self.tab == Tab::Home {
                        self.focus_home(self.geo.panes[pane].id);
                    }
                    if self.selectable(i) {
                        self.sel = i;
                        self.open_menu();
                        return true;
                    }
                }
                self.escape();
            }
            M::Down(MouseButton::Left) => {
                let g = &self.geo;
                let on_thumb = g.thumb.is_some_and(|(x0, x1, y0, y1)| x >= x0 && x <= x1 && y >= y0 && y <= y1);
                if y == g.bar_row {
                    if self.dur > 0.0 && x > g.bar_x0 && x <= g.bar_x0 + g.bar_w {
                        self.drag = true;
                        self.seek_at(x);
                    }
                } else if on_thumb {
                    self.full = !self.full;
                    self.trip_on = false;
                } else if y == g.now_row {
                    self.toggle_pause();
                } else if self.full {
                    self.full = false;
                } else if y >= TOP && y < TOP + g.body {
                    self.body_click(x, y);
                }
            }
            _ => return false,
        }
        true
    }

    fn start_typing(&mut self) {
        self.buf.clear();
        self.history_at = None;
        self.typing = true;
    }

    fn on_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let ch = match k.code {
            KeyCode::Char(c) => Some(c),
            _ => None,
        };
        if ctrl && ch == Some('c') {
            self.quit = true;
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        if self.menu.is_some() {
            self.menu_key(k.code);
        } else if self.typing {
            self.typing_key(k.code, ctrl);
        } else if !ctrl {
            self.command_key(k.code, ch, shift);
        }
    }

    fn menu_key(&mut self, code: KeyCode) {
        let Some(m) = &mut self.menu else { return };
        let n = m.items.len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => m.sel = m.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => m.sel = (m.sel + 1).min(n - 1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let i = m.sel;
                self.run_menu(i);
            }
            KeyCode::Esc | KeyCode::Char('q' | 'm') => self.menu = None,
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < n {
                    self.run_menu(i);
                }
            }
            _ => {}
        }
    }

    fn typing_key(&mut self, code: KeyCode, ctrl: bool) {
        match code {
            KeyCode::Enter => {
                self.typing = false;
                let text = self.buf.trim().to_string();
                if text.is_empty() {
                    self.reopen_results();
                } else {
                    self.run_search(text);
                }
            }
            KeyCode::Esc => self.typing = false,
            KeyCode::Tab | KeyCode::BackTab => {
                self.typing = false;
                self.cycle_tab(if code == KeyCode::BackTab { -1 } else { 1 });
            }
            KeyCode::Up | KeyCode::Down => {
                if self.history_at.is_none() {
                    self.history = self.db.recent_searches();
                    self.draft = self.buf.clone();
                }
                let at = self.history_at.map_or(-1, |i| i as i64) + if code == KeyCode::Up { 1 } else { -1 };
                self.history_at = usize::try_from(at.min(self.history.len() as i64 - 1)).ok();
                self.buf = self.history_at.map_or_else(|| self.draft.clone(), |i| self.history[i].clone());
            }
            KeyCode::Backspace => {
                self.buf.pop();
            }
            KeyCode::Char(c) if !ctrl && !c.is_control() => self.buf.push(c),
            _ => {}
        }
    }

    fn command_key(&mut self, code: KeyCode, ch: Option<char>, shift: bool) {
        use KeyCode as K;
        // capitals act as their lowercase unless they have a meaning of their own
        let name = match code {
            K::Char(c) => K::Char(c.to_ascii_lowercase()),
            other => other,
        };
        let n = self.data().len() as i64;
        if self.lyrics || self.full {
            // the list is hidden: movement scrolls the lyrics and list actions do nothing
            let step: i64 = match name {
                K::Up | K::Char('k') => -1,
                K::Down | K::Char('j') => 1,
                K::PageUp => -10,
                K::PageDown => 10,
                K::Home | K::Char('g') => -1_000_000,
                K::End => 1_000_000,
                _ => 0,
            };
            if step != 0 {
                self.lyric_scroll = (self.lyric_scroll as i64 + step).max(0) as usize;
                return;
            }
            if matches!(name, K::Char('a' | 'd' | 'm')) {
                return;
            }
        }
        match ch {
            Some('D') => return self.open_discover(),
            Some('V') => {
                self.trip_on = !(self.full && self.trip_on);
                self.full = self.trip_on;
                self.lyrics = false;
                self.trip_at = Instant::now();
                return;
            }
            Some('R') => {
                // radio from the highlighted row, or from what plays when the list is hidden
                let it = if self.full || self.lyrics { None } else { self.cur().cloned() };
                return self.start_radio(it);
            }
            _ => {}
        }
        if self.tab == Tab::Stats && !self.lyrics && (ch == Some('t') || (!shift && matches!(code, K::Left | K::Right))) {
            if ch == Some('t') {
                self.period = (self.period + 1) % PERIODS.len();
            } else {
                self.stat_cat = (self.stat_cat + if code == K::Right { 1 } else { CATS.len() - 1 }) % CATS.len();
            }
            self.sel = 0;
            self.load_stats();
            return;
        }
        let in_queue = self.tab == Tab::Queue;
        if in_queue && matches!(ch, Some('J' | 'K')) {
            if let Some(uid) = self.cur().map(|t| t.uid) {
                self.move_in_queue(uid, if ch == Some('J') { 1 } else { -1 });
            }
            return;
        }
        let sel = self.sel as i64;
        match name {
            K::Up | K::Char('k') => self.move_sel(sel - 1),
            K::Down | K::Char('j') => self.move_sel(sel + 1),
            K::PageUp => self.move_sel(sel - 10),
            K::PageDown => self.move_sel(sel + 10),
            K::Home | K::Char('g') => self.move_sel(0),
            K::End => self.move_sel(n - 1),
            K::Enter | K::Char(' ') => self.press(),
            K::Tab => self.cycle_tab(1),
            K::BackTab => self.cycle_tab(-1),
            K::Esc | K::Backspace | K::Char('h') => self.escape(),
            K::Char('n') => self.cmd(json!(["playlist-next"])),
            K::Char('p') => self.cmd(json!(["playlist-prev"])),
            // plain arrows never move the song
            K::Left if shift => self.cmd(json!(["seek", -10])),
            K::Right if shift => self.cmd(json!(["seek", 10])),
            K::Left if self.tab == Tab::Home => self.focus_home(PaneId::Songs),
            K::Right if self.tab == Tab::Home => self.focus_home(PaneId::Artists),
            K::Char('l') => self.rate(true, None),
            K::Char('x') => self.rate(false, None),
            K::Char('a') => match self.cur().filter(|t| t.kind == Kind::Track).cloned() {
                Some(t) => self.run(Action::Enqueue(t)),
                None => self.set_note("Select a track to queue".into()),
            },
            K::Char('d') => {
                if let Some(uid) = self.cur().map(|t| t.uid).filter(|_| in_queue) {
                    self.remove_from_queue(uid);
                }
            }
            K::Char('m') => self.open_menu(),
            K::Char('v') => {
                self.full = !self.full || self.trip_on;
                self.trip_on = false;
                self.lyrics = false;
            }
            K::Char('y') => self.toggle_lyrics(),
            K::Char('w') => {
                self.wave = !self.wave;
                self.db.set_setting("wave", if self.wave { "on" } else { "off" });
                self.apply_wave();
                self.set_note(format!("Visualizer {}", if self.wave { "on" } else { "off" }));
            }
            K::Char('q') => self.quit = true,
            K::Char('/') => self.start_typing(), // the only key that searches
            K::Char('?') => self.help = true,
            K::Char('+' | '=') => self.cmd(json!(["add", "volume", 5])),
            K::Char('-') => self.cmd(json!(["add", "volume", -5])),
            _ => {}
        }
    }
}
