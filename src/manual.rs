// `rymus help` and `rymus man`: what the commands, keys, settings and hooks are, from outside the app. The keys and
// settings come from the same tables the app draws, so they cannot fall behind.
use crate::config::SETTINGS;
use crate::i18n::t;
use crate::render::HELP;

const COMMANDS: [(&str, &str); 6] = [
    ("rymus", "open the player"),
    ("rymus ctl ACTION", "control the rymus that is playing"),
    ("rymus update", "install the latest rymus in place of this one, if there is a newer one"),
    ("rymus man", "read the manual (piped or redirected: write it as a man page)"),
    ("rymus help", "this help; also -h, --help. rymus --version (-V) says which one you run"),
    ("rymus translations", "every text with its translations, as CSV, to add a language"),
];
const CTL: [(&str, &str); 7] = [
    ("play-pause", "pause or go on"),
    ("next", "next song"),
    ("prev", "previous song"),
    ("volume-up", "5 louder"),
    ("volume-down", "5 quieter"),
    ("like", "like what plays, or take the like back (not while only the music plays on after Q)"),
    ("status", "one line: playing or paused, title and artist"),
];
const HOOKS: [(&str, &str); 12] = [
    ("liked", "you like a song"),
    ("unliked", "you remove the like, or dislike a liked song"),
    ("disliked", "you dislike a song"),
    ("playlist-add", "you add one song to a playlist"),
    ("playlist-remove", "you remove a song from a playlist"),
    ("playing", "a song starts"),
    ("played", "it reaches its end"),
    ("skipped", "you move on before its end"),
    ("download-started", "a song starts downloading"),
    ("downloaded", "a song's file is ready (its path in \"path\"; its cover, thumbnail and lyrics files in \"cover\", \"thumbnail\" and \"lyrics\", null when there are none)"),
    ("download-failed", "a download did not work"),
    ("download-deleted", "a downloaded file is deleted"),
];
const FILES: [(&str, &str); 3] = [
    ("~/.rymus/rymus.db", "likes, playlists, history and settings"),
    ("~/.rymus/hooks/NAME", "your executable for the event NAME"),
    ("~/.rymus/hooks.log", "what the hooks print"),
];
const ENVIRONMENT: [(&str, &str); 3] = [
    ("YM_DB", "use another database file"),
    ("YM_PLAIN=1", "no Nerd Font icons"),
    ("YM_MPV_ARGS", "more options for mpv"),
];
const ABOUT: &str = "Search YouTube Music and play it inside the terminal, without an account. Press / to search, Enter to play, Space to pause, ? for the keys and : to find any command.";

// Keys are drawn with symbols; a manual page must be plain ASCII to read well everywhere.
fn plain(s: &str) -> String {
    let words = [("⌥⌫", "Alt+Backspace"), ("⇧← ⇧→", "Shift+Left Shift+Right"), ("⇧space", "Shift+Space"), ("← →", "Left Right"), ("↑ ↓", "Up Down"), ("←", "Left"), ("→", "Right"), ("↑", "Up"), ("↓", "Down"), ("⏎", "Enter"), ("⇧", "Shift+"), ("·", "-"), ("…", "..."), ("—", "-"), ("✦", "*")];
    let s = words.iter().fold(s.to_string(), |s, (from, to)| s.replace(from, to));
    s.chars().filter(|c| c.is_ascii() || c.is_alphabetic() || matches!(c, '¿' | '¡')).collect()
}

fn columns(rows: &[(&str, &str)]) -> String {
    let w = rows.iter().map(|r| t(r.0).chars().count()).max().unwrap_or(0);
    rows.iter().map(|(a, b)| format!("  {}{}  {}\n", t(a), " ".repeat(w.saturating_sub(t(a).chars().count())), t(b))).collect()
}

pub fn help() -> String {
    let mut out = format!("rymus {} - {}\n\n{}\n\n{}\n{}\n{}\n{}", env!("CARGO_PKG_VERSION"), t("YouTube Music in the terminal"), t(ABOUT), t("Commands"), columns(&COMMANDS), t("Actions for rymus ctl"), columns(&CTL));
    out.push_str(&format!("\n{}\n", t("Keys (inside rymus)")));
    for (name, keys) in HELP {
        out.push_str(&format!(" {}\n{}", t(name), columns(keys)));
    }
    out.push_str(&format!("\n{}\n{}", t("Environment"), columns(&ENVIRONMENT)));
    out
}

