// Talking to mpv: its events, the settings it applies, what was heard (for Stats), and the queue it plays.
use super::*;

impl App {
    pub(super) fn cmd(&mut self, command: Value) {
        let filters = command[0] == "af";
        self.mpv.cmd(command);
        if filters {
            self.sync_eq(); // in case mpv rebuilt the equalizer flat
        }
    }
    // The measuring filter stays in whether the wave shows or not: putting a filter in or taking it out stops the
    // song for a moment. Adding it again when it is there already changes nothing. Off only hides the drawing.
    // Off → the whole queue → this song → off.
    pub(super) fn cycle_repeat(&mut self) {
        let (next, all, one, note) = match self.repeat {
            Repeat::Off => (Repeat::All, "inf", "no", "Repeat: the whole queue"),
            Repeat::All => (Repeat::One, "no", "inf", "Repeat: this song"),
            Repeat::One => (Repeat::Off, "no", "no", "Repeat off"),
        };
        self.repeat = next;
        self.cmd(json!(["set_property", "loop-playlist", all]));
        self.cmd(json!(["set_property", "loop-file", one]));
        self.set_note(note.into());
    }
    pub(super) fn apply_wave(&mut self) {
        self.cmd(json!(["af", "add", viz::filter(&self.mpv.viz)]));
        if !self.wave {
            self.bands = [0.0; BANDS];
        }
    }
    pub(super) fn apply_setting(&mut self, key: &str, v: &str) {
        match key {
            "wave" => {
                self.wave = v == "on";
                self.apply_wave();
            }
            "fade" => {
                self.fade = config::fade_secs(v);
                self.apply_fade();
            }
            "vizstyle" => self.viz_style = config::choices("vizstyle").iter().position(|c| *c == v).unwrap_or(0),
            "accent" => self.accent = config::accent(v),
            "player" => self.player_top = v == "top",
            "border" => crate::ui::set_border(config::choices("border").iter().position(|c| *c == v).unwrap_or(0)),
            "pixel" => self.kitty = cover::supported() && v == "off", // the half-block drawing is used wherever kitty is false
            "normalize" => self.cmd(if v == "on" { json!(["af", "add", config::NORMALIZE]) } else { json!(["af", "remove", "@norm"]) }),
            "quality" => {
                self.format = config::format(v);
                self.cmd(json!(["set_property", "ytdl-format", self.format])); // tracks yt-dlp has not resolved yet
            }
            _ => {}
        }
    }
    pub(super) fn apply_fade(&mut self) {
        self.cmd(json!(["af", "add", config::FADE])); // the same one again changes nothing
        if self.fade == 0.0 {
            self.stop_tails();
            self.unfade();
        }
    }
    fn unfade(&mut self) {
        if std::mem::take(&mut self.faded) {
            self.cmd(json!(["af-command", "fade", "volume", "1", "volume@fade"]));
        }
    }
    pub(super) fn stop_tails(&mut self) {
        for mut t in [self.tail.take(), self.fading.take()].into_iter().flatten() {
            t.stop();
        }
    }
    // Crossfade: 20 s ahead (yt-dlp needs a while) a second mpv waits paused on this song's last seconds; there it
    // plays them fading out while this one moves on to the next song, fading in.
    fn crossfade(&mut self, p: f64) {
        let Some(at) = config::fade_at(self.fade, self.dur) else { return };
        let next = usize::try_from(self.pos).is_ok_and(|pos| pos + 1 < self.queue.len());
        if !next || self.crossing || p < at - 20.0 {
            return;
        }
        if p < at {
            if self.tail.is_none() {
                // the stream yt-dlp already found: no wait, and only the last seconds are fetched again
                let Some(url) = self.now().map(|t| self.streams.get(&t.uid).cloned().unwrap_or_else(|| track_url(t))) else { return };
                // the same sound as the main player: its equalizer and loudness, set once
                let mut filters = format!("lavfi=[{}],", crate::eq::graph(&self.eq_current().1));
                if config::value(&self.db, "normalize") == "on" {
                    filters.push_str("lavfi=[dynaudnorm=f=500],");
                }
                self.tail = Mpv::tail(&url, self.format, at, self.fade, self.vol, &filters).ok();
            }
            return;
        }
        let Some(mut tail) = self.tail.take() else { return };
        // a seek past the start would replay what was just heard; a player that never came up has nothing to play
        if p > at + 1.0 || !tail.try_connect() {
            return tail.stop();
        }
        tail.cmd(json!(["set_property", "pause", false]));
        if let Some(mut old) = self.fading.replace(tail) {
            old.stop();
        }
        self.cmd(json!(["af-command", "fade", "volume", config::fade_in(self.fade), "volume@fade"]));
        self.faded = true;
        self.crossing = true;
        self.cmd(json!(["playlist-next"]));
    }
    pub(super) fn change_setting(&mut self, dir: i64) {
        let Some(it) = self.cur().filter(|i| i.kind == Kind::Setting).cloned() else { return };
        let v = config::step(&self.db, &it.id, dir);
        self.apply_setting(&it.id, v);
        self.settings = config::rows(&self.db);
        self.set_note(format!("{}: {v}", it.title));
    }
    pub(super) fn save_heard(&self, skipped: bool) {
        if let Some(id) = self.play_id {
            let duration = (self.play_dur > 0.0).then(|| self.play_dur.round() as i64);
            self.db.end_play(id, self.heard.round() as i64, duration, skipped);
        }
    }
    // A skip: you moved on (mpv reports "stop") before 30 s of a song that is longer than that.
    pub(super) fn log_end(&mut self, reason: &str) {
        self.save_heard(reason == "stop" && self.heard < 30.0 && !(self.play_dur > 0.0 && self.play_dur < 40.0));
        self.play_id = None;
    }

