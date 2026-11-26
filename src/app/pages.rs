// Everything that waits on the network (search, albums, artists, mixes) runs on its own thread and comes back as a page.
use crate::i18n::tr;
use super::*;

impl App {
    // Runs on its own thread with the spinner on; errors land in the red message.
    pub(super) fn run_busy(&mut self, label: String, work: impl FnOnce() -> Result<Apply, String> + Send + 'static) {
        self.busy = label.clone();
        self.msg.clear();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = work();
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                match result {
                    Ok(apply) => apply(app),
                    Err(e) => app.msg = tr!("{a} failed: {e}", a = label.replace('…', ""), e = e),
                }
                app.busy.clear();
            })));
        });
    }

    // Search results replace the body of whatever tab you were on; Esc (or any tab) brings it back.
    pub(super) fn open_results(&mut self) {
        self.show_lists(); // results that arrive while something else is open replace it
        if self.tab == Tab::Search {
            return;
        }
        if self.tab == Tab::Home {
            self.remember_home_sel();
        }
        if self.tab == Tab::Playlists {
            self.playlists_sel = self.sel;
        }
        self.prev_tab = self.tab;
        self.tab = Tab::Search;
        self.menu = None;
        self.lyrics = false;
    }
    pub(super) fn reopen_results(&mut self) {
        if self.items.is_empty() || self.tab == Tab::Search {
            return;
        }
        self.open_results();
        self.sel = self.search_sel.min(self.items.len() - 1);
    }
    pub(super) fn set_list(&mut self, title: String, items: Vec<Item>, numbered: bool, sel: usize) {
        self.title = title;
        self.items = items;
        self.numbered = numbered;
        self.sel = sel;
        self.head = None;
        self.list_id = self.id();
    }
    pub(super) fn push_back(&mut self) {
        self.back.push(Saved { items: std::mem::take(&mut self.items), title: std::mem::take(&mut self.title), numbered: self.numbered, sel: self.sel, list_id: self.list_id, head: self.head.take() });
    }
    // Show a list on top of the current tab; from inside the results, Esc returns to where you were.
    pub(super) fn enter_list(&mut self, title: String, items: Vec<Item>, numbered: bool, sel: usize) {
        if self.tab == Tab::Search {
            self.push_back();
        } else {
            self.open_results();
            self.back.clear();
        }
        self.set_list(title, items, numbered, sel);
    }
    pub(super) fn go_back(&mut self) {
        if let Some(s) = self.back.pop() {
            self.items = s.items;
            self.title = s.title;
            self.numbered = s.numbered;
            self.sel = s.sel;
            self.list_id = s.list_id;
            self.head = s.head;
            // more may have arrived while you were away
            if let Some(f) = self.found.as_ref().filter(|f| f.list_id == s.list_id) {
                self.items = f.lists[f.tab].clone();
            }
        }
    }

    pub(super) fn album_page(yt: &Yt, album: Item) -> Result<Apply, String> {
        let tracks = yt.album(&album)?;
        Ok(Box::new(move |app: &mut App| {
            let button = |id: &str, title: &str| Item { kind: Kind::Button, id: id.into(), title: title.into(), cover: album.cover.clone(), ..Default::default() };
            let mut rows = vec![button("play", tr!("▶ Play album")), button("shuffle", tr!("⤮ Shuffle")), Item::header(tr!("Songs"))];
            rows.extend(tracks);
            app.enter_list(format!("{} · {}", album.title, album.author), rows, true, 0);
            app.head = Some(album);
        }))
    }
    pub(super) fn open_album(&mut self, album: Item) {
        let yt = self.yt.clone();
        self.run_busy(tr!("Loading album…").into(), move || App::album_page(&yt, album));
    }
    // An album known only by name and artist (Stats).
    pub(super) fn go_album(&mut self, it: Item) {
        let yt = self.yt.clone();
        self.run_busy(tr!("Loading album…").into(), move || {
            let found = yt.albums(&format!("{} {}", it.title, it.artist))?;
            let wanted = it.title.to_lowercase();
            let a = found.iter().find(|a| a.title.to_lowercase() == wanted).or(found.first()).cloned();
            match a {
                Some(mut a) => {
                    if a.author.is_empty() {
                        a.author = it.artist.clone();
                    }
                    App::album_page(&yt, a)
                }
                None => Ok(Box::new(move |app: &mut App| app.msg = tr!("No album found: {a}", a = it.title)) as Apply),
            }
        });
    }
    // The artist's page: "This is …", popular songs, albums; their picture on the side.
    pub(super) fn artist_page(yt: &Yt, id: &str, name: String) -> Result<Apply, String> {
        let page = yt.artist(id, &name)?;
        let artist_id = id.to_string();
        Ok(Box::new(move |app: &mut App| {
            if page.songs.is_empty() && page.albums.is_empty() {
                app.msg = tr!("Nothing found for this artist").into();
                return;
            }
            let mut rows = vec![Item {
                kind: Kind::Button,
                id: "thisis".into(),
                title: tr!("✦ This is {name}", name = name),
                author: name.clone(),
                album: page.all_songs.clone(),
                cover: page.picture.clone(),
                ..Default::default()
            }];
            rows.extend(grouped(vec![(tr!("Popular songs"), page.songs), (tr!("Albums"), page.albums)]));
            app.enter_list(name.clone(), rows, false, 0);
            app.head = Some(Item { kind: Kind::Artist, id: artist_id, title: name, cover: page.picture, ..Default::default() });
        }))
    }
    // The artist's essential songs, put together when asked for: their most played, one version of each.
    pub(super) fn this_is(&mut self, button: Item) {
        let yt = self.yt.clone();
        self.run_busy(tr!("Building This is {a}…", a = button.author), move || {
            let found = if button.album.is_empty() { yt.songs(&button.author)? } else { yt.artist_songs(&button.album)? };
            let tracks = essentials(found, &button.author);
            Ok(Box::new(move |app: &mut App| {
                if tracks.is_empty() {
                    app.msg = tr!("No songs found for {a}", a = button.author);
                    return;
                }
                let title = tr!("This is {a}", a = button.author);
                let play = |id: &str, label: &str| Item { kind: Kind::Button, id: id.into(), title: label.into(), cover: button.cover.clone(), ..Default::default() };
                let mut rows = vec![play("play", tr!("▶ Play")), play("shuffle", tr!("⤮ Shuffle")), Item::header(tr!("Songs"))];
                rows.extend(tracks);
                app.enter_list(title.clone(), rows, true, 0);
                app.head = Some(Item { kind: Kind::Album, title, author: button.author, cover: button.cover, ..Default::default() });
            }) as Apply)
        });
    }
    pub(super) fn open_artist(&mut self, artist: Item) {
        let yt = self.yt.clone();
        self.run_busy(tr!("Loading artist…").into(), move || App::artist_page(&yt, &artist.id, artist.title));
    }
    // Open an artist known only by name (Home, and "Go to artist" on a song).
    pub(super) fn go_artist(&mut self, credit: &str) {
        let name = credit.split(", ").next().unwrap_or("").to_string();
        self.typing = false;
        let yt = self.yt.clone();
        self.run_busy(tr!("Loading artist…").into(), move || {
            let found = yt.artists(&name)?;
            let wanted = name.to_lowercase();
            match found.iter().find(|a| a.title.to_lowercase() == wanted).or(found.first()) {
                Some(a) => App::artist_page(&yt, &a.id, a.title.clone()),
                None => Ok(Box::new(move |app: &mut App| app.msg = tr!("No artist found: {name}", name = name)) as Apply),
            }
        });
    }

    // The Discover view: one mix of everything at once, and a mix per style as soon as they are worked out.
    pub(super) fn open_discover(&mut self) {
        if self.db.heard().is_empty() {
            self.msg = tr!("Play or like a few songs first").into();
            return;
        }
        self.open_results();
        self.back.clear();
        let all = mix_row("all", tr!("✦ Everything new").into(), tr!("from all your taste").into(), Spec::All);
        self.set_list(tr!("Discover · only songs you have not heard").into(), vec![all, Item::header(tr!("Reading your styles…"))], false, 0);
        let (list, path, tx) = (self.list_id, self.db_path.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let rows = match mixes::find_styles(&path) {
                Ok(found) if found.is_empty() => vec![Item::header(tr!("No styles yet: listen to more artists"))],
                Ok(found) => std::iter::once(Item::header(tr!("By style")))
                    .chain(found.into_iter().map(|s| {
                        let who = s.artists.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                        mix_row(&s.name, format!("✦ {}", s.name), who, Spec::Style(s.artists))
                    }))
                    .collect(),
                Err(e) => vec![Item::header(&tr!("Styles unavailable: {e}", e = e))],
            };
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                // the view may be open, or waiting behind Esc
                let items = if app.list_id == list { Some(&mut app.items) } else { app.back.iter_mut().find(|s| s.list_id == list).map(|s| &mut s.items) };
                if let Some(items) = items {
                    items.truncate(1);
                    items.extend(rows);
                }
            })));
        });
    }

    pub(super) fn play_mix(&mut self, spec: Spec, label: String) {
        if !self.busy.is_empty() {
            return;
        }
        let (yt, path) = (self.yt.clone(), self.db_path.clone());
        self.run_busy(tr!("Building {label}…", label = label), move || {
            let mix = mixes::build_mix(&yt, &path, &spec)?;
            Ok(Box::new(move |app: &mut App| {
                if mix.tracks.is_empty() {
                    app.msg = tr!("Nothing new found, try another mix").into();
                    return;
                }
                let fresh = mix.tracks.iter().filter(|t| t.fresh).count();
                let without = if mix.without_listenbrainz { tr!(" · without ListenBrainz") } else { "" };
                app.play(mix.tracks);
                app.set_tab(Tab::Queue);
                app.set_note(tr!("{label}: {fresh} new songs{without}", label = label, fresh = fresh, without = without));
            }) as Apply)
        });
    }
    // A mix of new songs around whatever a row is (or the playing song).
    pub(super) fn start_radio(&mut self, it: Option<Item>) {
        use Kind::*;
        let it = it
            .filter(|t| matches!(t.kind, Track | Artist | ArtistStat | Album | AlbumStat))
            .or_else(|| self.now().cloned().map(|t| Item { kind: Track, ..t }));
        let Some(it) = it else { return self.set_note(tr!("Nothing to start a radio from").into()) };
        if it.kind == Track {
            let label = tr!("Radio · {a}", a = it.title);
            let seed = Item { id: it.id, title: it.title, artist: it.artist, album: it.album, cover: it.cover, ..Default::default() };
            return self.play_mix(Spec::RadioTrack(Box::new(seed)), label);
        }
        let artist = match it.kind {
            Album => it.author,
            AlbumStat => it.artist,
            _ => it.title,
        };
        let label = tr!("Radio · {artist}", artist = artist);
        self.play_mix(Spec::RadioArtist(artist), label);
    }
}
