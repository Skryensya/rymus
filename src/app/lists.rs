// Which list is on screen and the cursor in it; loading Home, Stats and the other tabs.
use super::*;

impl App {
    pub fn data(&self) -> &[Item] {
        match self.tab {
            Tab::Home if self.home_focus == PaneId::Left => &self.home_left,
            Tab::Home => &self.home_right,
            Tab::Playlists => &self.playlists,
            Tab::Liked => &self.liked,
            Tab::Queue => &self.queue,
            Tab::Stats => &self.stat_rows,
            Tab::Config => &self.settings,
            Tab::Eq => &[], // its rows are drawn from the profiles
            Tab::Search => &self.items,
        }
    }
    pub fn cur(&self) -> Option<&Item> {
        self.data().get(self.sel)
    }
    pub fn kind_at(&self) -> Kind {
        self.cur().map_or(Kind::Track, |t| t.kind)
    }
    pub(super) fn in_search_results(&self) -> bool {
        self.tab == Tab::Search && self.result_lists.contains(&self.list_id)
    }
    // The songs from row i on (a mixed list also holds artists, albums and headers).
    // The Liked tab: your likes, then a small section with the songs you disliked most recently (x takes one back).
    pub(super) fn load_liked(&mut self) {
        const SHOWN: usize = 5;
        let mut rows = self.db.liked();
        let disliked = self.db.disliked();
        if !disliked.is_empty() {
            let more = if disliked.len() > SHOWN { format!(" · last {SHOWN} of {}", disliked.len()) } else { String::new() };
            rows.push(Item::header(&format!("Disliked{more}")));
            rows.extend(disliked.into_iter().take(SHOWN));
        }
        self.liked = rows;
    }
    pub(super) fn tracks_from(&self, i: usize) -> Vec<Item> {
        let songs = self.data().iter().skip(i);
        if matches!(self.tab, Tab::Home | Tab::Liked) {
            // a list of sections: play to the end of this one (Liked never runs on into the disliked songs)
            return songs.take_while(|t| t.kind == Kind::Track).cloned().collect();
        }
        songs.filter(|t| t.kind == Kind::Track).cloned().collect()
    }
    // Headers cannot be selected: step past them, turning back at either end of the list.
    pub(super) fn move_sel(&mut self, to: i64) {
        let data = self.data();
        if data.is_empty() {
            self.sel = 0;
            return;
        }
        let dir: i64 = if to < self.sel as i64 { -1 } else { 1 };
        let at = to.clamp(0, data.len() as i64 - 1);
        let header = |i: i64| i >= 0 && data.get(i as usize).is_some_and(|t| t.kind == Kind::Header);
        let mut i = at;
        while header(i) {
            i += dir;
        }
        if i < 0 || i >= data.len() as i64 {
            i = at;
            while header(i) {
                i -= dir;
            }
        }
        self.sel = i.max(0) as usize;
    }
    // Is this row the track that is playing? In the queue the same song can appear twice, so compare the entry itself.
    pub fn is_current(&self, it: &Item) -> bool {
        self.now().is_some_and(|n| if self.tab == Tab::Queue { it.uid == n.uid } else { it.id == n.id })
    }

