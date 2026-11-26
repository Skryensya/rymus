// Behaviour tests: the app is driven the way a person drives it (keys and mouse) and judged by what is on screen and by
// what the player is told. No network, no sound, no real mpv, a throwaway database each time.
// Each test is a promise the app has made; when one breaks on purpose, change the test with it.
use crate::app::{App, Msg, Tab};
use crate::db::Db;
use crate::item::Item;
use crate::mpv::Mpv;
use crate::ui::{visible, width};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Ui {
    app: App,
    inbox: Receiver<Msg>,
    player: Arc<Mutex<Vec<u8>>>,
    dir: PathBuf,
    size: (usize, usize),
    drawn: String, // every frame since last asked, escape codes and all
}

impl Drop for Ui {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fresh_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("rymus-flow-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn song(id: &str, title: &str) -> Item {
    Item { id: id.into(), title: title.into(), artist: "Band".into(), ..Default::default() }
}

impl Ui {
    fn new() -> Ui {
        Ui::with(|_| {})
    }
    // `seed` fills the database before the app reads it.
    fn with(seed: impl FnOnce(&Db)) -> Ui {
        let dir = fresh_dir();
        let path = dir.join("t.db");
        let db = Db::open(&path).unwrap();
        seed(&db);
        let (tx, inbox) = channel();
        let (mpv, player) = Mpv::fake();
        let mut app = App::new(db, path, tx, mpv, Arc::new(Mutex::new(Default::default())), 100);
        app.start();
        app.kitty = false; // the same drawing whatever terminal the tests run in; image tests turn it on
        let mut ui = Ui { app, inbox, player, dir, size: (100, 34), drawn: String::new() };
        ui.screen();
        ui.told();
        ui
    }

    // ----- doing -----
    fn press(&mut self, code: KeyCode, mods: KeyModifiers) {
        self.app.handle(Msg::Key(KeyEvent::new(code, mods)));
        self.screen(); // the real app draws after every key, and the mouse relies on what was drawn
    }
    fn key(&mut self, code: KeyCode) {
        self.press(code, KeyModifiers::NONE);
    }
    fn ch(&mut self, c: char) {
        let shift = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
        self.press(KeyCode::Char(c), shift);
    }
    fn text(&mut self, text: &str) {
        text.chars().for_each(|c| self.ch(c));
    }
    fn click(&mut self, x: usize, y: usize) {
        let event = MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x as u16 - 1, row: y as u16 - 1, modifiers: KeyModifiers::NONE };
        self.app.handle(Msg::Mouse(event));
        self.screen();
    }
    fn search(&mut self, text: &str) {
        self.ch('/');
        self.text(text);
        self.key(KeyCode::Enter);
        self.wait("search results", |ui| ui.app.tab == Tab::Search && !ui.app.items.is_empty());
    }
    // What mpv would report.
    fn mpv_says(&mut self, event: Value) {
        self.app.handle(Msg::Mpv(event));
        self.screen();
    }
    fn property(&mut self, name: &str, data: Value) {
        self.mpv_says(json!({"event": "property-change", "name": name, "data": data}));
    }
    // Runs whatever the background threads answer until `ready`.
    fn wait(&mut self, what: &str, ready: impl Fn(&Ui) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !ready(self) {
            assert!(Instant::now() < end, "timed out waiting for {what}\n{}", self.screen().join("\n"));
            if let Ok(message) = self.inbox.recv_timeout(Duration::from_millis(20)) {
                self.app.handle(message);
                self.screen();
            }
        }
    }

    // ----- seeing -----
    fn screen(&mut self) -> Vec<String> {
        let frame = self.app.render(self.size.0, self.size.1);
        self.drawn.push_str(&frame);
        frame.trim_start_matches("\x1b[H").split("\r\n").map(visible).collect()
    }
    fn shows(&mut self, text: &str) -> bool {
        self.screen().iter().any(|l| l.contains(text))
    }
    // (column, row) of a text on screen, counted from 1 as the mouse does
    fn at(&mut self, text: &str) -> (usize, usize) {
        let screen = self.screen();
        let row = screen.iter().position(|l| l.contains(text)).unwrap_or_else(|| panic!("{text:?} is not on screen\n{}", screen.join("\n")));
        let column = width(&screen[row][..screen[row].find(text).unwrap()]);
        (column + 1, row + 1)
    }
    // What was sent to the terminal since last asked, images included.
    fn drawn(&mut self) -> String {
        std::mem::take(&mut self.drawn)
    }
    // The commands sent to mpv since last asked.
    fn told(&mut self) -> Vec<Value> {
        let bytes = std::mem::take(&mut *self.player.lock().unwrap());
        String::from_utf8_lossy(&bytes).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).map(|v| v["command"].clone()).collect()
    }
    fn buf_starts_with(&self, text: &str) -> bool {
        self.app.buf.starts_with(text)
    }
    fn queue(&self) -> Vec<&str> {
        self.app.queue.iter().map(|t| t.title.as_str()).collect()
    }
    fn set_tab_home(&mut self) {
        self.app.set_tab(Tab::Home);
        self.screen();
    }
    // plays the first search result and waits for its radio
    fn play_a_result(&mut self) {
        self.search("band");
        self.key(KeyCode::Enter);
        self.wait("the radio", |ui| ui.app.queue.len() > 1);
        self.property("playlist-pos", json!(0));
        self.mpv_says(json!({"event": "file-loaded"}));
        self.told();
    }
}

fn loads(commands: &[Value]) -> usize {
    commands.iter().filter(|c| c[0] == "loadfile").count()
}

// ---------- the screen ----------
#[test]
fn every_view_fills_the_terminal_exactly() {
    let mut ui = Ui::with(|db| {
        db.toggle_like(&song("a", "A very long song title that does not fit in a narrow terminal at all ♫ 日本語"));
        db.create_playlist("Mine", None, &[song("a", "x")]);
    });
    for size in [(100, 34), (80, 24), (70, 22), (60, 18), (40, 12), (200, 60)] {
        ui.size = size;
        let check = |ui: &mut Ui, view: &str| {
            let screen = ui.screen();
            assert_eq!(screen.len(), size.1, "rows of {view} at {size:?}\n{}", screen.join("\n"));
            for line in &screen {
                assert_eq!(width(line), size.0, "width of {view} at {size:?}: {line:?}");
                assert!(!line.contains('\n'), "a bare line break leaves raw mode unreadable");
            }
        };
        for _ in 0..8 {
            let view = format!("{:?}", ui.app.tab);
            check(&mut ui, &view);
            ui.key(KeyCode::Tab);
        }
        for (open, view) in [('?', "help"), ('v', "full player"), ('y', "lyrics"), ('V', "trip"), ('m', "menu"), ('/', "typing")] {
            ui.ch(open);
            check(&mut ui, view);
            ui.key(KeyCode::Esc);
            ui.key(KeyCode::Esc);
        }
    }
}

// ---------- keys ----------
#[test]
fn space_only_plays_and_pauses_and_shift_space_is_enter() {
    let mut ui = Ui::new();
    ui.search("band");
    ui.ch(' ');
    assert_eq!(loads(&ui.told()), 0, "space never starts the highlighted song");
    ui.press(KeyCode::Char(' '), KeyModifiers::SHIFT);
    assert_eq!(loads(&ui.told()), 1);
    ui.property("playlist-pos", json!(0));
    ui.key(KeyCode::Down);
    ui.told();
    for view in [None, Some('?'), Some('m'), Some('v')] {
        if let Some(c) = view {
            ui.ch(c);
        }
        ui.ch(' ');
        assert_eq!(ui.told(), [json!(["cycle", "pause"])], "in {view:?}");
        if view.is_some() {
            ui.key(KeyCode::Esc);
        }
    }
    ui.ch('/');
    ui.text("a b");
    let told = ui.told().len();
    assert_eq!((ui.app.buf.as_str(), told), ("a b", 0), "while typing, space is a space");
}

#[test]
fn enter_on_the_playing_song_pauses_instead_of_restarting() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.key(KeyCode::Enter);
    assert_eq!(ui.told(), [json!(["cycle", "pause"])]);
}

#[test]
fn only_shift_arrows_seek() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.key(KeyCode::Left);
    ui.key(KeyCode::Right);
    assert!(ui.told().iter().all(|c| c[0] != "seek"));
    ui.press(KeyCode::Right, KeyModifiers::SHIFT);
    ui.press(KeyCode::Left, KeyModifiers::SHIFT);
    assert_eq!(ui.told(), [json!(["seek", 5]), json!(["seek", -5])]);
}

#[test]
fn only_the_slash_key_searches_and_the_field_erases() {
    let mut ui = Ui::new();
    ui.text("zzz");
    assert!(!ui.app.typing);
    ui.ch('/');
    ui.text("daft punk one");
    assert!(ui.shows("╭─ / Search") && ui.shows("daft punk one█"));
    ui.press(KeyCode::Backspace, KeyModifiers::ALT);
    assert_eq!(ui.app.buf, "daft punk ");
    ui.press(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(ui.app.buf, "daft ");
    ui.press(KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(ui.app.buf, "");
}

#[test]
fn the_keys_page_lists_the_keys_and_closes() {
    let mut ui = Ui::new();
    ui.ch('?');
    for text in ["PLAYBACK", "SONG", "add to queue", "save to playlist", "past searches"] {
        assert!(ui.shows(text), "{text}");
    }
    ui.ch('x');
    assert!(!ui.app.help);
}

// ---------- the queue ----------
#[test]
fn a_search_result_is_followed_by_similar_songs_not_by_the_next_result() {
    let mut ui = Ui::new();
    ui.play_a_result();
    assert_eq!(ui.queue(), ["Result One", "Similar A", "Similar B", "Similar C", "Similar D", "Similar E"]);
}

#[test]
fn what_you_add_plays_first_and_survives_playing_something_else() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.key(KeyCode::Down);
    ui.ch('a'); // Result Two
    ui.key(KeyCode::Down);
    ui.ch('a'); // Result Three
    assert_eq!(&ui.queue()[..4], ["Result One", "Result Two", "Result Three", "Similar A"]);
    assert!(ui.told().contains(&json!(["loadfile", "https://music.youtube.com/watch?v=s2", "insert-at", 1])), "mpv gets the same order");
    ui.key(KeyCode::Up);
    ui.key(KeyCode::Up);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Up); // back on Result One, which is playing
    ui.search("again");
    ui.key(KeyCode::Down); // Result Two: already waiting, so it is not queued twice
    ui.key(KeyCode::Enter);
    assert_eq!(ui.queue(), ["Result Two", "Result Three"]);
}

#[test]
fn a_playlist_plays_in_its_own_order() {
    let mut ui = Ui::with(|db| {
        db.create_playlist("Mine", None, &[song("a", "First"), song("b", "Second"), song("c", "Third")]);
    });
    ui.key(KeyCode::Tab);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter); // open it
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter); // play from the second
    assert_eq!(ui.queue(), ["Second", "Third"]);
}

// ---------- playlists ----------
#[test]
fn playlists_are_created_filled_renamed_and_deleted_with_the_keyboard() {
    let mut ui = Ui::new();
    ui.key(KeyCode::Tab);
    assert_eq!(ui.app.tab, Tab::Playlists);
    ui.key(KeyCode::Enter); // + New playlist
    ui.text("Road trip");
    ui.key(KeyCode::Enter);
    assert!(ui.shows("Road trip") && ui.shows("0 songs"));
    ui.search("band");
    ui.ch('s');
    ui.key(KeyCode::Enter); // the first playlist offered
    assert_eq!(ui.app.db.playlists()[0].detail.as_deref(), Some("1 song"));
    ui.key(KeyCode::Esc);
    ui.ch('m');
    ui.ch('3'); // Rename
    ui.press(KeyCode::Char('u'), KeyModifiers::CONTROL);
    ui.text("Chill");
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.db.playlists()[0].title, "Chill");
    ui.ch('d');
    ui.key(KeyCode::Enter); // the safe choice comes first
    assert_eq!(ui.app.db.playlists().len(), 1);
    ui.ch('d');
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert!(ui.app.db.playlists().is_empty());
}

#[test]
fn pasting_a_link_makes_the_playlist_once() {
    let mut ui = Ui::new();
    let link = "https://music.youtube.com/playlist?list=PLabc";
    for _ in 0..2 {
        ui.ch('/');
        ui.text(link);
        ui.key(KeyCode::Enter);
        ui.wait("the playlist", |ui| ui.app.tab == Tab::Search);
        assert!(ui.shows("Track One") && ui.shows("Track Two"));
        ui.key(KeyCode::Esc);
        assert_eq!(ui.app.tab, Tab::Playlists, "back lands on the list that now holds it");
        assert!(ui.shows("Road Songs"));
    }
    assert_eq!(ui.app.db.playlists().len(), 1);
}

