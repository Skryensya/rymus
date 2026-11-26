// Your playlists: making, filling, renaming, deleting and importing them.
use super::*;

impl App {
    // The playlist the visible list shows, if it shows one.
    pub fn viewed_playlist(&self) -> Option<i64> {
        self.playlist_views.get(&self.list_id).copied().filter(|_| self.tab == Tab::Search)
    }
    pub(super) fn open_playlist(&mut self, id: i64, name: String) {
        let tracks = self.db.playlist_tracks(id);
        self.enter_list(name, tracks, true, 0);
        self.playlist_views.insert(self.list_id, id);
    }
    // Anything that changed a playlist shows up wherever it is on screen.
    pub(super) fn playlists_changed(&mut self) {
        if let Some(id) = self.viewed_playlist() {
            self.items = self.db.playlist_tracks(id);
        }
        if self.tab == Tab::Home {
            self.remember_home_sel();
        }
        self.load_home();
        self.load_playlists();
        self.clamp_sel();
    }
    pub(super) fn pick_playlist(&mut self, tracks: Vec<Item>) {
        let Some(first) = tracks.first() else { return };
        let title = if tracks.len() == 1 { first.title.clone() } else { format!("{} songs", tracks.len()) };
        let mut items: Vec<(String, Action)> = self
            .db
            .playlists()
            .into_iter()
            .filter_map(|p| Some((p.title.clone(), Action::AddTo(p.id.parse().ok()?, p.title, tracks.clone()))))
            .collect();
        items.push(("+ New playlist…".into(), Action::NewPlaylist(tracks)));
        self.menu = Some(Menu { title: format!("add to playlist · {title}"), items, sel: 0 });
    }
    pub(super) fn start_naming(&mut self, naming: Naming, current: String) {
        self.start_typing();
        self.buf = current;
        self.naming = Some(naming);
    }
    pub(super) fn finish_naming(&mut self, naming: Naming, name: String) {
        if name.is_empty() {
            return;
        }
        match naming {
            Naming::New(tracks) => match self.db.create_playlist(&name, None, &tracks) {
                Some(id) => {
                    if let [t] = tracks.as_slice() {
                        self.hook("playlist-add", t, json!({"playlist": name}));
                    }
                    self.set_note(format!("Created {name}"));
                    self.playlists_changed();
                    self.point_at_playlist(id); // leave the cursor on what was just made
                    return;
                }
                None => self.msg = "Could not save the playlist".into(),
            },
            Naming::EqPreset => return self.new_eq(name),
            Naming::Rename(id) => {
                self.db.rename_playlist(id, &name);
                if self.viewed_playlist() == Some(id) {
                    self.title = name;
                }
            }
        }
        self.playlists_changed();
    }
    // Opens a playlist from the Playlists tab, so Esc lands on the list with it highlighted.
    pub(super) fn show_saved(&mut self, id: i64, name: String) {
        self.set_tab(Tab::Playlists);
        self.point_at_playlist(id);
        self.open_playlist(id, name);
    }
    // A pasted link becomes a playlist; pasting it again opens the one already made, keeping your changes to it.
    pub(super) fn import(&mut self, link: Link) {
        let source = link.url();
        if let Some((id, name)) = self.db.playlist_from(&source) {
            self.set_note(format!("Already imported: {name}"));
            return self.show_saved(id, name);
        }
        if self.importing {
            return self.set_note("Wait for the current import to finish".into());
        }
        self.importing = true;
        self.busy = "Importing playlist…".into();
        self.msg.clear();
        let (yt, tx) = (self.yt.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let progress = |done: usize, total: usize| {
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| app.busy = format!("Importing playlist… {done}/{total}"))));
            };
            let result = playlist::fetch(&yt, &link, &progress);
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                app.importing = false;
                app.busy.clear();
                match result {
                    Ok(got) => app.imported(got, &source),
                    Err(e) => app.msg = format!("Import failed: {e}"),
                }
            })));
        });
    }
    pub(super) fn imported(&mut self, got: playlist::Imported, source: &str) {
        let name = if got.name.is_empty() { "Imported playlist".to_string() } else { got.name };
        let Some(id) = self.db.create_playlist(&name, Some(source), &got.tracks) else {
            self.msg = "Could not save the playlist".into();
            return;
        };
        self.playlists_changed();
        if !self.typing {
            self.show_saved(id, name); // never pull someone out of what they are typing
        }
        let mut note = format!("Imported {} songs", got.tracks.len());
        if got.missing > 0 {
            note += &format!(" · {} not found", got.missing);
        }
        if got.partial {
            note += &format!(" · only the first {}", got.tracks.len() + got.missing);
        }
        self.set_note(note);
        self.note_until = Some(Instant::now() + Duration::from_secs(10));
    }
}
