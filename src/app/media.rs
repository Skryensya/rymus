// Lyrics (LRCLIB) and cover images, fetched in the background. Covers are kept for the session, lyrics on disk too
// (db::lyrics_*), so a song you play again has them at once.
use super::*;

impl App {
    // Where playback is right now, finer than the whole seconds of `time`.
    pub(super) fn position(&self) -> f64 {
        self.apos.map_or(0.0, |(p, at)| p + if self.paused || self.loading { 0.0 } else { at.elapsed().as_secs_f64().min(0.3) })
    }
    // Scrolling the words stops them following the song for a moment (karaoke always follows).
    pub(super) fn scroll_lyrics(&mut self, step: i64) {
        if self.karaoke {
            return;
        }
        self.lyric_scroll = (self.lyric_scroll as i64 + step).max(0) as usize;
        self.lyric_scrolled = Some(Instant::now());
    }
    pub fn lyrics_follow(&self) -> bool {
        self.lyric_scrolled.is_none_or(|t| t.elapsed() > Duration::from_secs(4))
    }
    // How many of karaoke's countdown dots are filled, on the way to the line after the one being sung (or to the
    // first one); None on the last line, when nothing comes next.
    pub fn dots_at(&self, lines: &[(f64, String)]) -> Option<usize> {
        let current = self.line_at(lines);
        let start = current.map_or(0.0, |c| lines[c].0);
        let next = lines.get(current.map_or(0, |c| c + 1))?.0;
        Some(lyrics::dots_filled(self.position() + 0.15, start, next))
    }
    pub fn line_at(&self, lines: &[(f64, String)]) -> Option<usize> {
        let pos = self.position() + 0.15; // a hair early reads as on time
        lines.iter().rposition(|l| l.0 <= pos)
    }
    // Makes sure the lyrics of the playing track are on their way; true once they are here.
    pub fn want_lyrics(&mut self) -> Option<String> {
        let t = self.now()?.clone();
        if self.dur <= 0.0 {
            return None;
        }
        if !self.lyric_cache.contains_key(&t.id) {
            if let Some(kept) = self.db.lyrics_get(&t.id).and_then(|s| lyrics::from_json(&s)).filter(|l| !stale(l, &t.id, &self.db)) {
                self.lyric_cache.insert(t.id.clone(), Some(kept));
                return Some(t.id);
            }
            self.lyric_cache.insert(t.id.clone(), None);
            let (tx, dur, id) = (self.tx.clone(), self.dur, t.id.clone());
            std::thread::spawn(move || {
                let result = lyrics::fetch(&t, dur);
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    if let Some(json) = lyrics::to_json(&result) {
                        app.db.lyrics_set(&id, &json);
                    }
                    app.lyric_cache.insert(id, Some(result));
                })));
            });
            return None;
        }
        let id = t.id;
        self.lyric_cache.get(&id).is_some_and(Option::is_some).then_some(id)
    }
    pub fn lyrics_ready(&self) -> Option<&Lyrics> {
        self.lyric_cache.get(&self.now()?.id)?.as_ref()
    }
    // y: the words, to read and scroll · Y: karaoke. The other key switches between them; the same key closes.
    pub(super) fn toggle_lyrics(&mut self) {
        self.open_lyrics(false);
    }
    pub(super) fn toggle_karaoke(&mut self) {
        self.open_lyrics(true);
    }
    pub(super) fn open_lyrics(&mut self, karaoke: bool) {
        self.lyrics = !(self.lyrics && self.karaoke == karaoke);
        self.karaoke = karaoke;
        self.lyric_scrolled = None;
        self.full = false;
        self.lyric_scroll = 0;
        if self.lyrics {
            self.lyric_cache.retain(|_, v| *v != Some(Lyrics::Error)); // a failed request is worth another try
        }
    }
    pub fn cover(&mut self, url: &str) -> Option<Arc<Got>> {
        if let Some(hit) = self.covers.get(url) {
            return hit.clone();
        }
        if cover::is_large(url) {
            self.covers.retain(|k, _| !cover::is_large(k)); // each is megabytes: keep only the one on screen
        }
        self.covers.insert(url.to_string(), None);
        let (url, tx) = (url.to_string(), self.tx.clone());
        std::thread::spawn(move || {
            if let Some(got) = cover::load(&url) {
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    // a big one that arrives after the song changed is not wanted any more
                    if !cover::is_large(&url) || app.covers.contains_key(&url) {
                        app.covers.insert(url, Some(Arc::new(got)));
                    }
                })));
            }
        });
        None
    }
    // Half-block rows of a loaded cover, computed off the main thread.
    pub fn cells(&mut self, url: &str, got: &Arc<Got>, w: usize, h: usize) -> Option<Vec<String>> {
        let key = (url.to_string(), w, h);
        if let Some(hit) = self.rasters.get(&key) {
            return hit.clone();
        }
        self.rasters.insert(key.clone(), None);
        let (got, tx) = (got.clone(), self.tx.clone());
        std::thread::spawn(move || {
            if let Some(rows) = cover::cells(&got.image, w, h) {
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    app.rasters.insert(key, Some(rows));
                })));
            }
        });
        None
    }
}

// "No lyrics" is asked again after a month: LRCLIB keeps getting new ones. Lyrics found never go stale.
fn stale(l: &Lyrics, id: &str, db: &crate::db::Db) -> bool {
    *l == Lyrics::None && db.lyrics_age(id).is_some_and(|age| age > std::time::Duration::from_secs(30 * 86_400))
}