// ---------- Home ----------
fn busy_library(db: &Db) {
    for i in 0..60 {
        let t = Item { artist: format!("Artist {}", i % 7), ..song(&format!("h{i}"), &format!("Heard {i:02}")) };
        listened(db, &t, 60 - i, 120);
        db.create_playlist(&format!("List {i:02}"), None, &[]);
    }
    db.save_search("old search");
}

#[test]
fn home_is_listen_and_library_and_every_section_stays_in_sight() {
    let mut ui = Ui::with(busy_library);
    let screen = ui.screen().join("\n");
    for text in ["Listen", "Library", "✦ Discover", "Recently played", "Heard 59", "Playlists", "List 59",
        "Top artists · this month", "Artist", "Recent searches", "old search"] {
        assert!(screen.contains(text), "{text:?} is on Home\n{screen}");
    }
    let (listen_x, _) = ui.at("Recently played");
    let (library_x, _) = ui.at("Top artists");
    assert!(library_x > listen_x, "Library is the right column");
}

#[test]
fn home_is_one_column_on_a_narrow_screen_and_keeps_every_section() {
    let mut ui = Ui::with(busy_library);
    ui.key(KeyCode::Right); // the cursor in Library…
    ui.size = (70, 30); // …when the screen gets narrow
    let screen = ui.screen().join("\n");
    assert!(!screen.contains("Library ─"), "one column, no Library frame\n{screen}");
    for text in ["✦ Discover", "Recently played", "Playlists", "Top artists · this month", "Recent searches"] {
        assert!(screen.contains(text), "{text:?} still shows\n{screen}");
    }
    assert!(ui.app.cur().is_some(), "the cursor lands on something");
    ui.key(KeyCode::Right);
    ui.key(KeyCode::Down);
    assert!(ui.app.cur().is_some());
    ui.size = (100, 34);
    assert!(ui.shows("Library"), "and two columns again when there is room");
}

#[test]
fn home_shows_what_plays_next() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.key(KeyCode::Esc); // back to Home
    assert_eq!(ui.app.tab, Tab::Home);
    assert!(ui.shows("Up next") && ui.shows("Similar A"), "{}", ui.screen().join("\n"));
}

#[test]
fn home_has_no_editor_any_more() {
    let mut ui = Ui::new();
    let before = ui.screen();
    ui.ch('e');
    assert_eq!(ui.screen(), before, "e does nothing on Home");
    assert!(!ui.shows("edit home"));
}

#[test]
fn home_starts_out_useful_with_nothing_heard() {
    let mut ui = Ui::new();
    assert!(ui.shows("✦ Discover") && ui.shows("+ New playlist"));
    assert!(!ui.shows("Recently played") && !ui.shows("Top artists"), "empty sections stay out of the way");
}

// ---------- mouse ----------
#[test]
fn clicks_land_where_things_are_drawn() {
    let mut ui = Ui::new();
    let (x, y) = ui.at("Liked");
    ui.click(x, y);
    assert_eq!(ui.app.tab, Tab::Liked);
    let (x, y) = ui.at("/ Search");
    ui.click(x, y + 1); // inside the search box
    assert!(ui.app.typing);
    ui.key(KeyCode::Esc);
    ui.search("band");
    let (x, y) = ui.at("Result Three");
    ui.click(x, y);
    assert_eq!(ui.app.cur().map(|t| t.title.as_str()), Some("Result Three"));
}

// ---------- what is remembered ----------
#[test]
fn likes_volume_and_the_session_are_kept() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.ch('l');
    assert!(ui.app.db.liked_ids().contains("s1"));
    ui.ch('x');
    assert!(ui.app.db.disliked_ids().contains("s1") && !ui.app.db.liked_ids().contains("s1"));
    ui.property("volume", json!(65.0));
    assert_eq!(ui.app.db.get_setting("volume").as_deref(), Some("65"));
    ui.app.shutdown();
    let session: Value = serde_json::from_str(&ui.app.db.get_setting("session").unwrap()).unwrap();
    assert_eq!(session["queue"][0]["id"], "s1");
}

// ---------- color ----------
#[test]
fn the_color_comes_from_the_playing_song_not_from_the_cursor() {
    use crate::cover::Got;
    let red = Item { cover: "test://red".into(), ..song("r", "Red Song") };
    let mut ui = Ui::with(move |db| {
        db.create_playlist("Mine", None, &[red, song("n", "No Cover")]);
    });
    ui.app.kitty = true; // images go to the terminal, so no ffmpeg is needed to draw them
    ui.app.covers.insert("test://red".into(), Some(Arc::new(Got { b64: String::new(), rgb: [220, 20, 20], palette: vec![], image: vec![] })));
    ui.key(KeyCode::Tab);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter); // the red song is highlighted, nothing plays
    ui.screen();
    assert_eq!(ui.app.theme.color, crate::ui::GREEN, "highlighting must not tint the app");
    ui.key(KeyCode::Enter); // now it plays
    ui.screen();
    assert_ne!(ui.app.theme.color, crate::ui::GREEN);
    let playing = ui.app.theme.color;
    ui.key(KeyCode::Down);
    ui.screen();
    assert_eq!(ui.app.theme.color, playing, "moving the cursor changes nothing");
}

// ---------- artists ----------
#[test]
fn an_artist_page_builds_this_is_when_asked() {
    let mut ui = Ui::new();
    ui.search("band");
    ui.ch('m');
    let go = ui.app.menu.as_ref().unwrap().items.iter().position(|x| x.0 == "Go to artist").unwrap();
    for _ in 0..go {
        ui.key(KeyCode::Down);
    }
    ui.key(KeyCode::Enter);
    ui.wait("the artist page", |ui| ui.app.title == "Band");
    assert!(ui.shows("✦ This is Band") && ui.shows("Popular songs"));
    assert!(!ui.shows("Deep Cut"), "nothing is built until asked");
    ui.told();
    ui.key(KeyCode::Enter); // the cursor starts on This is
    ui.wait("This is", |ui| ui.app.title == "This is Band");
    let songs: Vec<&str> = ui.app.items.iter().filter(|t| t.kind == crate::item::Kind::Track).map(|t| t.title.as_str()).collect();
    assert_eq!(songs, ["Hit", "Deep Cut", "Closer"], "theirs only, one version of each");
    assert_eq!(loads(&ui.told()), 0, "building does not start playing");
    ui.ch('m');
    assert!(ui.shows("Save as playlist…") && !ui.shows("Resume"));
    ui.key(KeyCode::Esc);
    ui.key(KeyCode::Enter); // ▶ Play
    assert_eq!(ui.queue(), ["Hit", "Deep Cut", "Closer"]);
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.title, "Band");
}

// ---------- leaving ----------
#[test]
fn escape_ends_up_on_home() {
    let mut ui = Ui::new();
    for _ in 0..3 {
        ui.key(KeyCode::Tab);
    }
    ui.search("band");
    let from = ui.app.prev_tab;
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.tab, from, "first out of the results");
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.tab, Tab::Home, "then, with nothing left, Home");
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.tab, Tab::Home);
}

#[test]
fn q_stops_the_music_and_capital_q_leaves_it_playing_to_be_found_again() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.ch('Q');
    assert!(ui.app.quit);
    ui.app.shutdown();
    let told = ui.told();
    assert!(!told.contains(&json!(["quit"])), "{told:?}");
    assert!(told.contains(&json!(["set_property", "idle", "no"])));
    assert!(told.iter().all(|c| c[0] != "af"), "touching the audio filters is heard: {told:?}");
    let left = crate::app::Detached::read(&ui.app.db.get_setting("detached").unwrap()).expect("a note for the next run");

    let mut next = Ui::new();
    next.app.reattach(left);
    assert!(next.told().contains(&json!(["script-message", crate::platform::BACK])));
    next.property("playlist-pos", json!(0));
    assert_eq!(next.queue(), ["Result One", "Similar A", "Similar B", "Similar C", "Similar D", "Similar E"]);
    assert!(next.shows("Result One") && next.app.queue[1].origin == crate::item::Origin::Radio);
    assert_eq!(loads(&next.told()), 0, "nothing is loaded again: it never stopped");
    next.ch('q');
    next.app.shutdown();
    assert!(next.told().contains(&json!(["quit"])));
}

// ---------- config ----------
fn open_config(ui: &mut Ui) {
    for _ in 0..7 {
        ui.key(KeyCode::Tab); // Home → Playlists → Liked → Library → Queue → History → Stats → Config
    }
    assert_eq!(ui.app.tab, Tab::Config);
    assert!(ui.app.config_menu, "it opens on its sections");
    assert_eq!(ui.app.cur().unwrap().id, "fade", "Sound comes first, Crossfade at its top: where these tests start");
    ui.key(KeyCode::Right); // into Sound
}

#[test]
fn the_config_tab_changes_settings_saves_them_and_tells_the_player() {
    let mut ui = Ui::new();
    open_config(&mut ui);
    for label in ["Crossfade", "Even out loudness", "Audio quality", "Pixel art covers", "Visualizer"] {
        assert!(ui.app.settings.iter().any(|r| r.title == label), "{label} is listed");
    }
    assert!(ui.shows("[off]"));
    ui.key(KeyCode::Right);
    assert!(ui.shows("[2 s]"), "→ moves to the next choice");
    assert_eq!(ui.app.db.get_setting("fade").as_deref(), Some("2 s"), "and it is saved");
    ui.key(KeyCode::Left);
    ui.key(KeyCode::Left);
    assert_eq!(ui.app.db.get_setting("fade").as_deref(), Some("12 s"), "← wraps around");
    ui.told();
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter); // Even out loudness: on
    assert!(ui.told().contains(&json!(["af", "add", crate::config::NORMALIZE])));
    ui.key(KeyCode::Enter); // and off again
    assert!(ui.told().contains(&json!(["af", "remove", "@norm"])));
    ui.key(KeyCode::Down); // past Channels and the equalizer
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Right); // Audio quality: data saver
    assert!(ui.told().contains(&json!(["set_property", "ytdl-format", "worstaudio/worst"])));
    ui.ch('W'); // the visualizer key and the Config row are the same setting
    assert_eq!(ui.app.db.get_setting("wave").as_deref(), Some("off"));
    assert_eq!(ui.app.settings.iter().find(|r| r.id == "wave").unwrap().author, "off", "and its row says so");
    ui.ch('w'); // off: w turns it back on, as it was
    assert_eq!((ui.app.db.get_setting("wave").as_deref(), crate::config::value(&ui.app.db, "vizstyle")), (Some("on"), "dots"));
    ui.ch('w'); // on: the next style
    assert_eq!((crate::config::value(&ui.app.db, "vizstyle"), ui.app.viz_style), ("bars", 1));
    assert_eq!(ui.app.settings.iter().find(|r| r.id == "vizstyle").unwrap().author, "bars", "and its row says so");
    for _ in 0..3 {
        ui.ch('w');
    }
    assert_eq!(crate::config::value(&ui.app.db, "vizstyle"), "dots", "around again after mirror");
}

#[test]
fn settings_are_applied_again_when_the_app_starts() {
    let mut ui = Ui::with(|db| {
        db.set_setting("quality", "data saver");
        db.set_setting("normalize", "on");
        db.set_setting("wave", "off");
    });
    ui.app.start(); // what the app does when it opens
    let told = ui.told();
    assert!(told.contains(&json!(["set_property", "ytdl-format", "worstaudio/worst"])), "{told:?}");
    assert!(told.contains(&json!(["af", "add", crate::config::NORMALIZE])));
    assert!(!told.contains(&json!(["af", "remove", "@viz"])), "a filter taken out is heard");
    assert!(!ui.app.wave);
}

