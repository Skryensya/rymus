// Draws the whole screen into one string. Every line is exactly as wide as the terminal.
use crate::app::{fmt, minutes, App, Geo, Marquee, Pane, PaneId, Tab, CATS, PERIODS, TOP};
use crate::cover::{self, Got};
use crate::item::{Item, Kind};
use crate::lyrics::{self, Lyrics};
use crate::ui::*;
use crate::db::now_ms;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spin() -> &'static str {
    SPIN[(now_ms() / 100) as usize % SPIN.len()]
}
// Selected row: accent bar + soft background. s is plain text of the row minus its first column.
fn selected(a: Rgb, s: &str) -> String {
    format!("{}{}▌{}\x1b[1m{s}{RESET}", bgc(RIB), fgc(a), fgc([255, 255, 255]))
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

impl App {
    // [glyph, color] of a track's rating.
    fn rating(&self, id: &str) -> Option<(&'static str, Rgb)> {
        if self.likes.contains(id) {
            Some((glyphs().like, PINK))
        } else if self.dislikes.contains(id) {
            Some((glyphs().dislike, BLUE))
        } else {
            None
        }
    }
    fn tag(&self, id: &str) -> String {
        self.rating(id).map_or(String::new(), |r| format!("{} ", r.0))
    }
    fn paint(&self, id: &str, text: &str) -> String {
        match self.rating(id) {
            Some((g, c)) => text.replacen(g, &fg(c, g), 1),
            None => text.to_string(),
        }
    }

    // The l rows of a list pane, each exactly w columns: marks, title column, detail column.
    // hl: draw the selection (only the focused pane).
    #[allow(clippy::too_many_arguments)]
    fn list_rows(&mut self, mq: &mut Marquee, data: &[Item], sel: usize, top: usize, w: usize, l: usize, hl: bool, a: Rgb, now: Option<&str>, empty: &str) -> Vec<String> {
        (0..l)
            .map(|i| {
                let idx = top + i;
                let Some(it) = data.get(idx) else {
                    return if i == 1 && data.is_empty() { dim(&cell(&format!("   {empty}"), w)) } else { spaces(w) };
                };
                if it.kind == Kind::Header {
                    return format!(" {}", fg(a, &bold(&cell(&it.title, w.saturating_sub(1)))));
                }
                let track = it.kind == Kind::Track;
                let playing = track && now == Some(it.id.as_str());
                let rating = if track { self.rating(&it.id) } else { None };
                let num = if it.rank > 0 {
                    format!("{:>3}  ", it.rank)
                } else if self.tab == Tab::Search && self.numbered {
                    format!("{:>2}  ", idx + 1)
                } else {
                    String::new()
                };
                let detail = it.detail.clone().unwrap_or_else(|| match it.kind {
                    Kind::Album | Kind::Artist => {
                        if it.year.is_empty() { it.author.clone() } else { format!("{} · {}", it.author, it.year) }
                    }
                    Kind::ArtistStat => {
                        let likes = if it.likes > 0 { format!(" · {} {}", glyphs().like, it.likes) } else { String::new() };
                        format!("{} plays{likes}", it.plays)
                    }
                    _ => it.artist.clone(),
                });
                let room = w.saturating_sub(5 + num.len()); // bar, two marks, space, right margin
                let tw = (room * 6).div_ceil(10);
                let rw = room.saturating_sub(tw + 1);
                let focused = hl && idx == sel;
                if focused {
                    let key = format!("{:?}|{idx}|{}", self.tab, it.id); // a new row starts from the beginning
                    if key != mq.key {
                        mq.key = key;
                        mq.at = Some(Instant::now());
                    }
                }
                let title = if focused { marquee(mq, &it.title, tw) } else { cell(&it.title, tw) };
                let rest = if focused && rw > 0 { marquee(mq, &detail, rw) } else { cell(&detail, rw) };
                let mark = if playing { "♪" } else if it.fresh { "✦" } else { " " };
                if focused {
                    let marks = format!("{mark}{} ", rating.map_or(" ", |r| r.0));
                    return selected(a, &format!("{marks}{num}{title} {rest} "));
                }
                let mark = if mark == " " { " ".to_string() } else { fg(a, mark) };
                let rating = rating.map_or(" ".to_string(), |r| fg(r.1, r.0));
                let title = if playing { fg(a, &title) } else { title };
                format!(" {mark}{rating} {}{title} {} ", dim(&num), dim(&rest))
            })
            .collect()
    }

    // Big cover + info for the full player. Returns the inner rows and where a real image goes (row, col, columns).
    fn full_pane(&mut self, iw: usize, l: usize, focus: Option<&Item>, got: Option<&Arc<Got>>) -> (Vec<String>, Option<(usize, usize, usize)>) {
        let mut c = iw.saturating_sub(4);
        if c / 2 + 6 > l {
            c = 2 * l.saturating_sub(6); // leave room for the info lines
        }
        let url = focus.map(|f| f.cover.clone()).unwrap_or_default();
        let show = !url.is_empty() && c >= 12;
        let r = if show { c.div_ceil(2) } else { 0 };
        let pad_l = iw.saturating_sub(c) / 2;
        let geo = (show && self.kitty).then_some((TOP, 2 + pad_l, c));
        let art = match got {
            Some(got) if show && !self.kitty => self.cells(&url, got, c, r),
            _ => None,
        };
        // (text, style)
        let info: Vec<(String, Option<Rgb>, u8)> = match focus {
            None => vec![],
            Some(f) => {
                let plain = |s: String| (s, None, 0);
                let strong = (f.title.clone(), None, 1);
                if f.kind == Kind::ArtistStat {
                    let likes = if f.likes > 0 { format!(" · {} {}", glyphs().like, f.likes) } else { String::new() };
                    vec![strong, plain(format!("{} plays{likes}", f.plays))]
                } else if !f.author.is_empty() {
                    let year = if f.year.is_empty() { String::new() } else { format!(" · {}", f.year) };
                    vec![strong, plain(format!("{}{year}", f.author))]
                } else {
                    let g = glyphs();
                    let heart = if self.likes.contains(&f.id) {
                        (format!("{}  Liked · l to remove", g.like), Some(PINK), 0)
                    } else if self.dislikes.contains(&f.id) {
                        (format!("{}  Disliked · x to remove", g.dislike), Some(BLUE), 0)
                    } else {
                        (format!("{}  l like  ·  {}  x dislike", g.unliked, g.dislike), None, 2)
                    };
                    vec![strong, plain(f.artist.clone()), (f.album.clone(), None, 2), plain(String::new()), heart]
                }
            }
        };
        let inner = (0..l)
            .map(|i| {
                if i < r {
                    return match art.as_ref().and_then(|a| a.get(i)) {
                        Some(row) => format!("{}{row}{}", spaces(pad_l), spaces(iw.saturating_sub(c + pad_l))),
                        None => spaces(iw),
                    };
                }
                let at = i as i64 - if r > 0 { r as i64 + 1 } else { 0 };
                let Some((text, color, style)) = usize::try_from(at).ok().and_then(|j| info.get(j)) else { return spaces(iw) };
                let t = cut(text, iw.saturating_sub(2));
                let p = iw.saturating_sub(width(&t)) / 2;
                let styled = match (color, style) {
                    (Some(c), _) => fg(*c, &t),
                    (None, 1) => bold(&t),
                    (None, 2) => dim(&t),
                    _ => t.clone(),
                };
                format!("{}{styled}{}", spaces(p), spaces(iw.saturating_sub(p + width(&t))))
            })
            .collect();
        (inner, geo)
    }

    // The player dock: cover, title, artist, wave and progress in r content rows.
    #[allow(clippy::too_many_arguments)]
    fn dock(&mut self, g: &mut Geo, w: usize, l: usize, r: usize, focus: Option<&Item>, got: Option<&Arc<Got>>, a: Rgb) -> (Vec<String>, Option<(usize, usize, usize)>) {
        let iw = w - 2;
        let url = focus.map(|f| f.cover.clone()).unwrap_or_default();
        let thumb = !url.is_empty() && !self.full; // the full player already shows the big cover
        let c = 2 * r;
        let tw = if thumb { c + 2 } else { 0 };
        let tx = iw.saturating_sub(2 + tw).max(10);
        let art = match got {
            Some(got) if thumb && !self.kitty => self.cells(&url, got, c, r),
            _ => None,
        };
        let geo = (thumb && self.kitty).then_some((l + 6, 3, c));
        let blank = spaces(tx);
        let now = self.now().cloned();
        let status = if self.loading { spin() } else if self.paused { "⏸" } else { "▶" };
        let count = now.as_ref().map_or(String::new(), |_| format!(" {}/{}", self.pos + 1, self.queue.len()));
        let (r0, r1) = if let Some(t) = &now {
            let line = cut(&format!("{status} {}{}", self.tag(&t.id), t.title), tx.saturating_sub(count.len()));
            let mut chars = line.chars();
            let head = chars.next().unwrap_or(' ').to_string();
            let r0 = format!(
                "{}{}{}{}",
                fg(a, &head),
                self.paint(&t.id, &bold(chars.as_str())),
                spaces(tx.saturating_sub(width(&line) + count.len())),
                dim(&count)
            );
            let album = if t.album.is_empty() { String::new() } else { format!(" · {}", t.album) };
            (r0, dim(&cell(&format!("  {}{album}", t.artist), tx)))
        } else if let Some((q, time)) = self.session.as_ref().and_then(|s| s.queue.first().map(|q| (q, s.time))) {
            (
                fg(a, &cell(&format!("⏯ {} — {}", q.title, q.artist), tx)),
                dim(&cell(&format!("  stopped at {} · resume it from Home", fmt(time as f64)), tx)),
            )
        } else {
            match focus {
                Some(f) => (bold(&cell(&f.title, tx)), dim(&cell(if f.artist.is_empty() { &f.author } else { &f.artist }, tx))),
                None => (dim(&cell("Nothing playing", tx)), blank.clone()),
            }
        };
        let left = format!("{} ", fmt(if now.is_some() { self.time as f64 } else { 0.0 }));
        let length = if now.is_some() && self.dur > 0.0 { fmt(self.dur) } else { "--:--".into() };
        let right = cut(&format!(" {length}   vol {}", self.vol), tx.saturating_sub(left.len()));
        let bw = tx.saturating_sub(left.len() + width(&right));
        let filled = if self.dur > 0.0 && bw > 0 { ((bw - 1) as f64 * (self.time as f64 / self.dur).min(1.0)).round() as usize } else { 0 };
        let progress = if now.is_some() && bw > 0 {
            format!(
                "{}{}{}{}",
                dim(&left),
                fg(a, &format!("{}●", "━".repeat(filled))),
                dim(&format!("{}{right}", "─".repeat(bw - 1 - filled))),
                spaces(tx.saturating_sub(left.len() + bw + width(&right)))
            )
        } else {
            blank.clone()
        };
        g.bar_row = l + 5 + r;
        g.bar_x0 = 2 + tw + left.len();
        g.bar_w = bw;
        g.now_row = l + 6;
        g.thumb = thumb.then_some((3, 2 + c, l + 6, l + 5 + r));
        let middle = if self.wave && now.is_some() { spectrum(&self.bands, tx, r - 3, a) } else { vec![blank.clone(); r - 3] };
        let inner: Vec<String> = std::iter::once(r0)
            .chain(std::iter::once(r1))
            .chain(middle)
            .chain(std::iter::once(progress))
            .enumerate()
            .map(|(i, row)| {
                let cover = if thumb { format!("{}  ", art.as_ref().and_then(|a| a.get(i).cloned()).unwrap_or_else(|| spaces(c))) } else { String::new() };
                format!(" {cover}{row} ")
            })
            .collect();
        let title = if now.is_some() { "Now playing" } else if self.session.is_some() { "Last session" } else if focus.is_some() { "Preview" } else { "Now playing" };
        (frame(title, TXT, &inner, w, GRAY), geo)
    }

    // The l rows of the lyrics view. Timed lyrics follow the song; untimed ones are scrolled by hand.
    fn lyric_rows(&mut self, w: usize, l: usize, a: Rgb) -> Vec<String> {
        let center = |text: &str, styled: String| {
            let n = width(text);
            let p = w.saturating_sub(n) / 2;
            format!("{}{styled}{}", spaces(p), spaces(w.saturating_sub(p + n)))
        };
        let ready = self.want_lyrics().is_some();
        let message = if self.now().is_none() {
            "Nothing playing".to_string()
        } else {
            match self.lyrics_ready().filter(|_| ready) {
                None => format!("{} Looking for lyrics…", spin()),
                Some(Lyrics::Error) => "Could not reach LRCLIB. Press y twice to retry.".into(),
                Some(Lyrics::Instrumental) => "♪ Instrumental ♪".into(),
                Some(Lyrics::Synced(v)) if !v.is_empty() => String::new(),
                Some(Lyrics::Plain(_)) => String::new(),
                _ => "No lyrics found on LRCLIB.".into(),
            }
        };
        if !message.is_empty() {
            let text = cut(&message, w);
            return (0..l).map(|i| if i == l / 2 { center(&text, dim(&text)) } else { spaces(w) }).collect();
        }
        let (current, synced, lines): (Option<usize>, bool, Vec<String>) = match self.lyrics_ready() {
            Some(Lyrics::Synced(v)) => {
                (self.line_at(v), true, v.iter().map(|x| if x.1.is_empty() { "♪".to_string() } else { x.1.clone() }).collect())
            }
            Some(Lyrics::Plain(v)) => (None, false, v.clone()),
            _ => (None, false, vec![]),
        };
        self.lyric_line = current;
        let rows: Vec<(String, usize)> =
            lines.iter().enumerate().flat_map(|(n, t)| lyrics::wrap(t, w.saturating_sub(4)).into_iter().map(move |p| (p, n))).collect();
        let top = if synced {
            rows.iter().position(|x| x.1 == current.unwrap_or(0)).unwrap_or(0).saturating_sub(l / 2)
        } else {
            self.lyric_scroll = self.lyric_scroll.min(rows.len().saturating_sub(l));
            self.lyric_scroll
        };
        (0..l)
            .map(|i| match rows.get(top + i) {
                None => spaces(w),
                Some((part, n)) => {
                    let styled = if !synced {
                        part.clone()
                    } else if current == Some(*n) {
                        fg(a, &bold(part))
                    } else {
                        dim(part)
                    };
                    center(part, styled)
                }
            })
            .collect()
    }

    fn hint_list(&mut self) -> Vec<(&'static str, &'static str)> {
        let pause = if self.paused { "resume" } else { "pause" };
        if self.menu.is_some() {
            return vec![("↑↓", "move"), ("⏎", "run"), ("esc", "close")];
        }
        if self.lyrics && !self.typing {
            let mut v = vec![("y", "close lyrics"), ("l", "like"), ("x", "dislike"), ("⏎/space", pause), ("n p", "next/prev")];
            if matches!(self.lyrics_ready(), Some(Lyrics::Plain(_))) {
                v.push(("↑↓", "scroll"));
            }
            v.extend([("⇧←→", "seek"), ("+ -", "volume"), ("?", "keys")]);
            return v;
        }
        if self.typing {
            return vec![("⏎", "search"), ("↑↓", "history"), ("esc", "cancel")];
        }
        if self.full && self.trip_on {
            let react = if self.wave { "react to music on" } else { "react to music off" };
            return vec![("V", "close"), ("v", "cover"), ("w", react), ("⏎/space", pause), ("n p", "next/prev"), ("+ -", "volume"), ("?", "keys")];
        }
        if self.full {
            return vec![("l", "like"), ("x", "dislike"), ("v", "close"), ("y", "lyrics"), ("⏎/space", pause), ("n p", "next/prev"), ("+ -", "volume"), ("?", "keys")];
        }
        let kind = self.kind_at();
        let enter = match kind {
            Kind::Resume => "resume",
            Kind::Mix => "play mix",
            Kind::Track if self.cur().is_some_and(|c| self.is_current(c)) => pause,
            Kind::Track if self.tab == Tab::Queue => "jump",
            Kind::Track => "play",
            _ => "open",
        };
        let mut k = vec![("⏎/space", enter), ("/", "search"), ("?", "keys"), ("tab", "sections")];
        if self.now().is_some() {
            k.push(("y", "lyrics"));
        }
        if self.tab == Tab::Home {
            k.extend([("D", "discover"), ("←→", "songs/artists")]);
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
            k.extend([("l", "like"), ("x", "dislike"), ("a", "queue"), ("R", "radio")]);
        }
        k.extend([("+ -", "volume"), ("⇧←→", "seek")]);
        if kind == Kind::Track {
            k.push(("m", "menu"));
        }
        k.extend([("v", "player"), ("q", "quit")]);
        k
    }

    pub fn render(&mut self, width_: usize, height: usize) -> String {
        let (w, h) = (width_.max(20), height.max(8));
        let compact = h < 22; // too short for the framed dock: two plain lines instead
        let r = if h >= 40 && w >= 80 { 8 } else if h >= 30 && w >= 70 { 6 } else { 4 };
        let used = 7 + if compact { usize::from(self.wave) } else { r };
        let l = h.saturating_sub(used).max(1); // rows inside the body frame
        let mut mq = std::mem::take(&mut self.mq);
        mq.on = false; // set again by the focused row if its text overflows
        let mut g = Geo::default();
        let now = self.now().cloned();
        let focus = now.clone().or_else(|| self.cur().cloned()); // the cover is about what plays, else what is highlighted
        let url = focus.as_ref().map(|f| f.cover.clone()).unwrap_or_default();

        // load the cover once the highlight settles (no fetch storm while scrolling)
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
        let a = accent(got.as_deref());

        let like = glyphs().like;
        let count = |n: usize| if n > 0 { format!(" {n}") } else { String::new() };
        let tabs = [
            ("Home".to_string(), "H"),
            (format!("{like} Liked{}", count(self.likes.len())), like),
            (format!("Queue{}", count(self.queue.len())), "Q"),
            ("Stats".to_string(), "S"),
        ];
        let shown_tab = if self.tab == Tab::Search { self.prev_tab } else { self.tab };
        let active = crate::app::TABS.iter().position(|&t| t == shown_tab).unwrap_or(0);
        let (bar, spans) = tab_bar(w, &tabs, active, a);
        g.tabs = spans;
        let search = search_bar(w, self.typing, if self.typing { &self.buf } else { &self.query }, "artists, albums, songs…", a);
        g.field_x1 = w;

        // pane title: busy / error / note take over the title of the focused pane
        let over: Option<(String, Rgb)> = if !self.busy.is_empty() {
            Some((format!("{} {}", spin(), self.busy), a))
        } else if !self.msg.is_empty() {
            Some((self.msg.clone(), RED))
        } else if !self.note.is_empty() {
            Some((self.note.clone(), TXT))
        } else {
            None
        };
        let title_for = |def: &str| over.clone().unwrap_or_else(|| (def.to_string(), a));
        let top_of = |s: usize, n: usize| s.saturating_sub(l / 2).min(n.saturating_sub(l));
        let pane = |id, x0, x1, top: i64, head| Pane { id, x0, x1, top, head };

        let mut image: Option<(usize, usize, usize)> = None;
        let iw = w - 2;
        let playing = now.as_ref().map(|t| t.id.as_str());
        let blank_to = |mut rows: Vec<String>| {
            rows.resize(l, spaces(iw));
            rows
        };
        let body: Vec<String> = if self.help {
            let rows = HELP
                .iter()
                .map(|(key, text)| {
                    let p = cut(&format!("{}{text}", pad(key, 8)), iw - 2);
                    let head = take(&p, 8);
                    format!(" {}{}{}", fg(a, &bold(head)), &p[head.len()..], spaces(iw.saturating_sub(1 + width(&p))))
                })
                .collect();
            frame("Keys · press any key to close", a, &blank_to(rows), w, a)
        } else if let Some(m) = &self.menu {
            let rows = m
                .items
                .iter()
                .enumerate()
                .map(|(i, (label, _))| {
                    let row = if i == m.sel {
                        selected(a, &cell(&format!(" {}  {label}", i + 1), iw - 3))
                    } else {
                        cell(&format!("  {}  {label}", i + 1), iw - 2)
                    };
                    format!(" {row} ")
                })
                .collect();
            frame(&format!("Actions · {}", m.title), a, &blank_to(rows), w, a)
        } else if self.lyrics {
            let rows = self.lyric_rows(iw, l, a);
            let playing = now.as_ref().map_or(String::new(), |t| format!(" · {} — {}", t.title, t.artist));
            let plain = if matches!(self.lyrics_ready(), Some(Lyrics::Plain(_))) { " · not synced" } else { "" };
            let (t, c) = title_for(&format!("Lyrics{playing}{plain}"));
            g.panes.push(pane(PaneId::Cover, 1, w, 0, 0));
            frame(&t, c, &rows, w, a)
        } else if self.full && self.trip_on {
            let playing = now.as_ref().map_or(String::new(), |t| format!(" · {} — {}", t.title, t.artist));
            let react = if self.wave { "" } else { " · w to react to the music" };
            let id = now.as_ref().map_or("", |t| &t.id);
            let palette = got.as_ref().map_or_else(|| vec![a], |g| g.palette.clone());
            let rows = self.trip.frame(iw, l, self.trip_t, &self.bands, &palette, id, self.energy);
            let (t, c) = title_for(&format!("Trip{playing}{react}"));
            g.panes.push(pane(PaneId::Cover, 1, w, 0, 0));
            frame(&t, c, &rows, w, a)
        } else if self.full {
            let (rows, at) = self.full_pane(iw, l, focus.as_ref(), got.as_ref());
            image = at;
            let (t, c) = title_for("Now playing");
            g.panes.push(pane(PaneId::Cover, 1, w, 0, 0));
            frame(&t, c, &rows, w, a)
        } else if self.tab == Tab::Home {
            let ws = (w * 58).div_ceil(100);
            let wa = w - ws;
            let songs_focus = self.home_focus == PaneId::Songs;
            let sel_s = if songs_focus { self.sel } else { self.home_sel[0] };
            let sel_a = if songs_focus { self.home_sel[1] } else { self.sel };
            let top_s = top_of(sel_s, self.home_songs.len());
            let top_a = top_of(sel_a, self.home_artists.len());
            let plain = |def: &str| (def.to_string(), TXT);
            let (ts, cs) = if songs_focus { title_for("Top songs") } else { plain("Top songs") };
            let (ta, ca) = if songs_focus { plain("Your artists") } else { title_for("Your artists") };
            // the lists are lent to the row builder and given back
            let songs = std::mem::take(&mut self.home_songs);
            let rows_s = self.list_rows(&mut mq, &songs, sel_s, top_s, ws - 2, l, songs_focus, a, playing, "Search with / and play something.");
            self.home_songs = songs;
            let artists = std::mem::take(&mut self.home_artists);
            let rows_a = self.list_rows(&mut mq, &artists, sel_a, top_a, wa.saturating_sub(2), l, !songs_focus, a, playing, "No artists yet.");
            self.home_artists = artists;
            let fs = frame(&ts, cs, &rows_s, ws, if songs_focus { a } else { GRAY });
            let fa = frame(&ta, ca, &rows_a, wa, if songs_focus { GRAY } else { a });
            g.panes.push(pane(PaneId::Songs, 1, ws, top_s as i64, 0));
            g.panes.push(pane(PaneId::Artists, ws + 1, w, top_a as i64, 0));
            fs.into_iter().zip(fa).map(|(x, y)| x + &y).collect()
        } else if self.tab == Tab::Stats {
            let n = l.saturating_sub(2).max(1);
            let data = std::mem::take(&mut self.stat_rows);
            let top = self.sel.saturating_sub(n / 2).min(data.len().saturating_sub(n));
            let label = if iw < 70 { "Listened" } else { "Minutes listened" };
            let totals: Vec<String> = PERIODS.iter().enumerate().map(|(i, p)| format!("{} {}", p.0, minutes(self.totals[i]))).collect();
            let total = cell(&format!(" {label} · {}", totals.join(" · ")), iw);
            let pick = |names: &[&str], on: usize| {
                names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let t = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                        if i == on { format!("[{t}]") } else { format!(" {t} ") }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            // the keys sit next to what they change
            let periods: Vec<&str> = PERIODS.iter().map(|p| p.0).collect();
            let left = format!(" {}  ←→", pick(&CATS, self.stat_cat));
            let right = format!("t  {} ", pick(&periods, self.period));
            let mut selector = cell(&format!("{left}{}{right}", spaces(iw.saturating_sub(width(&left) + width(&right)).max(2))), iw);
            for (names, on) in [(&CATS[..], self.stat_cat), (&periods[..], self.period)] {
                let name = names[on];
                let chosen = format!("[{}{}]", name[..1].to_uppercase(), &name[1..]);
                selector = selector.replacen(&chosen, &fg(a, &bold(&chosen)), 1);
            }
            let (t, c) = title_for(&format!("Stats · top 100 {} · last {} days", CATS[self.stat_cat], PERIODS[self.period].1));
            let sel = self.sel;
            let mut rows = vec![bold(&total), selector];
            rows.extend(self.list_rows(&mut mq, &data, sel, top, iw, n, true, a, playing, "Nothing listened in this period yet."));
            rows.truncate(l);
            self.stat_rows = data;
            g.panes.push(pane(PaneId::List, 1, w, top as i64 - 2, 2)); // two summary rows above the list
            frame(&t, c, &rows, w, a)
        } else {
            let sel = self.sel;
            let top = top_of(sel, self.data().len());
            let (def, empty) = match self.tab {
                Tab::Liked => ("Liked", "No likes yet. Press l on any track."),
                Tab::Queue => ("Queue", "The queue is empty. Press Enter on a track, or a to add one."),
                _ => (if self.title.is_empty() { "Results" } else { self.title.as_str() }, "No results."),
            };
            let (t, c) = title_for(def);
            let tab = self.tab;
            let data = match tab {
                Tab::Liked => std::mem::take(&mut self.liked),
                Tab::Queue => std::mem::take(&mut self.queue),
                _ => std::mem::take(&mut self.items),
            };
            let rows = self.list_rows(&mut mq, &data, sel, top, iw, l, true, a, playing, empty);
            match tab {
                Tab::Liked => self.liked = data,
                Tab::Queue => self.queue = data,
                _ => self.items = data,
            }
            g.panes.push(pane(PaneId::List, 1, w, top as i64, 0));
            frame(&t, c, &rows, w, a)
        };
        g.body = l;

        let mut lines = vec![bar, search];
        lines.extend(body);
        if compact {
            // two plain lines (three with the wave), no cover
            let status = if self.loading { spin() } else if self.paused { "⏸" } else { "▶" };
            let count = now.as_ref().map_or(String::new(), |_| format!(" {}/{}", self.pos + 1, self.queue.len()));
            match &now {
                Some(t) => {
                    let line = cut(&format!("{status} {}{} — {}", self.tag(&t.id), t.title, t.artist), w.saturating_sub(1 + count.len()));
                    let mut chars = line.chars();
                    let head = chars.next().unwrap_or(' ').to_string();
                    lines.push(format!(
                        "{}{}{}{}",
                        fg(a, &head),
                        self.paint(&t.id, chars.as_str()),
                        dim(&count),
                        spaces(w.saturating_sub(width(&line) + count.len()))
                    ));
                }
                None => lines.push(match self.session.as_ref().and_then(|s| s.queue.first()) {
                    Some(q) => fg(a, &cell(&format!("⏯ Resume: {} (Home)", q.title), w)),
                    None => format!("{}{}", dim("Nothing playing"), spaces(w.saturating_sub(15))),
                }),
            }
            if self.wave {
                lines.push(if now.is_some() { spectrum(&self.bands, w, 1, a).remove(0) } else { spaces(w) });
            }
            let left = format!("{} ", fmt(self.time as f64));
            let length = if self.dur > 0.0 { fmt(self.dur) } else { "--:--".into() };
            let right = format!(" {length}  vol {}", self.vol);
            let bw = w.saturating_sub(left.len() + right.len() + 1);
            let filled = if self.dur > 0.0 && bw > 0 { ((bw - 1) as f64 * (self.time as f64 / self.dur).min(1.0)).round() as usize } else { 0 };
            g.bar_row = h - 1;
            g.bar_x0 = left.len();
            g.bar_w = bw;
            g.now_row = h - if self.wave { 3 } else { 2 };
            lines.push(if now.is_some() && bw > 0 {
                format!("{left}{}{}{right} ", fg(a, &format!("{}●", "━".repeat(filled))), dim(&"─".repeat(bw - 1 - filled)))
            } else {
                spaces(w)
            });
        } else {
            let (rows, at) = self.dock(&mut g, w, l, r, focus.as_ref(), got.as_ref(), a);
            lines.extend(rows);
            image = image.or(at);
        }
        let hint_list = self.hint_list();
        lines.push(hints(&hint_list, w, a));
        self.geo = g;
        self.mq = mq;

        // real cover image: only re-sent when it changes; the old one stays until the new one has loaded
        let mut img = String::new();
        if self.kitty {
            let key = match image {
                Some((row, col, cols)) if !url.is_empty() => format!("{url}|{cols}|{row}|{col}"),
                _ => String::new(),
            };
            if (key.is_empty() || got.is_some()) && key != self.shown {
                if !self.shown.is_empty() {
                    img.push_str(cover::CLEAR);
                }
                if let (false, Some((row, col, cols)), Some(got)) = (key.is_empty(), image, &got) {
                    img.push_str(&cover::place(&got.b64, row, col, cols));
                }
                self.shown = key;
            }
        }
        // every line is exactly w wide, so no erase-to-end-of-line is needed (it would clip the last column).
        // "\r\n": raw mode turns off the terminal's own carriage return, a bare "\n" would only move down
        format!("\x1b[H{}{img}", lines.join("\r\n"))
    }
}

const HELP: [(&str, &str); 12] = [
    ("/", "search · Enter shows artists, albums and songs · ↑ ↓ past searches"),
    ("↑ ↓", "move · Esc goes back"),
    ("⏎ space", "open or play the highlighted row · on the playing song, pause or resume"),
    ("Tab", "switch between Home, Liked and Queue · ← → switches panel on Home"),
    ("n p", "next / previous · Shift ← → seek 10s · + - volume"),
    ("l x a m", "like · dislike (skips it, trains Discover) · add to queue · more actions"),
    ("d J K", "in Queue: remove · move down / up"),
    ("v y w q", "full player · lyrics · visualizer on / off · quit"),
    ("V", "trip: psychedelic visualizer that follows the song (needs w on)"),
    ("D R", "D discover new songs · R radio from the highlighted song, artist or album"),
    ("t", "in Stats: week / month / year · ← → songs / artists / albums"),
    ("mouse", "optional: click · double-click plays · right-click menu · drag the bar to seek"),
];
