// The radio: similar songs that keep the queue going when it runs out.
use super::*;

impl App {
    // Asks for songs like `seed` and puts them at the end of the queue.
    pub(super) fn fetch_similar(&mut self, seed: String) {
        let generation = self.generation;
        if self.filling == Some(generation) {
            return;
        }
        self.filling = Some(generation);
        let (yt, tx) = (self.yt.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let found = yt.radio(&seed);
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                if app.generation != generation {
                    return; // something else was played meanwhile
                }
                app.filling = None;
                match found {
                    Ok(found) => app.append_similar(found),
                    Err(e) => app.msg = format!("No similar songs: {e}"),
                }
            })));
        });
    }
    pub(super) fn append_similar(&mut self, found: Vec<Item>) {
        let unwanted: HashSet<String> = self.dislikes.union(&self.removed).cloned().collect();
        for mut t in fresh_songs(&self.queue, found, &unwanted).into_iter().take(25) {
            t.uid = self.id();
            self.cmd(json!(["loadfile", track_url(&t), "append-play"]));
            self.queue.push(t);
        }
        self.preload();
        if self.tab == Tab::Home {
            self.load_home();
            self.clamp_sel();
        }
    }
    // Tops the queue up when only a few songs are left.
    pub(super) fn keep_going(&mut self) {
        let Ok(pos) = usize::try_from(self.pos) else { return };
        if !self.endless || self.queue.len().saturating_sub(pos + 1) > 3 {
            return;
        }
        // follow where the radio has got to, else what is playing
        let seed = self.queue.iter().rev().find(|t| t.origin == Origin::Radio).or_else(|| self.queue.get(pos)).map(|t| t.id.clone());
        if let Some(seed) = seed {
            self.fetch_similar(seed);
        }
    }

    pub(super) fn remove_from_queue(&mut self, uid: u64) {
        let Some(idx) = self.queue.iter().position(|t| t.uid == uid) else { return };
        let gone = self.queue.remove(idx);
        self.removed.insert(gone.id);
        if (idx as i64) < self.pos {
            self.pos -= 1;
        }
        self.cmd(json!(["playlist-remove", idx]));
        self.sel = self.sel.min(self.queue.len().saturating_sub(1));
        self.preload();
        self.keep_going();
    }
    pub(super) fn move_in_queue(&mut self, uid: u64, dir: i64) {
        let Some(idx) = self.queue.iter().position(|t| t.uid == uid) else { return };
        let j = idx as i64 + dir;
        if j < 0 || j >= self.queue.len() as i64 {
            return;
        }
        let now = self.now().map(|t| t.uid);
        self.queue.swap(idx, j as usize);
        // mpv moves the entry to *before* the target index
        let target = if dir > 0 { idx as i64 + 2 } else { j };
        self.cmd(json!(["playlist-move", idx, target]));
        if let Some(p) = now.and_then(|u| self.queue.iter().position(|t| t.uid == u)) {
            self.pos = p as i64;
        }
        self.sel = j as usize;
    }
}