#[test]
fn the_config_tab_shows_every_choice_and_explains_the_one_you_are_on() {
    let mut ui = Ui::new();
    ui.size = (110, 54);
    open_config(&mut ui);
    for section in ["Sound", "Look", "Screen", "Downloads", "General"] {
        assert!(ui.shows(section), "{section}\n{}", ui.screen().join("\n"));
    }
    assert!(!ui.shows("Pixel art covers"), "only the section holding the cursor is shown");
    assert!(ui.shows("[off]") && ui.shows(" 12 s "), "every choice of the one being changed is in view");
    assert!(ui.shows("The end of each song plays over"), "and what it does, right under it\n{}", ui.screen().join("\n"));
    assert!(ui.shows("no overlap") && ui.shows("○ off") && ui.shows("▸ Equalizer"), "switches and windows look like what they are");
    assert!(!ui.shows("Preview"), "the player below does not preview a setting");
    ui.key(KeyCode::Right);
    ui.key(KeyCode::Right);
    assert!(ui.shows("[4 s]") && ui.shows("4 s together"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Down);
    assert!(ui.shows("Quiet and loud songs") && !ui.shows("together") && !ui.shows(" 12 s "), "{}", ui.screen().join("\n"));
    for _ in 0..9 {
        ui.key(KeyCode::Down); // to the end of Sound, and no further
    }
    assert_eq!(ui.app.cur().unwrap().id, "outputpause", "inside a section ↑ ↓ stay in it");
    ui.key(KeyCode::Esc); // back to the sections
    assert!(ui.app.config_menu && ui.app.tab == Tab::Config);
    ui.key(KeyCode::Down);
    assert!(ui.shows("▌ Look") && ui.shows("Mode") && !ui.shows("Even out loudness"), "one key to the next section, shown at once\n{}", ui.screen().join("\n"));
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert!(!ui.app.config_menu && ui.shows("Delete all my data"), "General, at the end, in four keys\n{}", ui.screen().join("\n"));
}

#[test]
fn the_eq_window_explains_the_bands_and_moves_them_with_sliders() {
    let mut ui = Ui::new();
    ui.size = (110, 34);
    ui.ch('E');
    assert!(ui.app.eq.open && ui.app.tab == Tab::Home);
    for part in ["EQ · Flat", "BUILT IN", "+12 ┤", "  0 ┼", "-12 ┤", "bass", "mids", "treble", "16k", "The music as it was mixed"] {
        assert!(ui.shows(part), "{part}\n{}", ui.screen().join("\n"));
    }
    for _ in 0..5 {
        ui.key(KeyCode::Down); // to Rock, heard on the way
    }
    assert!(ui.shows("EQ · Rock") && ui.shows("More bass and treble"));
    ui.told();
    ui.key(KeyCode::Right); // onto the sliders, at 31 Hz
    ui.key(KeyCode::Right); // 62 Hz
    assert!(ui.shows("62 Hz · Bass · +3 dB") && ui.shows("The weight of the kick drum"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Up);
    assert!(ui.app.eq_current().0 == "Rock" && ui.app.db.get_setting("eq_presets").is_none(), "a built-in profile is never changed or copied");
    assert!(ui.told().is_empty() && ui.shows("n makes your own from it"), "{}", ui.screen().join("\n"));
    ui.ch('n'); // also from the sliders
    ui.text("Loud");
    ui.key(KeyCode::Enter);
    ui.key(KeyCode::Right);
    ui.key(KeyCode::Up);
    assert!(ui.shows("● Loud") && ui.shows("YOURS") && ui.shows("62 Hz · Bass · +4 dB"), "{}", ui.screen().join("\n"));
    assert!(ui.told().contains(&json!(["af-command", "eq", "gain", "4", "equalizer@b1"])), "and the change is heard");
    ui.key(KeyCode::Esc);
    ui.ch('0');
    assert_eq!(ui.app.eq_current().0, "Flat", "back to the default");
    assert!(ui.told().contains(&json!(["af-command", "eq", "gain", "0", "equalizer@b1"])));
    assert_eq!(crate::eq::presets(None)[5].gains[1], 3, "Rock as it came");
    ui.key(KeyCode::Tab);
    assert!(ui.app.eq.open && ui.app.tab == Tab::Home, "the window keeps its keys");
    ui.key(KeyCode::Esc);
    assert!(!ui.app.eq.open);
}

#[test]
fn the_crossfade_overlaps_the_end_of_a_song_with_the_next() {
    let mut ui = Ui::new();
    ui.play_a_result();
    let fade_in = crate::config::fade_in(4.0);
    let next = |told: &[Value]| told.contains(&json!(["playlist-next"]));
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(190.0));
    ui.property("time-pos", json!(196.1));
    assert!(!next(&ui.told()), "off: the song plays to its end");

    open_config(&mut ui);
    ui.key(KeyCode::Right);
    ui.key(KeyCode::Right); // 4 s
    ui.mpv_says(json!({"event": "start-file"}));
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(150.0));
    ui.property("time-pos", json!(196.1));
    assert!(!next(&ui.told()), "jumping past the start without the end ready: no crossfade");

    ui.mpv_says(json!({"event": "start-file"}));
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(180.0)); // the end gets ready
    ui.property("time-pos", json!(196.1));
    let told = ui.told();
    assert!(next(&told) && told.contains(&json!(["af-command", "fade", "volume", fade_in, "volume@fade"])), "{told:?}");
    ui.mpv_says(json!({"event": "start-file"}));
    assert!(ui.told().is_empty(), "the crossfade's song fades in");
    ui.mpv_says(json!({"event": "start-file"}));
    let told = ui.told();
    assert!(told.contains(&json!(["af-command", "fade", "volume", "1", "volume@fade"])), "a song you skip to comes in at once");
    assert!(!told.iter().any(|c| c[0] == "af"), "and the filters are never rebuilt for it: {told:?}");

    ui.property("duration", json!(9.0));
    ui.property("time-pos", json!(4.0));
    ui.property("time-pos", json!(5.5));
    assert!(!next(&ui.told()), "a short song is not faded");
}

#[test]
fn a_narrow_config_shows_only_the_chosen_value() {
    let mut ui = Ui::new();
    ui.size = (50, 24); // too narrow for every choice
    open_config(&mut ui);
    assert!(ui.shows("[off]") && ui.shows("›") && !ui.shows("12 s"), "the ones around the chosen one: {}", ui.screen().join("\n"));
    for _ in 0..5 {
        ui.key(KeyCode::Right);
    }
    let line = ui.screen().into_iter().find(|l| l.contains("[12 s]")).unwrap_or_default();
    assert!(line.contains('‹') && !line.contains("off"), "and ← → still change it, the line following: {}", ui.screen().join("\n"));
}

// ---------- albums ----------
fn open_album(ui: &mut Ui) {
    // a solid red cover, already loaded, so nothing is fetched
    ui.app.covers.insert("test://road".into(), Some(Arc::new(crate::cover::Got { b64: "AAAA".into(), rgb: [200, 30, 30], palette: vec![], image: vec![] })));
    ui.search("record");
    ui.key(KeyCode::Right); // the Albums tab
    assert!(ui.shows("Long Road"));
    ui.key(KeyCode::Enter);
    ui.wait("the album page", |ui| ui.app.title.starts_with("Long Road"));
}

#[test]
fn an_album_page_has_its_cover_details_buttons_and_numbered_songs() {
    let mut ui = Ui::new();
    open_album(&mut ui);
    let screen = ui.screen().join("\n");
    for text in ["▶ Play album", "⤮ Shuffle", " 1  Opening", " 2  Middle", " 3  Ending", "Long Road", "2019 · 3 songs"] {
        assert!(screen.contains(text), "{text:?} is on the album page\n{screen}");
    }
    let (list_x, _) = ui.at("Opening");
    let (details_x, details_y) = ui.at("3 songs");
    let (_, button_y) = ui.at("▶ Play album");
    assert!(details_x > ui.size.0 / 2 && details_x > list_x, "the details sit on the right");
    assert!(details_y > button_y + 3, "under the cover, not at the top");
    assert_eq!(ui.app.cur().unwrap().title, "▶ Play album", "the cursor starts on Play album");
}

#[test]
fn play_album_plays_in_order_and_shuffle_plays_them_all() {
    let mut ui = Ui::new();
    open_album(&mut ui);
    ui.key(KeyCode::Enter);
    assert_eq!(ui.queue(), ["Opening", "Middle", "Ending"]);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter); // Shuffle
    let mut q: Vec<&str> = ui.queue();
    q.sort();
    assert_eq!(q, ["Ending", "Middle", "Opening"], "every song once, in any order");
    ui.key(KeyCode::Up);
    ui.key(KeyCode::Enter); // in order again, so Opening is the one playing
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down); // past the header, onto the second song
    ui.key(KeyCode::Enter);
    assert_eq!(ui.queue(), ["Middle", "Ending"], "a song still plays from there on");
}

#[test]
fn going_back_to_an_album_brings_its_cover_back() {
    let mut ui = Ui::new();
    open_album(&mut ui);
    ui.ch('m');
    ui.key(KeyCode::Esc);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down); // Opening
    ui.ch('m');
    let go = ui.app.menu.as_ref().unwrap().items.iter().position(|x| x.0 == "Go to artist").unwrap();
    for _ in 0..go {
        ui.key(KeyCode::Down);
    }
    ui.key(KeyCode::Enter);
    ui.wait("the artist page", |ui| ui.app.title == "Band");
    assert!(!ui.shows("3 songs"), "the album's details leave with it");
    ui.key(KeyCode::Esc);
    assert!(ui.app.title.starts_with("Long Road") && ui.shows("2019 · 3 songs"));
}

#[test]
fn the_album_page_fills_the_terminal_exactly_at_any_size() {
    let mut ui = Ui::new();
    open_album(&mut ui);
    for size in [(100, 34), (80, 24), (60, 18), (40, 12), (200, 60), (30, 40)] {
        ui.size = size;
        let screen = ui.screen();
        assert_eq!(screen.len(), size.1, "rows at {size:?}");
        for line in &screen {
            assert_eq!(width(line), size.0, "width at {size:?}: {line:?}");
        }
        assert!(ui.shows("Opening"), "the songs always show at {size:?}");
    }
}

#[test]
fn a_real_album_cover_and_the_players_live_side_by_side_until_pixel_art_is_on() {
    let mut ui = Ui::new();
    ui.app.kitty = true; // a terminal that draws images
    open_album(&mut ui);
    std::thread::sleep(Duration::from_millis(250)); // the player loads its picture once the cursor settles
    ui.screen();
    let drawn = ui.drawn();
    assert!(drawn.contains("i=2,"), "the album cover is image 2");
    assert!(drawn.contains("i=1,"), "the player's is image 1, at the same time");
    assert!(!drawn.contains("d=A"), "changing one image never wipes them all");
    let line = ui.screen().into_iter().find(|l| l.contains("3 songs")).unwrap();
    assert!(!line.contains('▀'), "a real image leaves its cells blank");
    ui.drawn();
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down); // a song: the player's preview may change…
    let drawn = ui.drawn();
    assert!(!drawn.contains("i=2"), "…and the album cover is left as it is");
    ui.key(KeyCode::Esc);
    ui.key(KeyCode::Esc);
    ui.set_tab_home();
    open_config(&mut ui);
    ui.key(KeyCode::Esc);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down); // Screen
    ui.key(KeyCode::Right);
    assert_eq!(ui.app.cur().unwrap().id, "pixel");
    ui.drawn();
    ui.key(KeyCode::Right); // Pixel art covers: on
    assert!(!ui.app.kitty, "every image is drawn with blocks now");
    assert_eq!(ui.app.db.get_setting("pixel").as_deref(), Some("on"));
    ui.key(KeyCode::Left); // and off: in a terminal that can show images, they come back
    assert_eq!(ui.app.kitty, crate::cover::supported());
}

#[test]
fn leaving_the_album_takes_its_real_cover_off_the_screen() {
    let mut ui = Ui::new();
    ui.app.kitty = true;
    open_album(&mut ui);
    ui.drawn();
    ui.key(KeyCode::Esc);
    assert!(ui.drawn().contains("d=I,i=2"));
}

// ---------- stats ----------
fn listened(db: &Db, t: &Item, minutes_ago: i64, seconds: i64) {
    let id = db.start_play(t, crate::db::now_ms() - minutes_ago * 60_000);
    db.end_play(id, seconds, None, false);
}

#[test]
fn stats_rank_by_minutes_only_and_never_count_plays() {
    let mut ui = Ui::with(|db| {
        let long = Item { artist: "Slow".into(), ..song("l", "Long One") };
        let short = Item { artist: "Fast".into(), ..song("s", "Short One") };
        listened(db, &long, 10, 600); // once, 10 min
        for ago in 20..25 {
            listened(db, &short, ago, 60); // five times, 5 min
        }
    });
    for _ in 0..6 {
        ui.key(KeyCode::Tab);
    }
    assert_eq!(ui.app.tab, Tab::Stats);
    let (_, long_y) = ui.at("Long One");
    let (_, short_y) = ui.at("Short One");
    assert!(long_y < short_y, "more minutes ranks first, however many plays");
    let screen = ui.screen().join("\n");
    assert!(screen.contains("10 min") && screen.contains("5 min"));
    assert!(!screen.contains('×') && !screen.contains(" plays"), "no play counts\n{screen}");
    ui.key(KeyCode::Right); // artists
    let (_, slow_y) = ui.at("Slow");
    let (_, fast_y) = ui.at("Fast");
    assert!(slow_y < fast_y);
    assert!(!ui.shows(" plays"));
}

