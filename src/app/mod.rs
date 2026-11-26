// Application state (the App struct) and the small types around it. What App does is split by topic into the files
// next to this one; drawing is in render/. One thread owns the state; slow work runs on its own thread and answers
// with a message (Msg).
use crate::i18n::tr;
use crate::cover::{self, Got};
use crate::charts;
use crate::config;
use crate::db::{now_ms, Db};
use crate::item::{Item, Kind, Origin, Spec};
use crate::lyrics::{self, Lyrics};
use crate::mixes;
use crate::mpv::Mpv;
use crate::playlist::{self, Link};
use crate::recommend;
use crate::scenes::Show;
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

// App is split by what it does; each file adds its part of `impl App`.
mod lists;
mod player;
mod radio;
mod pages;
mod playlists;
mod navigation;
mod media;
mod clock;
mod input;
mod equalizer;
mod data;
mod albums;
mod search;
mod extras;
mod filter;
mod library;
pub(crate) mod downloads;
pub use downloads::Downloads;
pub use filter::Filter;
pub use extras::{Sleep, Undo};
pub use search::{Found, KINDS};
pub use equalizer::EqView;
#[cfg(test)]
pub use data::WIPE_WORD;

pub const CATS: [&str; 3] = ["songs", "artists", "albums"];
pub const PERIODS: [(&str, i64); 3] = [("week", 7), ("month", 30), ("year", 365)]; // rolling windows ending now
pub const TABS: [Tab; 8] = [Tab::Home, Tab::Playlists, Tab::Liked, Tab::Library, Tab::Queue, Tab::History, Tab::Stats, Tab::Config];
const WHEEL: Duration = Duration::from_millis(40);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Home,
    Playlists,
    Liked,
    Library,
    Queue,
    History,
    Stats,
    Config,
    Search,
}

pub type Apply = Box<dyn FnOnce(&mut App) + Send>;
pub enum Msg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
    Mpv(Value),
    Quit,
    OutputChanged, // the sound now goes to another device
    Resolved { id: String, uid: u64, url: Option<String> },
    Apply(Apply),
}

