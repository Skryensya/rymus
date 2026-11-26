// The Config tab: player settings, each a short list of choices kept in the settings table. The first choice is the default.
use crate::db::Db;
use crate::item::{Item, Kind};

pub struct Setting {
    pub key: &'static str,
    pub label: &'static str,
    pub choices: &'static [&'static str],
    pub hint: &'static str,
    pub group: &'static str, // a heading over it when it differs from the one before
}

pub const SETTINGS: [Setting; 10] = [
    Setting { key: "fade", label: "Crossfade", choices: &["off", "2 s", "4 s", "6 s", "8 s", "12 s"], hint: "the end of each song plays over the start of the next one, one fading out while the other fades in", group: "Sound" },
    Setting { key: "normalize", label: "Even out loudness", choices: &["off", "on"], hint: "quiet and loud songs play at a similar level", group: "Sound" },
    Setting { key: "quality", label: "Audio quality", choices: &["best", "data saver"], hint: "data saver streams the smallest audio there is, for slow or metered connections", group: "Streaming" },
    Setting { key: "pixel", label: "Pixel art covers", choices: &["off", "on"], hint: "draw every cover as colored blocks, even where the terminal can show the image", group: "Screen" },
    Setting { key: "wave", label: "Visualizer", choices: &["on", "off"], hint: "the moving spectrum in the player (also w)", group: "Screen" },
    Setting { key: "vizstyle", label: "Visualizer style", choices: &["dots", "bars", "line", "mirror"], hint: "how the spectrum is drawn: dots rising, solid bars, a line of dots, or a mirror growing from the middle", group: "Screen" },
    Setting { key: "accent", label: "Color", choices: &["from cover", "green", "blue", "purple", "pink", "orange", "yellow", "red", "white"], hint: "the color of the frames, the selection and the player: taken from what plays, or always the same", group: "Look" },
    Setting { key: "player", label: "Player", choices: &["bottom", "top"], hint: "where the player sits: under the lists, or above them", group: "Look" },
    Setting { key: "border", label: "Borders", choices: &["rounded", "square", "heavy", "double"], hint: "how the frames and the player's buttons are drawn", group: "Look" },
    Setting { key: "outputpause", label: "Pause on output change", choices: &["on", "off"], hint: "pause when headphones are unplugged or the audio output changes", group: "Output" },
];

fn setting(key: &str) -> &'static Setting {
    SETTINGS.iter().find(|s| s.key == key).expect("known setting")
}

// The stored choice, or the default when there is none (or it is no longer offered).
pub fn value(db: &Db, key: &str) -> &'static str {
    let s = setting(key);
    let stored = db.get_setting(key);
    s.choices.iter().find(|c| Some(**c) == stored.as_deref()).unwrap_or(&s.choices[0])
}

// Moves to the next (dir 1) or previous (dir -1) choice, wrapping around, and stores it.
pub fn step(db: &Db, key: &str, dir: i64) -> &'static str {
    let s = setting(key);
    let at = s.choices.iter().position(|c| *c == value(db, key)).unwrap_or(0) as i64;
    let v = s.choices[(at + dir).rem_euclid(s.choices.len() as i64) as usize];
    db.set_setting(key, v);
    v
}

// The Config tab's rows: each group's heading, then its settings. author holds the chosen value, detail the hint.
pub fn rows(db: &Db) -> Vec<Item> {
    let mut rows = Vec::new();
    for (i, s) in SETTINGS.iter().enumerate() {
        if i == 0 || SETTINGS[i - 1].group != s.group {
            rows.push(Item { kind: Kind::Header, title: s.group.into(), ..Default::default() });
        }
        rows.push(Item { kind: Kind::Setting, id: s.key.into(), title: s.label.into(), author: value(db, s.key).into(), detail: Some(s.hint.into()), ..Default::default() });
    }
    rows
}
pub fn choices(key: &str) -> &'static [&'static str] {
    SETTINGS.iter().find(|s| s.key == key).map_or(&[], |s| s.choices)
}

pub fn fade_secs(v: &str) -> f64 {
    v.split(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0.0)
}

// Where the crossfade starts: None when it is off or the song is too short to fade.
pub fn fade_at(secs: f64, dur: f64) -> Option<f64> {
    (secs > 0.0 && dur >= secs * 3.0).then(|| dur - secs)
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
        "green" => crate::ui::GREEN,
        "blue" => [130, 170, 250],
        "purple" => [190, 150, 245],
        "pink" => [245, 150, 200],
        "orange" => [250, 170, 100],
        "yellow" => [240, 210, 120],
        "red" => [240, 120, 120],
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
        let dir = std::env::temp_dir().join(format!("ymus-config-{}", std::process::id()));
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
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Header).map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Sound", "Streaming", "Screen", "Look", "Output"]);
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