#[test]
fn stats_show_charts_when_there_is_room() {
    let mut ui = Ui::with(|db| listened(db, &song("a", "A"), 5, 300));
    for _ in 0..6 {
        ui.key(KeyCode::Tab);
    }
    assert!(ui.shows("Per day") && ui.shows("By hour of day"));
    assert!(ui.screen().iter().any(|l| l.contains('█') || l.contains('▁')), "bars are drawn");
    ui.ch('t');
    ui.ch('t'); // year
    assert!(ui.shows("Per month"));
    ui.size = (70, 34);
    assert!(ui.shows("Per month") && !ui.shows("By hour of day"), "too narrow for both charts");
    ui.size = (100, 18);
    assert!(!ui.shows("Per month") && ui.shows("A"), "too short: the list keeps the room");
    for size in [(100, 34), (70, 34), (80, 24)] {
        ui.size = size;
        for line in ui.screen() {
            assert_eq!(width(&line), size.0, "width at {size:?}: {line:?}");
        }
    }
}

#[test]
fn stats_charts_hide_when_nothing_was_heard() {
    let mut ui = Ui::new();
    for _ in 0..4 {
        ui.key(KeyCode::Tab);
    }
    assert!(!ui.shows("Per day"));
}

// ---------- audio output ----------
#[test]
fn a_new_audio_output_pauses_unless_turned_off() {
    let mut ui = Ui::new();
    ui.app.handle(Msg::OutputChanged);
    assert!(ui.told().is_empty(), "nothing playing, nothing to pause");
    ui.play_a_result();
    ui.app.handle(Msg::OutputChanged);
    assert_eq!(ui.told(), [json!(["set_property", "pause", true])]);
    assert!(ui.shows("the audio output changed"));
    ui.property("pause", json!(true));
    ui.app.handle(Msg::OutputChanged);
    assert!(ui.told().is_empty(), "a second change never starts it again");
    ui.property("pause", json!(false));
    ui.app.db.set_setting("outputpause", "off");
    ui.app.handle(Msg::OutputChanged);
    assert!(ui.told().is_empty());
}

// ---------- searching comes first ----------
#[test]
fn a_search_is_seen_from_anywhere() {
    for open in [Some('v'), Some('V'), Some('y'), Some('?'), Some('m'), None] {
        let mut ui = Ui::new();
        if let Some(c) = open {
            ui.ch(c);
        }
        ui.search("band");
        assert!(ui.shows("Result One"), "results hidden after {open:?}\n{}", ui.screen().join("\n"));
    }
    // opened while waiting for the answer
    let mut ui = Ui::new();
    ui.ch('/');
    ui.text("band");
    ui.key(KeyCode::Enter);
    ui.ch('v');
    ui.wait("results", |ui| ui.app.tab == Tab::Search && !ui.app.items.is_empty());
    assert!(!ui.app.full && ui.shows("Result One"));
}

// ---------- lyrics ----------
fn braille(c: char) -> bool {
    ('\u{2801}'..='\u{28ff}').contains(&c)
}

// A song playing at `at` seconds, with 40 synced lines "Line 00".."Line 39", one every 5 s.
fn singing(at: f64) -> Ui {
    let mut ui = Ui::new();
    ui.play_a_result();
    let lines = (0..40).map(|i| (i as f64 * 5.0, format!("Line {i:02}"))).collect();
    let id = ui.app.now().unwrap().id.clone();
    ui.app.lyric_cache.insert(id, Some(crate::lyrics::Lyrics::Synced(lines))); // as if LRCLIB had answered
    ui.property("duration", json!(200.0));
    ui.property("audio-pts", json!(at));
    ui
}

#[test]
fn the_words_follow_the_song_scroll_by_hand_and_come_back() {
    let mut ui = singing(100.0);
    ui.ch('y');
    assert!(ui.shows("Lyrics ·") && ui.shows("Line 20"), "{}", ui.screen().join("\n"));
    assert!(!ui.shows("Line 39"), "the end is not in sight yet");
    for _ in 0..3 {
        ui.key(KeyCode::PageDown);
    }
    assert!(ui.shows("Line 39"), "scrolling moves the words\n{}", ui.screen().join("\n"));
    ui.property("audio-pts", json!(105.0)); // the song moves on while you read: you stay where you are
    assert!(ui.shows("Line 39"));
    ui.app.lyric_scrolled = Some(Instant::now() - Duration::from_secs(5)); // 4 s without scrolling
    assert!(ui.app.tick(), "the screen is drawn again");
    assert!(ui.shows("Line 21") && !ui.shows("Line 39"), "back to the line being sung\n{}", ui.screen().join("\n"));
    ui.key(KeyCode::Home);
    assert!(ui.shows("Line 00"));
    ui.ch('y');
    assert!(!ui.app.lyrics, "y closes them");
}

#[test]
fn karaoke_shows_the_line_being_sung_big_and_its_neighbours_small() {
    let mut ui = singing(100.0);
    ui.ch('Y');
    let screen = ui.screen().join("\n");
    assert!(screen.contains("Karaoke ·"), "{screen}");
    assert!(screen.contains("Line 19") && screen.contains("Line 21"), "the line before and the line after, small\n{screen}");
    assert!(!screen.contains("Line 20"), "the line being sung is not small text…\n{screen}");
    assert!(screen.chars().any(braille), "…it is drawn in big braille letters\n{screen}");
    let (_, before) = ui.at("Line 19");
    let (_, after) = ui.at("Line 21");
    assert!(after > before + 4, "the big line sits between them");
    ui.property("audio-pts", json!(105.0));
    assert!(ui.shows("Line 20") && ui.shows("Line 22") && !ui.shows("Line 21"), "it moves with the song");
    ui.key(KeyCode::PageDown); // karaoke always follows
    assert!(ui.shows("Line 22"));
    ui.ch('y');
    assert!(ui.app.lyrics && !ui.app.karaoke && ui.shows("Lyrics ·"), "y switches to the words");
    ui.ch('Y');
    ui.ch('Y');
    assert!(!ui.app.lyrics, "Y again closes karaoke");
}

#[test]
fn karaoke_before_the_first_line_and_with_letters_it_cannot_draw_big() {
    let mut ui = singing(1.0); // on Line 00, with Line 01 coming next
    ui.ch('Y');
    assert!(ui.shows("Line 01"));
    let id = ui.app.now().unwrap().id.clone();
    ui.app.lyric_cache.insert(id.clone(), Some(crate::lyrics::Lyrics::Synced(vec![(50.0, "夜に駆ける".into()), (60.0, "next".into())])));
    ui.property("audio-pts", json!(10.0));
    assert!(ui.shows("next") && ui.screen().iter().any(|l| l.chars().any(braille)), "a note while waiting for the first line");
    ui.property("audio-pts", json!(51.0));
    assert!(ui.shows("夜に駆ける"), "no big form: the line shows as text instead of disappearing");
    ui.app.lyric_cache.insert(id, Some(crate::lyrics::Lyrics::Plain(vec!["just words".into()])));
    assert!(ui.shows("Karaoke needs synced lyrics"));
    ui.ch('y');
    assert!(ui.shows("just words"));
}

#[test]
fn lyrics_and_karaoke_fill_the_terminal_exactly_at_any_size() {
    let mut ui = singing(100.0);
    for key in ['y', 'Y'] {
        ui.ch(key);
        for size in [(100, 34), (80, 24), (60, 18), (40, 12), (200, 60), (30, 40), (20, 10)] {
            ui.size = size;
            let screen = ui.screen();
            assert_eq!(screen.len(), size.1, "rows of {key} at {size:?}");
            for line in &screen {
                assert_eq!(width(line), size.0, "width of {key} at {size:?}: {line:?}");
            }
        }
        ui.size = (100, 34);
    }
}

#[test]
fn restart_keeps_the_music_and_the_listening_count() {
    for (code, mods) in [('r', KeyModifiers::CONTROL), ('R', KeyModifiers::SUPER), ('r', KeyModifiers::SUPER | KeyModifiers::SHIFT)] {
        let mut ui = Ui::new();
        ui.play_a_result();
        ui.press(KeyCode::Char(code), mods);
        assert!(ui.app.restart && ui.app.detach && ui.app.quit, "{code} {mods:?}");
        ui.app.shutdown();
        assert!(!ui.told().contains(&json!(["quit"])));
        assert!(ui.app.db.get_setting("detached").unwrap().contains("counting\":{"));
    }
    let mut ui = Ui::new();
    ui.press(KeyCode::Char('q'), KeyModifiers::SUPER);
    ui.press(KeyCode::Char('r'), KeyModifiers::SUPER);
    assert!(!ui.app.quit && !ui.app.restart, "other Cmd keys do nothing");
}

// ---------- liked ----------
#[test]
fn the_liked_tab_ends_with_a_small_section_of_disliked_songs() {
    let mut ui = Ui::with(|db| {
        db.toggle_like(&song("l1", "Loved One"));
        db.toggle_like(&song("l2", "Loved Two"));
        for i in 0..8 {
            db.toggle_dislike(&song(&format!("d{i}"), &format!("Hated {i}")));
        }
    });
    ui.key(KeyCode::Tab);
    ui.key(KeyCode::Tab); // Liked
    assert_eq!(ui.app.tab, Tab::Liked);
    let screen = ui.screen().join("\n");
    assert!(screen.contains("Disliked · last 5 of 8"), "{screen}");
    assert!(screen.contains("Hated 7") && screen.contains("Hated 3") && !screen.contains("Hated 2"), "only the 5 most recent\n{screen}");
    let (_, liked_y) = ui.at("Loved Two");
    let (_, header_y) = ui.at("Disliked");
    assert!(header_y > liked_y, "under the likes");
    ui.key(KeyCode::Enter); // play from the first liked song (the most recent like)
    assert_eq!(ui.queue(), ["Loved Two", "Loved One"], "the likes only, never running on into the disliked songs");
    while ui.app.cur().is_some_and(|t| t.title != "Hated 7") {
        ui.key(KeyCode::Down);
    }
    ui.ch('x'); // take the dislike back
    let rows: Vec<String> = ui.screen().into_iter().filter(|l| l.starts_with('│')).collect(); // the list, not its title
    assert!(!rows.iter().any(|l| l.contains("Hated 7")) && rows.iter().any(|l| l.contains("Hated 2")), "it leaves, and the next one comes in\n{}", rows.join("\n"));
    assert!(ui.shows("Disliked · last 5 of 7"));
}


#[test]
fn karaoke_dots_fill_up_on_the_way_to_the_next_line() {
    let mut ui = singing(100.0); // Line 20 runs from 100 s to 105 s: a dot fills every second
    ui.ch('Y');
    let states = [(100.0, "○ ○ ○ ○"), (101.2, "● ○ ○ ○"), (102.2, "● ● ○ ○"), (103.2, "● ● ● ○"), (104.2, "● ● ● ●"), (105.0, "○ ○ ○ ○")];
    for (at, dots) in states {
        ui.property("audio-pts", json!(at));
        let row = ui.screen().into_iter().find(|l| l.contains('○') || l.contains("● ●")).unwrap_or_default();
        assert_eq!(row.trim_matches(|c| c == '│' || c == ' '), dots, "at {at} s");
    }
    let (_, dots_y) = ui.at("○ ○ ○ ○");
    let (_, next_y) = ui.at("Line 22");
    let screen = ui.screen();
    assert!(dots_y > next_y && screen[dots_y].starts_with('╰'), "the dots sit on the last row, against the bottom border\n{}", screen.join("\n"));
    ui.property("audio-pts", json!(101.5));
    ui.screen();
    ui.app.lyric_dots = Some(0); // what was drawn before the dot filled…
    assert!(ui.app.tick(), "…so the next tick draws again");
    ui.property("audio-pts", json!(197.0)); // the last line: nothing comes next
    assert!(!ui.shows("● ●") && !ui.shows("○"), "no countdown (the progress bar has a ● of its own)");
    ui.ch('y');
    assert!(!ui.shows("● ●") && !ui.shows("○"), "the words view has no countdown");
}

// The middle of what is drawn in a row inside a frame, in columns (braille counted dot by dot), and the frame's middle.
fn centre(row: &str) -> Option<(f64, f64)> {
    let inner: Vec<char> = row.chars().skip(1).collect::<Vec<_>>();
    let inner = &inner[..inner.len().saturating_sub(1)]; // without the frame's two borders
    let mut halves = vec![]; // half columns that have something in them
    for (c, ch) in inner.iter().enumerate() {
        let braille = ('\u{2801}'..='\u{28ff}').contains(ch);
        let bits = *ch as u32 - if braille { 0x2800 } else { 0 };
        if braille {
            if bits & 0x47 != 0 {
                halves.push(2 * c);
            }
            if bits & 0xb8 != 0 {
                halves.push(2 * c + 1);
            }
        } else if *ch != ' ' {
            halves.extend([2 * c, 2 * c + 1]);
        }
    }
    let (lo, hi) = (*halves.iter().min()?, *halves.iter().max()?);
    Some(((lo + hi + 1) as f64 / 4.0, inner.len() as f64 / 2.0))
}

