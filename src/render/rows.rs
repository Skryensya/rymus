// The rows of a list: marks, number, title and detail columns, with the focused row's marquee.
use super::*;

impl App {
    // The l rows of a list pane, each exactly w columns: marks, title column, detail column.
    // hl: draw the selection (only the focused pane).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn list_rows(&mut self, mq: &mut Marquee, data: &[Item], sel: usize, top: usize, w: usize, l: usize, hl: bool, a: Rgb, now: Option<&str>, empty: &str) -> Vec<String> {
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
                } else if self.tab == Tab::Search && self.numbered && track {
                    format!("{:>2}  ", data[..=idx].iter().filter(|t| t.kind == Kind::Track).count()) // songs only: not the buttons above them
                } else {
                    String::new()
                };
                let detail = it.detail.clone().unwrap_or_else(|| match it.kind {
                    Kind::Album | Kind::Artist => {
                        if it.year.is_empty() { it.author.clone() } else { format!("{} · {}", it.author, it.year) }
                    }
                    Kind::ArtistStat => {
                        let likes = if it.likes > 0 { format!(" · {} {}", glyphs().like, it.likes) } else { String::new() };
                        format!("{}{likes}", minutes(it.seconds))
                    }
                    _ if self.tab == Tab::Queue && it.origin == Origin::Added => format!("{} · added by you", it.artist),
                    _ if self.tab == Tab::Queue && it.origin == Origin::Radio => format!("{} · radio", it.artist),
                    _ => it.artist.clone(),
                });
                // bar, mark, space ... right margin; room holds the title, the rating column (a space each side) and the detail
                let room = w.saturating_sub(4 + num.len());
                let tw = (room * 6).div_ceil(10);
                let rw = room.saturating_sub(tw + 3);
                let focused = hl && idx == sel;
                if focused {
                    let key = format!("{:?}|{idx}|{}", self.tab, it.id); // a new row starts from the beginning
                    if key != mq.key {
                        mq.key = key;
                        mq.at = Some(Instant::now());
                    }
                }
                let title = if focused { marquee(mq, &it.title, tw) } else { cell(&it.title, tw) };
                let fixed = if it.kind == Kind::Setting { cut(&it.author, rw) } else { String::new() }; // a setting's value never scrolls
                let dw = rw.saturating_sub(width(&fixed));
                let rest = fixed + &if focused && dw > 0 { marquee(mq, &detail, dw) } else { cell(&detail, dw) };
                // what plays (♪) or is new (✦) before the title; the like or dislike in its own column after it
                let mark = if playing { "♪" } else if it.fresh { "✦" } else { " " };
                if focused {
                    return selected(a, &format!("{mark} {num}{title} {} {rest} ", rating.map_or(" ", |r| r.0)));
                }
                let mark = if mark == " " { " ".to_string() } else { fg(a, mark) };
                let rating = rating.map_or(" ".to_string(), |r| fg(r.1, r.0));
                let title = if playing { fg(a, &title) } else { title };
                format!(" {mark} {}{title} {rating} {} ", dim(&num), dim(&rest))
            })
            .collect()
    }

    // Every other list (Playlists, Liked, Queue, Config, search results, pages), with an album's cover beside its songs.
    pub(super) fn list_body(&mut self, v: &View, mq: &mut Marquee, g: &mut Geo, head_img: &mut Option<Placed>) -> Vec<String> {
        let (w, iw, l, a, playing) = (v.w, v.iw, v.l, v.a, v.playing.as_deref());
        let top_of = |s: usize, n: usize| s.saturating_sub(l / 2).min(n.saturating_sub(l));

        let sel = self.sel;
        let top = top_of(sel, self.data().len());
        let (def, empty) = match self.tab {
            Tab::Playlists => ("Playlists", ""),
            Tab::Config => ("Config · ← → or ⏎ to change", ""),
            Tab::Liked => ("Liked", "No likes yet. Press l on any track."),
            Tab::Queue => ("Queue", "The queue is empty. Press Enter on a track, or a to add one."),
            _ if self.viewed_playlist().is_some() => (self.title.as_str(), "Empty playlist. Press s on any song to add it."),
            _ => (if self.title.is_empty() { "Results" } else { self.title.as_str() }, "No results."),
        };
        let (t, c) = v.title(def);
        let tab = self.tab;
        let data = match tab {
            Tab::Playlists => std::mem::take(&mut self.playlists),
            Tab::Config => std::mem::take(&mut self.settings),
            Tab::Liked => std::mem::take(&mut self.liked),
            Tab::Queue => std::mem::take(&mut self.queue),
            _ => std::mem::take(&mut self.items),
        };
        // an album's page: the songs on the left, the cover on the right with the album's details under it.
        // The cover is square (twice as many columns as rows), leaves room for 5 lines of details, and takes at
        // most 45% of the width, and never leaves the songs less than 30 columns.
        let r = l.saturating_sub(5).min(iw * 45 / 200).min(iw.saturating_sub(32) / 2).min(16);
        let album = self.head.clone().filter(|al| tab == Tab::Search && r >= 4 && !al.cover.is_empty());
        let lw = album.as_ref().map_or(iw, |_| iw - (2 * r + 2));
        let songs = data.iter().filter(|t| t.kind == Kind::Track).count();
        let side = album.map(|al| self.album_side(&al, songs, l, r, g.top, lw + 3, head_img)); // border, list, space
        let list = self.list_rows(mq, &data, sel, top, lw, l, true, a, playing, empty);
        let rows: Vec<String> = match side {
            Some(side) => list.into_iter().zip(side).map(|(x, y)| x + &y).collect(),
            None => list,
        };
        match tab {
            Tab::Playlists => self.playlists = data,
            Tab::Config => self.settings = data,
            Tab::Liked => self.liked = data,
            Tab::Queue => self.queue = data,
            _ => self.items = data,
        }
        g.panes.push(pane(PaneId::List, 1, lw + 1, top as i64, 0));
        frame(&t, c, &rows, w, a)
    }
}
