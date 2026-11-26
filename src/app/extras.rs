// The sleep timer, emptying the queue, and taking back what was just removed.
use crate::i18n::tr;
use super::*;

#[derive(Clone, Debug)]
pub enum Sleep {
    At(Instant),
    Song,          // when this song ends
    Album(String), // when the songs move on from this album
}

// What can be taken back (u), newest last.
pub enum Undo {
    Queue(Vec<(usize, Item)>),                            // entries and where they were
    Playlist { name: String, source: Option<String>, songs: Vec<Item> }, // a deleted one
    Entry { playlist: i64, song: Item },                  // a song taken out of a playlist
    Likes(Vec<Item>),                                     // an album's likes
}

impl App {
    pub(super) fn sleep_menu(&mut self) {
        let mut items: Vec<(String, Action)> = [15, 30, 45, 60, 90].iter().map(|&m| (tr!("Stop in {m} minutes", m = m), Action::Sleep(Some(Sleep::At(Instant::now() + Duration::from_secs(m * 60)))))).collect();
        items.push((tr!("Stop when this song ends").into(), Action::Sleep(Some(Sleep::Song))));
        if let Some(album) = self.now().map(|t| t.album.clone()).filter(|a| !a.is_empty()) {
            items.push((tr!("Stop when {album} ends", album = album), Action::Sleep(Some(Sleep::Album(album)))));
        }
        if self.sleep.is_some() {
            items.insert(0, (tr!("Turn the timer off").into(), Action::Sleep(None)));
        }
        self.menu = Some(Menu { title: tr!("sleep timer").into(), items, sel: 0 });
    }
    pub(super) fn set_sleep(&mut self, sleep: Option<Sleep>) {
        self.set_note(if sleep.is_some() { tr!("Sleep timer on").into() } else { tr!("Sleep timer off").to_string() });
        self.sleep = sleep;
    }
    // What the player's title says while a timer runs.
    pub fn sleep_label(&self) -> Option<String> {
        Some(match self.sleep.as_ref()? {
            Sleep::At(at) => tr!("sleep in {a} min", a = at.saturating_duration_since(Instant::now()).as_secs().div_ceil(60).max(1)),
            Sleep::Song => tr!("sleep after this song").into(),
            Sleep::Album(_) => tr!("sleep after this album").into(),
        })
    }
    fn go_to_sleep(&mut self) {
        self.sleep = None;
        self.cmd(json!(["set_property", "pause", true]));
        self.set_note(tr!("Sleep timer: paused").into());
    }
    // True when the time was up.
    pub(super) fn sleep_tick(&mut self) -> bool {
        let up = matches!(self.sleep, Some(Sleep::At(at)) if Instant::now() >= at);
        if up {
            self.go_to_sleep();
        }
        up
    }
    // A song has just started: was the one before the last to hear?
    pub(super) fn sleep_on_song(&mut self) {
        let over = match &self.sleep {
            Some(Sleep::Song) => true,
            Some(Sleep::Album(album)) => self.now().is_some_and(|t| &t.album != album),
            _ => false,
        };
        if over {
            self.go_to_sleep();
        }
    }

    pub(super) fn clear_menu(&mut self) {
        let items = vec![
            (tr!("Keep everything").to_string(), Action::Nothing),
            (tr!("Remove the radio's songs").to_string(), Action::ClearQueue(false)),
            (tr!("Keep only what plays").to_string(), Action::ClearQueue(true)),
        ];
        self.menu = Some(Menu { title: tr!("clear the queue").into(), items, sel: 0 });
    }
    pub(super) fn clear_queue(&mut self, all: bool) {
        let current = self.now().map(|t| t.uid);
        let mut gone = vec![];
        // from the end, so the positions before stay right
        for i in (0..self.queue.len()).rev() {
            let t = &self.queue[i];
            if Some(t.uid) == current || (!all && (t.origin != Origin::Radio || (i as i64) < self.pos)) {
                continue;
            }
            gone.push((i, self.queue.remove(i)));
            if (i as i64) < self.pos {
                self.pos -= 1;
            }
            self.cmd(json!(["playlist-remove", i]));
        }
        if gone.is_empty() {
            return self.set_note(tr!("Nothing to remove").into());
        }
        self.endless = false; // or the radio would fill it right back
        gone.reverse();
        self.set_note(tr!("Removed {a} songs · u takes it back", a = gone.len()));
        self.remember(Undo::Queue(gone));
        self.sel = 0;
        self.clamp_sel();
    }

    pub(super) fn remember(&mut self, what: Undo) {
        self.undo.push(what);
        if self.undo.len() > 20 {
            self.undo.remove(0);
        }
    }
    pub(super) fn undo(&mut self) {
        let Some(what) = self.undo.pop() else { return self.set_note(tr!("Nothing to undo").into()) };
        match what {
            Undo::Queue(entries) => {
                let n = entries.len();
                for (i, t) in entries {
                    let at = i.min(self.queue.len());
                    let url = self.source(&t);
                    self.removed.remove(&t.id);
                    if at < self.queue.len() {
                        self.cmd(json!(["loadfile", url, "insert-at", at]));
                        if (at as i64) <= self.pos {
                            self.pos += 1;
                        }
                    } else {
                        self.cmd(json!(["loadfile", url, "append"]));
                    }
                    self.queue.insert(at, t);
                }
                self.preload();
                self.set_note(if n == 1 { tr!("Back in the queue").into() } else { tr!("{n} songs back in the queue", n = n) });
            }
            Undo::Playlist { name, source, songs } => {
                self.db.create_playlist(&name, source.as_deref(), &songs);
                self.playlists_changed();
                self.set_note(tr!("{name} is back", name = name));
            }
            Undo::Entry { playlist, song } => {
                self.db.add_to_playlist(playlist, &song);
                self.playlists_changed();
                self.set_note(tr!("{a} is back in the playlist", a = song.title));
            }
            Undo::Likes(songs) => {
                for t in songs.iter().rev().filter(|t| !self.likes.contains(&t.id)) {
                    self.db.toggle_like(t);
                    self.hook("liked", t, json!({}));
                }
                self.likes_changed();
                self.set_note(tr!("{a} likes are back", a = songs.len()));
            }
        }
        self.clamp_sel();
    }

    // For `rymus ctl status`: what is playing, where another process can read it.
    pub(super) fn tell_now(&self) {
        let now = self.now().map(|t| json!({"id": t.id, "title": t.title, "artist": t.artist, "album": t.album}));
        self.db.set_setting("now", &now.map_or(String::new(), |v| v.to_string()));
    }
}
