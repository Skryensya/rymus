// Your playlists: making, filling, renaming, deleting and importing them.
use crate::i18n::tr;
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
        let title = if tracks.len() == 1 { first.title.clone() } else { tr!("{a} songs", a = tracks.len()) };
        let mut items: Vec<(String, Action)> = self
            .db
            .playlists()
            .into_iter()
            .filter_map(|p| Some((p.title.clone(), Action::AddTo(p.id.parse().ok()?, p.title, tracks.clone()))))
            .collect();
        items.push((tr!("+ New playlist…").into(), Action::NewPlaylist(tracks)));
        self.menu = Some(Menu { title: tr!("add to playlist · {title}", title = title), items, sel: 0 });
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
            Naming::Filter => {} // applied as it is typed
            Naming::FreeCut => return self.free_cut(&name),
            Naming::FreeAll(ids) => return self.free_all(ids, &name),
            Naming::Folder => {
                self.db.set_setting("dlfolder", &name);
                self.settings = config::rows(&self.db);
                self.scan_library();
                self.set_note(tr!("Downloads go to {name} from now on", name = name));
            }
            Naming::New(tracks) => match self.db.create_playlist(&name, None, &tracks) {
                Some(id) => {
                    if let [t] = tracks.as_slice() {
                        self.hook("playlist-add", t, json!({"playlist": name}));
                    }
                    self.set_note(tr!("Created {name}", name = name));
                    self.playlists_changed();
                    self.point_at_playlist(id); // leave the cursor on what was just made
                    return;
                }
                None => self.msg = tr!("Could not save the playlist").into(),
            },
            Naming::EqPreset => return self.new_eq(name),
            Naming::Export => return self.export_data(&name),
            Naming::Import => return self.import_data(&name),
            Naming::Wipe => return self.wipe_data(&name),
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
            self.set_note(tr!("Already imported: {name}", name = name));
            return self.show_saved(id, name);
        }
        if self.importing {
            return self.set_note(tr!("Wait for the current import to finish").into());
        }
        self.importing = true;
        self.busy = tr!("Importing playlist…").into();
        self.msg.clear();
        let (yt, tx) = (self.yt.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let progress = |done: usize, total: usize| {
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| app.busy = tr!("Importing playlist… {done}/{total}", done = done, total = total))));
            };
            let result = playlist::fetch(&yt, &link, &progress);
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                app.importing = false;
                app.busy.clear();
                match result {
                    Ok(got) => app.imported(got, &source),
                    Err(e) => app.msg = tr!("Import failed: {e}", e = e),
                }
            })));
        });
    }
    pub(super) fn imported(&mut self, got: playlist::Imported, source: &str) {
        let name = if got.name.is_empty() { tr!("Imported playlist").to_string() } else { got.name };
        let Some(id) = self.db.create_playlist(&name, Some(source), &got.tracks) else {
            self.msg = tr!("Could not save the playlist").into();
            return;
        };
        self.playlists_changed();
        if !self.typing {
            self.show_saved(id, name); // never pull someone out of what they are typing
        }
        let mut note = tr!("Imported {a} songs", a = got.tracks.len());
        if got.missing > 0 {
            note += &tr!(" · {a} not found", a = got.missing);
        }
        if got.partial {
            note += &tr!(" · only the first {a}", a = got.tracks.len() + got.missing);
        }
        self.set_note(note);
        self.note_until = Some(Instant::now() + Duration::from_secs(10));
    }
}
