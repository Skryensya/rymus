// Everything that arrives goes through `handle`: keys, mouse, mpv events, resizes and the answers of background
// threads. Then the keys (on_key → menu, typing or command keys) and the mouse (on_mouse), by what is on screen.
use crate::i18n::tr;
use super::*;

impl App {
    // True when the screen has to be drawn again.
    pub fn handle(&mut self, m: Msg) -> bool {
        let redraw = self.handle_one(m);
        self.refilter(); // the list may have changed under it
        redraw
    }
    fn handle_one(&mut self, m: Msg) -> bool {
        match m {
            // Shift and friends on their own are reported too: they are not keys to act on
            Msg::Key(k) if k.kind != KeyEventKind::Release && !matches!(k.code, KeyCode::Modifier(_)) => self.on_key(k),
            Msg::Key(_) => return false,
            Msg::Mouse(e) => return self.on_mouse(e),
            Msg::Resize => {
                self.clear_screen = true;
                self.shown.clear();
                self.shown_head.clear();
            }
            Msg::Mpv(e) => return self.on_mpv(&e),
            Msg::Quit => self.quit = true,
            Msg::OutputChanged => return self.output_changed(),
            Msg::Resolved { id, uid, url } => {
                self.resolved(id, uid, url);
                return false;
            }
            Msg::Apply(f) => f(self),
        }
        true
    }

    // Pauses at once, so nothing blares out of the speakers; resuming is up to you.
    pub(super) fn output_changed(&mut self) -> bool {
        if config::value(&self.db, "outputpause") != "on" || self.pos < 0 || self.paused {
            return false;
        }
        self.cmd(json!(["set_property", "pause", true])); // not "cycle": a second change must not start it again
        self.set_note(tr!("Paused: the audio output changed · space resumes").into());
        true
    }

    pub(super) fn seek_at(&mut self, x: usize) {
        if self.dur <= 0.0 || self.geo.bar_w == 0 {
            return;
        }
        let pct = ((x as f64 - self.geo.bar_x0 as f64 - 0.5) / self.geo.bar_w as f64 * 100.0).clamp(0.0, 100.0);
        self.cmd(json!(["seek", pct.round(), "absolute-percent"]));
    }
    // (pane index, list index) of the row under the pointer.
    pub(super) fn row_at(&self, x: usize, y: usize) -> Option<(usize, usize)> {
        let (p, pane) = self.geo.panes.iter().enumerate().find(|(_, p)| x >= p.x0 && x <= p.x1)?;
        let top = self.geo.top;
        if pane.id == PaneId::Cover || y < top + pane.head || y >= top + self.geo.body {
            return None;
        }
        let index = match &pane.rows {
            Some(rows) => rows.get(y - top).copied(),
            None => usize::try_from(pane.top + (y - top) as i64).ok(),
        };
        index.map(|i| (p, i))
    }
    pub(super) fn selectable(&self, i: usize) -> bool {
        self.data().get(i).is_some_and(|t| t.kind != Kind::Header)
    }
    pub(super) fn body_click(&mut self, x: usize, y: usize) {
        let Some((pane, i)) = self.row_at(x, y) else { return };
        if self.tab == Tab::Home {
            self.focus_home(self.geo.panes[pane].id);
        }
        if !self.selectable(i) {
            return;
        }
        let double = self.last_click.is_some_and(|(p, j, at)| p == pane && j == i && at.elapsed() < Duration::from_millis(450));
        self.last_click = Some((pane, i, Instant::now()));
        self.sel = i;
        if self.tab == Tab::Config {
            // the menu is the first pane when it is shown: a click there opens that section, one on a setting goes in
            self.config_menu = self.geo.panes.len() > 1 && pane == 0;
            if self.config_menu {
                return;
            }
        }
        if double {
            self.activate();
        }
    }
    // Config's two levels: ↑ ↓ go through the sections and → or ⏎ goes into one; inside, ↑ ↓ stay in it and Esc
    // goes back out. True when the key was taken.
    fn config_keys(&mut self, code: KeyCode) -> bool {
        use KeyCode as K;
        let data = &self.settings;
        let header = |i: i64| i < 0 || data.get(i as usize).is_none_or(|t| t.kind == Kind::Header);
        if self.config_menu {
            let heads: Vec<usize> = data.iter().enumerate().filter(|(_, t)| t.kind == Kind::Header).map(|(i, _)| i).collect();
            let cur = heads.iter().rposition(|&h| h <= self.sel).unwrap_or(0) as i64;
            match code {
                K::Up | K::Char('k') | K::Down | K::Char('j') => {
                    let dir = if matches!(code, K::Up | K::Char('k')) { -1 } else { 1 };
                    let to = (cur + dir).clamp(0, heads.len() as i64 - 1) as usize;
                    self.sel = heads.get(to).map_or(self.sel, |h| h + 1);
                }
                K::Right | K::Char('l') | K::Enter => self.config_menu = false,
                _ => return false,
            }
            return true;
        }
        match code {
            K::Up | K::Char('k') | K::Down | K::Char('j') => {
                let to = self.sel as i64 + if matches!(code, K::Up | K::Char('k')) { -1 } else { 1 };
                if !header(to) {
                    self.sel = to as usize;
                }
            }
            K::Esc | K::Backspace | K::Char('h') if self.menu.is_none() => self.config_menu = true,
            _ => return false,
        }
        true
    }