#[derive(Clone)]
pub enum Action {
    Data(String), // export, import or wipe (Config > Data)
    OpenAlbum(Item),
    OpenArtist(Item),
    GoArtist(String),
    GoAlbum(Item),
    Discover,
    PlayMix(Spec, String),
    Resume,
    Jump(u64),
    PlayFrom(usize),
    PlayAll(bool), // every song of the list, shuffled or not
    PlayNext(Item),
    Enqueue(Item),
    Rate(bool, Item),
    LikeAlbum(Item),
    SearchTab(usize),
    UnlikeAlbum(String, Vec<Item>), // its title and songs
    Remove(u64),
    Radio(Item),
    OpenPlaylist(i64, String),
    PlayPlaylist(i64),
    PickPlaylist(Vec<Item>),
    AddTo(i64, String, Vec<Item>),
    NewPlaylist(Vec<Item>),
    RenamePlaylist(i64, String),
    AskDelete(i64, String),
    DeletePlaylist(i64),
    RemoveEntry(u64),
    Nothing,
    DeleteEq(String),
    ResetEq(String),
    OpenEq,
    Sleep(Option<Sleep>),
    Press(char),
    Choose(String, &'static str), // a setting's choice, clicked
    GoTab(Tab),
    Unfollow(String),
    DropAlbum(String),
    DropRelease(String),
    Download(Vec<Item>),
    AskDeleteDownload(Item),
    DeleteDownload(String),
    FreeSpace(Vec<String>),
    ConfirmFreeSpace(Vec<String>), // 90 % or more: asks for a typed phrase first
    OpenDownloads,
    PickDownloads(usize),
    DownloadLiked,
    DownloadFolder,
    ClearQueue(bool), // all but what plays, or only the radio's songs
    PickEq(usize),
    ThisIs(Item),
    Search(String),
}

// What the text field is naming, when it is not searching.
pub enum Naming {
    New(Vec<Item>),
    Rename(i64),
    EqPreset,
    Filter,
    Folder, // where downloads go
    FreeCut, // the share of downloads Free up space deletes
    FreeAll(Vec<String>), // the phrase that confirms freeing 90 % or more: those downloads
    Export, // the file to write a copy of your data to
    Import, // the file to read your data back from
    Wipe,   // the word that confirms deleting everything
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
    pub head: Option<Item>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PaneId {
    Left,
    Right,
    List,
    Cover,
}
pub struct Pane {
    pub id: PaneId,
    pub x0: usize,
    pub x1: usize,
    pub rows: Option<Vec<usize>>, // list index of each body row, when they are not consecutive (Home)
    pub top: i64,  // list index shown on the first row of the body
    pub head: usize, // fixed rows above the list
}
// Where things are on screen, for the mouse (1-indexed).
#[derive(Default)]
pub struct Geo {
    pub tabs: Vec<(usize, usize, usize)>,
    pub top: usize, // first screen row inside the body frame (below the tab bar, the search box and the frame border)
    pub field_x1: usize,
    pub field_y1: usize, // last screen row of the search field
    pub bar_row: usize,
    pub bar_x0: usize,
    pub bar_w: usize,
    pub now_row: usize,
    pub menu_top: usize, // first action shown, when they do not all fit
    pub thumb: Option<(usize, usize, usize, usize)>,
    pub controls: Vec<(Control, usize, usize, usize, usize)>, // the player's buttons: (which, x0, x1, y0, y1), boxes included
    pub panes: Vec<Pane>,
    pub links: Vec<(Action, usize, usize, usize)>, // names in the players that open their page: (it, x0, x1, y)
    pub body: usize,
    pub window: Option<(usize, usize, usize, usize)>, // the equalizer's window: (x0, x1, y0, y1)
}

// The player's buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    Prev,
    PlayPause,
    Next,
    Repeat,
}

// Repeat: off, the whole queue (the album or playlist that plays), or the one song.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
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
    pub endless: bool,
}

// A queue entry as kept in the settings table.
fn item_json(t: &Item) -> Value {
    let from = match t.origin {
        Origin::Added => Some("added"),
        Origin::Radio => Some("radio"),
        Origin::List => None,
    };
    json!({"id": t.id, "title": t.title, "artist": t.artist, "album": t.album, "cover": (!t.cover.is_empty()).then_some(&t.cover), "from": from})
}
fn item_from(t: &Value) -> Item {
    let text = |k: &str| t[k].as_str().unwrap_or("").to_string();
    let origin = match t["from"].as_str() {
        Some("added") => Origin::Added,
        Some("radio") => Origin::Radio,
        _ => Origin::List,
    };
    Item { id: text("id"), title: text("title"), artist: text("artist"), album: text("album"), cover: text("cover"), origin, ..Default::default() }
}

// What `Q` leaves behind for the next run: where the player is, and the queue as it holds it.
pub struct Detached {
    pub sock: String,
    pub viz: String,
    queue: Vec<Item>,
    endless: bool,
    counting: Option<(i64, f64, f64)>, // the play being timed: its row, seconds heard, song length
}
impl Detached {
    pub fn read(saved: &str) -> Option<Detached> {
        let v: Value = serde_json::from_str(saved).ok()?;
        let queue = v["queue"].as_array()?.iter().map(item_from).collect();
        let c = &v["counting"];
        let counting = c["id"].as_i64().map(|id| (id, c["heard"].as_f64().unwrap_or(0.0), c["duration"].as_f64().unwrap_or(0.0)));
        Some(Detached { sock: v["sock"].as_str()?.into(), viz: v["viz"].as_str()?.into(), queue, endless: v["endless"].as_bool().unwrap_or(false), counting })
    }
}

