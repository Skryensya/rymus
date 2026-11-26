// The player: the big full-screen view (v) and the dock at the bottom.
use crate::i18n::tr;
use super::*;

impl App {
    // Big cover + info for the full player. Returns the inner rows and where a real image goes (row, col, columns).
    pub(super) fn full_pane(&mut self, g: &mut Geo, iw: usize, l: usize, focus: Option<&Item>, got: Option<&Arc<Got>>) -> (Vec<String>, Option<(usize, usize, usize)>) {
        let top = g.top;
        let mut c = iw.saturating_sub(4);
        if c / 2 + 6 > l {
            c = 2 * l.saturating_sub(6); // leave room for the info lines
        }
        let url = focus.map(|f| f.cover.clone()).unwrap_or_default();
        let show = !url.is_empty() && c >= 12;
        let r = if show { c.div_ceil(2) } else { 0 };
        let pad_l = iw.saturating_sub(c) / 2;
        let geo = (show && self.kitty).then_some((top, 2 + pad_l, c));
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
                    vec![strong, plain(format!("{}{likes}", minutes(f.seconds)))]
                } else if !f.author.is_empty() {
                    let year = if f.year.is_empty() { String::new() } else { format!(" · {}", f.year) };
                    vec![strong, plain(format!("{}{year}", f.author))]
                } else {
                    let g = glyphs();
                    let heart = if self.likes.contains(&f.id) {
                        (tr!("{a}  Liked · l to remove", a = g.like), Some(readable(PINK)), 0)
                    } else if self.dislikes.contains(&f.id) {
                        (tr!("{a}  Disliked · x to remove", a = g.dislike), Some(readable(BLUE)), 0)
                    } else {
                        (tr!("{a}  l like  ·  {b}  x dislike", a = g.unliked, b = g.dislike), None, 2)
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
                let Some((j, (text, color, style))) = usize::try_from(at).ok().and_then(|j| Some((j, info.get(j)?))) else { return spaces(iw) };
                let t = cut(text, iw.saturating_sub(2));
                let p = iw.saturating_sub(width(&t)) / 2;
                // a song's artist and album lines open their pages
                if let Some(f) = focus.filter(|f| f.kind == Kind::Track && f.author.is_empty() && (j == 1 || j == 2)) {
                    let parts = if j == 1 { artist_parts(&f.artist) } else { vec![album_part(f)] };
                    let (line, n) = linked(&mut g.links, &parts, 2 + iw.saturating_sub(width(&t)) / 2, top + i, width(&t));
                    let p = iw.saturating_sub(n) / 2;
                    let line = if *style == 2 { dim(&line) } else { line };
                    return format!("{}{line}{}", spaces(p), spaces(iw.saturating_sub(p + n)));
                }
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
    // `first` is the screen row of its first line inside the frame.
    // `gap`: one more row, blank, between the artist and the visualizer, when the screen has the height for it.
    pub(super) fn dock(&mut self, g: &mut Geo, w: usize, first: usize, r: usize, gap: bool, focus: Option<&Item>, got: Option<&Arc<Got>>, a: Rgb) -> (Vec<String>, Option<(usize, usize, usize)>) {
        let last = first + r - 1 + usize::from(gap); // the progress line
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
        let geo = (thumb && self.kitty).then_some((first, 3, c));
        let blank = spaces(tx);
        let now = self.now().cloned();
        // the buttons (« ‖ » ↻) are small icons at the start of the progress line, so the song and the visualizer get
        // the whole width; the cover is a play/pause button too
        let buttons = tx >= 36;
        let status = if self.loading { spin() } else if self.paused { "⏸" } else { "▶" };
        let count = now.as_ref().map_or(String::new(), |_| format!(" {}/{}", self.pos + 1, self.queue.len()));
        let vol = tr!("vol {a}", a = self.vol);
        let (r0, r1) = if let Some(t) = &now {
            // without buttons, the state shows before the title
            let lead = if buttons { String::new() } else { format!("{status} ") };
            let line = cut(&format!("{lead}{}{}", self.tag(&t.id), t.title), tx.saturating_sub(count.len()));
            let r0 = format!("{}{}{}", self.song_name(&t.id, &line, a), spaces(tx.saturating_sub(width(&line) + count.len())), dim(&count));
            let indent = if buttons { "" } else { "  " };
            let room = tx.saturating_sub(vol.len() + 1);
            let mut parts = vec![(indent.to_string(), None)];
            parts.extend(artist_parts(&t.artist));
            if !t.album.is_empty() {
                parts.extend([(" · ".to_string(), None), album_part(t)]);
            }
            let (credits, n) = linked(&mut g.links, &parts, 3 + tw, first + 1, room);
            (r0, format!("{} {}", dim(&format!("{credits}{}", spaces(room.saturating_sub(n)))), dim(&vol)))
        } else if let Some((q, time)) = self.session.as_ref().and_then(|s| s.queue.first().map(|q| (q, s.time))) {
            (fg(a, &cell(&format!("{} — {}", q.title, q.artist), tx)), dim(&cell(&tr!("stopped at {a} · resume it from Home", a = fmt(time as f64)), tx)))
        } else {
            match focus {
                Some(f) => (bold(&cell(&f.title, tx)), dim(&cell(if f.artist.is_empty() { &f.author } else { &f.artist }, tx))),
                None => (dim(&cell(tr!("Nothing playing"), tx)), spaces(tx)),
            }
        };
        let first_x = 3 + tw; // screen column of the text's first character
        let controls = if buttons { self.controls(g, first_x, last, a) } else { String::new() };
        let cw = if buttons { 13 } else { 0 }; // four buttons of 3 and a space
        let left = format!("{} ", fmt(if now.is_some() { self.time as f64 } else { 0.0 }));
        let length = if now.is_some() && self.dur > 0.0 { fmt(self.dur) } else { "--:--".into() };
        let right = cut(&format!(" {length}"), tx.saturating_sub(cw + left.len()));
        let bw = tx.saturating_sub(cw + left.len() + width(&right));
        let filled = if self.dur > 0.0 && bw > 0 { ((bw - 1) as f64 * (self.time as f64 / self.dur).min(1.0)).round() as usize } else { 0 };
        let bar = if now.is_some() && bw > 0 {
            format!("{}{}{}", dim(&left), fg(a, &format!("{}●", "━".repeat(filled))), dim(&format!("{}{right}", "─".repeat(bw - 1 - filled))))
        } else {
            spaces(tx - cw)
        };
        let progress = format!("{controls}{bar}");
        g.bar_row = last;
        g.bar_x0 = first_x - 1 + cw + left.len();
        g.bar_w = bw;
        g.thumb = thumb.then_some((3, 2 + c, first, first + r - 1));
        // without buttons, a click on the title line plays or pauses
        g.now_row = if buttons { 0 } else { first };
        let middle = if self.wave && now.is_some() { spectrum(&self.bands, tx, r - 3, a, self.viz_style) } else { vec![blank.clone(); r - 3] };
        let inner: Vec<String> = [r0, r1]
            .into_iter()
            .chain(gap.then(|| blank.clone()))
            .chain(middle)
            .chain(std::iter::once(progress))
            .enumerate()
            .map(|(i, row)| {
                let cover = if thumb { format!("{}  ", art.as_ref().and_then(|a| a.get(i).cloned()).unwrap_or_else(|| spaces(c))) } else { String::new() };
                format!(" {cover}{row} ")
            })
            .collect();
        let title = if now.is_some() { tr!("Now playing") } else if self.session.is_some() { tr!("Last session") } else if focus.is_some() { tr!("Preview") } else { tr!("Now playing") };
        let title = match self.sleep_label() {
            Some(sleep) => format!("{title} · {sleep}"),
            None => title.to_string(),
        };
        (frame(&title, txt(), &inner, w, gray()), geo)
    }

    // The four buttons (previous, play or pause, next, repeat) as 13 columns: each icon with a space either side, then a
    // space. Starts at screen column x on row y; each is recorded as its own click area. Play or pause is in the accent
    // colour and spins while loading; the others are dim, repeat lit when on.
    fn controls(&self, g: &mut Geo, x: usize, y: usize, a: Rgb) -> String {
        let gl = glyphs();
        let play = if self.loading { spin() } else if self.now().is_some() && !self.paused { gl.pause } else { gl.play };
        let (repeat, rc) = match self.repeat {
            Repeat::Off => (gl.repeat, gray()),
            Repeat::All => (gl.repeat, a),
            Repeat::One => (gl.repeat_one, a),
        };
        let buttons = [(Control::Prev, gl.prev, gray()), (Control::PlayPause, play, a), (Control::Next, gl.next, gray()), (Control::Repeat, repeat, rc)];
        let mut out = String::new();
        for (k, (control, icon, color)) in buttons.into_iter().enumerate() {
            out += &format!(" {} ", fg(color, &cell(icon, 1)));
            g.controls.push((control, x + 3 * k, x + 3 * k + 2, y, y));
        }
        out + " "
    }

    // On a short screen the player is two plain lines (three with the wave) instead of the framed dock.
    pub(super) fn compact_player(&mut self, v: &View, g: &mut Geo, h: usize, now: Option<&Item>) -> Vec<String> {
        let (w, a) = (v.w, v.a);
        let mut lines = Vec::new();

        // two plain lines (three with the wave), no cover
        let status = if self.loading { spin() } else if self.paused { "⏸" } else { "▶" };
        let count = now.map_or(String::new(), |_| format!(" {}/{}", self.pos + 1, self.queue.len()));
        match now {
            Some(t) => {
                let room = w.saturating_sub(1 + count.len());
                let name = cut(&format!("{status} {}{}", self.tag(&t.id), t.title), room);
                let by = cut(&format!(" — {}", t.artist), room.saturating_sub(width(&name)));
                let used = width(&name) + width(&by) + count.len();
                lines.push(format!("{}{by}{}{}", self.song_name(&t.id, &name, a), dim(&count), spaces(w.saturating_sub(used))));
            }
            None => lines.push(match self.session.as_ref().and_then(|s| s.queue.first()) {
                Some(q) => fg(a, &cell(&tr!("⏯ Resume: {a} (Home)", a = q.title), w)),
                None => format!("{}{}", dim(tr!("Nothing playing")), spaces(w.saturating_sub(15))),
            }),
        }
        if self.wave {
            lines.push(if now.is_some() { spectrum(&self.bands, w, 1, a, self.viz_style).remove(0) } else { spaces(w) });
        }
        let left = format!("{} ", fmt(self.time as f64));
        let length = if self.dur > 0.0 { fmt(self.dur) } else { "--:--".into() };
        let right = tr!(" {length}  vol {a}", length = length, a = self.vol);
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
        lines
    }
}

// Each credited artist on its own, so each opens its own page.
pub(super) fn artist_parts(credit: &str) -> Vec<(String, Option<Action>)> {
    let mut parts = vec![];
    for name in credit.split(", ").filter(|n| !n.is_empty()) {
        if !parts.is_empty() {
            parts.push((", ".to_string(), None));
        }
        parts.push((name.to_string(), Some(Action::GoArtist(name.to_string()))));
    }
    parts
}
pub(super) fn album_part(t: &Item) -> (String, Option<Action>) {
    let artist = t.artist.split(", ").next().unwrap_or("").to_string();
    let album = Item { kind: Kind::AlbumStat, title: t.album.clone(), artist, ..Default::default() };
    (t.album.clone(), (!t.album.is_empty()).then_some(Action::GoAlbum(album)))
}
// A name that opens its page: (what it does, x0, x1, y).
pub(super) type Links = Vec<(Action, usize, usize, usize)>;
// The parts on one line of at most `room` columns, links underlined and recorded where they land (from column x on row
// y). Returns the line and how many columns it takes.
pub(super) fn linked(links: &mut Links, parts: &[(String, Option<Action>)], x: usize, y: usize, room: usize) -> (String, usize) {
    let (mut out, mut used) = (String::new(), 0);
    for (text, action) in parts {
        if used >= room {
            break;
        }
        let shown = if width(text) > room - used { cut(text, room - used) } else { text.clone() };
        let n = width(&shown);
        match action {
            Some(action) if n > 0 => {
                links.push((action.clone(), x + used, x + used + n - 1, y));
                out += &format!("\x1b[4m{shown}\x1b[24m");
            }
            _ => out += &shown,
        }
        used += n;
    }
    (out, used)
}