    pub(super) fn on_mouse(&mut self, e: MouseEvent) -> bool {
        use MouseEventKind as M;
        let (x, y) = (e.column as usize + 1, e.row as usize + 1);
        let wheel: i64 = match e.kind {
            M::ScrollUp => -1,
            M::ScrollDown => 1,
            _ => 0,
        };
        match e.kind {
            M::Up(_) => {
                self.drag = false;
                return false;
            }
            M::Moved | M::ScrollLeft | M::ScrollRight => return false,
            _ => {}
        }
        // the cover viewer: any click closes it
        if self.viewer.url.is_some() {
            if matches!(e.kind, M::Down(_)) {
                self.close_viewer();
            }
            return true;
        }
        // Shift+click on a cover shows it big
        if e.kind == M::Down(MouseButton::Left) && e.modifiers.contains(KeyModifiers::SHIFT) && !self.help && self.menu.is_none() {
            if let Some(url) = self.geo.images.iter().find(|i| x >= i.0 && x <= i.1 && y >= i.2 && y <= i.3).map(|i| i.4.clone()) {
                self.open_viewer(url);
                return true;
            }
        }
        if self.help || self.credits.is_some() {
            if matches!(e.kind, M::Down(_)) {
                self.help = false;
                self.credits = None;
                self.help_scroll = 0;
                return true;
            }
            return false;
        }
        if wheel != 0 {
            if self.last_wheel.elapsed() < WHEEL {
                return false; // trackpads report in bursts: one notch is one row
            }
            self.last_wheel = Instant::now();
        }
        if self.menu.is_some() {
            if let Some(m) = &mut self.menu {
                if wheel != 0 {
                    m.sel = (m.sel as i64 + wheel).clamp(0, m.items.len() as i64 - 1) as usize;
                    return true;
                }
            }
            if let M::Down(button) = e.kind {
                let i = y.checked_sub(self.geo.top).map(|i| i + self.geo.menu_top);
                match (button, i) {
                    (MouseButton::Left, Some(i)) if self.menu.as_ref().is_some_and(|m| i < m.items.len()) => self.run_menu(i),
                    _ => self.menu = None,
                }
            }
            return true;
        }
        if e.kind == M::Down(MouseButton::Left) {
            // tabs and the search field never get trapped by the search box
            if y == 1 {
                if let Some(&(i, _, _)) = self.geo.tabs.iter().find(|s| x >= s.1 && x <= s.2) {
                    self.typing = false;
                    self.set_tab(TABS[i]);
                    return true;
                }
            }
            if y >= 2 && y <= self.geo.field_y1 && x <= self.geo.field_x1 {
                self.start_typing();
                return true;
            }
        }
        if self.typing {
            return false;
        }
        if (self.eq.open || self.downloads.open) && !self.lyrics && !self.full {
            if wheel != 0 {
                self.window_key(if wheel < 0 { KeyCode::Up } else { KeyCode::Down });
                return true;
            }
            if let (M::Down(_), Some((x0, x1, y0, y1))) = (e.kind, self.geo.window) {
                let inside = x >= x0 && x <= x1 && y >= y0 && y <= y1;
                if let Some(link) = self.geo.links.iter().find(|l| x >= l.1 && x <= l.2 && y == l.3) {
                    let action = link.0.clone();
                    self.run(action);
                    return true;
                }
                if inside {
                    return false;
                }
                if y >= self.geo.top && y < self.geo.top + self.geo.body {
                    if self.eq.open { self.close_eq() } else { self.close_downloads() }
                    return true;
                }
            }
        }
        match e.kind {
            M::Drag(_) => {
                // dragging along the progress bar scrubs
                if self.drag && self.last_seek.elapsed() > Duration::from_millis(60) {
                    self.last_seek = Instant::now();
                    self.seek_at(x);
                }
                return false;
            }
            M::ScrollUp | M::ScrollDown if self.lyrics => self.scroll_lyrics(wheel),
            M::ScrollUp | M::ScrollDown => {
                if self.tab == Tab::Home {
                    if let Some(id) = self.geo.panes.iter().find(|p| x >= p.x0 && x <= p.x1).map(|p| p.id) {
                        self.focus_home(id);
                    }
                }
                self.move_sel(self.sel as i64 + wheel);
            }
            M::Down(MouseButton::Right) => {
                let row = if self.full || self.lyrics { None } else { self.row_at(x, y) };
                if let Some((pane, i)) = row {
                    if self.tab == Tab::Home {
                        self.focus_home(self.geo.panes[pane].id);
                    }
                    if self.selectable(i) {
                        self.sel = i;
                        self.open_menu();
                        return true;
                    }
                }
                self.escape();
            }
            M::Down(MouseButton::Left) => {
                let g = &self.geo;
                let on_thumb = g.thumb.is_some_and(|(x0, x1, y0, y1)| x >= x0 && x <= x1 && y >= y0 && y <= y1);
                if let Some(&(control, ..)) = g.controls.iter().find(|&&(_, x0, x1, y0, y1)| x >= x0 && x <= x1 && y >= y0 && y <= y1) {
                    match control {
                        Control::Prev => self.cmd(json!(["playlist-prev"])),
                        Control::PlayPause => self.toggle_pause(),
                        Control::Next => self.cmd(json!(["playlist-next"])),
                        Control::Repeat => self.cycle_repeat(),
                    }
                } else if let Some(link) = g.links.iter().find(|l| x >= l.1 && x <= l.2 && y == l.3) {
                    let action = link.0.clone();
                    self.run(action);
                } else if y == g.bar_row {
                    if self.dur > 0.0 && x > g.bar_x0 && x <= g.bar_x0 + g.bar_w {
                        self.drag = true;
                        self.seek_at(x);
                    }
                } else if on_thumb {
                    self.toggle_pause(); // the cover is a play/pause button too
                } else if y == g.now_row {
                    self.toggle_pause();
                } else if self.full {
                    self.full = false;
                } else if y >= g.top && y < g.top + g.body {
                    self.body_click(x, y);
                }
            }
            _ => return false,
        }
        true
    }

