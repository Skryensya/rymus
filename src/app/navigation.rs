// Moving around: tabs, going back, the action behind each row, and the m menu.
use super::*;

impl App {
    pub(super) fn remember_home_sel(&mut self) {
        self.home_sel[usize::from(self.home_focus == PaneId::Right)] = self.sel;
    }
    pub(super) fn focus_home(&mut self, pane: PaneId) {
        if pane == self.home_focus || !matches!(pane, PaneId::Left | PaneId::Right) {
            return;
        }
        let empty = if pane == PaneId::Left { self.home_left.is_empty() } else { self.home_right.is_empty() };
        if empty {
            return;
        }
        self.remember_home_sel();
        self.home_focus = pane;
        self.sel = self.home_sel[usize::from(pane == PaneId::Right)];
        self.clamp_sel();
    }
    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab == Tab::Home {
            self.remember_home_sel();
        }
        if self.tab == Tab::Search {
            self.search_sel = self.sel;
        }
        if self.tab == Tab::Playlists {
            self.playlists_sel = self.sel;
        }
        self.tab = tab;
        self.typing = false;
        self.menu = None;
        self.lyrics = false;
        match tab {
            Tab::Liked => self.load_liked(),
            Tab::Home => self.load_home(),
            Tab::Stats => self.load_stats(),
            Tab::Playlists => self.load_playlists(),
            Tab::Config => self.settings = config::rows(&self.db),
            Tab::Eq => self.enter_eq(),
            _ => {}
        }
        self.sel = match tab {
            Tab::Playlists => self.playlists_sel,
            Tab::Search => self.search_sel,
            Tab::Home => self.home_sel[usize::from(self.home_focus == PaneId::Right)],
            Tab::Queue => self.pos.max(0) as usize,
            _ => 0,
        };
        self.clamp_sel();
    }
    pub(super) fn cycle_tab(&mut self, dir: i64) {
        let base = if self.tab == Tab::Search { self.prev_tab } else { self.tab };
        let i = TABS.iter().position(|&t| t == base).unwrap_or(0) as i64;
        self.set_tab(TABS[(i + dir).rem_euclid(TABS.len() as i64) as usize]);
    }

    pub fn set_note(&mut self, text: String) {
        self.note = text;
        self.note_until = Some(Instant::now() + Duration::from_secs(3));
    }

    // Likes or dislikes t; without t, the highlighted track, or the playing one when the highlight is not a track
    // or the list is hidden (full player, lyrics).
    pub(super) fn rate(&mut self, like: bool, t: Option<Item>) {
        let t = t.or_else(|| {
            if !self.full && !self.lyrics && self.kind_at() == Kind::Track { self.cur().cloned() } else { self.now().cloned() }
        });
        let verb = if like { "like" } else { "dislike" };
        let Some(t) = t else { return self.set_note(format!("Nothing to {verb}")) };
        let on = if like { self.db.toggle_like(&t) } else { self.db.toggle_dislike(&t) };
        self.likes = self.db.liked_ids();
        self.dislikes = self.db.disliked_ids();
        match self.tab {
            Tab::Liked => self.load_liked(),
            Tab::Home => self.load_home(),
            Tab::Stats => self.load_stats(),
            _ => {}
        }
        self.clamp_sel();
        let what = match (on, like) {
            (false, _) => format!("Removed {verb}"),
            (true, true) => format!("{} Liked", glyphs().like),
            (true, false) => format!("{} Disliked", glyphs().dislike),
        };
        self.set_note(format!("{what}: {}", t.title));
        // disliking what is playing moves on to the next song
        if on && !like && self.now().is_some_and(|n| n.id == t.id) && self.pos < self.queue.len() as i64 - 1 {
            self.cmd(json!(["playlist-next"]));
        }
    }

    pub(super) fn run(&mut self, action: Action) {
        match action {
            Action::OpenAlbum(a) => self.open_album(a),
            Action::OpenArtist(a) => self.open_artist(a),
            Action::GoArtist(name) => self.go_artist(&name),
            Action::GoAlbum(a) => self.go_album(a),
            Action::Discover => self.open_discover(),
            Action::PlayMix(spec, label) => self.play_mix(spec, label),
            Action::Resume => self.resume(),
            Action::Jump(uid) => {
                if let Some(i) = self.queue.iter().position(|t| t.uid == uid) {
                    self.cmd(json!(["playlist-play-index", i]));
                }
            }
            // from search results: that song, then songs like it (not the next result)
            Action::PlayFrom(i) if self.in_search_results() => {
                let Some(t) = self.data().get(i).cloned() else { return };
                let seed = t.id.clone();
                self.play(vec![t]);
                self.endless = true;
                self.fetch_similar(seed);
            }
            Action::PlayFrom(i) => {
                let tracks = self.tracks_from(i);
                self.play(tracks);
            }
            Action::PlayAll(shuffle) => {
                let mut tracks = self.tracks_from(0);
                if shuffle {
                    for i in (1..tracks.len()).rev() {
                        tracks.swap(i, (recommend::random() * (i + 1) as f64) as usize);
                    }
                }
                self.play(tracks);
            }
            Action::PlayNext(t) => {
                self.set_note(format!("Next: {}", t.title));
                self.queue_track(t, true);
            }
            Action::Enqueue(t) => {
                self.set_note(format!("Queued: {}", t.title));
                self.queue_track(t, false);
            }
            Action::Rate(like, t) => self.rate(like, Some(t)),
            Action::Remove(uid) => self.remove_from_queue(uid),
            Action::Radio(t) => self.start_radio(Some(t)),
            Action::OpenPlaylist(id, name) => self.open_playlist(id, name),
            Action::PlayPlaylist(id) => {
                let tracks = self.db.playlist_tracks(id);
                if tracks.is_empty() {
                    self.set_note("This playlist is empty".into());
                } else {
                    self.play(tracks);
                }
            }
            Action::PickPlaylist(tracks) => self.pick_playlist(tracks),
            Action::AddTo(id, name, tracks) => {
                let added = tracks.iter().filter(|t| self.db.add_to_playlist(id, t)).count();
                self.set_note(match added {
                    0 => format!("Already in {name}"),
                    1 => format!("Added to {name}"),
                    n => format!("Added {n} songs to {name}"),
                });
                self.playlists_changed();
            }
            Action::NewPlaylist(tracks) => self.start_naming(Naming::New(tracks), String::new()),
            Action::RenamePlaylist(id, name) => self.start_naming(Naming::Rename(id), name),
            Action::DeleteEq(name) => self.delete_eq(&name),
            Action::AskDelete(id, name) => {
                let items = vec![("Keep it".to_string(), Action::Nothing), ("Delete".to_string(), Action::DeletePlaylist(id))];
                self.menu = Some(Menu { title: format!("delete {name}?"), items, sel: 0 });
            }
            Action::DeletePlaylist(id) => {
                self.db.delete_playlist(id);
                self.playlists_changed();
                self.set_note("Playlist deleted".into());
            }
            Action::RemoveEntry(entry) => {
                self.db.remove_entry(entry);
                self.playlists_changed();
            }
            Action::Nothing => {}
            Action::ThisIs(button) => self.this_is(button),
            Action::Search(text) => {
                self.query = text.clone();
                self.run_search(text);
            }
        }
    }

    // What Enter does on a row that is not a plain track.
    pub(super) fn open_action(it: &Item) -> Option<Action> {
        Some(match it.kind {
            Kind::Album => Action::OpenAlbum(it.clone()),
            Kind::Artist => Action::OpenArtist(it.clone()),
            Kind::ArtistStat => Action::GoArtist(it.title.clone()),
            Kind::AlbumStat => Action::GoAlbum(it.clone()),
            Kind::Discover => Action::Discover,
            Kind::Mix => Action::PlayMix(it.spec.clone()?, it.title.replace("✦ ", "")),
            Kind::Resume => Action::Resume,
            Kind::Playlist => Action::OpenPlaylist(it.id.parse().ok()?, it.title.clone()),
            Kind::NewPlaylist => Action::NewPlaylist(vec![]),
            Kind::Query => Action::Search(it.title.clone()),
            Kind::Button if it.id == "thisis" => Action::ThisIs(it.clone()),
            Kind::Button => Action::PlayAll(it.id == "shuffle"),
            Kind::Track | Kind::Header | Kind::Setting => return None,
        })
    }

    pub(super) fn activate(&mut self) {
        let Some(it) = self.cur().cloned() else { return };
        if let Some(action) = App::open_action(&it) {
            self.run(action);
        } else if it.kind == Kind::Header {
        } else if it.kind == Kind::Setting {
            self.change_setting(1);
        } else if self.is_current(&it) {
            self.cmd(json!(["cycle", "pause"])); // never restarts what is already playing
        } else if self.tab == Tab::Queue || (self.tab == Tab::Home && it.uid != 0 && self.queue.iter().any(|t| t.uid == it.uid)) {
            self.run(Action::Jump(it.uid));
        } else {
            self.run(Action::PlayFrom(self.sel));
        }
    }
    // Enter (or Shift+Space): act on the highlighted row; with no row in sight, play or pause.
    pub(super) fn press(&mut self) {
        if !self.full && !self.lyrics && self.cur().is_some() {
            self.activate();
        } else {
            self.toggle_pause();
        }
    }
    pub(super) fn toggle_pause(&mut self) {
        if self.now().is_some() {
            self.cmd(json!(["cycle", "pause"]));
        } else {
            self.resume();
        }
    }
    pub(super) fn quit_keeping_music(&mut self) {
        self.detach = true;
        self.quit = true;
    }
    pub(super) fn escape(&mut self) {
        if self.menu.is_some() {
            self.menu = None;
        } else if self.full {
            self.full = false;
        } else if self.lyrics {
            self.lyrics = false;
        } else if self.tab == Tab::Search {
            if self.back.is_empty() {
                self.set_tab(self.prev_tab);
            } else {
                self.go_back();
            }
        } else if self.tab != Tab::Home {
            self.set_tab(Tab::Home);
        }
    }

    // Actions on the highlighted row (key m, or right-click on a row).
    pub(super) fn open_menu(&mut self) {
        let Some(it) = self.cur().cloned() else { return };
        if it.kind == Kind::Header {
            return;
        }
        let item = |label: &str, a: Action| (label.to_string(), a);
        let mut items = match App::open_action(&it) {
            Some(a) => {
                let label = match it.kind {
                    Kind::Album | Kind::AlbumStat => "Open album",
                    Kind::Artist | Kind::ArtistStat => "Open artist",
                    Kind::Discover => "Open Discover",
                    Kind::Mix => "Play mix",
                    Kind::Playlist => "Open playlist",
                    Kind::NewPlaylist => "New playlist",
                    Kind::Query => "Search again",
                    Kind::Button if it.id == "thisis" => "Build the list",
                    Kind::Button if it.id == "shuffle" => "Shuffle",
                    Kind::Button => "Play all",
                    _ => "Resume",
                };
                let mut v = vec![item(label, a)];
                if let (Kind::Playlist, Ok(id)) = (it.kind, it.id.parse()) {
                    v.push(item("Play", Action::PlayPlaylist(id)));
                    v.push(item("Rename", Action::RenamePlaylist(id, it.title.clone())));
                    v.push(item("Delete…", Action::AskDelete(id, it.title.clone())));
                }
                if it.kind == Kind::Button && it.id != "thisis" {
                    v.push(item("Save as playlist…", Action::NewPlaylist(self.tracks_from(0))));
                }
                v
            }
            None => {
                let queue = self.tab == Tab::Queue;
                let credit = if it.artist.is_empty() { &it.title } else { &it.artist };
                let mut v = vec![
                    if queue {
                        item("Jump to this track", Action::Jump(it.uid))
                    } else {
                        item(if self.in_search_results() { "Play, then similar songs" } else { "Play from here" }, Action::PlayFrom(self.sel))
                    },
                    item("Play next", Action::PlayNext(it.clone())),
                    item("Add to queue", Action::Enqueue(it.clone())),
                    item(if self.likes.contains(&it.id) { "Remove like" } else { "Like" }, Action::Rate(true, it.clone())),
                    item(if self.dislikes.contains(&it.id) { "Remove dislike" } else { "Dislike" }, Action::Rate(false, it.clone())),
                    item("Go to artist", Action::GoArtist(credit.clone())),
                    item("Add to playlist…", Action::PickPlaylist(vec![it.clone()])),
                ];
                if queue {
                    v.push(item("Remove from queue", Action::Remove(it.uid)));
                    v.push(item("Save queue as playlist…", Action::NewPlaylist(self.queue.clone())));
                }
                if self.viewed_playlist().is_some() {
                    v.push(item("Remove from playlist", Action::RemoveEntry(it.uid)));
                }
                v
            }
        };
        if matches!(it.kind, Kind::Track | Kind::Artist | Kind::ArtistStat | Kind::Album | Kind::AlbumStat) {
            items.push(item("Start radio", Action::Radio(it.clone())));
        }
        self.menu = Some(Menu { title: it.title, items, sel: 0 });
    }
    pub(super) fn run_menu(&mut self, i: usize) {
        if let Some(action) = self.menu.take().and_then(|m| m.items.get(i).map(|x| x.1.clone())) {
            self.run(action);
        }
    }
}