fn load_session(db: &Db) -> Option<Session> {
    let v: Value = serde_json::from_str(&db.get_setting("session")?).ok()?;
    let pos = v["pos"].as_u64()? as usize;
    let queue: Vec<Item> = v["queue"].as_array()?.iter().skip(pos).map(item_from).collect();
    (!queue.is_empty()).then(|| Session { queue, time: v["time"].as_i64().unwrap_or(0), endless: v["endless"].as_bool().unwrap_or(false) })
}

pub fn fmt(seconds: f64) -> String {
    let s = seconds.max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}
pub fn minutes(s: i64) -> String {
    if s > 0 && s < 60 {
        tr!("<1 min").into()
    } else {
        tr!("{a} min", a = (s as f64 / 60.0).round())
    }
}

fn grouped(groups: Vec<(&str, Vec<Item>)>) -> Vec<Item> {
    groups
        .into_iter()
        .filter(|g| !g.1.is_empty())
        .flat_map(|(title, items)| std::iter::once(Item::header(title)).chain(items))
        .collect()
}

// Where a song you add goes: right after the playing one when it is to play next, else after the ones you added before.
// Either way ahead of the rest of the list and of the radio.
fn add_position(queue: &[Item], pos: i64, next: bool) -> usize {
    let Ok(pos) = usize::try_from(pos) else { return queue.len() };
    let first = (pos + 1).min(queue.len());
    if next {
        return first;
    }
    queue[first..].iter().rposition(|t| t.origin == Origin::Added).map_or(first, |i| first + i + 1)
}
// What you added and has not played yet goes along when something else starts: right after its first song.
fn with_added(mut tracks: Vec<Item>, waiting: &[Item]) -> Vec<Item> {
    let first = tracks.first().map(|t| t.id.clone()).unwrap_or_default();
    let added: Vec<Item> = waiting.iter().filter(|t| t.origin == Origin::Added && t.id != first).cloned().collect();
    let at = tracks.len().min(1);
    tracks.splice(at..at, added);
    tracks
}
// The radio's songs that are not in the queue already (also not as another release of the same song) nor disliked.
fn fresh_songs(queue: &[Item], found: Vec<Item>, disliked: &HashSet<String>) -> Vec<Item> {
    let key = |t: &Item| format!("{}|{}", t.title.to_lowercase(), t.main_artist().to_lowercase());
    let mut ids: HashSet<String> = queue.iter().map(|t| t.id.clone()).collect();
    let mut names: HashSet<String> = queue.iter().map(key).collect();
    found
        .into_iter()
        .filter(|t| !disliked.contains(&t.id) && ids.insert(t.id.clone()) && names.insert(key(t)))
        .map(|t| Item { origin: Origin::Radio, ..t })
        .collect()
}

// Up to 50 songs the artist is on, in the order given, without second versions (live, remix…) of the same one.
fn essentials(found: Vec<Item>, artist: &str) -> Vec<Item> {
    let wanted = artist.to_lowercase();
    let mut seen = HashSet::new();
    found.into_iter().filter(|t| t.artist.to_lowercase().contains(&wanted) && seen.insert(playlist::plain(&t.title))).take(50).collect()
}

// Removes the last word, and what separates it from the cursor; punctuation separates, so a link goes piece by piece.
fn erase_word(s: &mut String) {
    let words = s.trim_end_matches(|c: char| !c.is_alphanumeric());
    let keep = words.trim_end_matches(char::is_alphanumeric).len();
    s.truncate(keep);
}

fn track_url(t: &Item) -> String {
    format!("https://music.youtube.com/watch?v={}", t.id)
}

fn mix_row(id: &str, title: String, detail: String, spec: Spec) -> Item {
    Item { kind: Kind::Mix, id: id.into(), title, detail: Some(detail), spec: Some(spec), ..Default::default() }
}