    pub(super) fn resume_row(&self) -> Option<Item> {
        let s = self.session.as_ref()?;
        let t = s.queue.first()?;
        Some(Item {
            kind: Kind::Resume,
            id: "resume".into(),
            title: format!("⏯ Resume: {}", t.title),
            detail: Some(format!("{} · at {}", t.artist, fmt(s.time as f64))),
            ..Default::default()
        })
    }
    pub(super) fn new_playlist_row() -> Item {
        Item { kind: Kind::NewPlaylist, id: "new".into(), title: "+ New playlist".into(), detail: Some("or paste a link with /".into()), ..Default::default() }
    }
    pub(super) fn load_playlists(&mut self) {
        self.playlists = std::iter::once(App::new_playlist_row()).chain(self.db.playlists()).collect();
    }
    // Puts the cursor of the Playlists tab (or Home's list) on a playlist.
    pub(super) fn point_at_playlist(&mut self, id: i64) {
        let id = id.to_string();
        let find = |list: &[Item]| list.iter().position(|t| t.kind == Kind::Playlist && t.id == id);
        if let Some(i) = find(&self.playlists) {
            self.playlists_sel = i;
            if self.tab == Tab::Playlists {
                self.sel = i;
            }
        }
        for (column, pane) in [PaneId::Left, PaneId::Right].into_iter().enumerate() {
            let list = if column == 0 { &self.home_left } else { &self.home_right };
            if let Some(i) = find(list) {
                if self.tab == Tab::Home && self.home_focus == pane {
                    self.sel = i;
                } else {
                    self.home_sel[column] = i;
                }
            }
        }
    }
    // The rows of one block of Home; list blocks start with their name.
    // Home: what to play now on the left (Listen), your things on the right (Library). Fixed, so it reads the same
    // every time; on a narrow screen the two are one column. Each section keeps its name in sight (see home::window).
    pub(crate) fn load_home(&mut self) {
        let section = |name: &str, list: Vec<Item>| -> Vec<Item> {
            if list.is_empty() { vec![] } else { std::iter::once(Item::header(name)).chain(list).collect() }
        };
        let discover = Item { kind: Kind::Discover, id: "discover".into(), title: "✦ Discover".into(), detail: Some("new songs · mixes by style".into()), ..Default::default() };
        let mut listen: Vec<Item> = self.resume_row().into_iter().chain([discover]).collect();
        listen.extend(section("Up next", self.queue.iter().skip((self.pos + 1).max(0) as usize).take(5).cloned().collect()));
        listen.extend(section("Recently played", self.db.recent(50)));

        let month = self.db.recap(now_ms() - PERIODS[1].1 * 86_400_000);
        let artists: Vec<Item> = month.artists.into_iter().take(5).map(|r| Item { kind: Kind::ArtistStat, detail: Some(minutes(r.seconds)), ..r }).collect();
        let searches: Vec<Item> = self.db.recent_searches().into_iter().take(5).map(|q| Item { kind: Kind::Query, id: q.clone(), title: q, detail: Some("search again".into()), ..Default::default() }).collect();
        // always there, so there is a place to create the first playlist
        let mut library: Vec<Item> = std::iter::once(Item::header("Playlists")).chain(self.db.playlists()).chain([App::new_playlist_row()]).collect();
        library.extend(section("Top artists · this month", artists));
        library.extend(section("Recent searches", searches));

        if self.home_wide {
            self.home_titles = ["Listen".into(), "Library".into()];
            self.home_left = listen;
            self.home_right = library;
        } else {
            self.home_titles = ["Home".into(), String::new()];
            listen.extend(library);
            self.home_left = listen;
            self.home_right = vec![];
        }
        // never leave the cursor in a column with nothing in it
        let empty = [self.home_left.is_empty(), self.home_right.is_empty()];
        let at = usize::from(self.home_focus == PaneId::Right);
        if empty[at] && !empty[1 - at] {
            self.home_focus = if at == 0 { PaneId::Right } else { PaneId::Left };
            if self.tab == Tab::Home {
                self.sel = self.home_sel[1 - at];
            }
        }
    }
    pub(super) fn load_stats(&mut self) {
        let since = |days: i64| now_ms() - days * 86_400_000;
        self.totals = std::array::from_fn(|i| self.db.listened(since(PERIODS[i].1)));
        let recap = self.db.recap(since(PERIODS[self.period].1));
        // the timeline reaches back to the start of its first day (or month), a little past the rolling window
        let plays = self.db.listening(since(PERIODS[self.period].1 + 31));
        self.timeline = charts::timeline(&plays, now_ms(), PERIODS[self.period].1);
        let window = since(PERIODS[self.period].1);
        self.hours = charts::by_hour(&plays.iter().copied().filter(|p| p.0 >= window).collect::<Vec<_>>());
        let (rows, kind) = match self.stat_cat {
            0 => (recap.songs, Kind::Track),
            1 => (recap.artists, Kind::ArtistStat),
            _ => (recap.albums, Kind::AlbumStat),
        };
        let artists = self.stat_cat == 1;
        self.stat_rows = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let by = if artists { String::new() } else { format!(" · {}", r.artist) };
                Item { kind, rank: i + 1, detail: Some(format!("{}{by}", minutes(r.seconds))), ..r }
            })
            .collect();
    }
    pub(crate) fn clamp_sel(&mut self) {
        self.sel = self.sel.min(self.data().len().saturating_sub(1));
        if self.kind_at() == Kind::Header {
            self.move_sel(self.sel as i64);
        }
    }
}
