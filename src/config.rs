// The Config tab: player settings, each a short list of choices kept in the settings table. The first choice is the default.
use crate::i18n::t;
use crate::i18n::tr;
use crate::db::Db;
use crate::item::{Item, Kind};

pub struct Setting {
    pub key: &'static str,
    pub label: &'static str,
    pub choices: &'static [&'static str],
    pub hint: &'static str,
    pub group: &'static str, // a heading over it when it differs from the one before
}

pub const SETTINGS: [Setting; 17] = [
    Setting { key: "fade", label: "Crossfade", choices: &["off", "2 s", "4 s", "6 s", "8 s", "12 s"], hint: "the end of each song plays over the start of the next one, one fading out while the other fades in", group: "Sound" },
    Setting { key: "normalize", label: "Even out loudness", choices: &["off", "on"], hint: "quiet and loud songs play at a similar level", group: "Sound" },
    Setting { key: "channels", label: "Channels", choices: &["stereo", "mono"], hint: "mono plays the same mix of left and right on both sides, for one earbud or one good ear", group: "Sound" },
    Setting { key: "quality", label: "Audio quality", choices: &["best", "data saver"], hint: "data saver streams the smallest audio there is, for slow or metered connections", group: "Sound" },
    Setting { key: "outputpause", label: "Pause on output change", choices: &["on", "off"], hint: "pause when headphones are unplugged or the audio output changes", group: "Sound" },
    Setting { key: "mode", label: "Mode", choices: &["dark", "light"], hint: "match your terminal's background: every color is chosen to read well on it, the accent included", group: "Look" },
    Setting { key: "accent", label: "Color", choices: &["from cover", "red", "coral", "orange", "yellow", "lime", "green", "teal", "cyan", "blue", "purple", "magenta", "pink", "white"], hint: "the color of the frames, the selection and the player: taken from what plays, or always the same", group: "Look" },
    Setting { key: "player", label: "Player", choices: &["bottom", "top"], hint: "where the player sits: under the lists, or above them", group: "Look" },
    Setting { key: "border", label: "Borders", choices: &["rounded", "square", "heavy", "double", "dashed", "dotted", "deco", "block", "ascii", "ornate"], hint: "how the frames are drawn: lines, dashes, dots, blocks, or ornate scrolls in plain ASCII", group: "Look" },
    Setting { key: "pixel", label: "Pixel art covers", choices: &["off", "on"], hint: "draw every cover as colored blocks, even where the terminal can show the image", group: "Screen" },
    Setting { key: "wave", label: "Visualizer", choices: &["on", "off"], hint: "the moving spectrum in the player (also W)", group: "Screen" },
    Setting { key: "vizstyle", label: "Visualizer style", choices: &["dots", "bars", "line", "mirror"], hint: "how the spectrum is drawn: dots rising, solid bars, a line of dots, or a mirror growing from the middle (also w, one after another)", group: "Screen" },
    Setting { key: "download", label: "Keep offline", choices: &["off", "liked songs"], hint: "download every song you like from now on; it plays from the file, with or without network (b downloads any song)", group: "Downloads" },
    Setting { key: "dlformat", label: "File format", choices: &["mp3", "m4a", "opus", "original"], hint: "what downloaded songs are saved as; original keeps what YouTube sends, without converting", group: "Downloads" },
    Setting { key: "library", label: "Your own music", choices: &["off", "on"], hint: "the songs already in the folder, yours from anywhere (mp3, flac, m4a…), show in Library and play from disk", group: "Downloads" },
    Setting { key: "language", label: "Language", choices: &[], hint: "the language of the app, rymus help and the manual; add one with rymus translations (see the manual)", group: "General" },
    Setting { key: "updates", label: "Check for updates", choices: &["on", "off"], hint: "once a day rymus asks GitHub for the name of its latest release, and tells you when there is a newer one; nothing about you is sent", group: "General" },
];

