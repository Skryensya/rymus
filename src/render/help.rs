// Help for the keys: the hints line at the bottom and the ? page.
use crate::i18n::t;
use crate::i18n::tr;
use super::*;

impl App {
    pub(super) fn hint_list(&mut self) -> Vec<(&'static str, &'static str)> {
        if self.help {
            return vec![("space", tr!("play / pause")), ("esc", "close")];
        }
        // the equalizer's keys are written at the foot of its window
        if (self.eq.open || self.downloads.open) && self.menu.is_none() && !self.typing && !self.lyrics && !self.full {
            return vec![("esc", tr!("back / close")), ("space", tr!("play / pause")), ("+ -", "volume")];
        }
        let pause = if self.paused { "resume" } else { "pause" };
        if self.menu.is_some() {
            return vec![("↑↓", "move"), ("⏎", "run"), ("esc", "close")];
        }
        if self.lyrics && !self.typing {
            let mut v = if self.karaoke {
                vec![("Y", tr!("close karaoke")), ("y", "words"), ("f", "letters")]
            } else {
                vec![("y", tr!("close lyrics")), ("↑↓", "scroll"), ("Y", "karaoke")]
            };
            v.extend([("l", "like"), ("x", "dislike"), ("space", pause), ("n p", "next/prev")]);
            v.extend([("⇧←→", "seek"), ("+ -", "volume"), ("?", "keys")]);
            return v;
        }
        if self.typing {
            if self.naming.is_some() {
                return vec![("⏎", "save"), ("⌥⌫", tr!("erase word")), ("^U", "clear"), ("esc", "cancel")];
            }
            return vec![("⏎", tr!("search or import link")), ("⌥⌫", tr!("erase word")), ("^U", "clear"), ("↑↓", "history"), ("esc", "cancel")];
        }
        if self.full && self.trip_on {
            let react = if self.wave { tr!("react to music on") } else { tr!("react to music off") };
            let lyrics = if self.trip_lyrics { tr!("lyrics on") } else { tr!("lyrics off") };
            return vec![("[ ]", "scene"), (".", "hold"), ("y", lyrics), ("V", "close"), ("space", pause), ("w", react), ("n p", "next/prev"), ("?", "keys")];
        }
        if self.full {
            return vec![("l", "like"), ("x", "dislike"), ("v", "close"), ("y", "lyrics"), ("space", pause), ("n p", "next/prev"), ("+ -", "volume"), ("?", "keys")];
        }
        let kind = self.kind_at();
        let enter = match kind {
            _ if self.tab == Tab::Config && self.config_menu => "open",
            Kind::Resume => "resume",
            Kind::Mix => tr!("play mix"),
            Kind::Button if self.tab == Tab::Config => "choose",
            Kind::Button => "play",
            Kind::Setting => "change",
            Kind::Track if self.cur().is_some_and(|c| self.is_current(c)) => pause,
            Kind::Track if self.tab == Tab::Queue => "jump",
            Kind::Track => "play",
            _ => "open",
        };
        let mut k = vec![("⏎", enter), ("/", "search"), ("?", "keys"), ("tab", "sections")];
        if self.now().is_some() {
            k.insert(1, ("space", pause));
            k.push(("y", "lyrics"));
        }
        if self.tab == Tab::Home {
            k.push(("D", "discover"));
            if self.home_wide {
                k.push(("←→", tr!("Listen/Library")));
            }
        }
        if kind == Kind::Playlist {
            k.extend([("m", "play/rename"), ("d", "delete")]);
        }
        if self.tab == Tab::Stats {
            k.extend([("←→", "songs/artists/albums"), ("t", "week/month/year")]);
        }
        if matches!(self.tab, Tab::Liked | Tab::Queue | Tab::History | Tab::Playlists) {
            k.push(("f", "filter"));
        }
        if self.tab == Tab::Library {
            k.extend([("d", "remove"), ("f", "filter")]);
        }
        if self.tab == Tab::Liked {
            k.push(("←→", tr!("by album")));
        }
        if self.results().is_some() {
            k.push(("←→", "songs/albums/artists"));
        }
        if self.tab == Tab::Search {
            k.push(("esc", "back"));
        }
        if self.tab == Tab::Queue {
            k.extend([("d", "remove"), ("J K", "move"), ("c", "clear"), ("u", "undo")]);
        } else if kind == Kind::Track {
            if self.viewed_playlist().is_some() {
                k.push(("d", "remove"));
            }
            k.extend([("s", tr!("to playlist")), ("l", "like"), ("x", "dislike"), ("a", "queue"), ("R", "radio")]);
        }
        k.extend([("+ -", "volume"), ("⇧←→", "seek")]);
        if kind == Kind::Track {
            k.push(("m", "menu"));
        }
        k.extend([("v", "player"), ("q", "quit")]);
        k
    }
}