#[test]
fn karaoke_is_centred_the_big_line_the_dots_the_note_and_the_lines_around() {
    let mut ui = singing(101.5);
    ui.ch('Y');
    let id = ui.app.now().unwrap().id.clone();
    for size in [(100, 34), (81, 30), (60, 24)] {
        ui.size = size;
        for (lines, at, what) in [(None, 101.5, "a line"), (Some(vec![(50.0, "first".to_string())]), 10.0, "the note before the first line")] {
            if let Some(lines) = lines {
                ui.app.lyric_cache.insert(id.clone(), Some(crate::lyrics::Lyrics::Synced(lines)));
            }
            ui.property("audio-pts", json!(at));
            let screen = ui.screen();
            let top = screen.iter().position(|l| l.contains("Karaoke ·")).unwrap();
            let bottom = top + screen[top..].iter().position(|l| l.starts_with('╰')).unwrap();
            let mut seen = 0;
            for row in &screen[top + 1..bottom] {
                if let Some((middle, frame)) = centre(row) {
                    seen += 1;
                    assert!((middle - frame).abs() <= 1.0, "{what} at {size:?}: centred at {middle}, the middle is {frame}\n{row}");
                }
            }
            assert!(seen >= 4, "{what}: the big rows, the dots and a small line are all there\n{}", screen.join("\n"));
        }
        let lines = (0..40).map(|i| (i as f64 * 5.0, format!("Line {i:02}"))).collect();
        ui.app.lyric_cache.insert(id.clone(), Some(crate::lyrics::Lyrics::Synced(lines)));
    }
}

#[test]
fn karaoke_letters_can_be_changed_and_are_remembered() {
    use crate::bigtext::Style;
    let mut ui = singing(100.0);
    ui.size = (110, 34);
    // the karaoke panel only: the player under it draws its own braille
    let words = |ui: &mut Ui| ui.screen().iter().take_while(|l| !l.contains("Now playing")).cloned().collect::<String>();
    assert_eq!(ui.app.karaoke_style, Style::Dots);
    ui.ch('Y');
    ui.ch('f');
    assert_eq!(ui.app.db.get_setting("karaoke_style").as_deref(), Some("solid"));
    assert!(words(&mut ui).chars().any(|c| ('\u{1FB00}'..='\u{1FB3B}').contains(&c)), "{}", ui.screen().join("\n"));
    ui.ch('f');
    assert!(ui.app.karaoke_style == Style::Blocks && words(&mut ui).contains('█') && !words(&mut ui).chars().any(braille));
    ui.size = (60, 20); // too small for blocks: the dots, not plain text
    assert!(words(&mut ui).chars().any(braille), "{}", ui.screen().join("\n"));
    ui.ch('f');
    assert_eq!(ui.app.karaoke_style, Style::Dots);
    assert_eq!(Ui::with(|db| db.set_setting("karaoke_style", "blocks")).app.karaoke_style, Style::Blocks);
}

// ---------- equalizer ----------
#[test]
fn the_equalizer_switches_profiles_and_makes_your_own() {
    let mut ui = Ui::new();
    ui.ch('E');
    assert!(ui.shows("EQ · Flat") && ui.shows("Bass boost") && ui.shows("+ New profile"));
    ui.told();
    ui.key(KeyCode::Down); // Bass boost, heard at once
    let told = ui.told();
    assert!(told.contains(&json!(["af-command", "eq", "gain", "6", "equalizer@b0"])), "{told:?}");
    assert!(told.iter().all(|c| c[0] != "af"), "the filter itself is never replaced: {told:?}");
    ui.ch('n');
    ui.text("Mine");
    ui.key(KeyCode::Enter);
    assert!(ui.shows("● Mine") && ui.app.eq_current().0 == "Mine", "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Right); // second band
    ui.key(KeyCode::Up);
    ui.key(KeyCode::Up);
    assert!(ui.told().contains(&json!(["af-command", "eq", "gain", "7", "equalizer@b1"])), "copied from Bass boost, then +2");
    ui.key(KeyCode::Esc);
    assert!(ui.app.eq.band.is_none(), "esc: back to the profiles");
    let saved = ui.app.db.get_setting("eq_presets").unwrap();
    assert!(saved.contains("Mine") && saved.contains("[6,7,4,2"), "{saved}");
    // what you chose is set again when the app starts, and yours can be deleted
    let mut next = Ui::with(|db| {
        db.set_setting("eq_presets", r#"[{"name":"Mine","gains":[6,7,4,2,0,0,0,0,0,0]}]"#);
        db.set_setting("eq", "Mine");
    });
    next.app.start();
    let told = next.told();
    assert!(told.contains(&json!(["af", "add", crate::eq::filter()])) && told.contains(&json!(["af-command", "eq", "gain", "7", "equalizer@b1"])));
    next.ch('E');
    // reset: back to the profile it came from (none recorded: flat), after asking
    next.ch('r');
    next.key(KeyCode::Enter);
    assert!(next.app.eq_current().1[1] == 7, "keeping is the default");
    next.ch('r');
    next.key(KeyCode::Down);
    next.key(KeyCode::Enter);
    assert_eq!((next.app.eq_current().0.as_str(), next.app.eq_current().1), ("Mine", [0; 10]));
    assert!(next.app.eq.open, "still in the window");
    next.ch('d');
    next.key(KeyCode::Down);
    next.key(KeyCode::Enter);
    assert!(next.app.eq_current().0 == "Flat" && !next.app.db.get_setting("eq_presets").unwrap().contains("Mine"));
}

#[test]
fn turning_the_visualizer_off_and_on_never_touches_the_audio_filters() {
    let mut ui = Ui::new();
    ui.play_a_result();
    for _ in 0..2 {
        ui.ch('w');
        let told = ui.told();
        assert!(told.iter().all(|c| c[0] != "af"), "{told:?}");
    }
}

#[test]
fn karaoke_keeps_five_columns_from_each_side() {
    let mut ui = singing(100.0);
    let id = ui.app.now().unwrap().id.clone();
    let long = "this line is much too long to fit on one row of the screen at all".to_string();
    ui.app.lyric_cache.insert(id, Some(crate::lyrics::Lyrics::Synced(vec![(0.0, long.clone()), (90.0, long.clone()), (200.0, long)])));
    for key in ['Y', 'y'] {
        ui.ch(key);
        let screen = ui.screen();
        let top = screen.iter().position(|l| l.contains(" · Result One")).unwrap();
        let bottom = top + screen[top..].iter().position(|l| l.starts_with('╰')).unwrap();
        for row in &screen[top + 1..bottom] {
            let inner: String = row.chars().skip(1).take(width(row) - 2).collect();
            if inner.trim().is_empty() {
                continue;
            }
            let left = inner.chars().take_while(|&c| c == ' ').count();
            let right = inner.chars().rev().take_while(|&c| c == ' ').count();
            assert!(left >= 5 && right >= 5, "{key}: {left} and {right} columns of margin\n{row}");
        }
    }
}

#[test]
fn the_player_buttons_are_small_icons_on_the_progress_line_and_each_is_its_click_area() {
    let mut ui = Ui::new();
    ui.play_a_result();
    let g = crate::ui::glyphs();
    let screen = ui.screen();
    let row = screen.iter().find(|l| l.contains(g.pause)).unwrap_or_else(|| panic!("{}", screen.join("\n")));
    assert!(row.contains(g.prev) && row.contains(g.next) && row.contains("0:00"), "the buttons share the progress line: {row}");
    assert!(!screen.join("\n").contains("╭───╮"), "no boxes");
    let (x, y) = ui.at(g.pause);
    for dx in [-1, 0, 1] {
        // the icon and a column either side of it
        ui.click((x as i64 + dx) as usize, y);
        assert!(ui.told().contains(&json!(["cycle", "pause"])), "a click at {dx} from the icon plays or pauses");
    }
    let (x, y) = ui.at(g.next);
    ui.click(x, y);
    assert!(ui.told().contains(&json!(["playlist-next"])));
    let (x, y) = ui.at(g.prev);
    ui.click(x, y);
    assert!(ui.told().contains(&json!(["playlist-prev"])));
    let (_, y) = ui.at("1/6"); // the dock's title line
    ui.click(5, y); // is no longer a button
    assert!(!ui.told().contains(&json!(["cycle", "pause"])));
    ui.mpv_says(json!({"event": "property-change", "name": "pause", "data": true}));
    assert!(ui.shows(g.play), "paused: the button offers to play");
}

#[test]
fn a_narrow_player_has_no_buttons_and_its_title_line_plays_or_pauses() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.size = (36, 30);
    assert!(!ui.shows(crate::ui::glyphs().prev), "{}", ui.screen().join("\n"));
    let (x, y) = ui.at("▶ Result One");
    ui.click(x, y);
    assert!(ui.told().contains(&json!(["cycle", "pause"])));
    for size in [(100, 34), (80, 24), (60, 30), (50, 30), (44, 30), (200, 60)] {
        ui.size = size;
        for line in ui.screen() {
            assert_eq!(width(&line), size.0, "width at {size:?}: {line:?}");
        }
    }
}


// ---------- the trip ----------
#[test]
fn the_trip_changes_holds_and_shows_lyrics_by_key() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.ch('V');
    let first = ui.app.trip.scene_name();
    assert!(ui.shows(&format!("Trip · {first}")));
    ui.ch(']');
    let second = ui.app.trip.scene_name();
    assert_ne!(first, second);
    assert!(ui.shows(&format!("Trip · {second}")));
    ui.ch('[');
    assert_eq!(ui.app.trip.scene_name(), first);
    ui.ch('.');
    assert!(ui.app.trip.locked && ui.shows("· held"));
    assert!(!ui.app.trip_lyrics, "lyrics are off by default");
    ui.ch('y');
    assert!(ui.app.trip_lyrics && ui.app.full && ui.app.trip_on, "y toggles the sung line, it does not leave the trip");
    assert_eq!(ui.app.db.get_setting("trip_lyrics").as_deref(), Some("on"));
    let told = ui.told();
    assert!(told.iter().all(|c| c[0] != "af"), "drawing only: {told:?}");
    ui.ch('V');
    assert!(!ui.app.trip_on);
}

// ---------- repeat ----------
#[test]
fn repeat_goes_from_the_queue_to_the_song_to_off_by_key_and_button() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.ch('r');
    assert_eq!(ui.told(), [json!(["set_property", "loop-playlist", "inf"]), json!(["set_property", "loop-file", "no"])]);
    assert_eq!(ui.app.repeat, crate::app::Repeat::All);
    ui.ch('r');
    assert_eq!(ui.app.repeat, crate::app::Repeat::One);
    assert!(ui.told().contains(&json!(["set_property", "loop-file", "inf"])));
    // the button does the same (the fourth box, right of next)
    let (x, y) = {
        let c = ui.app.geo.controls.iter().find(|c| c.0 == crate::app::Control::Repeat).expect("a repeat button");
        (c.1 + 1, c.3)
    };
    ui.click(x, y);
    assert_eq!(ui.app.repeat, crate::app::Repeat::Off);
    // what mpv reports wins, e.g. after re-attaching to a player left repeating
    ui.property("loop-file", json!("inf"));
    assert_eq!(ui.app.repeat, crate::app::Repeat::One);
    ui.property("loop-file", json!(false));
    ui.property("loop-playlist", json!("inf"));
    assert_eq!(ui.app.repeat, crate::app::Repeat::All);
}

// ---------- player layout ----------
#[test]
fn the_visualizer_gets_the_whole_width_of_the_player() {
    let mut ui = Ui::new();
    ui.size = (140, 34);
    ui.play_a_result();
    ui.app.bands = [1.0; crate::viz::BANDS];
    let rows = ui.screen();
    let c = ui.app.geo.controls.clone();
    assert_eq!(c.iter().map(|c| c.3).collect::<Vec<_>>(), [ui.app.geo.bar_row; 4], "the buttons sit on the progress line");
    let title = rows.iter().position(|r| r.contains("Result One") && r.contains("1/")).expect("song and count on one line");
    assert!(rows[title + 1].contains("vol 100"), "the volume under the count");
    assert!(rows[title + 2].trim_matches(|c| c == ' ' || c == '│').is_empty(), "a blank row between the artist and the visualizer");
    let wave = &rows[title + 3];
    let drawn = wave.chars().filter(|c| !matches!(c, ' ' | '│')).count();
    assert!(drawn > 120, "the visualizer spans the dock: {drawn} columns\n{wave}");
    assert!(rows.iter().all(|r| r.chars().count() == 140), "every row exactly as wide as the screen");
}

