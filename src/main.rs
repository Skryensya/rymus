// rymus (ymus in Rust): search YouTube Music and play it inside the terminal.
// mpv runs headless and is driven over its JSON IPC socket. No account: everything is anonymous.
// Likes, play history, search history, settings and the last session live in a local SQLite file (see db.rs).
//
// Where things are (each file starts by saying what it does):

// The app: its state and everything it does, and how it is drawn
mod app; //      state, keys, mouse, playback, lists; split by topic in app/
mod render; //   the whole screen as one string; split by view in render/
mod manual; //   `rymus help` and `rymus man`
mod i18n; //     the texts in other languages
mod item; //     one row of any list (a song, an album, a header…)

// Talking to the outside
mod mpv; //      the player, driven over its IPC socket
mod yt; //       YouTube Music without an account
mod lyrics; //   lyrics from LRCLIB
mod playlist; // importing a playlist from a pasted link
mod cover; //    cover images: download, colours, kitty images and half blocks
mod output; //   which device the sound goes to
mod platform; // what differs between macOS and Linux

// Your data and taste
mod csv; //      CSV files: your data exported and read back
mod db; //       the local SQLite log: songs, plays, likes, playlists, settings
mod disk; //     the music folder: its files, their size, what to delete first
mod config; //   the Config tab's settings
mod eq; //       the equalizer: bands, profiles, the filter and its commands
mod recommend; // the taste model and the ranking of new songs
mod mixes; //    radios and Discover mixes, built from candidates

// Look
mod ui; //       frames, tab bar, search field, hints, text widths
mod theme; //    the accent colour and how it changes
mod home; //     how Home's sections share the height
mod hooks; //    your own commands, run on likes, playlists and playback
mod charts; //   the Stats charts
mod bigtext; //  the big pixel letters of karaoke
mod viz; //      the spectrum analyser (inside mpv's audio filter)
mod trip; //     the old smoke, one of the trip's scenes
mod update; //   new versions: checked once a day, installed by `rymus update`
mod scenes; //   the full screen visualizer (V): its scenes, what they hear, how they change

#[cfg(test)]
mod flows; //    behaviour tests: the app driven by keys and mouse, judged by the screen

use crate::i18n::tr;
use app::{App, Msg};
use crossterm::event::{self, Event};
use crossterm::terminal;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(40);
const FRAME_DELAY: Duration = Duration::from_millis(30); // several changes in a row make one frame

// Alt screen, hidden cursor, autowrap off (a wide glyph then clips instead of wrapping), mouse press/drag + SGR.
// Restored when dropped, also if something panics.
// Shift+Space looks exactly like Space unless the terminal reports every key as an escape code (kitty keyboard protocol,
// with the shifted character too). Terminals without it ignore this. Off while typing, so composed text arrives as text.
const ALL_KEYS: &[u8] = b"\x1b[>13u";
const NORMAL_KEYS: &[u8] = b"\x1b[<u";

struct Screen;
impl Screen {
    fn enter() -> std::io::Result<Screen> {
        terminal::enable_raw_mode()?;
        let mut out = std::io::stdout();
        out.write_all(b"\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J\x1b[?1000h\x1b[?1002h\x1b[?1006h")?;
        out.flush()?;
        Ok(Screen)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let mut out = std::io::stdout();
        let _ = out.write_all(NORMAL_KEYS);
        let _ = write!(out, "{}\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[?7h\x1b[?25h\x1b[?1049l", cover::CLEAR);
        let _ = out.flush();
        let _ = terminal::disable_raw_mode();
    }
}

// Only the lines that differ from what is on screen are written, each at its own row.
// The image commands ride on the last line, so a new image makes that line differ.
fn changes(frame: &str, shown: &mut Vec<String>) -> String {
    let lines: Vec<&str> = frame.strip_prefix("\x1b[H").unwrap_or(frame).split("\r\n").collect();
    shown.resize(lines.len(), String::new());
    let mut out = String::new();
    for (row, (line, old)) in lines.into_iter().zip(shown).enumerate() {
        if line != old {
            out += &format!("\x1b[{};1H\x1b[0m{line}", row + 1);
            *old = line.to_string();
        }
    }
    out
}

