// rymus (ymus in Rust): search YouTube Music and play it inside the terminal.
// mpv runs headless and is driven over its JSON IPC socket. No account: everything is anonymous.
// Likes, play history, search history, settings and the last session live in a local SQLite file (see db.rs).
//
// Where things are (each file starts by saying what it does):

// The app: its state and everything it does, and how it is drawn
mod app; //      state, keys, mouse, playback, lists; split by topic in app/
mod render; //   the whole screen as one string; split by view in render/
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
mod db; //       the local SQLite log: songs, plays, likes, playlists, settings
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
mod scenes; //   the full screen visualizer (V): its scenes, what they hear, how they change

#[cfg(test)]
mod flows; //    behaviour tests: the app driven by keys and mouse, judged by the screen

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

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--drain") {
        platform::drain(&args.next().unwrap_or_default());
        return Ok(());
    }
    let program = platform::this_program();
    // before mpv exists, so an early signal cannot leave it behind
    platform::watch_for_quit();
    let path = db::default_path();
    let database = db::Db::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let volume = database.get_setting("volume").and_then(|v| v.parse().ok()).unwrap_or(100);
    // a player left playing by `Q` is taken over; the note is read once, so two runs never share one
    let left = database.get_setting("detached").and_then(|s| app::Detached::read(&s));
    database.set_setting("detached", "");
    let found = left.and_then(|l| mpv::Mpv::attach(&l.sock, &l.viz).map(|player| (player, l)));
    let ((mut player, mut measurements), left) = match found {
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

    // the visualizer's measurements
    let feed = Arc::new(Mutex::new(viz::Feed::default()));
    let sink = feed.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n @ 1..) = measurements.read(&mut buf) {
            sink.lock().unwrap_or_else(|e| e.into_inner()).push(&buf[..n]);
        }
    });

    let socket = match player.connect() {
        Ok(s) => s,
        Err(e) => {
            player.stop();
            return Err(format!("cannot talk to mpv: {e}"));
        }
    };
    let events = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(socket).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str(&line) {
                let _ = events.send(Msg::Mpv(v));
            }
        }
        let _ = events.send(Msg::Quit); // mpv is gone
    });

    let mut app = App::new(database, path, tx.clone(), player, feed, volume);
    if let Some(left) = left {
        app.reattach(left);
    }
    app.start();
    let screen = Screen::enter().map_err(|e| e.to_string())?;

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
    let mut on_screen: Vec<String> = Vec::new();
    while !app.quit && !platform::asked_to_quit() {
        let wake = draw_at.map_or(next_tick, |d| d.min(next_tick));
        let mut dirty = false;
        match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(m) => {
                let mut next = Some(m);
                while let Some(m) = next {
                    if matches!(m, Msg::Key(_)) {
                        on_screen.clear(); // any key repaints all, e.g. after the terminal was cleared by hand
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
        if dirty && draw_at.is_none() {
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
        let error = std::process::Command::new(program.ok_or("cannot find rymus to restart it")?).args(std::env::args_os().skip(1)).exec();
        return Err(format!("could not restart: {error}{}", if kept { " · the music keeps playing, run rymus again" } else { "" }));
    }
    if kept {
        println!("The music keeps playing. Run rymus to control it; q there stops it.");
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
