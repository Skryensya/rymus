// The rows of a list: marks, number, title and detail columns, with the focused row's marquee.
use crate::i18n::tr;
use super::*;
use super::player::{album_part, artist_parts, linked, Links};

impl App {
    // The l rows of a list pane, each exactly w columns: marks, title column, detail column.
    // hl: draw the selection (only the focused pane). Names that open a page are underlined and added to `links`, placed
    // from the row's first column and the pane's first row.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn list_rows(&mut self, mq: &mut Marquee, data: &[Item], sel: usize, top: usize, w: usize, l: usize, hl: bool, a: Rgb, now: Option<&str>, empty: &str, links: &mut Links) -> Vec<String> {
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
                    _ if self.tab == Tab::Queue && track => {
                        let album = if it.album.is_empty() { String::new() } else { format!(" · {}", it.album) };
                        let how = match it.origin {
                            Origin::Added => tr!(" · added by you"),
                            Origin::Radio => tr!(" · radio"),
                            _ => "",
                        };
                        format!("{}{album}{how}", it.artist)
                    }
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
                let mut title = if focused { marquee(mq, &it.title, tw) } else { cell(&it.title, tw) };
                // an album or artist row: its name is a link
                if matches!(it.kind, Kind::Album | Kind::Artist | Kind::ArtistStat | Kind::AlbumStat) {
                    if let Some(action) = App::open_action(it) {
                        let (x, moving) = (3 + num.len(), focused && width(&it.title) > tw);
                        let name = if moving { title.clone() } else { cut(&it.title, tw) };
                        let n = width(&name);
                        if n > 0 {
                            links.push((action, x, x + n - 1, i));
                            title = format!("\x1b[4m{name}\x1b[24m{}", spaces(tw.saturating_sub(n)));
                        }
                    }
                }
                let fixed = if it.kind == Kind::Setting { cut(&it.author, rw) } else { String::new() }; // a setting's value never scrolls
                let dw = rw.saturating_sub(width(&fixed));
                // the artist named in the detail opens their page
                let artist = match it.kind {
                    Kind::Track | Kind::AlbumStat => it.artist.as_str(),
                    Kind::Album => it.author.as_str(),
                    _ => "",
                };
                let credit = detail.rfind(artist).filter(|_| !artist.is_empty() && dw > 0);
                let shown = match credit {
                    // scrolling text: the whole column is the link
                    Some(_) if focused && width(&detail) > dw => {
                        let first = artist.split(", ").next().unwrap_or(artist);
                        links.push((Action::GoArtist(first.to_string()), 6 + num.len() + tw, 5 + num.len() + tw + dw, i));
                        format!("\x1b[4m{}\x1b[24m", marquee(mq, &detail, dw))
                    }
                    Some(at) => {
                        let mut parts = vec![(detail[..at].to_string(), None)];
                        parts.extend(artist_parts(artist));
                        let mut after = &detail[at + artist.len()..];
                        // the queue names the album too
                        if let Some(more) = after.strip_prefix(" · ").and_then(|s| s.strip_prefix(it.album.as_str())).filter(|_| self.tab == Tab::Queue && !it.album.is_empty()) {
                            parts.extend([(" · ".to_string(), None), album_part(it)]);
                            after = more;
                        }
                        parts.push((after.to_string(), None));
                        let (line, n) = linked(links, &parts, 6 + num.len() + tw, i, dw);
                        format!("{line}{}", spaces(dw.saturating_sub(n)))
                    }
                    None if focused && dw > 0 => marquee(mq, &detail, dw),
                    None => cell(&detail, dw),
                };
                let rest = fixed + &shown;
                // what plays (♪) or is new (✦) before the title; the like or dislike in its own column after it
                let mark = if playing { "♪" } else if it.fresh { "✦" } else if track && self.downloaded(&it.id) { "↓" } else { " " };
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
        let sel = self.sel;
        // search results have their tabs on a line of their own, above the list
        let tabs = self.results().map(|f| (f.tab, [0, 1, 2].map(|i| format!("{} {}{}", crate::i18n::t(KINDS[i]), f.lists[i].len(), if f.next[i].is_some() { "+" } else { "" }))));
        let head = usize::from(tabs.is_some());
        let l = l.saturating_sub(head);
        let top = sel.saturating_sub(l / 2).min(self.data().len().saturating_sub(l));
        let (def, empty) = match self.tab {
            Tab::Playlists => (tr!("Playlists"), ""),
            Tab::Config => (tr!("Config · ← → or ⏎ to change"), ""),
            Tab::Liked => (if self.liked_by_album { tr!("Liked · by album") } else { tr!("Liked · ← → by album") }, tr!("No likes yet. Press l on any track.")),
            Tab::Queue => (tr!("Queue"), tr!("The queue is empty. Press Enter on a track, or a to add one.")),
            Tab::History => (tr!("History"), tr!("Nothing listened yet.")),
            Tab::Library => (tr!("Library"), tr!("Nothing yet: l on an album saves it, F follows an artist.")),
            _ if self.viewed_playlist().is_some() => (self.title.as_str(), tr!("Empty playlist. Press s on any song to add it.")),
            _ => (if self.title.is_empty() { tr!("Results") } else { self.title.as_str() }, tabs.as_ref().map_or(tr!("No results."), |t| [tr!("No songs found."), tr!("No albums found."), tr!("No artists found.")][t.0])),
        };
        let narrowed = self.filtered().map(|f| tr!("Filter \"{a}\" · {b} · esc clears", a = f.text, b = f.rows.len()));
        let (t, c) = v.title(narrowed.as_deref().unwrap_or(def));
        let empty = if narrowed.is_some() { tr!("Nothing matches.") } else { empty };
        let tab = self.tab;
        let data = match tab {
            _ if narrowed.is_some() => self.filter.as_mut().map_or(vec![], |f| std::mem::take(&mut f.rows)),
            Tab::History => std::mem::take(&mut self.played),
            Tab::Library => std::mem::take(&mut self.library),
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
        // in the Library, the cover of the row you are on
        let picked = self.cur().filter(|i| tab == Tab::Library && matches!(i.kind, Kind::Album | Kind::Artist)).cloned();
        let album = picked.or_else(|| self.head.clone().filter(|_| tab == Tab::Search)).filter(|al| r >= 4 && !al.cover.is_empty());
        let lw = album.as_ref().map_or(iw, |_| iw - (2 * r + 2));
        let songs = data.iter().filter(|t| t.kind == Kind::Track).count();
        let liked = songs > 0 && data.iter().filter(|t| t.kind == Kind::Track).all(|t| self.likes.contains(&t.id) || self.dislikes.contains(&t.id));
        if let Some(al) = &album {
            g.images.push((lw + 3, lw + 2 + 2 * r, g.top, g.top + r - 1, al.cover.clone())); // the cover, c = 2r wide
        }
        let side = album.map(|al| self.album_side(&al, songs, liked, l, r, g.top, lw + 3, head_img)); // border, list, space
        let mut links = vec![];
        let list = self.list_rows(mq, &data, sel, top, lw, l, true, a, playing, empty, &mut links);
        g.links.extend(links.into_iter().map(|(act, x0, x1, y)| (act, x0 + 2, x1 + 2, y + g.top + head)));
        let rows: Vec<String> = match side {
            Some(side) => list.into_iter().zip(side).map(|(x, y)| x + &y).collect(),
            None => list,
        };
        let rows: Vec<String> = match tabs {
            Some((on, names)) => {
                // each name is clickable; ← → moves between them
                let (mut line, mut used) = (" ".to_string(), 1);
                for (i, name) in names.iter().enumerate() {
                    let text = if i == on { format!("[{name}]") } else { format!(" {name} ") };
                    let n = width(&text);
                    if used + n > iw {
                        break;
                    }
                    g.links.push((Action::SearchTab(i), used + 2, used + n + 1, g.top));
                    line += &if i == on { fg(a, &bold(&text)) } else { dim(&text) };
                    line += " ";
                    used += n + 1;
                }
                if used + 3 <= iw {
                    line += &dim("← →");
                    used += 3;
                }
                std::iter::once(format!("{line}{}", spaces(iw.saturating_sub(used)))).chain(rows).collect()
            }
            None => rows,
        };
        match tab {
            _ if narrowed.is_some() => {
                if let Some(f) = &mut self.filter {
                    f.rows = data;
                }
            }
            Tab::History => self.played = data,
            Tab::Library => self.library = data,
            Tab::Playlists => self.playlists = data,
            Tab::Config => self.settings = data,
            Tab::Liked => self.liked = data,
            Tab::Queue => self.queue = data,
            _ => self.items = data,
        }
        g.panes.push(pane(PaneId::List, 1, lw + 1, top as i64 - head as i64, head));
        frame(&t, c, &rows, w, a)
    }
}