    // Whatever covers the lists (full player, trip, lyrics, help, a menu) gives way: a search is always seen.
    // The keys of the window that is open (the equalizer or Downloads).
    fn window_key(&mut self, code: KeyCode) -> bool {
        if self.eq.open { self.eq_key(code) } else { self.downloads_key(code) }
    }
    pub(super) fn show_lists(&mut self) {
        self.full = false;
        self.trip_on = false;
        self.lyrics = false;
        self.help = false;
        self.credits = None;
        self.help_scroll = 0;
        self.menu = None;
    }
    pub(super) fn start_typing(&mut self) {
        self.show_lists();
        self.buf.clear();
        self.history_at = None;
        self.naming = None;
        self.typing = true;
    }

    pub(super) fn on_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let ch = match k.code {
            KeyCode::Char(c) => Some(c),
            _ => None,
        };
        if ctrl && ch == Some('c') {
            self.quit = true;
            return;
        }
        if ctrl && ch == Some('k') && !self.typing {
            return self.open_palette();
        }
        // Cmd Shift R (terminals that pass Cmd on) or Ctrl R anywhere: restart without stopping the music
        let cmd = k.modifiers.contains(KeyModifiers::SUPER);
        if matches!(ch, Some('r' | 'R')) && (cmd && (shift || ch == Some('R')) || k.modifiers.contains(KeyModifiers::CONTROL)) {
            self.restart = true;
            // the new run opens on the same tab (read once, in main)
            let tab = if self.tab == Tab::Search { self.prev_tab } else { self.tab };
            if let Some(i) = TABS.iter().position(|&t| t == tab) {
                self.db.set_setting("restart_tab", &i.to_string());
            }
            return self.quit_keeping_music();
        }
        if cmd {
            return; // other Cmd keys are not ours, and must not act as the letter alone
        }
        // Space only plays and pauses, wherever you are (except while typing); Shift+Space is Enter.
        let mut code = k.code;
        // (and in the command palette, where it separates words)
        if !self.typing && !ctrl && code == KeyCode::Char(' ') && !(self.menu.is_some() && self.palette.is_some()) {
            if !shift {
                return self.toggle_pause();
            }
            code = KeyCode::Enter;
        }
        if self.viewer.url.is_some() {
            return self.close_viewer(); // any key but space (it plays and pauses, above)
        }
        if self.credits.is_some() && !ctrl {
            // the credits panel: ↑ ↓ scroll it, s shares what it is about, any other key closes it
            match code {
                KeyCode::Up => self.help_scroll = self.help_scroll.saturating_sub(1),
                KeyCode::Down => self.help_scroll += 1, // drawing keeps it within the page
                KeyCode::Char('s') => {
                    let it = self.credits.as_ref().map(|c| c.item.clone());
                    self.share(it);
                }
                _ => {
                    self.credits = None;
                    self.help_scroll = 0;
                }
            }
            return;
        }
        if self.help && !ctrl && code != KeyCode::Char('/') {
            match code {
                KeyCode::Up => self.help_scroll = self.help_scroll.saturating_sub(1),
                KeyCode::Down => self.help_scroll += 1, // drawing keeps it within the page
                _ => {
                    self.help = false;
                    self.help_scroll = 0;
                }
            }
            return;
        }
        if self.menu.is_some() {
            self.menu_key(code);
        } else if (self.eq.open || self.downloads.open) && !self.typing && !ctrl && !self.lyrics && !self.full && self.window_key(code) {
            // the window used it
        } else if self.typing {
            self.typing_key(code, ctrl);
        } else if !ctrl {
            self.command_key(code, ch, shift);
        }
    }

    pub(super) fn menu_key(&mut self, code: KeyCode) {
        if self.palette_key(code) {
            return;
        }
        let Some(m) = &mut self.menu else { return };
        let n = m.items.len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => m.sel = m.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => m.sel = (m.sel + 1).min(n - 1),
            KeyCode::Enter => {
                let i = m.sel;
                self.run_menu(i);
            }
            KeyCode::Esc | KeyCode::Char('q' | 'm') => self.menu = None,
            KeyCode::Char('/') => self.start_typing(),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < n {
                    self.run_menu(i);
                }
            }
            _ => {}
        }
    }

    pub(super) fn typing_key(&mut self, code: KeyCode, ctrl: bool) {
        if matches!(self.naming, Some(Naming::Filter)) {
            match code {
                // Enter keeps it (empty: no filter); Esc clears it
                KeyCode::Enter | KeyCode::Esc => {
                    self.typing = false;
                    self.naming = None;
                    if code == KeyCode::Esc || self.buf.trim().is_empty() {
                        self.drop_filter();
                    }
                    return;
                }
                KeyCode::Up | KeyCode::Down => return self.move_sel(self.sel as i64 + if code == KeyCode::Up { -1 } else { 1 }),
                KeyCode::Tab | KeyCode::BackTab => {}
                _ => {
                    match code {
                        KeyCode::Backspace if ctrl => erase_word(&mut self.buf),
                        KeyCode::Char('w' | 'h') if ctrl => erase_word(&mut self.buf),
                        KeyCode::Char('u') if ctrl => self.buf.clear(),
                        KeyCode::Backspace => {
                            self.buf.pop();
                        }
                        KeyCode::Char(c) if !ctrl && !c.is_control() => self.buf.push(c),
                        _ => {}
                    }
                    return self.filter_typed();
                }
            }
        }
        match code {
            KeyCode::Enter => {
                self.typing = false;
                let text = self.buf.trim().to_string();
                if let Some(naming) = self.naming.take() {
                    self.finish_naming(naming, text);
                } else if text.is_empty() {
                    self.reopen_results();
                } else if let Some(link) = playlist::link(&text) {
                    self.import(link);
                } else {
                    self.run_search(text);
                }
            }
            KeyCode::Esc => self.typing = false,
            KeyCode::Tab | KeyCode::BackTab => {
                self.typing = false;
                self.cycle_tab(if code == KeyCode::BackTab { -1 } else { 1 });
            }
            KeyCode::Up | KeyCode::Down if self.naming.is_none() => {
                if self.history_at.is_none() {
                    self.history = self.db.recent_searches();
                    self.draft = self.buf.clone();
                }
                let at = self.history_at.map_or(-1, |i| i as i64) + if code == KeyCode::Up { 1 } else { -1 };
                self.history_at = usize::try_from(at.min(self.history.len() as i64 - 1)).ok();
                self.buf = self.history_at.map_or_else(|| self.draft.clone(), |i| self.history[i].clone());
            }
            // by the word with ⌥/Ctrl ⌫ or Ctrl W; all of it with Ctrl U (what Ghostty sends for ⌘⌫)
            KeyCode::Backspace if ctrl => erase_word(&mut self.buf),
            KeyCode::Char('w' | 'h') if ctrl => erase_word(&mut self.buf),
            KeyCode::Char('u') if ctrl => self.buf.clear(),
            KeyCode::Backspace => {
                self.buf.pop();
            }
            KeyCode::Char(c) if !ctrl && !c.is_control() => self.buf.push(c),
            _ => {}
        }
    }

    fn toggle_wave(&mut self) {
        self.wave = !self.wave;
        self.db.set_setting("wave", if self.wave { "on" } else { "off" });
        if !self.wave {
            self.bands = [0.0; BANDS]; // nothing is drawn; the filter keeps measuring unseen
        }
        if self.tab == Tab::Config {
            self.settings = config::rows(&self.db);
        }
        self.set_note(tr!("Visualizer {a}", a = if self.wave { tr!("on") } else { tr!("off") }));
    }
    pub(super) fn command_key(&mut self, code: KeyCode, ch: Option<char>, shift: bool) {
        use KeyCode as K;
        // the trip's own keys: [ ] change the scene, . holds it, y shows the sung line
        if self.full && self.trip_on {
            match ch {
                Some('[') => return self.trip.step(-1),
                Some(']') => return self.trip.step(1),
                Some('.') => return self.trip.toggle_lock(),
                Some('y') => {
                    self.trip_lyrics = !self.trip_lyrics;
                    self.db.set_setting("trip_lyrics", if self.trip_lyrics { "on" } else { "off" });
                    return self.set_note(tr!("Lyrics {a}", a = if self.trip_lyrics { tr!("on") } else { tr!("off") }));
                }
                _ => {}
            }
        }
        // capitals act as their lowercase unless they have a meaning of their own
        let name = match code {
            K::Char(c) => K::Char(c.to_ascii_lowercase()),
            other => other,
        };
        let n = self.data().len() as i64;
        if self.lyrics || self.full {
            // the list is hidden: movement scrolls the lyrics and list actions do nothing
            let step: i64 = match name {
                K::Up | K::Char('k') => -1,
                K::Down | K::Char('j') => 1,
                K::PageUp => -10,
                K::PageDown => 10,
                K::Home | K::Char('g') => -1_000_000,
                K::End => 1_000_000,
                _ => 0,
            };
            if step != 0 {
                self.scroll_lyrics(step);
                return;
            }
            if matches!(name, K::Char('a' | 'd' | 'm')) {
                return;
            }
        }
        match ch {
            Some('Q') => return self.quit_keeping_music(),
            Some('r') => return self.cycle_repeat(),
            Some('E') => return self.open_eq(),
            Some('W') => return self.toggle_wave(), // w alone steps through the styles
            Some('B') => return self.open_downloads(),
            Some('D') => return self.open_discover(),
            Some('V') => {
                self.trip_on = !(self.full && self.trip_on);
                self.full = self.trip_on;
                self.lyrics = false;
                self.trip_at = Instant::now();
                self.trip.opened(self.trip_t);
                return;
            }
            Some('Y') => return self.toggle_karaoke(),
            Some('F') => return self.toggle_follow(),
            // the playing song's pages, from the keyboard: o its album, O its artist
            Some('o') => return self.open_playing(false),
            Some('O') => return self.open_playing(true),
            Some('f') if self.lyrics && self.karaoke => return self.cycle_karaoke_style(),
            Some('R') => {
                // radio from the highlighted row, or from what plays when the list is hidden
                let it = if self.full || self.lyrics { None } else { self.cur().cloned() };
                return self.start_radio(it);
            }
            _ => {}
        }
        if self.tab == Tab::Config && !self.lyrics && !self.full && self.filtered().is_none() && !shift && self.config_keys(code) {
            return;
        }
        let on_setting = self.cur().is_some_and(|i| i.kind == Kind::Setting);
        if self.tab == Tab::Config && !self.lyrics && !self.full && !shift && on_setting && matches!(code, K::Left | K::Right | K::Enter) {
            return self.change_setting(if code == K::Left { -1 } else { 1 });
        }
        if self.tab == Tab::Stats && !self.lyrics && (ch == Some('t') || (!shift && matches!(code, K::Left | K::Right))) {
            if ch == Some('t') {
                self.period = (self.period + 1) % PERIODS.len();
            } else {
                self.stat_cat = (self.stat_cat + if code == K::Right { 1 } else { CATS.len() - 1 }) % CATS.len();
            }
            self.sel = 0;
            self.load_stats();
            return;
        }
        if self.tab == Tab::Liked && !self.lyrics && !self.full && !shift && matches!(code, K::Left | K::Right) {
            return self.switch_liked_view();
        }
        if self.results().is_some() && !self.lyrics && !self.full && !shift && matches!(code, K::Left | K::Right) {
            return self.step_results_tab(if code == K::Left { KINDS.len() - 1 } else { 1 });
        }
        let in_queue = self.tab == Tab::Queue;
        if in_queue && matches!(ch, Some('J' | 'K')) {
            if let Some(uid) = self.cur().map(|t| t.uid) {
                self.move_in_queue(uid, if ch == Some('J') { 1 } else { -1 });
            }
            return;
        }
        let sel = self.sel as i64;
        match name {
            K::Up | K::Char('k') => self.move_sel(sel - 1),
            K::Down | K::Char('j') => self.move_sel(sel + 1),
            K::PageUp => self.move_sel(sel - 10),
            K::PageDown => self.move_sel(sel + 10),
            K::Home | K::Char('g') => self.move_sel(0),
            K::End => self.move_sel(n - 1),
            K::Enter => self.press(),
            K::Tab => self.cycle_tab(1),
            K::BackTab => self.cycle_tab(-1),
            K::Esc | K::Backspace | K::Char('h') => self.escape(),
            K::Char('n') => self.cmd(json!(["playlist-next"])),
            K::Char('p') => self.cmd(json!(["playlist-prev"])),
            // plain arrows never move the song
            K::Left if shift => self.cmd(json!(["seek", -5])),
            K::Right if shift => self.cmd(json!(["seek", 5])),
            K::Left if self.tab == Tab::Home => self.focus_home(PaneId::Left),
            K::Right if self.tab == Tab::Home => self.focus_home(PaneId::Right),
            K::Char('l') => match self.album_at() {
                Some(album) => self.like_album(album),
                None => self.rate(true, None),
            },
            K::Char('x') => self.rate(false, None),
            K::Char('a') => match self.cur().filter(|t| t.kind == Kind::Track).cloned() {
                Some(t) => self.run(Action::Enqueue(t)),
                None => self.set_note(tr!("Select a track to queue").into()),
            },
            K::Char('d') => {
                let Some(it) = self.cur().cloned() else { return };
                if self.tab == Tab::Library {
                    self.ask_forget();
                } else if in_queue {
                    self.remove_from_queue(it.uid);
                } else if self.viewed_playlist().is_some() {
                    self.run(Action::RemoveEntry(it.uid));
                } else if let (Kind::Playlist, Ok(id)) = (it.kind, it.id.parse()) {
                    self.run(Action::AskDelete(id, it.title));
                }
            }
            K::Char('s') => {
                // the highlighted song, or the playing one when the list is hidden
                let shown = if self.full || self.lyrics { None } else { self.cur().filter(|t| t.kind == Kind::Track) };
                match shown.or_else(|| self.now()).cloned() {
                    Some(t) => self.pick_playlist(vec![t]),
                    None => self.set_note(tr!("Select a song to save").into()),
                }
            }
            K::Char('m') => self.open_menu(),
            K::Char('v') => {
                self.full = !self.full || self.trip_on;
                self.trip_on = false;
                self.lyrics = false;
            }
            K::Char('y') => self.toggle_lyrics(),
            // w: the next way of drawing it (in the trip: whether it reacts to the music); W: on or off
            K::Char('w') if self.full && self.trip_on => self.toggle_wave(),
            K::Char('w') if !self.wave => self.toggle_wave(),
            K::Char('w') => {
                let v = config::step(&self.db, "vizstyle", 1);
                self.apply_setting("vizstyle", v);
                if self.tab == Tab::Config {
                    self.settings = config::rows(&self.db);
                }
                self.set_note(tr!("Visualizer style: {a}", a = crate::i18n::t(v)));
            }
            K::Char('q') => self.quit = true,
            K::Char('z') => self.sleep_menu(),
            K::Char('b') => self.download_here(),
            K::Char('f') => self.start_filter(),
            K::Char(':') => self.open_palette(),
            K::Char('u') => self.undo(),
            K::Char('c') if in_queue && !self.full && !self.lyrics => self.clear_menu(),
            K::Char('/') => self.start_typing(), // the only key that searches
            K::Char('?') => self.help = true,
            K::Char('i') => {
                // the highlighted song or album, or what plays when the list is hidden
                let it = if self.full || self.lyrics { None } else { self.cur().filter(|t| matches!(t.kind, Kind::Track | Kind::Album)).cloned() };
                self.show_credits(it);
            }
            K::Char('+' | '=') => self.cmd(json!(["add", "volume", 5])),
            K::Char('-') => self.cmd(json!(["add", "volume", -5])),
            _ => {}
        }
    }
}
