// Draws the whole screen into one string. Every line is exactly as wide as the terminal.
use crate::i18n::tr;
use crate::app::{fmt, minutes, Action, App, KINDS, Control, Geo, Repeat, Marquee, Naming, Pane, PaneId, Tab, CATS, PERIODS};
use crate::cover::{self, Got};
use crate::item::{Item, Kind, Origin};
use crate::lyrics::{self, Lyrics};
use crate::charts;
use crate::ui::*;
use crate::db::now_ms;
use std::sync::Arc;
use std::time::{Duration, Instant};

// The screen is drawn by `render` (below), which asks these for their part.
mod rows;
mod album;
mod player;
mod words;
mod home;
mod stats;
mod images;
mod help;
mod eq;
mod config;
mod downloads;
use help::help_rows;
pub use help::HELP;

// Text that does not fit scrolls sideways (only used on the focused row): it holds for a moment, then loops.
const MQ_STEP: u128 = 200;
const MQ_HOLD: usize = 2;

pub fn marquee_step(m: &Marquee) -> usize {
    m.at.map_or(0, |at| ((at.elapsed().as_millis() / MQ_STEP) as usize).saturating_sub(MQ_HOLD))
}

fn marquee(m: &mut Marquee, text: &str, w: usize) -> String {
    if width(text) <= w {
        return pad(text, w);
    }
    m.on = true;
    m.step = marquee_step(m);
    let round = format!("{text}   ·   ");
    let from = m.step % round.chars().count();
    let shown: String = round.chars().chain(round.chars()).skip(from).collect();
    pad(take(&shown, w), w)
}

const PINK: Rgb = [244, 114, 182];
const BLUE: Rgb = [125, 160, 220];
// Only the four middle dots of the braille cell (not its top row), so it turns level with the text around it.
const SPIN: [&str; 4] = ["⠖", "⠲", "⠴", "⠦"];

fn spin() -> &'static str {
    SPIN[(now_ms() / 100) as usize % SPIN.len()]
}
// Selected row: accent bar + soft background. s is plain text of the row minus its first column.
fn selected(a: Rgb, s: &str) -> String {
    format!("{}{}▌{}\x1b[1m{s}{RESET}", bgc(rib()), fgc(a), fgc(strong()))
}
// A cover color brightened for a dark background.
fn lift(c: Rgb) -> Rgb {
    let top = c.iter().copied().max().unwrap_or(1).max(1) as f64;
    c.map(|v| (v as f64 * (230.0 / top) * 0.8 + 50.0).round().min(255.0) as u8)
}
// Accent = the most vivid color of the current cover; green until there is one, or if it has no color.
fn accent(got: Option<&Got>) -> Rgb {
    match got.map(|g| g.rgb) {
        Some(c) if c.iter().max().unwrap_or(&0) - c.iter().min().unwrap_or(&0) >= 40 => lift(c),
        _ => GREEN,
    }
}

// A real cover image to place: (image, its address, row, column, width in columns).
pub(super) type Placed = (Arc<Got>, String, usize, usize, usize);

// What every part of one frame needs: its size, the accent colour, the id of the playing song, and the message
// (busy, error, note) that takes over the title of the focused pane.
pub(super) struct View {
    pub w: usize,  // terminal width
    pub iw: usize, // width inside a frame
    pub l: usize,  // rows inside the body frame
    pub a: Rgb,    // accent colour
    pub over: Option<(String, Rgb)>,
    pub playing: Option<String>,
}

impl View {
    // A pane's title, unless a message has taken it over.
    pub fn title(&self, def: &str) -> (String, Rgb) {
        self.over.clone().unwrap_or_else(|| (def.to_string(), self.a))
    }
}

// Where a pane is on screen, so clicks can find its rows.
pub(super) fn pane(id: PaneId, x0: usize, x1: usize, top: i64, head: usize) -> Pane {
    Pane { id, x0, x1, top, head, rows: None }
}