// ---------- look ----------
#[test]
fn the_player_can_sit_on_top_and_still_answers_clicks() {
    let mut ui = Ui::with(|db| db.set_setting("player", "top"));
    ui.play_a_result();
    let screen = ui.screen();
    let player = screen.iter().position(|l| l.contains("Now playing")).unwrap();
    let body = screen.iter().position(|l| l.contains("Results for")).unwrap();
    assert!(player < body, "the player above the lists\n{}", screen.join("\n"));
    assert!(screen.iter().all(|r| r.chars().count() == 100));
    // its buttons, the search field and the lists still answer where they are drawn
    let c = ui.app.geo.controls.iter().find(|c| c.0 == crate::app::Control::PlayPause).unwrap().clone();
    ui.click(c.1 + 1, c.3);
    assert!(ui.told().iter().any(|m| m.to_string().contains("pause")), "play/pause clicked");
    let (x, y) = ui.at("Result Two");
    ui.click(x, y);
    assert_eq!(ui.app.cur().map(|i| i.title.as_str()), Some("Result Two"));
    ui.click(5, 3);
    assert!(ui.app.typing, "the search field");
}

#[test]
fn a_fixed_color_and_another_border_style() {
    let mut ui = Ui::with(|db| {
        db.set_setting("accent", "blue");
        db.set_setting("border", "heavy");
    });
    ui.play_a_result();
    let _ = ui.screen();
    assert_eq!(ui.app.theme.color, crate::config::accent("blue").unwrap());
    assert!(ui.shows("┏━ / Search") && ui.shows("┃"), "{}", ui.screen().join("\n"));
}

// ---------- hooks ----------
// Every hook writes the JSON it got to one file per event.
fn hooks_into(ui: &Ui, events: &[&str], exit: u8) {
    use std::os::unix::fs::PermissionsExt;
    let hooks = ui.dir.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for event in events {
        let script = hooks.join(event);
        std::fs::write(&script, format!("#!/bin/sh\ncat >> '{}'\nexit {exit}\n", ui.dir.join(format!("got-{event}")).display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
// What a hook got so far, waiting for `n` of them.
fn got(ui: &mut Ui, event: &str, n: usize) -> Vec<Value> {
    let file = ui.dir.join(format!("got-{event}"));
    let read = move || -> Vec<Value> { std::fs::read_to_string(&file).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect() };
    ui.wait(event, |_| read().len() >= n);
    std::thread::sleep(Duration::from_millis(100)); // and no more than that
    let all = read();
    assert_eq!(all.len(), n, "{event}: {all:?}");
    all
}

#[test]
fn a_whole_album_is_liked_from_its_page_and_seen_by_album() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["liked", "unliked"], 0);
    open_album(&mut ui);
    assert!(ui.shows("l likes the album"), "{}", ui.screen().join("\n"));
    ui.ch('l'); // on Play album
    assert_eq!(ui.app.likes.len(), 3, "every song, not whatever was playing");
    assert_eq!(got(&mut ui, "liked", 3).len(), 3);
    assert!(ui.shows("Liked Long Road: 3 songs"), "{}", ui.screen().join("\n"));
    assert_eq!(ui.app.db.liked().iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Opening", "Middle", "Ending"]);
    ui.ch('l'); // again: asks, and keeping is the default
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.likes.len(), 3);
    ui.app.set_tab(Tab::Liked);
    ui.key(KeyCode::Right);
    assert!(ui.app.liked_by_album && ui.shows("Long Road · Band · 3 songs"), "{}", ui.screen().join("\n"));
    assert_eq!(ui.app.cur().unwrap().title, "Opening", "the cursor skips the heading");
    ui.key(KeyCode::Enter);
    assert_eq!(ui.queue(), ["Opening", "Middle", "Ending"]);
    ui.key(KeyCode::Left);
    assert!(!ui.app.liked_by_album && !ui.shows("Long Road · Band · 3 songs"));
}

#[test]
fn an_album_row_is_liked_from_the_results_and_unliked_after_asking() {
    let mut ui = Ui::new();
    ui.search("record");
    ui.key(KeyCode::Right); // the Albums tab
    ui.ch('l');
    ui.wait("the likes", |ui| ui.app.likes.len() == 3);
    assert_eq!(ui.app.tab, Tab::Search, "you stay where you were");
    ui.ch('l');
    ui.wait("the question", |ui| ui.app.menu.is_some());
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert!(ui.app.likes.is_empty());
}

#[test]
fn the_names_in_the_player_open_their_pages() {
    let mut ui = Ui::new();
    open_album(&mut ui);
    ui.key(KeyCode::Enter); // Play album
    ui.app.set_tab(Tab::Home);
    // the player's credits line, under the song's title: "Band · Long Road"
    let credits = |ui: &mut Ui, what: &str| {
        let screen = ui.screen();
        let row = screen.iter().position(|l| l.contains("Band · Long Road")).unwrap_or_else(|| panic!("{}", screen.join("\n")));
        (width(&screen[row][..screen[row].find(what).unwrap()]) + 1, row + 1)
    };
    let (x, y) = credits(&mut ui, "Long Road");
    ui.click(x + 2, y);
    ui.wait("the album", |ui| ui.app.tab == Tab::Search && ui.app.title.starts_with("Long Road"));
    ui.app.set_tab(Tab::Home);
    let (x, y) = credits(&mut ui, "Band");
    ui.click(x, y);
    ui.wait("the artist", |ui| ui.app.tab == Tab::Search && ui.app.title == "Band");
    ui.app.set_tab(Tab::Home);
    ui.ch('o');
    ui.wait("the album, by key", |ui| ui.app.title.starts_with("Long Road"));
    ui.app.set_tab(Tab::Home);
    ui.ch('v'); // the big player: the artist on a line of its own
    let screen = ui.screen();
    let row = screen.iter().position(|l| l.trim_matches(['│', ' ', '·', '∘']) == "Band").unwrap_or_else(|| panic!("{}", screen.join("\n")));
    ui.click(width(&screen[row][..screen[row].find("Band").unwrap()]) + 2, row + 1);
    ui.wait("the artist, from the big player", |ui| !ui.app.full && ui.app.title == "Band");
}

#[test]
fn names_in_lists_open_their_pages_with_a_click() {
    let mut ui = Ui::new();
    ui.search("record");
    ui.key(KeyCode::Right); // the Albums tab
    // the album's row: its artist, then (back) its name
    let on_row = |ui: &mut Ui, name: &str, what: &str| {
        let screen = ui.screen();
        let row = screen.iter().take_while(|l| !l.contains("Now playing")).position(|l| l.contains(name) && l.contains(what) && !l.contains("Results for")).unwrap_or_else(|| panic!("{}", screen.join("\n")));
        (width(&screen[row][..screen[row].rfind(what).unwrap()]) + 2, row + 1)
    };
    let (x, y) = on_row(&mut ui, "Long Road", "Band");
    ui.click(x, y);
    ui.wait("the artist", |ui| ui.app.title == "Band");
    ui.key(KeyCode::Esc);
    let (x, y) = on_row(&mut ui, "Band", "Long Road");
    ui.click(x, y);
    ui.wait("the album", |ui| ui.app.title.starts_with("Long Road"));
    // Stats and Home place their lists elsewhere: the same click there
    ui.key(KeyCode::Enter); // Play album
    ui.mpv_says(json!({"event": "file-loaded"}));
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(1.0));
    ui.property("time-pos", json!(2.0));
    ui.mpv_says(json!({"event": "end-file", "reason": "eof"}));
    ui.size = (110, 40);
    for tab in [Tab::Stats, Tab::Home] {
        ui.app.set_tab(tab);
        let (x, y) = on_row(&mut ui, "Opening", "Band");
        ui.click(x, y);
        ui.wait("the artist", |ui| ui.app.tab == Tab::Search && ui.app.title == "Band");
    }
}

#[test]
fn search_results_have_a_tab_for_each_kind_and_load_more() {
    let mut ui = Ui::new();
    ui.search("record");
    assert!(ui.shows("[Songs 3]") && ui.shows("Albums 1") && ui.shows("Artists 0") && ui.shows("Result One"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Right);
    assert!(ui.shows("[Albums 1]") && ui.shows("Long Road") && !ui.shows("Result One"));
    ui.key(KeyCode::Right);
    assert!(ui.shows("No artists found."));
    ui.key(KeyCode::Right); // round to the songs
    let (x, y) = ui.at("Albums 1");
    ui.click(x, y);
    assert!(ui.shows("[Albums 1]"), "a tab is clickable");
    ui.key(KeyCode::Enter);
    ui.wait("the album", |ui| ui.app.title.starts_with("Long Road"));
    ui.key(KeyCode::Esc);
    assert!(ui.shows("[Albums 1]"), "back to the tab you were on");
    // a long tab asks for its next page when you get near the end
    ui.search("many");
    ui.wait("the new results", |ui| ui.app.title.contains("many"));
    assert!(ui.shows("[Songs 8+]"), "{}", ui.screen().join("\n"));
    for _ in 0..4 {
        ui.key(KeyCode::Down);
    }
    ui.wait("the next page", |ui| ui.app.items.len() == 10);
    assert!(ui.shows("[Songs 10]") && ui.shows("Later Two"));
    assert_eq!(ui.app.sel, 4, "the cursor stays");
}

#[test]
fn the_queue_names_the_album_and_it_opens() {
    let mut ui = Ui::new();
    ui.size = (130, 34);
    open_album(&mut ui);
    ui.key(KeyCode::Enter); // Play album
    ui.app.set_tab(Tab::Queue);
    let screen = ui.screen();
    let row = screen.iter().position(|l| l.contains("Middle") && l.contains("Band · Long Road")).unwrap_or_else(|| panic!("{}", screen.join("\n")));
    ui.click(width(&screen[row][..screen[row].find("Long Road").unwrap()]) + 2, row + 1);
    ui.wait("the album", |ui| ui.app.tab == Tab::Search && ui.app.title.starts_with("Long Road"));
}

#[test]
fn mono_is_switched_without_replacing_a_filter() {
    let mut ui = Ui::new();
    ui.app.set_tab(Tab::Config);
    ui.key(KeyCode::Right); // into Sound
    while ui.app.cur().is_some_and(|i| i.id != "channels") {
        ui.key(KeyCode::Down);
    }
    ui.told();
    for (m, shown) in [("0", "mono"), ("1", "stereo")] {
        ui.key(KeyCode::Right);
        let told = ui.told();
        assert!(told.contains(&json!(["af-command", "eq", "m", m, "extrastereo@mono"])) && told.iter().all(|c| c[0] != "af"), "{told:?}");
        assert!(ui.shows(shown));
    }
    let mut next = Ui::with(|db| db.set_setting("channels", "mono"));
    next.app.start();
    assert!(next.told().contains(&json!(["af-command", "eq", "m", "0", "extrastereo@mono"])), "mono again on the next run");
}

#[test]
fn the_equalizer_opens_from_config_as_a_window_and_resets_to_its_origin() {
    let mut ui = Ui::new();
    ui.size = (120, 36);
    ui.app.set_tab(Tab::Config);
    ui.key(KeyCode::Right); // into Sound
    while ui.app.cur().is_some_and(|i| i.id != "eq") {
        ui.key(KeyCode::Down);
    }
    assert!(ui.shows("Equalizer: Flat…"));
    ui.key(KeyCode::Enter);
    let screen = ui.screen();
    assert!(ui.app.eq.open && screen.iter().any(|l| l.contains("EQ · Flat")), "{}", screen.join("\n"));
    assert!(screen.iter().all(|l| width(l) == 120), "the window over the page keeps every row exact");
    assert!(screen.iter().any(|l| l.contains("Config")), "the page stays behind it");
    let (x, y) = ui.at("Pop");
    ui.click(x, y);
    assert_eq!(ui.app.eq_current().0, "Pop");
    ui.ch('n');
    ui.text("Mi pop");
    ui.key(KeyCode::Enter);
    ui.key(KeyCode::Down); // 31 Hz, down one
    assert_eq!(ui.app.eq_current().1[0], -2);
    assert!(ui.shows("reset") && ui.shows("delete"), "{}", ui.screen().join("\n"));
    ui.ch('r');
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.eq_current().1, crate::eq::built_in("Pop"), "as Pop came");
    ui.key(KeyCode::Esc); // off the sliders
    ui.key(KeyCode::Esc); // closed
    assert!(!ui.app.eq.open && ui.shows("Equalizer: Mi pop…"), "{}", ui.screen().join("\n"));
}

#[test]
fn the_sleep_timer_stops_the_music() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.ch('z');
    assert!(ui.shows("Stop in 30 minutes"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter); // when this song ends
    assert!(ui.shows("sleep after this song"), "{}", ui.screen().join("\n"));
    ui.told();
    ui.mpv_says(json!({"event": "file-loaded"})); // the next one starts
    assert!(ui.told().contains(&json!(["set_property", "pause", true])));
    assert!(!ui.shows("sleep after"));
}

#[test]
fn the_queue_can_be_cleared_and_removals_taken_back() {
    let mut ui = Ui::new();
    ui.play_a_result();
    let full = ui.queue().len();
    assert!(full > 3);
    ui.app.set_tab(Tab::Queue);
    ui.key(KeyCode::Down);
    ui.ch('d');
    assert_eq!(ui.queue().len(), full - 1);
    ui.ch('u');
    assert_eq!(ui.queue().len(), full, "the removed song is back");
    ui.ch('c');
    ui.key(KeyCode::Enter); // keeping is the default
    assert_eq!(ui.queue().len(), full);
    ui.ch('c');
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert_eq!(ui.queue(), ["Result One"]);
    ui.ch('u');
    assert_eq!(ui.queue().len(), full);
}

#[test]
fn ctl_like_likes_what_plays() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.mpv_says(json!({"event": "client-message", "args": ["rymus-like"]}));
    assert!(ui.app.likes.contains("s1"));
}