static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
fn trace(what: &str) {
    if let Some(f) = std::env::var_os("RYMUS_TRACE") {
        use std::io::Write as _;
        let t = T0.get_or_init(Instant::now).elapsed();
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(f).and_then(|mut o| writeln!(o, "{:>7.1} ms  {what}", t.as_secs_f64() * 1000.0));
    }
}
// `rymus ctl <action>`: controls the rymus that is playing (or the music left playing by Q) from outside, e.g. from
// buttons on the Touch Bar or a keyboard shortcut. It talks to the newest player; rymus follows what changed.
fn ctl(action: &str) -> Result<(), String> {
    let command = match action {
        "play-pause" => serde_json::json!(["cycle", "pause"]),
        "next" => serde_json::json!(["playlist-next"]),
        "prev" => serde_json::json!(["playlist-prev"]),
        "volume-up" => serde_json::json!(["add", "volume", 5]),
        "volume-down" => serde_json::json!(["add", "volume", -5]),
        // like (or take the like back from) what plays: the running rymus does it, so your hooks run too
        "like" => serde_json::json!(["script-message", "rymus-like"]),
        "status" => serde_json::json!(["get_property", "pause"]),
        _ => return Err(tr!("usage: rymus ctl play-pause | next | prev | volume-up | volume-down | like | status").into()),
    };
    let mut players: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir("/tmp")
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rymus-") && n.ends_with(".sock") && !n.contains("-tail")))
        .filter_map(|p| Some((p.metadata().and_then(|m| m.modified()).ok()?, p)))
        .collect();
    // the one whose rymus is running first (the name holds its pid), then the newest: a player left by Q has none
    let running = |p: &std::path::Path| {
        let pid = p.file_stem().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("rymus-")).and_then(|n| n.parse::<i32>().ok());
        pid.is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0) // SAFETY: signal 0 only asks whether it exists
    };
    players.sort_by_key(|(at, p)| (std::cmp::Reverse(running(p)), std::cmp::Reverse(*at)));
    for (_, path) in players {
        if let Ok((from, mut to)) = platform::connect(&path.to_string_lossy()) {
            to.write_all(format!("{}\n", serde_json::json!({ "command": command, "request_id": 1 })).as_bytes()).map_err(|e| e.to_string())?;
            return if action == "status" { status(from) } else { Ok(()) };
        }
    }
    Err(tr!("rymus is not playing").into())
}

// One line for a status bar: "▶ Title — Artist" (⏸ when paused).
fn status(from: platform::Input) -> Result<(), String> {
    let paused = std::io::BufRead::lines(std::io::BufReader::new(from))
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .find(|v| v["request_id"] == 1)
        .is_some_and(|v| v["data"] == true);
    let db = db::Db::open(&db::default_path()).map_err(|e| e.to_string())?;
    let now: serde_json::Value = db.get_setting("now").and_then(|s| serde_json::from_str(&s).ok()).ok_or(tr!("nothing is playing"))?;
    println!("{} {} — {}", if paused { "⏸" } else { "▶" }, now["title"].as_str().unwrap_or(""), now["artist"].as_str().unwrap_or(""));
    Ok(())
}

// Reads your translations and the language you chose; without a database yet, English.
fn language() {
    let path = db::default_path();
    i18n::load(path.parent().unwrap_or(std::path::Path::new(".")));
    if let Some(chosen) = path.is_file().then(|| db::Db::open(&path).ok()).flatten().and_then(|db| db.get_setting("language")) {
        i18n::set(&chosen);
    }
}

