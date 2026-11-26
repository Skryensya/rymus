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
    assert_eq!(ui.told(), [json!(["seek", 10]), json!(["seek", -10])]);
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
    for _ in 0..5 {
        ui.key(KeyCode::Tab); // Home → Playlists → Liked → Queue → Stats → Config
    }
    assert_eq!(ui.app.tab, Tab::Config);
}

#[test]
fn the_config_tab_changes_settings_saves_them_and_tells_the_player() {
    let mut ui = Ui::new();
    open_config(&mut ui);
    for label in ["Crossfade", "Even out loudness", "Audio quality", "Pixel art covers", "Visualizer"] {
        assert!(ui.shows(label), "{label} is listed\n{}", ui.screen().join("\n"));
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
    ui.key(KeyCode::Down);
    ui.key(KeyCode::Right); // Audio quality: data saver
    assert!(ui.told().contains(&json!(["set_property", "ytdl-format", "worstaudio/worst"])));
    ui.ch('w'); // the visualizer key and the Config row are the same setting
    assert_eq!(ui.app.db.get_setting("wave").as_deref(), Some("off"));
    let wave = ui.screen().into_iter().find(|l| l.contains("Visualizer") && l.contains('[')).unwrap();
    assert!(wave.contains("[off]"), "{wave}");
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
    ui.size = (110, 46);
    open_config(&mut ui);
    for heading in ["SOUND", "STREAMING", "SCREEN", "LOOK", "OUTPUT"] {
        assert!(ui.shows(heading), "{heading}\n{}", ui.screen().join("\n"));
    }
    assert!(ui.shows("[off]") && ui.shows(" 12 s "), "every choice is in view");
    assert!(ui.shows("Crossfade · off") && ui.shows("no overlap"), "the setting under the cursor, explained");
    assert!(!ui.shows("Preview"), "the player below does not preview a setting");
    ui.key(KeyCode::Right);
    ui.key(KeyCode::Right);
    assert!(ui.shows("[4 s]") && ui.shows("4 s together"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Down);
    assert!(ui.shows("Even out loudness · off") && !ui.shows("together"));
}

#[test]
fn the_eq_tab_explains_the_bands_and_moves_them_with_sliders() {
    let mut ui = Ui::new();
    ui.size = (110, 34);
    ui.ch('E');
    assert_eq!(ui.app.tab, Tab::Eq);
    for part in ["EQ · Flat", "PROFILES", "+12 ┤", "  0 ┼", "-12 ┤", "bass", "mids", "treble", "16k", "The music as it was mixed"] {
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
    assert_eq!(ui.app.eq_current().0, "My Rock", "a built-in profile is copied, never changed");
    assert!(ui.told().contains(&json!(["af-command", "eq", "gain", "4", "equalizer@b1"])), "and the change is heard");
    assert!(ui.shows("● My Rock ✎") && ui.shows("62 Hz · Bass · +4 dB"));
    ui.key(KeyCode::Tab);
    assert_eq!(ui.app.tab, Tab::Home, "the keys it does not use still work");
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
    assert!(ui.shows("‹ off ›") && !ui.shows("12 s"), "{}", ui.screen().join("\n"));
    ui.key(KeyCode::Right);
    assert!(ui.shows("‹ 2 s ›"), "and ← → still change it");
}

// ---------- albums ----------
fn open_album(ui: &mut Ui) {
    // a solid red cover, already loaded, so nothing is fetched
    ui.app.covers.insert("test://road".into(), Some(Arc::new(crate::cover::Got { b64: "AAAA".into(), rgb: [200, 30, 30], palette: vec![], image: vec![] })));
    ui.search("record");
    assert!(ui.shows("Long Road"));
    ui.key(KeyCode::Enter); // the album is the first result
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
    for _ in 0..3 {
        ui.key(KeyCode::Down);
    }
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
    for _ in 0..4 {
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
    for _ in 0..4 {
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
    assert!(ui.shows("● Mine ✎") && ui.app.eq_current().0 == "Mine", "{}", ui.screen().join("\n"));
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
    let wave = &rows[title + 2];
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