// Most settings offer a fixed list; the language offers what the translations have.
fn offered(s: &'static Setting) -> &'static [&'static str] {
    if s.key == "language" { crate::i18n::languages() } else { s.choices }
}
fn setting(key: &str) -> &'static Setting {
    SETTINGS.iter().find(|s| s.key == key).expect("known setting")
}

// The stored choice, or the default when there is none (or it is no longer offered).
pub fn value(db: &Db, key: &str) -> &'static str {
    let s = setting(key);
    let stored = db.get_setting(key).map(|v| if v == "baroque" { "ornate".to_string() } else { v });
    let offered = offered(s);
    offered.iter().find(|c| Some(**c) == stored.as_deref()).unwrap_or(&offered[0])
}

// Stores one of the choices offered (a click on it).
pub fn choose(db: &Db, key: &str, v: &str) -> Option<&'static str> {
    let v = *offered(setting(key)).iter().find(|c| **c == v)?;
    db.set_setting(key, v);
    Some(v)
}

// Moves to the next (dir 1) or previous (dir -1) choice, wrapping around, and stores it.
pub fn step(db: &Db, key: &str, dir: i64) -> &'static str {
    let s = setting(key);
    let offered = offered(s);
    let at = offered.iter().position(|c| *c == value(db, key)).unwrap_or(0) as i64;
    let v = offered[(at + dir).rem_euclid(offered.len() as i64) as usize];
    db.set_setting(key, v);
    v
}

// The Config tab's rows: each group's heading, then its settings. author holds the chosen value, detail the hint.
pub fn rows(db: &Db) -> Vec<Item> {
    let mut rows = Vec::new();
    for (i, s) in SETTINGS.iter().enumerate() {
        if i == 0 || SETTINGS[i - 1].group != s.group {
            rows.push(Item { kind: Kind::Header, title: t(s.group).into(), ..Default::default() });
        }
        rows.push(Item { kind: Kind::Setting, id: s.key.into(), title: t(s.label).into(), author: value(db, s.key).into(), detail: Some(t(s.hint).into()), ..Default::default() });
        if s.key == "updates" {
            let latest = db.get_setting("update_latest").unwrap_or_default();
            let title = if crate::update::newer(&latest, crate::update::VERSION) {
                tr!("Update to {tag}…", tag = latest)
            } else {
                tr!("rymus {v} · check now…", v = crate::update::VERSION)
            };
            let hint = tr!("downloads the new rymus, checks it, and puts it in place of this one; Cmd Shift R then restarts into it");
            rows.push(Item { kind: Kind::Button, id: "update".into(), title, detail: Some(hint.into()), ..Default::default() });
        }
        if s.key == "library" {
            // the folder, the room it takes and freeing some live in their own window (B)
            let folder = db.get_setting("dlfolder").filter(|f| !f.is_empty()).unwrap_or_else(|| "~/Music/rymus".into());
            let hint = tr!("keep songs as files, read your own music, see the room it takes and free some (also B from anywhere)");
            rows.push(Item { kind: Kind::Button, id: "downloads".into(), title: tr!("Downloads and your music: {folder}…", folder = folder), detail: Some(hint.into()), ..Default::default() });
        }
        if s.key == "channels" {
            let title = tr!("Equalizer: {a}…", a = crate::i18n::any(&db.get_setting("eq").unwrap_or_else(|| "Flat".into())));
            let hint = tr!("ten bands, ready made profiles and your own (also E from anywhere)");
            rows.push(Item { kind: Kind::Button, id: "eq".into(), title, detail: Some(hint.into()), ..Default::default() });
        }
    }
    // what you do with all of it at once, at the end of General: each asks first
    for (id, title, hint) in [
        ("export", tr!("Export your data…"), tr!("everything (likes, playlists, history, settings) as CSV files in a new folder: open them in any spreadsheet, keep them, or take them to another computer")),
        ("import", tr!("Import data…"), tr!("replace everything here with an exported folder: what is here now is lost, so export it first if you want to keep it")),
        ("wipe", tr!("Delete all my data…"), tr!("erases your likes, playlists, listening history, searches and settings for good, and rymus starts empty, as on its first run. To be sure, you type \"delete all my data\"")),
    ] {
        rows.push(Item { kind: Kind::Button, id: id.into(), title: title.into(), detail: Some(hint.into()), ..Default::default() });
    }
    rows
}
pub fn label(key: &str) -> &'static str {
    SETTINGS.iter().find(|s| s.key == key).map_or("", |s| s.label)
}
pub fn choices(key: &str) -> &'static [&'static str] {
    SETTINGS.iter().find(|s| s.key == key).map_or(&[], offered)
}