pub struct App {
    // connections: the database, YouTube Music, the player, and the way back to the main loop
    pub db: Db,
    pub db_path: PathBuf,
    pub yt: Yt,
    pub tx: Sender<Msg>, // background threads answer through this
    pub mpv: Mpv,
    pub feed: Arc<Mutex<viz::Feed>>, // the visualizer's measurements, filled from mpv's pipe
    pub quit: bool,
    pub clear_screen: bool,

    // what is on screen: the tab, the search field, messages, and the views that cover the lists
    pub tab: Tab,
    pub prev_tab: Tab, // the tab under the search results
    pub typing: bool,  // the search (or name) field has the keys
    pub buf: String,   // what is typed in it
    history_at: Option<usize>,
    history: Vec<String>,
    draft: String,
    pub msg: String, // an error, in red in the pane title
    pub note: String, // a short confirmation ("Liked: …"), gone after 3 s
    note_until: Option<Instant>,
    pub busy: String, // what is loading, with a spinner
    pub theme: crate::theme::Theme, // the accent color and its change from one song to the next
    pub help: bool,
    pub eq: EqView, // the EQ tab
    pub detach: bool, // quit leaving the music playing
    pub restart: bool, // and start again at once: the way to pick up a new build
    pub help_scroll: usize,
    pub full: bool, // the full player (v)
    pub menu: Option<Menu>, // the actions menu (m)
    pub lyrics: bool, // the lyrics view (y), or karaoke (Y) when `karaoke` is on too
    pub lyric_scroll: usize,
    pub karaoke: bool,                    // the lyrics as karaoke: the line being sung big, the ones around it small
    pub lyric_scrolled: Option<Instant>,  // when you last scrolled synced lyrics: they follow the song again 4 s later
    pub lyric_line: Option<usize>,
    pub karaoke_style: crate::bigtext::Style, // its big letters (f while it is open)
    pub lyric_dots: Option<usize>, // karaoke's countdown dots filled on screen (to know when to draw again)

    // results view: current list and the stack of lists to go back to
    pub items: Vec<Item>,
    pub title: String,
    pub numbered: bool,
    pub back: Vec<Saved>,
    pub head: Option<Item>, // the album whose page is open: its cover and details beside its songs
    pub query: String,
    list_id: u64,
    next_id: u64,

    // home tab: two lists, one focused at a time (sel belongs to the focused one)
    pub home_left: Vec<Item>,
    pub home_right: Vec<Item>,
    pub home_titles: [String; 2],
    pub home_wide: bool, // two columns; set by the drawing from the terminal's width
    pub home_focus: PaneId,
    pub home_sel: [usize; 2],

    pub liked: Vec<Item>,
    pub liked_by_album: bool, // the Liked tab grouped by album (← →)
    pub sel: usize,
    search_sel: usize,

    pub stat_rows: Vec<Item>,
    pub timeline: (Vec<i64>, Vec<String>), // seconds per day (or month) of the period, and their labels
    pub hours: Vec<i64>,                   // seconds per hour of day in the period
    pub totals: [i64; 3],
    pub stat_cat: usize,
    pub period: usize,
    stats_at: Instant,
    heard: f64,
    raw_pos: Option<f64>,
    play_dur: f64,

    pub naming: Option<Naming>,
    pub playlists: Vec<Item>, // the Playlists tab: "new", then every playlist
    playlists_sel: usize,
    playlist_views: HashMap<u64, i64>, // list_id of an open list -> the playlist it shows
    importing: bool,