    // The song that was sounding is over: "played" if it reached its end, "skipped" if you moved on before.
    fn song_over(&mut self, reason: &str) {
        let event = match reason {
            "eof" => "played",
            "stop" if self.crossing => "played", // the crossfade moves on while the last seconds still sound
            "stop" => "skipped",
            _ => return, // quitting, or it could not be played
        };
        if let Some(t) = self.sounding.take() {
            let extra = json!({"seconds": self.heard.round() as i64, "duration": self.play_dur.round() as i64});
            self.hook(event, &t, extra);
        }
    }

    // False when nothing on screen changed.
    pub(super) fn on_mpv(&mut self, e: &Value) -> bool {
        match e["event"].as_str().unwrap_or("") {
            "property-change" => {
                let v = &e["data"];
                match e["name"].as_str().unwrap_or("") {
                    "audio-pts" => {
                        self.apos = v.as_f64().map(|p| (p, Instant::now())); // the position being heard
                        return false;
                    }
                    "time-pos" => {
                        // seconds actually heard: small steps forward count, seeks (big jumps) do not
                        if let (Some(raw), Some(p)) = (self.raw_pos, v.as_f64()) {
                            let step = p - raw;
                            if step > 0.0 && step < 2.0 {
                                let before = (self.heard / 5.0).floor();
                                self.heard += step;
                                if (self.heard / 5.0).floor() != before {
                                    self.save_heard(false); // every 5 s, so stats are live
                                }
                            }
                        }
                        self.raw_pos = v.as_f64();
                        if let Some(p) = self.raw_pos {
                            self.crossfade(p);
                        }
                        let t = v.as_f64().unwrap_or(0.0).floor() as i64;
                        if t == self.time {
                            return false;
                        }
                        self.time = t;
                    }
                    "duration" => {
                        self.dur = v.as_f64().unwrap_or(0.0);
                        if self.dur > 0.0 && self.dur != self.play_dur {
                            self.play_dur = self.dur;
                        }
                    }
                    "pause" => {
                        self.paused = v.as_bool().unwrap_or(false);
                        if let Some(f) = &mut self.fading {
                            f.cmd(json!(["set_property", "pause", self.paused])); // the fade-out pauses with you
                        }
                    }
                    // mpv says "inf", a count, or false
                    "loop-file" | "loop-playlist" => {
                        let on = !matches!(v, Value::Bool(false) | Value::Null) && v != "no";
                        let one = e["name"] == "loop-file";
                        self.repeat = match (one, on, self.repeat) {
                            (true, true, _) => Repeat::One,
                            (false, true, Repeat::Off) => Repeat::All,
                            (true, false, Repeat::One) => Repeat::Off,
                            (false, false, Repeat::All) => Repeat::Off,
                            (_, _, r) => r,
                        };
                    }
                    "volume" => {
                        if let Some(v) = v.as_f64() {
                            self.vol = v.round() as i64;
                            self.db.set_setting("volume", &self.vol.to_string());
                            for t in [&mut self.tail, &mut self.fading].into_iter().flatten() {
                                if t.try_connect() {
                                    t.cmd(json!(["set_property", "volume", v])); // the crossfade's end follows you
                                }
                            }
                        }
                    }
                    "playlist-pos" => {
                        self.pos = v.as_i64().unwrap_or(-1);
                        self.time = 0;
                        self.dur = 0.0;
                        // after a restart the song was already loaded: it is the one being counted
                        if self.sounding.is_none() && self.play_id.is_some() {
                            self.sounding = self.now().cloned();
                        }
                    }
                    _ => return false,
                }
            }
            "start-file" => {
                // a song you chose comes in at once, and what was fading out stops
                if !std::mem::take(&mut self.crossing) {
                    self.stop_tails();
                    self.unfade();
                }
                self.loading = true;
                self.msg.clear();
                self.play_dur = 0.0;
            }
            "file-loaded" => {
                self.loading = false;
                self.heard = 0.0;
                self.raw_pos = None;
                self.sounding = self.now().cloned();
                if let Some(t) = &self.sounding {
                    self.play_id = Some(self.db.start_play(t, now_ms()));
                    self.hook("playing", t, json!({}));
                }
                self.sync_eq();
                self.preload();
                self.keep_going();
                if self.tab == Tab::Home {
                    self.load_home();
                    self.clamp_sel();
                }
            }
            "end-file" => {
                self.loading = false;
                let reason = e["reason"].as_str().unwrap_or("");
                self.song_over(reason);
                self.log_end(reason);
                if reason == "error" {
                    self.msg = match e["file_error"].as_str() {
                        Some("loading failed") => "Could not load this track (yt-dlp failed). It may be restricted or unavailable.".into(),
                        other => format!("Playback error: {}", other.unwrap_or("unknown")),
                    };
                }
                if reason != "redirect" && reason != "error" && self.pos >= self.queue.len() as i64 - 1 {
                    self.pos = -1;
                }
            }
            _ => return false,
        }
        true
    }