// The ? page: short entries under a few headings, so a key is found by looking, not reading.
pub type Section = (&'static str, &'static [(&'static str, &'static str)]);
pub const HELP: [Section; 7] = [
    ("Playback", &[("space", "play / pause"), ("n  p", "next / previous"), ("r", "repeat: queue, song, off"), ("⇧← ⇧→", "seek 5 s"), ("+  -", "volume")]),
    ("Move", &[("↑ ↓", "move"), ("⏎ ⇧space", "open / play"), ("esc", "back, then Home"), ("tab", "next section"), ("← →", "Home column")]),
    ("Song", &[("a", "add to queue"), ("m", "play next, more…"), ("l", "like song or album"), ("x", "dislike"), ("s", "save to playlist"), ("R", "radio"), ("o  O", "playing: album, artist"), ("i", "credits, share link"), ("F", "follow artist"), ("b", "download"), ("B", "downloads and your music")]),
    ("Queue & playlists", &[("d", "remove / delete"), ("J  K", "move in queue"), ("c", "clear the queue"), ("m", "rename, play")]),
    ("Search", &[("/", "search"), ("f", "filter this list"), (":", "any command"), ("← →", "result tabs"), ("↑ ↓", "past searches"), ("⌥⌫", "erase word"), ("ctrl U", "erase all"), ("paste link", "import playlist")]),
    ("Views", &[("v", "full player"), ("V", "trip · [ ] scene · . hold"), ("y", "lyrics"), ("Y", "karaoke · f letters"), ("w", "visualizer style"), ("W", "visualizer on / off"), ("E", "equalizer")]),
    ("More", &[("D", "discover"), ("z", "sleep timer"), ("u", "undo"), ("← → t", "Stats, Liked views"), ("q", "quit"), ("Q", "quit, music stays"), ("ctrl R", "restart")]),
];
const MOUSE: &str = "mouse: click · double-click plays · right-click menu · drag the bar to seek · shift-click a cover to see it big";

// The sections laid out in as many columns as fit, each section going to the shortest column; rows are exactly w wide.
pub(super) fn help_rows(w: usize, a: Rgb) -> Vec<String> {
    let n = (w / 34).clamp(1, 3);
    let cw = w / n;
    let mut columns: Vec<Vec<String>> = vec![Vec::new(); n];
    for (title, keys) in HELP {
        let shortest = (0..n).min_by_key(|&i| columns[i].len()).unwrap_or(0);
        let column = &mut columns[shortest];
        column.push(format!(" {}", fg(a, &bold(&cell(&t(title).to_uppercase(), cw.saturating_sub(1))))));
        for (key, what) in keys {
            let k = cell(t(key), 11.min(cw.saturating_sub(2)));
            let rest = cell(t(what), cw.saturating_sub(2 + width(&k)));
            column.push(format!("  {}{rest}", bold(&k)));
        }
        column.push(spaces(cw));
    }
    let height = columns.iter().map(Vec::len).max().unwrap_or(1) - 1; // no blank line at the very end
    let mut rows: Vec<String> = (0..height)
        .map(|i| {
            let line: String = columns.iter().map(|c| c.get(i).cloned().unwrap_or_else(|| spaces(cw))).collect();
            line + &spaces(w - cw * n)
        })
        .collect();
    rows.push(dim(&cell(&format!(" {}", t(MOUSE)), w)));
    rows
}