#[test]
fn f_filters_a_list_and_esc_clears_it() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.app.set_tab(Tab::Queue);
    let all = ui.app.data().len();
    ui.ch('f');
    ui.text("similar c");
    assert_eq!(ui.app.data().iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Similar C"]);
    assert!(ui.shows("Filter \"similar c\" · 1"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Enter); // keep it
    ui.key(KeyCode::Enter); // and play what was found
    assert!(ui.told().contains(&json!(["playlist-play-index", 3])));
    ui.key(KeyCode::Esc);
    assert!(ui.app.data().len() == all && ui.app.cur().unwrap().title == "Similar C", "cleared, still on what you found");
}

#[test]
fn history_lists_what_you_heard_by_day() {
    let mut ui = Ui::new();
    ui.play_a_result();
    ui.mpv_says(json!({"event": "file-loaded"}));
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(1.0));
    ui.property("time-pos", json!(2.0));
    ui.mpv_says(json!({"event": "end-file", "reason": "eof"}));
    ui.app.set_tab(Tab::History);
    assert!(ui.shows("Today") && ui.shows("Result One") && ui.shows("Band · "), "{}", ui.screen().join("\n"));
    assert_eq!(ui.app.cur().unwrap().title, "Result One");
}

#[test]
fn the_command_palette_finds_a_command_and_does_it() {
    let mut ui = Ui::new();
    ui.ch(':');
    assert!(ui.shows("commands · type to find"));
    ui.text("equa");
    assert!(ui.shows("commands: equa") && ui.shows("Equalizer") && !ui.shows("Next song"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Enter);
    assert!(ui.app.eq.open);
    ui.key(KeyCode::Esc);
    ui.ch(':');
    ui.text("go hist");
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.tab, Tab::History);
}

#[test]
fn liking_an_album_puts_it_in_the_library() {
    let mut ui = Ui::new();
    open_album(&mut ui);
    ui.ch('l');
    ui.app.set_tab(Tab::Library);
    assert!(ui.shows("Albums") && ui.shows("Long Road"), "{}", ui.screen().join("\n"));
    assert_eq!(ui.app.cur().unwrap().title, "Long Road");
    ui.ch('d');
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert!(ui.app.db.saved_albums().is_empty() && ui.app.likes.len() == 3, "the songs stay liked");
}

#[test]
fn followed_artists_show_their_new_releases() {
    let mut ui = Ui::new();
    ui.app.db.follow("UCnews", "Newsy", "", &json!([]));
    ui.app.start();
    ui.wait("the news", |ui| ui.app.releases == 1);
    assert!(ui.shows("Library ✦1"), "{}", ui.screen().join("\n"));
    ui.app.set_tab(Tab::Library);
    assert!(ui.shows("New releases") && ui.shows("Fresh Record") && ui.shows("Newsy"));
    ui.key(KeyCode::Enter); // opening it: no longer news
    assert_eq!(ui.app.releases, 0);
}

#[test]
fn f_capital_follows_and_unfollows_an_artist() {
    let mut ui = Ui::new();
    ui.search("Band");
    ui.key(KeyCode::Left); // the Artists tab
    assert_eq!(ui.app.cur().unwrap().title, "Band");
    ui.ch('F');
    ui.wait("followed", |ui| ui.app.db.follows().len() == 1);
    ui.ch('F');
    assert!(ui.app.db.follows().is_empty());
}

#[test]
fn a_downloaded_song_plays_from_its_file() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["downloaded"], 0);
    ui.search("band");
    ui.ch('b');
    ui.wait("the file", |ui| ui.app.downloaded("s1"));
    let file = ui.app.downloads.files["s1"].clone();
    assert!(file.starts_with(ui.dir.join("music")) && file.to_string_lossy().ends_with("Band - Result One [s1].mp3"), "{file:?}");
    assert_eq!(got(&mut ui, "downloaded", 1)[0]["path"], file.to_string_lossy().as_ref());
    assert!(ui.shows("↓"), "marked in the list: {}", ui.screen().join("\n"));
    ui.told();
    ui.key(KeyCode::Enter);
    assert!(ui.told().contains(&json!(["loadfile", file.to_string_lossy(), "replace"])));
    ui.ch('m');
    assert!(ui.shows("Delete download"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Esc);
    ui.app.delete_download("s1");
    assert!(!file.exists() && !ui.app.downloaded("s1"));
}

#[test]
fn keeping_likes_offline_downloads_each_like() {
    let mut ui = Ui::with(|db| db.set_setting("download", "liked songs"));
    ui.app.start();
    ui.search("band");
    ui.ch('l');
    ui.wait("the file", |ui| ui.app.downloaded("s1"));
}

#[test]
fn the_language_is_chosen_in_config_and_kept() {
    let mut ui = Ui::new();
    ui.app.set_tab(Tab::Config);
    for _ in 0..4 {
        ui.key(KeyCode::Down); // to General
    }
    ui.key(KeyCode::Right); // into it: the language is first
    assert_eq!(ui.app.cur().unwrap().id, "language");
    ui.key(KeyCode::Right);
    assert!(ui.shows("Inicio") && ui.shows("Ajustes") && ui.shows("[Español]") && ui.shows("Sonido"), "{}", ui.screen().join("\n"));
    assert!(crate::manual::help().contains("Teclas (dentro de rymus)"));
    ui.key(KeyCode::Left);
    assert!(ui.shows("Home") && ui.shows("Sound"));
    let mut next = Ui::with(|db| db.set_setting("language", "español"));
    next.app.start();
    assert!(next.shows("Inicio"), "{}", next.screen().join("\n"));
    crate::i18n::set("english");
}

#[test]
fn a_choice_in_config_is_clicked() {
    let mut ui = Ui::new();
    ui.size = (120, 40);
    ui.app.set_tab(Tab::Config);
    let (x, y) = ui.at("Channels");
    ui.click(x, y); // the row first: its choices open
    assert_eq!(ui.app.cur().unwrap().id, "channels");
    let (x, y) = ui.at(" mono ");
    ui.click(x + 1, y);
    assert_eq!(ui.app.db.get_setting("channels").as_deref(), Some("mono"));
    assert!(ui.shows("[mono]"));
    let (x, y) = ui.at("○ off"); // Even out loudness, a switch
    ui.click(x, y);
    assert_eq!(ui.app.db.get_setting("normalize").as_deref(), Some("on"));
    let (x, y) = ui.at("Look"); // a section in the menu
    ui.click(x, y);
    assert_eq!(ui.app.cur().unwrap().id, "mode", "opens at its first setting");
    assert!(ui.app.config_menu, "with the cursor still on the sections");
}

#[test]
fn likes_and_dislikes_run_their_hooks() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["liked", "unliked", "disliked"], 0);
    ui.search("band");
    ui.ch('l');
    let liked = got(&mut ui, "liked", 1);
    assert_eq!((liked[0]["event"].as_str(), liked[0]["id"].as_str(), liked[0]["title"].as_str()), (Some("liked"), Some("s1"), Some("Result One")));
    assert_eq!(liked[0]["url"], "https://music.youtube.com/watch?v=s1");
    ui.ch('l'); // remove it
    got(&mut ui, "unliked", 1);
    ui.ch('l');
    got(&mut ui, "liked", 2);
    ui.ch('x'); // disliking a liked song also removes the like
    got(&mut ui, "unliked", 2);
    got(&mut ui, "disliked", 1);
    ui.ch('x'); // removing a dislike is no event
    got(&mut ui, "disliked", 1);
}

#[test]
fn a_failing_hook_is_told() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["liked"], 3);
    ui.search("band");
    ui.ch('l');
    ui.wait("the failure", |ui| ui.app.msg.contains("Hook liked failed"));
}

#[test]
fn playback_hooks_tell_a_finished_song_from_a_skipped_one() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["playing", "played", "skipped"], 0);
    ui.play_a_result(); // loads the first song
    assert_eq!(got(&mut ui, "playing", 1)[0]["id"], "s1");
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(1.0));
    ui.property("time-pos", json!(2.0));
    ui.mpv_says(json!({"event": "end-file", "reason": "eof"}));
    let played = got(&mut ui, "played", 1);
    assert_eq!((played[0]["id"].as_str(), played[0]["seconds"].as_i64(), played[0]["duration"].as_i64()), (Some("s1"), Some(1), Some(200)));
    ui.property("playlist-pos", json!(1));
    ui.mpv_says(json!({"event": "file-loaded"}));
    ui.search("again");
    ui.key(KeyCode::Enter); // another song: the queue is already new when mpv reports the old one stopped
    ui.mpv_says(json!({"event": "end-file", "reason": "stop"}));
    assert_eq!(got(&mut ui, "skipped", 1)[0]["title"], "Similar A");
    got(&mut ui, "played", 1);
}

#[test]
fn playlist_hooks_name_the_playlist() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["playlist-add", "playlist-remove"], 0);
    ui.key(KeyCode::Tab);
    ui.key(KeyCode::Enter); // + New playlist
    ui.text("Radio");
    ui.key(KeyCode::Enter);
    ui.search("band");
    ui.ch('s');
    ui.key(KeyCode::Enter);
    let added = got(&mut ui, "playlist-add", 1);
    assert_eq!((added[0]["id"].as_str(), added[0]["playlist"].as_str()), (Some("s1"), Some("Radio")));
    ui.ch('s');
    ui.key(KeyCode::Enter); // already there: nothing new
    got(&mut ui, "playlist-add", 1);
    ui.key(KeyCode::Esc);
    ui.key(KeyCode::Enter); // open Radio
    ui.ch('d');
    assert_eq!(got(&mut ui, "playlist-remove", 1)[0]["playlist"], "Radio");
}

// ---------- your data ----------
// Config > Data: the row with this id, highlighted.
fn to_data_row(ui: &mut Ui, id: &str) {
    for _ in 0..10 {
        if ui.app.tab == Tab::Config {
            break;
        }
        ui.key(KeyCode::Tab);
    }
    assert_eq!(ui.app.tab, Tab::Config);
    let at = ui.app.settings.iter().position(|r| r.id == id).expect("a Data row");
    ui.app.sel = at;
    ui.app.config_menu = false; // inside its section
}

#[test]
fn your_data_exports_wipes_only_with_the_word_and_imports_back() {
    let mut ui = Ui::new();
    ui.search("band");
    ui.ch('l'); // a like to keep
    assert_eq!(ui.app.likes.len(), 1);
    let file = ui.dir.join("export");

    // export: Enter asks for a file, with a dated name already written
    to_data_row(&mut ui, "export");
    ui.key(KeyCode::Enter);
    assert!(ui.app.typing && ui.buf_starts_with("~/rymus-export-"), "{}", ui.app.buf);
    ui.app.buf = file.display().to_string();
    ui.key(KeyCode::Enter);
    assert!(file.join("likes.csv").is_file() && file.join("tracks.csv").is_file(), "exported as CSV: {}", ui.app.msg);

    // wipe: the field is a red warning, and anything but the word deletes nothing
    to_data_row(&mut ui, "wipe");
    ui.key(KeyCode::Enter);
    assert!(ui.shows("⚠ This erases all your data"), "{}", ui.screen().join("\n"));
    ui.text("yes");
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.likes.len(), 1, "not the word: nothing deleted");
    ui.key(KeyCode::Enter);
    ui.text("wipe"); // not the sentence either
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.likes.len(), 1, "a single word deletes nothing");
    ui.key(KeyCode::Enter);
    ui.text("delete all my data");
    ui.key(KeyCode::Enter);
    assert!(ui.app.likes.is_empty() && ui.app.db.liked_ids().is_empty(), "wiped");
    assert!(ui.dir.join("t.db").is_file(), "an empty database is made again at once");
    // the song still playing is counted in the new database, not written over some old play's row
    ui.play_a_result();
    ui.mpv_says(json!({"event": "file-loaded"}));
    ui.app.wipe_data(crate::app::WIPE_WORD);
    ui.property("duration", json!(200.0));
    ui.property("time-pos", json!(1.0));
    ui.property("time-pos", json!(2.0));
    ui.mpv_says(json!({"event": "end-file", "reason": "eof"}));
    assert_eq!(ui.app.db.listening(0).len(), 1, "one play, in the new database");
    ui.search("again"); // and it works as on the first run
    ui.ch('l');
    assert_eq!(ui.app.db.liked_ids().len(), 1);

    // import: everything is replaced by the file, the like that was exported is back
    to_data_row(&mut ui, "import");
    ui.key(KeyCode::Enter);
    ui.app.buf = ui.dir.join("not-there.db").display().to_string();
    ui.key(KeyCode::Enter);
    assert!(ui.app.msg.contains("Not a rymus export"), "{}", ui.app.msg);
    ui.key(KeyCode::Enter);
    ui.app.buf = file.display().to_string();
    ui.key(KeyCode::Enter);
    assert_eq!(ui.app.likes.len(), 1, "{}", ui.app.msg);
    assert!(ui.app.likes.contains("s1"), "the like from the file");
}

