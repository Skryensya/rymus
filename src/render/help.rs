// Help for the keys: the hints line at the bottom and the ? page.
use super::*;

impl App {
    pub(super) fn hint_list(&mut self) -> Vec<(&'static str, &'static str)> {
        if self.help {
            return vec![("space", "play / pause"), ("esc", "close")];
        }
        let on_eq = self.tab == Tab::Eq && self.menu.is_none() && !self.typing && !self.lyrics && !self.full;
        if on_eq && self.eq.band.is_some() {
            return vec![("← →", "band"), ("↑ ↓", "gain"), ("0", "back to 0 dB"), ("esc", "profiles"), ("space", "play / pause")];
        }
        if on_eq {
            return vec![("↑ ↓", "profile, heard at once"), ("→ ⏎", "sliders"), ("n", "new"), ("d", "delete yours"), ("tab", "sections"), ("space", "play / pause")];
        }
        let pause = if self.paused { "resume" } else { "pause" };
        if self.menu.is_some() {
            return vec![("↑↓", "move"), ("⏎", "run"), ("esc", "close")];
        }
        if self.lyrics && !self.typing {
            let mut v = if self.karaoke {
                vec![("Y", "close karaoke"), ("y", "words")]
            } else {
                vec![("y", "close lyrics"), ("↑↓", "scroll"), ("Y", "karaoke")]
            };
            v.extend([("l", "like"), ("x", "dislike"), ("space", pause), ("n p", "next/prev")]);
            v.extend([("⇧←→", "seek"), ("+ -", "volume"), ("?", "keys")]);
            return v;
        }
        if self.typing {
            if self.naming.is_some() {
                return vec![("⏎", "save"), ("⌥⌫", "erase word"), ("^U", "clear"), ("esc", "cancel")];
            }
            return vec![("⏎", "search or import link"), ("⌥⌫", "erase word"), ("^U", "clear"), ("↑↓", "history"), ("esc", "cancel")];
        }
        if self.full && self.trip_on {
            let react = if self.wave { "react to music on" } else { "react to music off" };
            let lyrics = if self.trip_lyrics { "lyrics on" } else { "lyrics off" };
            return vec![("[ ]", "scene"), (".", "hold"), ("y", lyrics), ("V", "close"), ("space", pause), ("w", react), ("n p", "next/prev"), ("?", "keys")];
        }
        if self.full {
            return vec![("l", "like"), ("x", "dislike"), ("v", "close"), ("y", "lyrics"), ("space", pause), ("n p", "next/prev"), ("+ -", "volume"), ("?", "keys")];
        }
        let kind = self.kind_at();
        let enter = match kind {
            Kind::Resume => "resume",
            Kind::Mix => "play mix",
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
                k.push(("←→", "Listen/Library"));
            }
        }
        if kind == Kind::Playlist {
            k.extend([("m", "play/rename"), ("d", "delete")]);
        }
        if self.tab == Tab::Stats {
            k.extend([("←→", "songs/artists/albums"), ("t", "week/month/year")]);
        }
        if self.tab == Tab::Search {
            k.push(("esc", "back"));
        }
        if self.tab == Tab::Queue {
            k.extend([("d", "remove"), ("J K", "move")]);
        } else if kind == Kind::Track {
            if self.viewed_playlist().is_some() {
                k.push(("d", "remove"));
            }
            k.extend([("s", "to playlist"), ("l", "like"), ("x", "dislike"), ("a", "queue"), ("R", "radio")]);
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
type Section = (&'static str, &'static [(&'static str, &'static str)]);
const HELP: [Section; 7] = [
    ("Playback", &[("space", "play / pause"), ("n  p", "next / previous"), ("r", "repeat: queue, song, off"), ("⇧← ⇧→", "seek 10 s"), ("+  -", "volume")]),
    ("Move", &[("↑ ↓", "move"), ("⏎ ⇧space", "open / play"), ("esc", "back, then Home"), ("tab", "next section"), ("← →", "Home column")]),
    ("Song", &[("a", "add to queue"), ("m", "play next, more…"), ("l", "like"), ("x", "dislike"), ("s", "save to playlist"), ("R", "radio")]),
    ("Queue & playlists", &[("d", "remove / delete"), ("J  K", "move in queue"), ("m", "rename, play")]),
    ("Search", &[("/", "search"), ("↑ ↓", "past searches"), ("⌥⌫", "erase word"), ("ctrl U", "erase all"), ("paste link", "import playlist")]),
    ("Views", &[("v", "full player"), ("V", "trip · [ ] scene · . hold"), ("y", "lyrics"), ("Y", "karaoke"), ("w", "visualizer on / off"), ("E", "the EQ tab")]),
    ("More", &[("D", "discover"), ("← → t", "Stats filters"), ("q", "quit"), ("Q", "quit, music stays"), ("ctrl R", "restart")]),
];
const MOUSE: &str = "mouse: click · double-click plays · right-click menu · drag the bar to seek";

// The sections laid out in as many columns as fit, each section going to the shortest column; rows are exactly w wide.
pub(super) fn help_rows(w: usize, a: Rgb) -> Vec<String> {
    let n = (w / 34).clamp(1, 3);
    let cw = w / n;
    let mut columns: Vec<Vec<String>> = vec![Vec::new(); n];
    for (title, keys) in HELP {
        let shortest = (0..n).min_by_key(|&i| columns[i].len()).unwrap_or(0);
        let column = &mut columns[shortest];
        column.push(format!(" {}", fg(a, &bold(&cell(&title.to_uppercase(), cw.saturating_sub(1))))));
        for (key, what) in keys {
            let k = cell(key, 11.min(cw.saturating_sub(2)));
            let rest = cell(what, cw.saturating_sub(2 + width(&k)));
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
    rows.push(dim(&cell(&format!(" {MOUSE}"), w)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::visible;

    #[test]
    fn help_fits_its_width() {
        for w in [30, 70, 98, 110, 200] {
            let rows = help_rows(w, GREEN);
            assert!(rows.iter().all(|r| width(&visible(r)) == w), "width {w}");
        }
        let wide = help_rows(110, GREEN);
        assert!(wide.len() <= 22, "three columns fit a small screen: {}", wide.len());
        assert!(wide.iter().any(|r| visible(r).contains("add to queue")));
    }
}