    pub queue: Vec<Item>,
    pub pos: i64,
    pub paused: bool,
    sounding: Option<Item>, // the song mpv has loaded, for the hooks (the queue may already be another)
    pub repeat: Repeat, // as mpv reports it, so it is right after re-attaching too
    pub loading: bool,
    endless: bool,        // keep adding similar songs as the queue runs out
    generation: u64,      // changes with every new queue, so late answers are dropped
    filling: Option<u64>, // the generation similar songs are being fetched for
    removed: HashSet<String>, // taken out of this queue: the radio does not bring them back
    result_lists: HashSet<u64>, // list_id of search results: playing there starts a radio
    found: Option<Found>,       // the last search, by tab
    sleep: Option<Sleep>,       // the sleep timer (z)
    undo: Vec<Undo>,            // what u takes back
    pub filter: Option<Filter>, // f: the list narrowed
    palette: Option<String>,    // : what has been typed to find a command
    pub played: Vec<Item>,      // the History tab
    pub library: Vec<Item>,     // the Library tab
    pub releases: usize,        // new releases not opened yet
    pub downloads: Downloads,   // songs kept as files
    pub time: i64,
    pub dur: f64,
    pub vol: i64,
    play_id: Option<i64>,
    direct: HashSet<String>,
    streams: HashMap<u64, String>, // what yt-dlp resolved, by queue entry: the crossfade's end plays it
    resolving: HashSet<String>,
    pub session: Option<Session>,
    pub likes: HashSet<String>,
    pub dislikes: HashSet<String>,

    pub wave: bool,
    pub accent: Option<crate::ui::Rgb>, // a fixed accent color (Config); None takes it from the cover
    pub player_top: bool,
    pub viz_style: usize, // how the player's spectrum is drawn: dots, bars, line, mirror    // the dock above the lists instead of under them
    fade: f64,            // crossfade seconds, 0 = off
    tail: Option<Mpv>,    // the crossfade's second player, waiting on this song's end
    fading: Option<Mpv>,  // the one playing the last song's end
    crossing: bool,       // the next start-file is the crossfade's
    faded: bool,          // the fade-in filter is in
    format: &'static str, // the yt-dlp / mpv audio format
    pub settings: Vec<Item>, // the Config tab
    apos: Option<(f64, Instant)>,
    levels: viz::Levels,
    pub bands: [f64; BANDS],
    pub energy: f64, // how loud it really is, for the trip visualizer
    pub trip_on: bool,
    pub trip: Show,
    pub trip_lyrics: bool, // the sung line over the trip (y while it is open)
    pub trip_t: f64,
    trip_at: Instant,