fn run() -> Result<(), String> {
    trace("main");
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--drain") => {
            platform::drain(&args.next().unwrap_or_default());
            return Ok(());
        }
        Some("ctl") => return ctl(&args.next().unwrap_or_default()),
        Some("update") => {
            language();
            return update::run();
        }
        Some("translations") => {
            language();
            let _ = std::io::stdout().write_all(i18n::export().as_bytes());
            return Ok(());
        }
        Some("help" | "-h" | "--help") => {
            language();
            // not print!: piped into head, a closed pipe is no error
            let _ = std::io::stdout().write_all(manual::help().as_bytes());
            return Ok(());
        }
        Some("man") => {
            language();
            return manual::man();
        }
        Some("-V" | "--version") => {
            println!("rymus {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(other) => return Err(tr!("unknown command: {other}\nrymus help lists them", other = other)),
        None => {}
    }
    let program = platform::this_program();
    // before mpv exists, so an early signal cannot leave it behind
    platform::watch_for_quit();
    let path = db::default_path();
    language();
    let database = db::Db::open(&path).map_err(|e| tr!("cannot open {a}: {e}", a = path.display(), e = e))?;
    trace("db open");
    let volume = database.get_setting("volume").and_then(|v| v.parse().ok()).unwrap_or(100);
    // a player left playing by `Q` is taken over; the note is read once, so two runs never share one
    let left = database.get_setting("detached").and_then(|s| app::Detached::read(&s));
    database.set_setting("detached", "");
    let found = left.and_then(|l| mpv::Mpv::attach(&l.sock, &l.viz).map(|player| (player, l)));
    let ((player, mut measurements), left) = match found {
        Some((player, left)) => (player, Some(left)),
        None => {
            let started = mpv::Mpv::spawn(volume).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => platform::INSTALL_HINT.to_string(),
                _ => e.to_string(),
            })?;
            (started, None)
        }
    };
    let (tx, rx) = mpsc::channel::<Msg>();
    trace("mpv spawned");

    // the visualizer's measurements
    let feed = Arc::new(Mutex::new(viz::Feed::default()));
    let sink = feed.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n @ 1..) = measurements.read(&mut buf) {
            sink.lock().unwrap_or_else(|e| e.into_inner()).push(&buf[..n]);
        }
    });


    let mut app = App::new(database, path, tx.clone(), player, feed, volume);
    trace("App::new");
    if let Some(left) = left {
        app.reattach(left);
    }
    app.start();
    app.check_for_update();
    // after a restart (Cmd Shift R), back on the tab it was on
    if let Some(i) = app.db.get_setting("restart_tab").and_then(|v| v.parse::<usize>().ok()) {
        app.db.set_setting("restart_tab", "");
        if let Some(&tab) = app::TABS.get(i) {
            app.set_tab(tab);
        }
    }
    trace("app.start");
    let screen = Screen::enter().map_err(|e| e.to_string())?;
    trace("screen enter");

    // the first screen goes up before mpv is listening (it takes a tenth of a second or more to start); what the app
    // tells it meanwhile waits in Mpv and is sent once it connects
    let mut on_screen: Vec<String> = Vec::new();
    {
        let (w, h) = terminal::size().unwrap_or((80, 24));
        let frame = app.render(w as usize, h as usize);
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(changes(&frame, &mut on_screen).as_bytes()).and_then(|_| out.flush());
    }
    trace("first frame written");
    // mpv takes a fifth of a second to start (loading its libraries): the loop below reaches it without waiting, so
    // keys work from the first frame; until then what the app tells it waits in Mpv
    let mut events = Some(tx.clone());
    let reach_by = Instant::now() + Duration::from_secs(5);

    let outputs = tx.clone();
    output::watch(move || outputs.send(Msg::OutputChanged).is_ok());
    let input = tx;
    std::thread::spawn(move || loop {
        let msg = match event::read() {
            Ok(Event::Key(k)) => Msg::Key(k),
            Ok(Event::Mouse(m)) => Msg::Mouse(m),
            Ok(Event::Resize(..)) => Msg::Resize,
            Ok(_) => continue,
            Err(_) => Msg::Quit, // the terminal is gone
        };
        let gone = matches!(msg, Msg::Quit);
        if input.send(msg).is_err() || gone {
            break;
        }
    });

    let mut all_keys = false;
    let mut next_tick = Instant::now() + TICK;
    let mut draw_at = Some(Instant::now());
    let mut repainted = Instant::now(); // when every line was last written again
    while !app.quit && !platform::asked_to_quit() {
        if let Some(to) = events.as_ref() {
            if let Some(socket) = app.mpv.connect_now() {
                trace("mpv connected");
                let to = to.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(socket).lines().map_while(Result::ok) {
                        if let Ok(v) = serde_json::from_str(&line) {
                            let _ = to.send(Msg::Mpv(v));
                        }
                    }
                    let _ = to.send(Msg::Quit); // mpv is gone
                });
                events = None;
            } else if Instant::now() > reach_by {
                app.mpv.stop();
                return Err(tr!("cannot talk to mpv: it did not start").into());
            }
        }
        let mut wake = draw_at.map_or(next_tick, |d| d.min(next_tick));
        if events.is_some() {
            wake = wake.min(Instant::now() + Duration::from_millis(5)); // asked again soon
        }
        let (mut dirty, mut typed) = (false, false);
        match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(m) => {
                let mut next = Some(m);
                while let Some(m) = next {
                    typed |= matches!(m, Msg::Key(_) | Msg::Mouse(_));
                    // a key repaints every line (e.g. after the terminal was cleared by hand), at most once a second:
                    // holding ↓ through a list writes only the lines that change
                    if matches!(m, Msg::Key(_)) && repainted.elapsed() > Duration::from_secs(1) {
                        on_screen.clear();
                        repainted = Instant::now();
                    }
                    dirty |= app.handle(m);
                    next = rx.try_recv().ok();
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if all_keys == app.typing {
            all_keys = !app.typing;
            let mut out = std::io::stdout();
            let _ = out.write_all(if all_keys { ALL_KEYS } else { NORMAL_KEYS }).and_then(|_| out.flush());
        }
        let now = Instant::now();
        if now >= next_tick {
            dirty |= app.tick();
            next_tick = now + TICK;
        }
        // what you do shows at once; what moves by itself waits a moment, so several changes make one frame
        if dirty && typed {
            draw_at = Some(now);
        } else if dirty && draw_at.is_none() {
            draw_at = Some(now + FRAME_DELAY);
        }
        if draw_at.is_some_and(|d| Instant::now() >= d) && !app.quit {
            draw_at = None;
            let (w, h) = terminal::size().unwrap_or((80, 24));
            let frame = app.render(w as usize, h as usize);
            let mut out = std::io::stdout().lock();
            if std::mem::take(&mut app.clear_screen) {
                let _ = out.write_all(b"\x1b[2J");
                on_screen.clear();
            }
            if out.write_all(changes(&frame, &mut on_screen).as_bytes()).and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    let kept = app.detach && app.now().is_some();
    let restart = app.restart;
    app.shutdown();
    drop(screen);
    if restart {
        use std::os::unix::process::CommandExt;
        // replaces this process; the new one finds the player left playing. Only returns if it could not start.
        let error = std::process::Command::new(program.ok_or(tr!("cannot find rymus to restart it"))?).args(std::env::args_os().skip(1)).exec();
        return Err(tr!("could not restart: {error}{a}", error = error, a = if kept { tr!(" · the music keeps playing, run rymus again") } else { "" }));
    }
    if kept {
        println!("{}", tr!("The music keeps playing. Run rymus to control it; q there stops it."));
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_changed_lines_are_written() {
        let mut shown = Vec::new();
        assert_eq!(super::changes("\x1b[Ha\r\nb", &mut shown), "\x1b[1;1H\x1b[0ma\x1b[2;1H\x1b[0mb");
        assert_eq!(super::changes("\x1b[Ha\r\nc", &mut shown), "\x1b[2;1H\x1b[0mc");
        assert_eq!(super::changes("\x1b[Ha\r\nc", &mut shown), "");
    }
}
