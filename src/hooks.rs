// Your own commands, run on what happens in rymus: an executable in `hooks/` next to the database (~/.rymus/hooks/),
// named after the event, gets the song as one JSON object on its standard input. rymus knows nothing about what it
// does. Titles come from outside, so they only ever travel in that JSON, never in a command line.
//
// Events: liked, unliked, disliked, playlist-add, playlist-remove, playing, played, skipped, download-started,
// downloaded, download-failed, download-deleted.
use crate::i18n::tr;
use crate::item::Item;
use crate::platform;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn payload(event: &str, t: &Item, mut extra: Value) -> Value {
    // a file of your own has no page on YouTube: its path instead
    let (url, path) = match t.id.strip_prefix(crate::disk::OWN) {
        Some(path) => (Value::Null, json!(path)),
        None => (json!(format!("https://music.youtube.com/watch?v={}", t.id)), Value::Null),
    };
    let mut out = json!({"event": event, "id": t.id, "url": url, "title": t.title, "artist": t.artist, "album": t.album, "cover": t.cover});
    if !path.is_null() {
        out["path"] = path;
    }
    if let (Some(out), Some(extra)) = (out.as_object_mut(), extra.as_object_mut()) {
        out.append(extra);
    }
    out
}

// Starts the hook for `event`, if you have one, and calls `failed` from another thread if it did not go well. It runs
// on its own: the music does not wait for it, and quitting rymus does not stop it. What it prints goes to hooks.log.
// shortcut: every event starts its process at once, so two quick ones can finish out of order; queue them per song if
// a hook ever depends on the order
pub fn run(data: &Path, event: &'static str, t: &Item, extra: Value, failed: impl FnOnce(String) + Send + 'static) {
    let hook = data.join("hooks").join(event);
    if !hook.is_file() {
        return;
    }
    let log = data.join("hooks.log");
    let output = || std::fs::OpenOptions::new().create(true).append(true).open(&log).map_or_else(|_| Stdio::null(), Stdio::from);
    let mut command = Command::new(&hook);
    command.stdin(Stdio::piped()).stdout(output()).stderr(output());
    platform::detached(&mut command);
    let input = format!("{}\n", payload(event, t, extra));
    std::thread::spawn(move || {
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return failed(tr!("Hook {event} could not start ({e}): is it executable?", event = event, e = e)),
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes()); // a hook may well not read it
        }
        if !child.wait().is_ok_and(|status| status.success()) {
            failed(tr!("Hook {event} failed, see {a}", event = event, a = log.display()));
        }
    });
}