    pub covers: HashMap<String, Option<Arc<Got>>>, // None: loading, or failed (never retried this session)
    pub rasters: HashMap<(String, usize, usize), Option<Vec<String>>>,
    pub kitty: bool,
    pub shown: String,
    pub shown_head: String, // the album page's real cover, image 2 (the player's is image 1)
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
        let liked_by_album = db.get_setting("liked_view").as_deref() == Some("albums");
        let trip_lyrics = db.get_setting("trip_lyrics").as_deref() == Some("on");
        let karaoke_style = db.get_setting("karaoke_style").and_then(|v| crate::bigtext::STYLES.iter().find(|s| s.1 == v)).map_or(crate::bigtext::Style::Dots, |s| s.0);
        App {
            db, db_path, yt: Yt::new(), tx, mpv, feed, quit: false, clear_screen: false,
            tab: Tab::Home, prev_tab: Tab::Home, typing: false, buf: String::new(), history_at: None, history: vec![], draft: String::new(),
            msg: String::new(), note: String::new(), note_until: None, busy: String::new(), help: false, eq: EqView::default(), detach: false, restart: false, help_scroll: 0, full: false, menu: None,
            lyrics: false, lyric_scroll: 0, karaoke: false, lyric_scrolled: None, lyric_line: None, lyric_dots: None, karaoke_style,
            items: vec![], title: String::new(), numbered: false, back: vec![], head: None, query: String::new(), list_id: 0, next_id: 1,
            home_left: vec![], home_right: vec![], home_titles: Default::default(), home_wide: true, home_focus: PaneId::Left, home_sel: [0, 0],
            liked: vec![], liked_by_album, sel: 0, search_sel: 0,
            stat_rows: vec![], timeline: Default::default(), hours: vec![], totals: [0; 3], stat_cat: 0, period: 0, stats_at: now, heard: 0.0, raw_pos: None, play_dur: 0.0,
            endless: false, generation: 0, filling: None, removed: HashSet::new(), result_lists: HashSet::new(), found: None, sleep: None, undo: vec![], filter: None, palette: None, played: vec![], library: vec![], releases: 0, downloads: Downloads::default(),
            naming: None, playlists: vec![], playlists_sel: 0, playlist_views: HashMap::new(), importing: false,
            queue: vec![], pos: -1, paused: false, sounding: None, repeat: Repeat::Off, loading: false, time: 0, dur: 0.0, vol, play_id: None,
            direct: HashSet::new(), streams: HashMap::new(), resolving: HashSet::new(), session, likes, dislikes,
            wave, accent: None, player_top: false, viz_style: 0, fade: 0.0, tail: None, fading: None, crossing: false, faded: false, format: config::format("best"), settings: vec![], apos: None, levels: Default::default(), bands: [0.0; BANDS], energy: 0.0, trip_on: false, trip: Show::default(), trip_lyrics, trip_t: 0.0, trip_at: now,
            covers: HashMap::new(), rasters: HashMap::new(), kitty: cover::supported(), shown: String::new(), shown_head: String::new(), want: String::new(), want_at: now,
            redraw_at: None, theme: Default::default(), lyric_cache: HashMap::new(),
            mq: Default::default(), geo: Default::default(), drag: false, last_click: None, last_seek: now, last_wheel: now,
        }
    }

    pub fn start(&mut self) {
        // playlist-pos first: it clears the time and length, which the others then fill (it matters when re-attaching)
        for (i, p) in ["playlist-pos", "time-pos", "duration", "pause", "volume", "audio-pts", "loop-file", "loop-playlist"].iter().enumerate() {
            self.mpv.cmd(json!(["observe_property", i + 1, p]));
        }
        for s in &config::SETTINGS {
            let v = config::value(&self.db, s.key);
            self.apply_setting(s.key, v);
        }
        self.start_eq();
        self.load_downloads(); // your own music is read by apply_setting("library") above
        self.load_home();
        self.load_library();
        self.check_releases(false);
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn now(&self) -> Option<&Item> {
        usize::try_from(self.pos).ok().and_then(|p| self.queue.get(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_you_add_goes_first() {
        let song = |id: &str, origin| Item { id: id.into(), title: id.into(), origin, ..Default::default() };
        let ids = |v: &[Item]| v.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        let queue = [song("now", Origin::List), song("mine", Origin::Added), song("radio", Origin::Radio)];
        assert_eq!(add_position(&queue, 0, false), 2); // after what was added before, ahead of the radio
        assert_eq!(add_position(&queue, 0, true), 1);
        assert_eq!(add_position(&queue[..1], 0, false), 1);
        assert_eq!(add_position(&queue, -1, false), 3);
        let new = with_added(vec![song("a", Origin::List), song("b", Origin::List)], &queue[1..]);
        assert_eq!(ids(&new), ["a", "mine", "b"]); // the radio is dropped, yours stays
        assert_eq!(ids(&with_added(vec![song("mine", Origin::List)], &queue[1..])), ["mine"]);
        let found = vec![song("now", Origin::List), song("x", Origin::List), Item { id: "x2".into(), ..song("x", Origin::List) }, song("bad", Origin::List), song("y", Origin::List)];
        let fresh = fresh_songs(&queue, found, &HashSet::from(["bad".to_string()]));
        assert_eq!(ids(&fresh), ["x", "y"]);
        assert!(fresh.iter().all(|t| t.origin == Origin::Radio));
    }

    #[test]
    fn erasing_by_words() {
        let mut s = String::from("daft punk  ");
        super::erase_word(&mut s);
        assert_eq!(s, "daft ");
        super::erase_word(&mut s);
        super::erase_word(&mut s); // nothing left to erase
        assert_eq!(s, "");
        let mut link = String::from("a.com/playlist?list=canción");
        super::erase_word(&mut link);
        assert_eq!(link, "a.com/playlist?list=");
    }
}