// Text as roff takes it: backslashes shown, and no line mistaken for a request.
fn roff(s: &str) -> String {
    let s = plain(s).replace('\\', "\\e").replace('-', "\\-");
    if s.starts_with(['.', '\'']) { format!("\\&{s}") } else { s }
}
fn entries(rows: &[(&str, &str)]) -> String {
    rows.iter().map(|(a, b)| format!(".TP\n.B {}\n{}\n", roff(t(a)), roff(t(b)))).collect()
}

// A section heading, in capitals as manual pages have them.
fn section(name: &'static str) -> String {
    format!(".SH {}\n", roff(&t(name).to_uppercase()))
}

pub fn page() -> String {
    // the first line tells groff the text is UTF-8: translations have accents
    let mut out = format!(".\\\" -*- coding: utf-8 -*-\n.TH RYMUS 1 \"\" \"rymus {}\" \"{}\"\n", env!("CARGO_PKG_VERSION"), roff(t("User Commands")));
    out.push_str(&format!("{}rymus \\- {}\n", section("Name"), roff(t("YouTube Music in the terminal"))));
    out.push_str(&format!("{}.B rymus\n.br\n.B rymus ctl\n.I action\n.br\n.B rymus\n.RB [ man | help | translations | update | \\-\\-version ]\n", section("Synopsis")));
    out.push_str(&format!("{}{}\n{}{}.SS {}\n{}{}", section("Description"), roff(t(ABOUT)), section("Commands"), entries(&COMMANDS), roff(t("Actions for rymus ctl")), entries(&CTL), section("Keys")));
    for (name, keys) in HELP {
        out.push_str(&format!(".SS {}\n{}", roff(t(name)), entries(keys)));
    }
    out.push_str(&format!("{}{}\n", section("Settings"), roff(t("Changed in the Config tab, or in the windows it opens; the first value is the default."))));
    let mut group = "";
    for s in &SETTINGS {
        if s.group != group {
            group = s.group;
            out.push_str(&format!(".SS {}\n", roff(t(group))));
        }
        let values: Vec<&str> = crate::config::choices(s.key).iter().map(|c| t(c)).collect();
        out.push_str(&format!(".TP\n.B {}\n{}\n.br\n{}: {}.\n", roff(t(s.label)), roff(t(s.hint)), roff(t("Values")), roff(&values.join(", "))));
    }
    out.push_str(&format!("{}{}\n{}", section("Hooks"), roff(t("Executables of yours, named after what happens. Each gets one JSON object (event, id, url, title, artist, album, cover) on its standard input.")), entries(&HOOKS)));
    out.push_str(&format!("{}{}{}{}", section("Files"), entries(&FILES), section("Environment"), entries(&ENVIRONMENT)));
    out.push_str(&format!("{}mpv, yt\\-dlp, ffmpeg\n{}.BR mpv (1),\n.BR yt\\-dlp (1)\n.PP\nhttps://github.com/Skryensya/rymus\n", section("Requirements"), section("See also")));
    out
}

// `rymus man`: opens the page in man; into a pipe or a file, writes the page itself.
pub fn man() -> Result<(), String> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdout().is_terminal() {
        return std::io::stdout().write_all(page().as_bytes()).map_err(|e| e.to_string());
    }
    // in your own data folder, not a shared one where someone else could put a file in its place
    let file = crate::db::default_path().with_file_name("rymus.1");
    if let Some(dir) = file.parent() {
        crate::platform::private_dir(dir);
    }
    std::fs::write(&file, page()).map_err(|e| e.to_string())?;
    let shown = std::process::Command::new("man").arg(&file).status().map_err(|e| format!("man: {e}"))?;
    if shown.success() { Ok(()) } else { Err("man could not show the page".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_and_the_help_cover_the_commands_keys_settings_and_hooks() {
        let (page, help) = (page(), help());
        assert!(!page.chars().any(|c| c.is_control() && c != '\n') && page.lines().nth(1).is_some_and(|l| l.starts_with(".TH RYMUS 1")), "{}", &page[..200]);
        assert!(page.lines().all(|l| !l.starts_with('\'')), "no line read as a request by accident");
        for part in ["play\\-pause", "status", "Crossfade", "playlist\\-add", "downloaded", "Shift+Left", "YM_DB", ".SS Playback"] {
            assert!(page.contains(part), "{part} is missing");
        }
        for part in ["rymus ctl ACTION", "sleep timer", "volume-up", "YM_PLAIN"] {
            assert!(help.contains(part), "{part} is missing");
        }
    }
}