impl App {
    // [glyph, color] of a track's rating.
    fn rating(&self, id: &str) -> Option<(&'static str, Rgb)> {
        if self.likes.contains(id) {
            Some((glyphs().like, readable(PINK)))
        } else if self.dislikes.contains(id) {
            Some((glyphs().dislike, readable(BLUE)))
        } else {
            None
        }
    }
    fn tag(&self, id: &str) -> String {
        self.rating(id).map_or(String::new(), |r| format!("{} ", r.0))
    }
    // The playing song's name: all of it in the accent colour, its rating mark keeping its own.
    fn song_name(&self, id: &str, text: &str, a: Rgb) -> String {
        match self.rating(id).and_then(|(g, c)| text.split_once(g).map(|(before, after)| (before, g, c, after))) {
            Some((before, g, c, after)) => bold(&format!("{}{}{}", fg(a, before), fg(c, g), fg(a, after))),
            None => bold(&fg(a, text)),
        }
    }

    // The whole screen, top to bottom:
    //   1. sizes  2. the cover and the accent colour  3. tab bar and search  4. the body (whatever view is open)
    //   5. the player  6. the key hints  7. real cover images
    pub fn render(&mut self, width_: usize, height: usize) -> String {
        // 1. Sizes. Rows go to the tab bar (1), search (3, or 1 on a short screen), the body frame, the player and
        //    the hints (1); the body gets what is left.
        let (w, h) = (width_.max(20), height.max(8));
        let compact = h < 22; // too short for the framed dock: two plain lines instead
        let r = if h >= 40 && w >= 80 { 8 } else if h >= 30 && w >= 70 { 6 } else { 4 };
        let gap = !compact && h >= 26; // a blank row in the player between the artist and the visualizer
        let used = 7 + if compact { usize::from(self.wave) } else { r + usize::from(gap) };
        // rows inside the body frame; the ? page takes the player's place too
        // the search box takes two rows more than the single line of a short screen
        let extra = if compact { 0 } else { 2 };
        let l = h.saturating_sub(if self.help { 5 } else { used } + extra).max(1);
        let mut mq = std::mem::take(&mut self.mq);
        mq.on = false; // set again by the focused row if its text overflows
        // the player above the lists moves the body down by the dock's height (its r rows and two borders)
        let top_dock = self.player_top && !compact && !self.help;
        let mut g = Geo { top: 4 + extra + if top_dock { r + 2 + usize::from(gap) } else { 0 }, ..Default::default() };
        let now = self.now().cloned();
        // the cover is about what plays, else what is highlighted (a setting has nothing to show)
        let focus = now.clone().or_else(|| self.cur().filter(|i| !matches!(i.kind, Kind::Setting | Kind::Header) && self.tab != Tab::Config).cloned());
        let url = focus.as_ref().map(|f| f.cover.clone()).unwrap_or_default();

        // 2. The cover: loaded once the highlight settles (no fetch storm while scrolling)
        let mut got = None;
        if !url.is_empty() {
            if url != self.want {
                self.want = url.clone();
                self.want_at = Instant::now();
                self.redraw_at = Some(Instant::now() + Duration::from_millis(220));
            }
            if now.is_some() || self.want_at.elapsed() > Duration::from_millis(200) {
                got = self.cover(&url);
            }
        }
        // the color follows what plays, never the cursor; while its cover loads, the previous color stays
        match &now {
            _ if self.accent.is_some() => self.theme.aim(readable(self.accent.unwrap_or(GREEN))),
            Some(t) if !t.cover.is_empty() => {
                if let Some(got) = got.as_deref() {
                    self.theme.aim(readable(accent(Some(got))));
                }
            }
            _ => self.theme.aim(readable(GREEN)),
        }
        let a = self.theme.color;
        // what the terminal image shows: the full player may swap in a sharper copy
        let (mut shown_url, mut sharp_got) = (url.clone(), None);

        // 3. Tab bar and search field
        let count = |n: usize| if n > 0 { format!(" {n}") } else { String::new() };
        let tabs = [
            (tr!("Home").to_string(), "H"),
            (tr!("Playlists").to_string(), "P"),
            (tr!("Liked{a}", a = count(self.likes.len())), "L"),
            (if self.releases > 0 { tr!("Library ✦{a}", a = self.releases) } else { tr!("Library").to_string() }, "Lb"),
            (tr!("Queue{a}", a = count(self.queue.len())), "Q"),
            (tr!("History").to_string(), "Hi"),
            (tr!("Stats").to_string(), "S"),
            (tr!("Config").to_string(), "C"),
        ];
        let shown_tab = if self.tab == Tab::Search { self.prev_tab } else { self.tab };
        let active = crate::app::TABS.iter().position(|&t| t == shown_tab).unwrap_or(0);
        let (bar, spans) = tab_bar(w, &tabs, active, a);
        g.tabs = spans;
        let (label, hint, field_color) = match (self.typing, &self.naming) {
            (true, Some(Naming::Export)) => (tr!("Export to · a new folder of CSV files · Enter saves, Esc cancels"), tr!("a folder path"), a),
            (true, Some(Naming::Import)) => (tr!("Import from · replaces all your data · Enter, or Esc"), tr!("the folder of an export"), a),
            (true, Some(Naming::Wipe)) => (tr!("⚠ This erases all your data for good · type \"delete all my data\" and Enter, or Esc to keep it"), "delete all my data", RED),
            (true, Some(Naming::FreeCut)) => (tr!("Free up space · the least played share of your downloads, in % · Enter, or Esc"), "25", a),
            (true, Some(Naming::Folder)) => (tr!("Downloads folder · Enter saves, Esc cancels"), tr!("a folder path"), a),
            (true, Some(Naming::Filter)) => (tr!("Filter this list · Enter keeps · Esc clears"), tr!("type a title, artist or album"), a),
            (true, Some(_)) => (tr!("Name"), "", a),
            _ => (tr!("/ Search"), tr!("artists, albums, songs, or a Spotify / YouTube playlist link"), a),
        };
        let search = search_bar(w, label, self.typing, if self.typing { &self.buf } else { &self.query }, hint, field_color, !compact);
        g.field_x1 = w;
        g.field_y1 = 1 + search.len();

        // 4. The body. A busy spinner, an error or a note takes over the title of the focused pane.
        let over: Option<(String, Rgb)> = if !self.busy.is_empty() {
            Some((format!("{} {}", spin(), self.busy), a))
        } else if !self.msg.is_empty() {
            Some((self.msg.clone(), RED))
        } else if !self.note.is_empty() {
            Some((self.note.clone(), txt()))
        } else {
            None
        };

        let mut image: Option<(usize, usize, usize)> = None; // where the player's real cover goes: (row, col, columns)
        let mut head_img: Option<Placed> = None; // the album page's real cover
        let iw = w - 2;
        let v = View { w, iw, l, a, over, playing: now.as_ref().map(|t| t.id.clone()) };
        let title_for = |def: &str| v.title(def);
        let blank_to = |mut rows: Vec<String>| {
            rows.resize(l, spaces(iw));
            rows
        };
        let body: Vec<String> = if self.help {
            let mut rows = help_rows(iw, a);
            self.help_scroll = self.help_scroll.min(rows.len().saturating_sub(l));
            let more = if rows.len() > l { tr!(" · ↑↓ for more") } else { "" };
            rows.drain(..self.help_scroll);
            frame(&tr!("Keys{more} · any other key closes", more = more), a, &blank_to(rows.into_iter().take(l).collect()), w, a)
        } else if let Some(m) = &self.menu {
            g.menu_top = (m.sel + 1).saturating_sub(l);
            let rows = m
                .items
                .iter()
                .enumerate()
                .skip(g.menu_top)
                .map(|(i, (label, _))| {
                    let row = if i == m.sel {
                        selected(a, &cell(&format!(" {}  {label}", i + 1), iw - 3))
                    } else {
                        cell(&format!("  {}  {label}", i + 1), iw - 2)
                    };
                    format!(" {row} ")
                })
                .collect();
            frame(&tr!("Actions · {a}", a = m.title), a, &blank_to(rows), w, a)
        } else if self.lyrics {
            let rows = self.lyric_rows(iw, l, a);
            let playing = now.as_ref().map_or(String::new(), |t| format!(" · {} — {}", t.title, t.artist));
            let plain = if matches!(self.lyrics_ready(), Some(Lyrics::Plain(_))) { tr!(" · not synced") } else { "" };
            let mode = if self.karaoke { tr!("Karaoke") } else { tr!("Lyrics") };
            let (t, c) = title_for(&format!("{mode}{playing}{plain}"));
            g.panes.push(pane(PaneId::Cover, 1, w, 0, 0));
            frame(&t, c, &rows, w, a)
        } else if self.full && self.trip_on {
            let react = if self.wave { "" } else { tr!(" · w to react to the music") };
            let (id, title, artist) = now.as_ref().map_or(("", "", ""), |t| (t.id.as_str(), t.title.as_str(), t.artist.as_str()));
            let palette = got.as_ref().filter(|_| now.is_some()).map_or_else(|| vec![a], |g| g.palette.clone());
            let lyric = if self.trip_lyrics && now.is_some() {
                self.want_lyrics();
                match self.lyrics_ready() {
                    Some(Lyrics::Synced(lines)) => self.line_at(lines).map(|i| lines[i].1.clone()),
                    _ => None,
                }
            } else {
                None
            };
            let rows = self.trip.frame(iw, l, self.trip_t, self.time as f64, &self.bands, self.energy, &palette, id, title, artist, lyric.as_deref());
            let held = if self.trip.locked { tr!(" · held") } else { "" };
            let (t, c) = title_for(&tr!("Trip · {a}{held}{react}", a = self.trip.scene_name(), held = held, react = react));
            g.panes.push(pane(PaneId::Cover, 1, w, 0, 0));
            frame(&t, c, &rows, w, a)
        } else if self.full {
            // opening the full player fetches a sharper copy; until it is here, the one already loaded shows
            if let Some(big) = cover::large(&url).filter(|_| self.kitty && got.is_some()) {
                if let Some(sharp) = self.cover(&big) {
                    shown_url = big;
                    sharp_got = Some(sharp);
                }
            }
            let (rows, at) = self.full_pane(&mut g, iw, l, focus.as_ref(), got.as_ref());
            image = at;
            let (t, c) = title_for(tr!("Now playing"));
            g.panes.push(pane(PaneId::Cover, 1, w, 0, 0));
            frame(&t, c, &rows, w, a)
        } else if self.tab == Tab::Home {
            self.home_body(&v, &mut mq, &mut g)
        } else if self.tab == Tab::Stats {
            self.stats_body(&v, &mut mq, &mut g)
        } else if self.tab == Tab::Config {
            self.config_body(&v, &mut g)
        } else {
            self.list_body(&v, &mut mq, &mut g, &mut head_img)
        };
        // the equalizer's window sits over whatever is there
        let body = if self.eq.open && !self.help && self.menu.is_none() && !self.lyrics && !self.full {
            g.panes.clear();
            g.links.clear();
            head_img = None; // a real image would show through the window
            self.eq_window(&v, &mut g, body)
        } else if self.downloads.open && !self.help && self.menu.is_none() && !self.lyrics && !self.full {
            g.panes.clear();
            g.links.clear();
            head_img = None;
            self.downloads_window(&v, &mut g, body)
        } else {
            body
        };
        g.body = l;

        // 5. The player under the body: the framed dock, or two plain lines on a short screen
        let mut lines = vec![bar];
        lines.extend(search);
        if top_dock {
            let (rows, at) = self.dock(&mut g, w, lines.len() + 2, r, gap, focus.as_ref(), got.as_ref(), a);
            lines.extend(rows);
            image = image.or(at);
        }
        lines.extend(body);
        if self.help || top_dock {
            // nothing under the ? page; the dock is already above
        } else if compact {
            let player = self.compact_player(&v, &mut g, h, now.as_ref());
            lines.extend(player);
        } else {
            let first = l + g.top + 2;
            let (rows, at) = self.dock(&mut g, w, first, r, gap, focus.as_ref(), got.as_ref(), a);
            lines.extend(rows);
            image = image.or(at);
        }
        // 6. The keys that work right now
        let hint_list: Vec<_> = self.hint_list().into_iter().map(|(key, what)| (crate::i18n::t(key), crate::i18n::t(what))).collect();
        lines.push(hints(&hint_list, w, a));
        self.geo = g;
        self.mq = mq;

        // 7. Real cover images go after the text (they are placed by position)
        let img = self.send_images(image, head_img, &shown_url, sharp_got.or(got));
        self.theme.paint(&mut lines);
        // every line is exactly w wide, so no erase-to-end-of-line is needed (it would clip the last column).
        // "\r\n": raw mode turns off the terminal's own carriage return, a bare "\n" would only move down
        format!("\x1b[H{}{img}", lines.join("\r\n"))
    }
}