pub fn fade_secs(v: &str) -> f64 {
    v.split(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0.0)
}

// Where the crossfade starts: None when it is off or the song is too short to fade.
pub fn fade_at(secs: f64, dur: f64) -> Option<f64> {
    (secs > 0.0 && dur >= secs * 3.0).then_some(dur - secs)
}

// The next song fades in on the main player while a second one plays this song's end fading out (Mpv::tail).
// The filter is put in once and only its expression changes (af-command): adding or removing one is heard.
pub const FADE: &str = "@fade:lavfi=graph=%31%volume@fade=volume=1:eval=frame";
pub fn fade_in(secs: f64) -> String {
    format!("min(1,t/{secs})") // t restarts at 0 with every song
}

// A fixed accent color, or None to take it from the cover of what plays.
pub fn accent(v: &str) -> Option<[u8; 3]> {
    Some(match v {
        // as they look on a dark background; a light one darkens them (ui::readable)
        "red" => [240, 120, 120],
        "coral" => [250, 140, 115],
        "orange" => [250, 170, 100],
        "yellow" => [240, 210, 120],
        "lime" => [180, 225, 100],
        "green" => crate::ui::GREEN,
        "teal" => [90, 205, 185],
        "cyan" => [110, 205, 235],
        "blue" => [130, 170, 250],
        "purple" => [190, 150, 245],
        "magenta" => [230, 125, 230],
        "pink" => [245, 150, 200],
        "white" => [225, 225, 225],
        _ => return None,
    })
}

pub const NORMALIZE: &str = "@norm:lavfi=graph=%16%dynaudnorm=f=500";

pub fn format(quality: &str) -> &'static str {
    if quality == "data saver" { "worstaudio/worst" } else { "bestaudio/best" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_default_step_and_wrap() {
        let dir = std::env::temp_dir().join(format!("rymus-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Db::open(&dir.join("t.db")).unwrap();
        assert_eq!(value(&db, "fade"), "off");
        assert_eq!(step(&db, "fade", 1), "2 s");
        assert_eq!(value(&db, "fade"), "2 s");
        assert_eq!(step(&db, "fade", -1), "off");
        assert_eq!(step(&db, "fade", -1), "12 s");
        db.set_setting("wave", "off"); // what the w key stores
        assert_eq!(value(&db, "wave"), "off");
        db.set_setting("quality", "bogus");
        assert_eq!(value(&db, "quality"), "best");
        let rows = rows(&db);
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Setting).count(), SETTINGS.len());
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Header).map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Sound", "Look", "Screen", "Downloads", "General"]);
    }

    #[test]
    fn fade() {
        assert_eq!(fade_secs("off"), 0.0);
        assert_eq!(fade_secs("12 s"), 12.0);
        assert_eq!(fade_at(0.0, 200.0), None);
        assert_eq!(fade_at(4.0, 10.0), None);
        assert_eq!(fade_at(4.0, 200.0), Some(196.0));
        assert_eq!(fade_in(4.0), "min(1,t/4)");
        let g = "volume@fade=volume=1:eval=frame";
        assert_eq!(FADE, format!("@fade:lavfi=graph=%{}%{g}", g.len()));
    }
}