    // Once something plays, the last session's row leaves Home.
    pub(super) fn drop_resume_row(&mut self) {
        self.session = None;
        for (column, pane) in [PaneId::Left, PaneId::Right].into_iter().enumerate() {
            let list = if column == 0 { &mut self.home_left } else { &mut self.home_right };
            let Some(at) = list.iter().position(|t| t.kind == Kind::Resume) else { continue };
            list.remove(at);
            let sel = if self.tab == Tab::Home && self.home_focus == pane { &mut self.sel } else { &mut self.home_sel[column] };
            if *sel > at {
                *sel -= 1;
            }
        }
        if self.tab == Tab::Home {
            self.clamp_sel();
        }
    }

    pub(super) fn set_queue(&mut self, mut tracks: Vec<Item>) {
        for t in &mut tracks {
            t.uid = self.id();
        }
        self.generation += 1;
        self.removed.clear();
        self.endless = false;
        self.queue = tracks;
        self.direct.clear();
        self.streams.clear();
        self.pos = 0;
        self.loading = true;
        self.paused = false;
    }
    pub(super) fn play(&mut self, tracks: Vec<Item>) {
        // songs you added and have not heard yet: from the queue, or from the last run if nothing has played in this one
        let waiting: Vec<Item> = match (&self.session, self.queue.is_empty(), usize::try_from(self.pos)) {
            (Some(s), true, _) => s.queue.iter().skip(1).cloned().collect(),
            (_, _, Ok(pos)) => self.queue.iter().skip(pos + 1).cloned().collect(),
            _ => vec![],
        };
        let tracks = with_added(tracks.into_iter().map(|t| Item { origin: Origin::List, ..t }).collect(), &waiting);
        self.drop_resume_row();
        self.set_queue(tracks);
        let urls: Vec<String> = self.queue.iter().map(track_url).collect();
        for (i, url) in urls.iter().enumerate() {
            self.cmd(json!(["loadfile", url, if i > 0 { "append" } else { "replace" }]));
        }
        self.cmd(json!(["set_property", "pause", false]));
    }
    // Continue the previous run: from the saved track and second onward.
    pub(super) fn resume(&mut self) {
        let Some(s) = self.session.take() else { return };
        self.drop_resume_row();
        self.set_queue(s.queue);
        self.endless = s.endless;
        let urls: Vec<String> = self.queue.iter().map(track_url).collect();
        for (i, url) in urls.iter().enumerate() {
            let command = if i > 0 { json!(["loadfile", url, "append"]) } else { json!(["loadfile", url, "replace", -1, format!("start={}", s.time)]) };
            self.cmd(command);
        }
        self.cmd(json!(["set_property", "pause", false]));
    }

    pub(super) fn save_session(&self) {
        if self.queue.is_empty() {
            return; // never played this run: keep whatever was saved
        }
        let value = if self.pos >= 0 {
            let queue: Vec<Value> = self
                .queue
                .iter()
                .skip(self.pos as usize)
                .take(500)
                .map(item_json)
                .collect();
            json!({"queue": queue, "pos": 0, "time": self.time, "endless": self.endless}).to_string()
        } else {
            String::new() // finished playing: nothing to resume
        };
        self.db.set_setting("session", &value);
    }

