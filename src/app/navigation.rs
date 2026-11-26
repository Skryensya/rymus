// Moving around: tabs, going back, the action behind each row, and the m menu.
use crate::i18n::tr;
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
        self.eq.open = false;
        self.downloads.open = false;
        self.filter = None;
        self.menu = None;
        self.lyrics = false;
        match tab {
            Tab::Liked => self.load_liked(),
            Tab::History => self.load_history(),
            Tab::Library => {
                self.load_library();
                self.check_releases(false);
            }
            Tab::Home => self.load_home(),
            Tab::Stats => self.load_stats(),
            Tab::Playlists => self.load_playlists(),
            Tab::Config => self.settings = config::rows(&self.db),
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
    // Runs your hook for an event, if there is one; a failure is told in the pane title.
    pub(super) fn hook(&self, event: &'static str, t: &Item, extra: Value) {
        let tx = self.tx.clone();
        let data = self.db_path.parent().map(PathBuf::from).unwrap_or_default();
        crate::hooks::run(&data, event, t, extra, move |problem| {
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| app.msg = problem)));
        });
    }

    pub(super) fn rate(&mut self, like: bool, t: Option<Item>) {
        let t = t.or_else(|| {
            if !self.full && !self.lyrics && self.kind_at() == Kind::Track { self.cur().cloned() } else { self.now().cloned() }
        });
        let verb = if like { tr!("like") } else { tr!("dislike") };
        let Some(t) = t else { return self.set_note(tr!("Nothing to {verb}", verb = verb)) };
        let was_liked = self.likes.contains(&t.id);
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
            (false, _) => tr!("Removed {verb}", verb = verb),
            (true, true) => tr!("{a} Liked", a = glyphs().like),
            (true, false) => tr!("{a} Disliked", a = glyphs().dislike),
        };
        self.set_note(format!("{what}: {}", t.title));
        if was_liked && !self.likes.contains(&t.id) {
            self.hook("unliked", &t, json!({})); // by removing the like, or by disliking
        }
        if on {
            self.hook(if like { "liked" } else { "disliked" }, &t, json!({}));
        }
        if on && like {
            self.download_like(&t);
        }
        // disliking what is playing moves on to the next song
        if on && !like && self.now().is_some_and(|n| n.id == t.id) && self.pos < self.queue.len() as i64 - 1 {
            self.cmd(json!(["playlist-next"]));
        }
    }

    pub(super) fn open_playing(&mut self, artist: bool) {
        let Some(t) = self.now().cloned() else { return self.set_note(tr!("Nothing playing").into()) };
        if artist {
            self.run(Action::GoArtist(t.artist));
        } else if t.album.is_empty() {
            self.set_note(tr!("No album known for this song").into());
        } else {
            let artist = t.artist.split(", ").next().unwrap_or("").to_string();
            self.run(Action::GoAlbum(Item { kind: Kind::AlbumStat, title: t.album, artist, ..Default::default() }));
        }
    }

    pub(super) fn run(&mut self, action: Action) {
        match action {
            Action::OpenAlbum(a) => {
                self.saw_release(&a.id);
                self.open_album(a);
            }
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
                self.set_note(tr!("Next: {a}", a = t.title));
                self.queue_track(t, true);
            }
            Action::Enqueue(t) => {
                self.set_note(tr!("Queued: {a}", a = t.title));
                self.queue_track(t, false);
            }
            Action::Rate(like, t) => self.rate(like, Some(t)),
            Action::LikeAlbum(a) => self.like_album(a),
            Action::SearchTab(i) => self.results_tab(i),
            Action::UnlikeAlbum(title, songs) => self.unlike_songs(&title, songs),
            Action::Remove(uid) => self.remove_from_queue(uid),
            Action::Radio(t) => self.start_radio(Some(t)),
            Action::OpenPlaylist(id, name) => self.open_playlist(id, name),
            Action::PlayPlaylist(id) => {
                let tracks = self.db.playlist_tracks(id);
                if tracks.is_empty() {
                    self.set_note(tr!("This playlist is empty").into());
                } else {
                    self.play(tracks);
                }
            }
            Action::PickPlaylist(tracks) => self.pick_playlist(tracks),
            Action::AddTo(id, name, tracks) => {
                let added = tracks.iter().filter(|t| self.db.add_to_playlist(id, t)).count();
                // one song you put there, not a whole queue at once: hundreds of hooks would start together
                if let ([t], 1) = (tracks.as_slice(), added) {
                    self.hook("playlist-add", t, json!({"playlist": name}));
                }
                self.set_note(match added {
                    0 => tr!("Already in {name}", name = name),
                    1 => tr!("Added to {name}", name = name),
                    n => tr!("Added {n} songs to {name}", n = n, name = name),
                });
                self.playlists_changed();
            }
            Action::NewPlaylist(tracks) => self.start_naming(Naming::New(tracks), String::new()),
            Action::RenamePlaylist(id, name) => self.start_naming(Naming::Rename(id), name),
            Action::DeleteEq(name) => self.delete_eq(&name),
            Action::ResetEq(name) => self.reset_eq(&name),
            Action::OpenEq => self.open_eq(),
            Action::Sleep(sleep) => self.set_sleep(sleep),
            Action::ClearQueue(all) => self.clear_queue(all),
            Action::Press(key) => self.press_key(key),
            Action::Choose(key, v) => self.pick_setting(&key, v),
            Action::GoTab(tab) => self.set_tab(tab),
            Action::Download(tracks) => self.download(tracks, true),
            Action::AskDeleteDownload(t) => self.ask_delete_download(t),
            Action::DeleteDownload(id) => self.delete_download(&id),
            Action::DownloadLiked => self.download_liked(),
            Action::OpenDownloads => self.open_downloads(),
            Action::PickDownloads(i) => self.downloads.sel = i,
            Action::FreeSpace(ids) => self.free_space(ids),
            Action::ConfirmFreeSpace(ids) => self.start_naming(Naming::FreeAll(ids), String::new()),
            Action::DownloadFolder => {
                let now = self.db.get_setting("dlfolder").filter(|f| !f.is_empty()).unwrap_or_else(|| "~/Music/rymus".into());
                self.start_naming(Naming::Folder, now);
            }
            forget @ (Action::Unfollow(_) | Action::DropAlbum(_) | Action::DropRelease(_)) => self.forget(forget),
            Action::PickEq(i) => self.pick_eq(i),
            Action::AskDelete(id, name) => {
                let items = vec![(tr!("Keep it").to_string(), Action::Nothing), (tr!("Delete").to_string(), Action::DeletePlaylist(id))];
                self.menu = Some(Menu { title: tr!("delete {name}?", name = name), items, sel: 0 });
            }
            Action::DeletePlaylist(id) => {
                if let Some(p) = self.db.playlists().into_iter().find(|p| p.id == id.to_string()) {
                    let source = self.db.playlist_source(id);
                    self.remember(Undo::Playlist { name: p.title, source, songs: self.db.playlist_tracks(id) });
                }
                self.db.delete_playlist(id);
                self.playlists_changed();
                self.set_note(tr!("Playlist deleted").into());
            }
            Action::RemoveEntry(entry) => {
                let song = self.items.iter().find(|t| t.uid == entry).cloned();
                let name = self.viewed_playlist().and_then(|id| self.db.playlists().into_iter().find(|p| p.id == id.to_string()));
                self.db.remove_entry(entry);
                if let (Some(t), Some(id)) = (song.clone(), self.viewed_playlist()) {
                    self.remember(Undo::Entry { playlist: id, song: t });
                }
                if let (Some(t), Some(p)) = (song, name) {
                    self.hook("playlist-remove", &t, json!({"playlist": p.title}));
                }
                self.playlists_changed();
            }
            Action::Nothing => {}
            Action::Data(what) => self.start_data(&what),
            Action::ThisIs(button) => self.this_is(button),
            Action::Search(text) => {
                self.query = text.clone();
                self.run_search(text);
            }
        }
    }

    // What Enter does on a row that is not a plain track.
    pub(crate) fn open_action(it: &Item) -> Option<Action> {
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
            Kind::Button if it.id == "eq" => Action::OpenEq,
            Kind::Button if it.id == "dlfolder" => Action::DownloadFolder,
            Kind::Button if it.id == "dlall" => Action::DownloadLiked,
            Kind::Button if it.id == "downloads" => Action::OpenDownloads,
            Kind::Button if matches!(it.id.as_str(), "export" | "import" | "wipe") => Action::Data(it.id.clone()),
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
        } else if self.filtered().is_some() && !self.full && !self.lyrics {
            self.drop_filter();
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
                    Kind::Album | Kind::AlbumStat => tr!("Open album"),
                    Kind::Artist | Kind::ArtistStat => tr!("Open artist"),
                    Kind::Discover => tr!("Open Discover"),
                    Kind::Mix => tr!("Play mix"),
                    Kind::Playlist => tr!("Open playlist"),
                    Kind::NewPlaylist => tr!("New playlist"),
                    Kind::Query => tr!("Search again"),
                    Kind::Button if it.id == "thisis" => tr!("Build the list"),
                    Kind::Button if it.id == "shuffle" => tr!("Shuffle"),
                    Kind::Button => tr!("Play all"),
                    _ => tr!("Resume"),
                };
                let mut v = vec![item(label, a)];
                if let (Kind::Playlist, Ok(id)) = (it.kind, it.id.parse()) {
                    v.push(item(tr!("Play"), Action::PlayPlaylist(id)));
                    v.push(item(tr!("Rename"), Action::RenamePlaylist(id, it.title.clone())));
                    v.push(item(tr!("Delete…"), Action::AskDelete(id, it.title.clone())));
                }
                if it.kind == Kind::Album {
                    v.push(item(tr!("Like the whole album"), Action::LikeAlbum(it.clone())));
                }
                if it.kind == Kind::Button && it.id != "thisis" {
                    v.push(item(tr!("Save as playlist…"), Action::NewPlaylist(self.tracks_from(0))));
                }
                v
            }
            None => {
                let queue = self.tab == Tab::Queue;
                let credit = if it.artist.is_empty() { &it.title } else { &it.artist };
                let mut v = vec![
                    if queue {
                        item(tr!("Jump to this track"), Action::Jump(it.uid))
                    } else {
                        item(if self.in_search_results() { tr!("Play, then similar songs") } else { tr!("Play from here") }, Action::PlayFrom(self.sel))
                    },
                    item(tr!("Play next"), Action::PlayNext(it.clone())),
                    item(tr!("Add to queue"), Action::Enqueue(it.clone())),
                    item(if self.likes.contains(&it.id) { tr!("Remove like") } else { tr!("Like") }, Action::Rate(true, it.clone())),
                    item(if self.dislikes.contains(&it.id) { tr!("Remove dislike") } else { tr!("Dislike") }, Action::Rate(false, it.clone())),
                    item(tr!("Go to artist"), Action::GoArtist(credit.clone())),
                    item(tr!("Add to playlist…"), Action::PickPlaylist(vec![it.clone()])),
                ];
                if !it.album.is_empty() {
                    v.push(item(tr!("Go to album"), Action::GoAlbum(Item { kind: Kind::AlbumStat, title: it.album.clone(), artist: credit.clone(), ..Default::default() })));
                }
                if it.id.starts_with(crate::disk::OWN) {
                    // your own file: nothing to download, and never deleted from here
                } else if self.downloaded(&it.id) {
                    v.push(item(tr!("Delete download"), Action::AskDeleteDownload(it.clone())));
                } else {
                    v.push(item(tr!("Download"), Action::Download(vec![it.clone()])));
                }
                if queue {
                    v.push(item(tr!("Remove from queue"), Action::Remove(it.uid)));
                    v.push(item(tr!("Save queue as playlist…"), Action::NewPlaylist(self.queue.clone())));
                }
                if self.viewed_playlist().is_some() {
                    v.push(item(tr!("Remove from playlist"), Action::RemoveEntry(it.uid)));
                }
                v
            }
        };
        if matches!(it.kind, Kind::Track | Kind::Artist | Kind::ArtistStat | Kind::Album | Kind::AlbumStat) {
            items.push(item(tr!("Start radio"), Action::Radio(it.clone())));
        }
        self.menu = Some(Menu { title: it.title, items, sel: 0 });
    }
    pub(super) fn run_menu(&mut self, i: usize) {
        self.palette = None;
        if let Some(action) = self.menu.take().and_then(|m| m.items.get(i).map(|x| x.1.clone())) {
            self.run(action);
        }
    }
}
