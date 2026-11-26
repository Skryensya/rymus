// The player: the big full-screen view (v) and the dock at the bottom.
use super::*;

impl App {
    // Big cover + info for the full player. Returns the inner rows and where a real image goes (row, col, columns).
    pub(super) fn full_pane(&mut self, top: usize, iw: usize, l: usize, focus: Option<&Item>, got: Option<&Arc<Got>>) -> (Vec<String>, Option<(usize, usize, usize)>) {
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
    pub(super) fn dock(&mut self, g: &mut Geo, w: usize, l: usize, r: usize, focus: Option<&Item>, got: Option<&Arc<Got>>, a: Rgb) -> (Vec<String>, Option<(usize, usize, usize)>) {
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
        let geo = (thumb && self.kitty).then_some((l + g.top + 2, 3, c));
        let blank = spaces(tx);
        let now = self.now().cloned();
        // the buttons (⏮ ⏯ ⏭, boxed) take the top right corner when there is room; the text keeps the rest
        let buttons = tx >= 50;
        let bw_buttons = if buttons { 21 } else { 0 }; // four boxes of 5 and a space before them
        let tx_top = tx - bw_buttons; // width of the text beside the buttons
        let status = if self.loading { spin() } else if self.paused { "⏸" } else { "▶" };
        let count = now.as_ref().map_or(String::new(), |_| format!(" {}/{}", self.pos + 1, self.queue.len()));
        let (r0, r1) = if let Some(t) = &now {
            // without buttons, the state shows before the title
            let lead = if buttons { String::new() } else { format!("{status} ") };
            let line = cut(&format!("{lead}{}{}", self.tag(&t.id), t.title), tx_top.saturating_sub(count.len()));
            let mut chars = line.chars();
            let head = chars.next().unwrap_or(' ').to_string();
            let head = if buttons { fg(a, &bold(&head)) } else { fg(a, &head) }; // with buttons, it is the title's first letter
            let r0 = format!(
                "{}{}{}{}",
                head,
                self.paint(&t.id, &bold(chars.as_str())),
                spaces(tx_top.saturating_sub(width(&line) + count.len())),
                dim(&count)
            );
            let album = if t.album.is_empty() { String::new() } else { format!(" · {}", t.album) };
            let indent = if buttons { "" } else { "  " };
            (r0, dim(&cell(&format!("{indent}{}{album}", t.artist), tx_top)))
        } else if let Some((q, time)) = self.session.as_ref().and_then(|s| s.queue.first().map(|q| (q, s.time))) {
            (
                fg(a, &cell(&format!("{} — {}", q.title, q.artist), tx_top)),
                dim(&cell(&format!("stopped at {} · resume it from Home", fmt(time as f64)), tx_top)),
            )
        } else {
            match focus {
                Some(f) => (bold(&cell(&f.title, tx_top)), dim(&cell(if f.artist.is_empty() { &f.author } else { &f.artist }, tx_top))),
                None => (dim(&cell("Nothing playing", tx_top)), spaces(tx_top)),
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
        g.bar_row = l + g.top + 1 + r;
        g.bar_x0 = 2 + tw + left.len();
        g.bar_w = bw;
        let first = l + g.top + 2; // screen row of the dock's first line
        g.thumb = thumb.then_some((3, 2 + c, first, l + g.top + 1 + r));
        // the buttons are their own click areas; without them, a click on the title line plays or pauses
        g.now_row = if buttons { 0 } else { first };
        let boxes = if buttons { self.control_boxes(g, 3 + tw + tx_top + 1, first, a) } else { vec![] };
        let middle = if self.wave && now.is_some() { spectrum(&self.bands, tx_top, r - 3, a) } else { vec![spaces(tx_top); r - 3] };
        let inner: Vec<String> = std::iter::once(r0)
            .chain(std::iter::once(r1))
            .chain(middle)
            .enumerate()
            .map(|(i, row)| {
                // beside the buttons on their 3 rows, blank under them
                let right = match boxes.get(i) {
                    Some(b) => format!(" {b}"),
                    None => spaces(bw_buttons),
                };
                row + &right
            })
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

    // The four boxed buttons (previous, play or pause, next, repeat) as 3 rows of 20 columns, starting at screen column x and
    // row y; each box is recorded as its own click area. Play or pause is in the accent colour, and spins while loading.
    fn control_boxes(&self, g: &mut Geo, x: usize, y: usize, a: Rgb) -> Vec<String> {
        let gl = glyphs();
        let play = if self.loading { spin() } else if self.now().is_some() && !self.paused { gl.pause } else { gl.play };
        let (repeat, rc) = match self.repeat {
            Repeat::Off => (gl.repeat, GRAY),
            Repeat::All => (gl.repeat, a),
            Repeat::One => (gl.repeat_one, a),
        };
        let buttons = [(Control::Prev, gl.prev, TXT), (Control::PlayPause, play, a), (Control::Next, gl.next, TXT), (Control::Repeat, repeat, rc)];
        let mut rows = [String::new(), String::new(), String::new()];
        for (k, (control, icon, color)) in buttons.into_iter().enumerate() {
            let border = if control == Control::PlayPause || (control == Control::Repeat && self.repeat != Repeat::Off) { a } else { GRAY };
            rows[0] += &fg(border, "╭───╮");
            rows[1] += &format!("{}{}{}", fg(border, "│ "), fg(color, &cell(icon, 1)), fg(border, " │"));
            rows[2] += &fg(border, "╰───╯");
            g.controls.push((control, x + 5 * k, x + 5 * k + 4, y, y + 2));
        }
        rows.to_vec()
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
        lines
    }
}