    pub fn shutdown(&mut self) {
        self.stop_tails();
        self.save_session();
        if self.detach && self.pos >= 0 {
            return self.leave_playing();
        }
        self.log_end("quit");
        self.mpv.stop();
    }
    // `Q`: the player goes on by itself, and the next run picks it up.
    pub(super) fn leave_playing(&mut self) {
        self.save_heard(false);
        let queue: Vec<Value> = self.queue.iter().map(item_json).collect();
        // a restart is back within a second, so the song being counted stays counted
        let counting = self.play_id.filter(|_| self.restart).map(|id| json!({"id": id, "heard": self.heard, "duration": self.play_dur}));
        let state = json!({"sock": self.mpv.path, "viz": self.mpv.viz, "queue": queue, "endless": self.endless, "counting": counting});
        self.db.set_setting("detached", &state.to_string());
        // the filters stay as they are: changing them is heard. The fade-in is undone: alone, every song would come in slowly
        self.unfade();
        if !self.mpv.keep_measurements_read() {
            self.cmd(json!(["af", "remove", "@viz"])); // nobody would read it, and the audio would stall
        }
        self.cmd(json!(["set_property", "idle", "no"])); // ends by itself when the queue does
        std::thread::sleep(Duration::from_millis(200)); // let it read all that before the socket closes
    }
    // Take over from where `Q` left: the queue is mpv's playlist entry by entry; the rest it reports itself.
    pub fn reattach(&mut self, left: Detached) {
        let mut queue = left.queue;
        for t in &mut queue {
            t.uid = self.id();
        }
        self.queue = queue;
        self.endless = left.endless;
        self.session = None;
        if let Some((id, heard, duration)) = left.counting {
            (self.play_id, self.heard, self.play_dur) = (Some(id), heard, duration);
        }
        self.cmd(json!(["set_property", "idle", "yes"]));
        self.cmd(json!(["script-message", crate::platform::BACK])); // we read the measurements again
    }

    // Resolve the stream URL of every upcoming track with yt-dlp (two at a time) and swap it into mpv's playlist, so
    // mpv can prefetch it and jumping to a queued track starts in under a second.
    pub(super) fn preload(&mut self) {
        if cfg!(test) {
            return; // tests never run yt-dlp
        }
        let mut i = (self.pos + 1).max(0) as usize;
        // only the next few: a playlist can hold hundreds of songs, and the addresses expire after some hours
        let end = self.queue.len().min((self.pos + 1).max(0) as usize + 5);
        while i < end && self.resolving.len() < 2 {
            let t = &self.queue[i];
            if !self.direct.contains(&t.id) && !self.resolving.contains(&t.id) {
                self.resolving.insert(t.id.clone());
                let (id, uid, url, tx, format) = (t.id.clone(), t.uid, track_url(t), self.tx.clone(), self.format);
                std::thread::spawn(move || {
                    let out = std::process::Command::new("yt-dlp")
                        .args(["-g", "-f", format, "--no-playlist", &url])
                        .stdin(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output();
                    let url = out.ok().filter(|o| o.status.success()).and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.trim().to_string()));
                    let _ = tx.send(Msg::Resolved { id, uid, url });
                });
            }
            i += 1;
        }
    }
    pub(super) fn resolved(&mut self, id: String, uid: u64, url: Option<String>) {
        self.resolving.remove(&id);
        match url.filter(|u| !u.is_empty()) {
            Some(url) => {
                // the queue may have changed while yt-dlp ran
                if let Some(idx) = self.queue.iter().position(|t| t.uid == uid).filter(|&i| i as i64 > self.pos) {
                    self.direct.insert(id);
                    self.streams.insert(uid, url.clone());
                    self.cmd(json!(["playlist-remove", idx]));
                    self.cmd(json!(["loadfile", url, "insert-at", idx]));
                }
            }
            None => {
                self.direct.insert(id); // do not retry a failing track forever
            }
        }
        self.preload();
    }

    // ---------- queue editing (queue mirrors mpv's playlist, entry by entry) ----------
    pub(super) fn queue_track(&mut self, mut t: Item, next: bool) {
        t.uid = self.id();
        t.origin = Origin::Added;
        let url = track_url(&t);
        let at = add_position(&self.queue, self.pos, next);
        if at < self.queue.len() {
            self.queue.insert(at, t);
            self.cmd(json!(["loadfile", url, "insert-at", at]));
        } else {
            self.queue.push(t);
            self.cmd(json!(["loadfile", url, "append-play"])); // plays right away if nothing is playing
        }
        self.preload();
    }
}
