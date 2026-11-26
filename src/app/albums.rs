// Liking a whole album: every song of it is liked, one by one, so Liked (grouped by album or not), your hooks and the
// recommendations all see ordinary likes. An album counts as liked while its songs are.
use crate::i18n::tr;
use super::*;

impl App {
    // The album the highlight stands for: an album row, or the Play and Shuffle buttons of an album's page.
    pub(super) fn album_at(&self) -> Option<Item> {
        if self.full || self.lyrics {
            return None;
        }
        let it = self.cur()?;
        match it.kind {
            Kind::Album => Some(it.clone()),
            Kind::Button if self.tab == Tab::Search => self.head.clone().filter(|al| al.kind == Kind::Album && !al.id.is_empty()),
            _ => None,
        }
    }
    // l on an album: likes its songs, or asks to take the likes back when there is nothing left to like.
    pub(super) fn like_album(&mut self, album: Item) {
        if self.tab == Tab::Search && self.head.as_ref().is_some_and(|al| al.id == album.id) {
            let songs = self.items.iter().filter(|t| t.kind == Kind::Track).cloned().collect();
            return self.like_songs(album, songs);
        }
        let yt = self.yt.clone();
        self.run_busy(tr!("Loading album…").into(), move || {
            let songs = yt.album(&album)?;
            Ok(Box::new(move |app: &mut App| app.like_songs(album, songs)) as Apply)
        });
    }
    // A song you disliked stays disliked.
    fn like_songs(&mut self, album: Item, songs: Vec<Item>) {
        let title = album.title.clone();
        let mut seen = self.likes.clone();
        let disliked = songs.iter().filter(|t| self.dislikes.contains(&t.id)).count();
        let new: Vec<&Item> = songs.iter().filter(|t| !self.dislikes.contains(&t.id) && seen.insert(t.id.clone())).collect();
        if new.is_empty() {
            self.db.save_album(&album); // liked song by song before
            let liked: Vec<Item> = songs.into_iter().filter(|t| self.likes.contains(&t.id)).collect();
            if liked.is_empty() {
                return self.set_note(tr!("Nothing to like in {title}", title = title));
            }
            let items = vec![(tr!("Keep them").to_string(), Action::Nothing), (tr!("Remove {a} likes", a = liked.len()), Action::UnlikeAlbum(title.clone(), liked))];
            self.menu = Some(Menu { title: tr!("{title} is liked", title = title), items, sel: 0 });
            return;
        }
        self.db.save_album(&album); // it is in your Library too
        // last song first: Liked shows the newest first, so the album reads in its own order
        for t in new.iter().rev() {
            self.db.toggle_like(t);
            self.hook("liked", t, json!({}));
            self.download_like(t);
        }
        self.likes_changed();
        let left = if disliked == 0 { String::new() } else { tr!(" · {disliked} disliked left out", disliked = disliked) };
        let what = if new.len() == 1 { tr!("song") } else { tr!("songs") };
        self.set_note(tr!("{a} Liked {title}: {b} {what}{left}", a = glyphs().like, title = title, b = new.len(), what = what, left = left));
    }
    pub(super) fn unlike_songs(&mut self, title: &str, songs: Vec<Item>) {
        self.remember(Undo::Likes(songs.clone()));
        for t in songs {
            if self.likes.remove(&t.id) {
                self.db.toggle_like(&t);
                self.hook("unliked", &t, json!({}));
            }
        }
        self.likes_changed();
        self.set_note(tr!("Removed the likes of {title}", title = title));
    }
    pub(super) fn likes_changed(&mut self) {
        self.likes = self.db.liked_ids();
        match self.tab {
            Tab::Liked => self.load_liked(),
            Tab::Home => self.load_home(),
            Tab::Stats => self.load_stats(),
            _ => {}
        }
        self.clamp_sel();
    }
    // ← → on the Liked tab: your likes by song (newest first) or grouped by album.
    pub(super) fn switch_liked_view(&mut self) {
        self.liked_by_album = !self.liked_by_album;
        self.db.set_setting("liked_view", if self.liked_by_album { "albums" } else { "songs" });
        self.load_liked();
        self.sel = 0;
        self.clamp_sel();
    }
}

// The songs under a heading per album, albums in the order of their newest like; songs with no album go last. Two
// albums with one title are told apart by the first credited artist.
pub(super) fn by_album(songs: Vec<Item>) -> Vec<Item> {
    let mut groups: Vec<((String, String), Vec<Item>)> = vec![];
    for t in songs {
        let artist = if t.album.is_empty() { "" } else { t.artist.split(", ").next().unwrap_or("") };
        let key = (t.album.to_lowercase(), artist.to_lowercase());
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(t),
            None => groups.push((key, vec![t])),
        }
    }
    groups.sort_by_key(|g| g.0 .0.is_empty()); // stable: the rest keep their order
    groups
        .into_iter()
        .flat_map(|(_, songs)| {
            let first = &songs[0];
            let name = if first.album.is_empty() {
                tr!("No album").to_string()
            } else {
                format!("{} · {}", first.album, first.artist.split(", ").next().unwrap_or(""))
            };
            let what = if songs.len() == 1 { tr!("song") } else { tr!("songs") };
            std::iter::once(Item::header(&format!("{name} · {} {what}", songs.len()))).chain(songs)
        })
        .collect()
}