// ---------- speed (run by hand: cargo test --release frame_time -- --ignored --nocapture) ----------
#[test]
#[ignore]
fn frame_time() {
    for (name, size) in [("100x34", (100, 34)), ("200x60", (200, 60))] {
        let mut ui = Ui::new();
        ui.size = size;
        ui.play_a_result();
        ui.property("duration", json!(200.0));
        let n = 2000;
        let t = Instant::now();
        for i in 0..n {
            ui.app.bands = [((i % 10) as f64) / 10.0; crate::viz::BANDS];
            ui.app.time = (i / 25) as i64;
            let _ = ui.app.render(size.0, size.1);
        }
        let per = t.elapsed() / n;
        ui.app.full = true;
        let t = Instant::now();
        for _ in 0..n {
            let _ = ui.app.render(size.0, size.1);
        }
        let full = t.elapsed() / n;
        ui.app.full = false;
        ui.ch('y');
        let t = Instant::now();
        for _ in 0..n {
            let _ = ui.app.render(size.0, size.1);
        }
        let lyr = t.elapsed() / n;
        ui.ch('y');
        ui.ch('V');
        let t = Instant::now();
        for i in 0..n {
            ui.app.trip_t = i as f64 * 0.04;
            let _ = ui.app.render(size.0, size.1);
        }
        println!("{name}: list {per:?} · full player {full:?} · lyrics {lyr:?} · trip {:?} ({})", t.elapsed() / n, ui.app.trip.scene_name());
    }
}

#[test]
#[ignore]
fn trip_scene_times() {
    let mut ui = Ui::new();
    ui.size = (200, 60);
    ui.play_a_result();
    ui.property("duration", json!(200.0));
    ui.ch('V');
    for _ in 0..14 {
        let (t, mut bytes) = (Instant::now(), 0);
        for i in 0..300 {
            ui.app.trip_t = i as f64 * 0.04;
            ui.app.bands = [((i % 10) as f64) / 10.0; crate::viz::BANDS];
            bytes += ui.app.render(200, 60).len();
        }
        println!("{:>8.0?} {:>6} KB/frame  {}", t.elapsed() / 300, bytes / 300 / 1024, ui.app.trip.scene_name());
        ui.ch(']');
    }
}

#[test]
fn light_mode_draws_dark_text_on_light_tones_and_darkens_the_accent() {
    let mut ui = Ui::with(|db| {
        db.set_setting("mode", "light");
        db.set_setting("accent", "yellow");
    });
    ui.play_a_result();
    ui.search("band");
    let frame = ui.app.render(100, 34);
    assert!(frame.contains("48;2;216;216;222"), "the light tone behind the selected row");
    assert!(!frame.contains("48;2;48;48;52"), "not the dark one");
    let a = ui.app.theme.color;
    let lum = 0.2126 * a[0] as f64 + 0.7152 * a[1] as f64 + 0.0722 * a[2] as f64;
    assert!(lum <= 111.0, "yellow is darkened enough to read on white: {a:?}");
    // and back to dark
    to_data_row(&mut ui, "export");
    let at = ui.app.settings.iter().position(|r| r.id == "mode").unwrap();
    ui.app.sel = at;
    ui.key(KeyCode::Right);
    assert!(ui.app.render(100, 34).contains("48;2;48;48;52"), "dark again, at once");
}

#[test]
fn your_own_music_plays_from_the_folder_and_is_never_deleted() {
    let mut ui = Ui::with(|db| db.set_setting("library", "on"));
    let song = ui.dir.join("music/Album/Band - Mine.flac");
    std::fs::create_dir_all(song.parent().unwrap()).unwrap();
    std::fs::write(&song, "audio").unwrap();
    ui.app.start();
    ui.wait("the folder read", |ui| !ui.app.downloads.own.is_empty());
    ui.app.set_tab(Tab::Library);
    assert!(ui.shows("Your music") && ui.shows("Mine"), "{}", ui.screen().join("\n"));
    while ui.app.cur().is_some_and(|t| t.title != "Mine") {
        ui.key(KeyCode::Down);
    }
    ui.told();
    ui.key(KeyCode::Enter);
    assert!(ui.told().contains(&json!(["loadfile", song.to_string_lossy(), "replace"])));
    ui.ch('m');
    assert!(!ui.shows("Delete download"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Esc);
    let id = format!("file:{}", song.to_string_lossy());
    ui.app.delete_download(&id);
    assert!(song.exists(), "your own file stays");
}

#[test]
fn freeing_space_deletes_the_least_played_downloads() {
    let mut ui = Ui::new();
    ui.app.start();
    ui.search("band");
    ui.ch('b');
    ui.wait("the first file", |ui| ui.app.downloaded("s1"));
    ui.key(KeyCode::Down);
    let second = ui.app.cur().cloned().unwrap();
    ui.ch('b');
    ui.wait("the second file", |ui| ui.app.downloaded(&second.id));
    let play = ui.app.db.start_play(&second, 1);
    ui.app.db.end_play(play, 100, None, false);
    let (first, kept) = (ui.app.downloads.files["s1"].clone(), ui.app.downloads.files[&second.id].clone());
    ui.app.ask_free_space(50);
    assert!(ui.shows("least played 50 %") && ui.shows("Delete 1 song,"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    assert!(!first.exists() && !ui.app.downloaded("s1"), "the one never heard goes");
    assert!(kept.exists() && ui.app.downloaded(&second.id), "the one heard stays");
    assert!(ui.app.msg.contains("Freed") || ui.shows("Freed"), "{}", ui.app.msg);
}

#[test]
fn the_downloads_window_holds_the_settings_and_frees_space_by_a_typed_share() {
    let mut ui = Ui::new();
    ui.ch('B');
    assert!(ui.shows("Downloads") && ui.shows("Keep offline") && ui.shows("Free up space…") && ui.shows("songs downloaded"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Right); // Keep offline: liked songs
    assert_eq!(crate::config::value(&ui.app.db, "download"), "liked songs");
    for _ in 0..10 {
        ui.key(KeyCode::Down); // to the last row, Free up space
    }
    ui.key(KeyCode::Enter);
    assert!(ui.app.typing, "asks how much");
    assert_eq!(ui.app.buf, "25", "the last share offered again");
    ui.app.buf = "150".into();
    ui.key(KeyCode::Enter);
    assert!(ui.app.msg.contains("1 to 100") || ui.shows("1 to 100"), "a share out of range is refused: {}", ui.app.msg);
    ui.key(KeyCode::Esc);
    assert!(!ui.app.downloads.open, "Esc closes it");
}

#[test]
fn a_short_screen_keeps_the_player_without_the_blank_row() {
    let mut ui = Ui::new();
    ui.size = (100, 24);
    ui.play_a_result();
    ui.app.bands = [1.0; crate::viz::BANDS];
    let rows = ui.screen();
    let title = rows.iter().position(|r| r.contains("Result One") && r.contains("1/")).expect("the player");
    assert!(!rows[title + 2].trim_matches(|c| c == ' ' || c == '│').is_empty(), "the visualizer right under the artist");
    let (x, y) = ui.at(crate::ui::glyphs().pause);
    ui.click(x, y);
    assert!(ui.told().contains(&json!(["cycle", "pause"])), "the buttons still answer where they are drawn");
}

#[test]
fn a_restart_opens_on_the_tab_it_was_on() {
    let mut ui = Ui::new();
    ui.key(KeyCode::Tab);
    ui.key(KeyCode::Tab);
    let tab = ui.app.tab;
    ui.press(KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(ui.app.restart);
    let i: usize = ui.app.db.get_setting("restart_tab").unwrap().parse().unwrap();
    assert_eq!(crate::app::TABS[i], tab);
}

#[test]
fn downloads_run_their_hooks_from_start_to_deletion() {
    let mut ui = Ui::new();
    hooks_into(&ui, &["download-started", "downloaded", "download-deleted"], 0);
    ui.search("band");
    ui.ch('b');
    let started = got(&mut ui, "download-started", 1);
    assert_eq!((started[0]["id"].as_str(), started[0]["format"].as_str(), started[0]["waiting"].as_u64()), (Some("s1"), Some("mp3"), Some(0)));
    let done = got(&mut ui, "downloaded", 1);
    assert_eq!(done[0]["format"], "mp3");
    ui.app.ask_free_space(100);
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    ui.text("delete my downloads"); // all of them: the phrase is asked for
    ui.key(KeyCode::Enter);
    let gone = got(&mut ui, "download-deleted", 1);
    assert_eq!((gone[0]["id"].as_str(), gone[0]["reason"].as_str()), (Some("s1"), Some("free up space")));
    assert!(gone[0]["bytes"].as_u64().unwrap() > 0 && gone[0]["path"].as_str().unwrap().ends_with(".mp3"));
}

#[test]
fn a_song_of_your_own_tells_hooks_its_path_not_a_url() {
    let mut ui = Ui::with(|db| db.set_setting("library", "on"));
    hooks_into(&ui, &["liked"], 0);
    let song = ui.dir.join("music/Band - Mine.flac");
    std::fs::create_dir_all(song.parent().unwrap()).unwrap();
    std::fs::write(&song, "audio").unwrap();
    ui.app.start();
    ui.wait("the folder read", |ui| !ui.app.downloads.own.is_empty());
    ui.app.set_tab(Tab::Library);
    while ui.app.cur().is_some_and(|t| t.title != "Mine") {
        ui.key(KeyCode::Down);
    }
    ui.ch('l');
    let liked = got(&mut ui, "liked", 1);
    assert!(liked[0]["url"].is_null(), "{liked:?}");
    assert_eq!(liked[0]["path"], song.to_string_lossy().as_ref());
}

#[test]
fn a_downloaded_song_reads_its_lyrics_from_the_file_beside_it() {
    let mut ui = Ui::new();
    ui.app.start();
    ui.search("band");
    ui.ch('b');
    ui.wait("the file", |ui| ui.app.downloaded("s1"));
    let file = ui.app.downloads.files["s1"].clone();
    std::fs::write(file.with_extension("lrc"), "[00:01.00]from the file\n[00:04.00]second line\n").unwrap();
    ui.key(KeyCode::Enter); // play it
    ui.property("duration", json!(200.0));
    ui.ch('y');
    ui.app.want_lyrics();
    match ui.app.lyrics_ready() {
        Some(crate::lyrics::Lyrics::Synced(lines)) => assert_eq!(lines[0].1, "from the file"),
        other => panic!("expected the file's lyrics, got {:?}", other.is_some()),
    }
}

#[test]
fn freeing_nearly_all_downloads_asks_for_the_phrase() {
    let mut ui = Ui::new();
    ui.app.start();
    ui.search("band");
    ui.ch('b');
    ui.wait("the file", |ui| ui.app.downloaded("s1"));
    let file = ui.app.downloads.files["s1"].clone();
    ui.app.free_cut("100");
    ui.key(KeyCode::Down); // past "Keep them"
    ui.key(KeyCode::Enter);
    assert!(file.exists(), "one Enter deletes nothing at 100 %");
    assert!(ui.shows("⚠ This deletes nearly all your downloads"), "{}", ui.screen().join("\n"));
    ui.text("yes");
    ui.key(KeyCode::Enter);
    assert!(file.exists() && ui.app.downloaded("s1"), "the wrong words keep them");
    ui.app.free_cut("100");
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);
    ui.text("delete my downloads");
    ui.key(KeyCode::Enter);
    assert!(!file.exists() && !ui.app.downloaded("s1"), "the phrase frees them");
}
