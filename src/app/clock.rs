// The 40 ms tick: what moves on screen without any input (the wave, the progress bar, synced lyrics, notes that
// expire), and whether the screen needs drawing again.
use super::*;

impl App {
    // Called every 40 ms; true when the screen has to be drawn again.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut need = !self.busy.is_empty() || self.loading;
        let playing = self.pos >= 0 && !self.paused && !self.loading;
        if self.note_until.is_some_and(|t| now >= t) {
            self.note_until = None;
            self.note.clear();
            need = true;
        }
        need |= self.theme.step();
        if self.redraw_at.is_some_and(|t| now >= t) {
            self.redraw_at = None;
            need = true;
        }
        if self.wave {
            // move the bars at a steady pace; they fall to the baseline when nothing plays
            let db = self.apos.filter(|_| playing).and_then(|(p, at)| {
                // where playback will be when this frame reaches the screen
                let pos = p + at.elapsed().as_secs_f64().min(0.3) + 0.03;
                self.feed.lock().unwrap_or_else(|e| e.into_inner()).at(pos)
            });
            let loud = db.map_or(0.0, |db| viz::energy(&db));
            self.energy += (loud - self.energy) * 0.15; // smoothed over about a quarter second
            let next = match db {
                Some(db) => self.levels.next(&db, &self.bands),
                None => self.bands.map(|v| if v > 0.02 { v * 0.74 } else { 0.0 }),
            };
            need |= next != self.bands; // no redraw once everything is still
            self.bands = next;
        }
        if self.full && self.trip_on {
            // its clock is playback time: it stops with the music
            if playing {
                self.trip_t += now.duration_since(self.trip_at).as_secs_f64().min(0.1);
                need = true;
            }
            self.trip_at = now;
        }
        if self.mq.on && crate::render::marquee_step(&self.mq) != self.mq.step {
            need = true;
        }
        if self.tab == Tab::Stats && self.pos >= 0 && !self.paused && now.duration_since(self.stats_at) > Duration::from_secs(2) {
            self.stats_at = now;
            self.load_stats();
            need = true;
        }
        if self.lyrics {
            match self.want_lyrics() {
                None => need |= self.now().is_some(),
                Some(_) => {
                    if let Some(Lyrics::Synced(lines)) = self.lyrics_ready() {
                        need |= self.line_at(lines) != self.lyric_line;
                        need |= self.karaoke && self.dots_at(lines) != self.lyric_dots;
                    }
                    // 4 s after the last scroll, back to the line being sung
                    if self.lyric_scrolled.is_some() && self.lyrics_follow() {
                        self.lyric_scrolled = None;
                        need = true;
                    }
                }
            }
        }
        need
    }
}
