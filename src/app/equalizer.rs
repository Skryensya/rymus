// The equalizer, a window over whatever is on screen (Config, or E): choose a profile and hear it at once, make
// profiles of your own, move their sliders, reset or delete them. The built-in ones are never edited.
use crate::i18n::tr;
use super::*;
use crate::eq::{self, Gains, Preset, FREQS, LIMIT};

#[derive(Default)]
pub struct EqView {
    pub open: bool,
    pub sel: usize,          // row in the list: the profiles, then "+ New profile"
    pub band: Option<usize>, // the slider being moved; None while choosing a profile
}

impl App {
    pub fn eq_presets(&self) -> Vec<Preset> {
        eq::presets(self.db.get_setting("eq_presets").as_deref())
    }
    pub fn eq_current(&self) -> (String, Gains) {
        let all = self.eq_presets();
        let name = self.db.get_setting("eq").unwrap_or_default();
        let p = all.iter().find(|p| p.name == name).unwrap_or(&all[0]);
        (p.name.clone(), p.gains)
    }
    // Puts the filter in (flat, unchanged across runs) and turns the bands to the chosen profile.
    pub(super) fn start_eq(&mut self) {
        self.mpv.cmd(json!(["af", "add", eq::filter()]));
        self.sync_eq();
    }
    // mpv may rebuild the filters (another one added or removed, a new song): the bands are turned again.
    pub(super) fn sync_eq(&mut self) {
        for c in eq::commands(&self.eq_current().1, self.mono()) {
            self.mpv.cmd(c);
        }
    }
    pub(super) fn mono(&self) -> bool {
        config::value(&self.db, "channels") == "mono"
    }
    // Opens the window with the cursor on the profile you hear.
    pub(super) fn open_eq(&mut self) {
        self.show_lists();
        self.typing = false;
        let name = self.eq_current().0;
        let sel = self.eq_presets().iter().position(|p| p.name == name).unwrap_or(0);
        self.eq = EqView { open: true, sel, band: None };
    }
    pub(super) fn close_eq(&mut self) {
        self.eq.open = false;
        if self.tab == Tab::Config {
            self.settings = config::rows(&self.db); // its row names the profile
        }
    }
    pub(super) fn pick_eq(&mut self, i: usize) {
        self.eq.band = None;
        self.eq_move(i, self.eq_presets().len());
    }
    fn choose_eq(&mut self, i: usize) {
        if let Some(p) = self.eq_presets().get(i) {
            self.db.set_setting("eq", &p.name);
            self.sync_eq();
        }
    }
    // The window takes every key but play/pause, volume and the key list (false: left to the app).
    pub(super) fn eq_key(&mut self, code: KeyCode) -> bool {
        use KeyCode as K;
        let (sel, band) = (self.eq.sel, self.eq.band);
        let all = self.eq_presets();
        let last = all.len(); // the "+ New profile" row
        if matches!(code, K::Char(' ' | '+' | '-' | '=' | '?')) {
            return false;
        }
        let current = self.eq_current().0;
        let yours = all.iter().find(|p| p.name == current && p.custom);
        match code {
            // on a profile of yours, from the list or its sliders
            K::Char('r') => match yours {
                Some(p) => {
                    let items = vec![(tr!("Keep it").to_string(), Action::Nothing), (tr!("Reset to {a}", a = p.base), Action::ResetEq(p.name.clone()))];
                    self.menu = Some(Menu { title: tr!("reset {a}?", a = p.name), items, sel: 0 });
                }
                None => self.set_note(tr!("{current} is as it came", current = current)),
            },
            K::Char('d') => match yours {
                Some(p) => {
                    let items = vec![(tr!("Keep it").to_string(), Action::Nothing), (tr!("Delete").to_string(), Action::DeleteEq(p.name.clone()))];
                    self.menu = Some(Menu { title: tr!("delete {a}?", a = p.name), items, sel: 0 });
                }
                None => self.set_note(tr!("{current} is built in: it stays", current = current)),
            },
            K::Char('n') => self.start_naming(Naming::EqPreset, String::new()),
            _ => match band {
                // on the sliders: every change is kept and heard at once
                Some(b) => match code {
                    K::Up | K::Char('k') => self.move_band(b, 1),
                    K::Down | K::Char('j') => self.move_band(b, -1),
                    K::Char('0') => self.move_band(b, -self.eq_current().1[b]),
                    K::Left | K::Char('h') if b == 0 => self.eq.band = None, // back to the profiles
                    K::Left | K::Char('h') => self.eq.band = Some(b - 1),
                    K::Right | K::Char('l') => self.eq.band = Some((b + 1).min(FREQS.len() - 1)),
                    K::Esc | K::Enter | K::Backspace => self.eq.band = None,
                    _ => {}
                },
                None => match code {
                    K::Up | K::Char('k') => self.eq_move(sel.saturating_sub(1), last),
                    K::Down | K::Char('j') => self.eq_move((sel + 1).min(last), last),
                    K::Enter if sel == last => self.start_naming(Naming::EqPreset, String::new()),
                    K::Enter | K::Right | K::Char('l') if sel < last => self.eq.band = Some(0),
                    K::Char('0') => self.eq_move(0, last), // back to Flat, the default
                    K::Esc | K::Backspace | K::Char('q' | 'E') => self.close_eq(),
                    _ => {}
                },
            },
        }
        true
    }
    fn eq_move(&mut self, to: usize, last: usize) {
        self.eq.sel = to;
        if to < last {
            self.choose_eq(to); // heard while moving, like flipping a switch
        }
    }
    // One band of the profile you hear, if it is yours.
    fn move_band(&mut self, b: usize, step: i64) {
        let mut all = self.eq_presets();
        let name = self.eq_current().0;
        let Some(at) = all.iter().position(|p| p.name == name) else { return };
        if !all[at].custom {
            return self.set_note(tr!("{name} is built in · n makes your own from it", name = name));
        }
        all[at].gains[b] = (all[at].gains[b] + step).clamp(-LIMIT, LIMIT);
        self.db.set_setting("eq_presets", &eq::save(&all));
        self.eq.sel = at;
        self.sync_eq();
    }
    // A new profile of yours starts from what you hear now, and opens on its sliders.
    pub(super) fn new_eq(&mut self, name: String) {
        let mut all = self.eq_presets();
        if all.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
            return self.set_note(tr!("There is already a profile called {name}", name = name));
        }
        let (current, gains) = self.eq_current();
        let base = all.iter().find(|p| p.name == current).map_or(tr!("Flat").to_string(), |p| p.base.clone());
        all.push(Preset { name: name.clone(), gains, custom: true, base });
        self.db.set_setting("eq_presets", &eq::save(&all));
        self.db.set_setting("eq", &name);
        self.eq = EqView { open: true, sel: all.len() - 1, band: Some(0) };
        self.set_note(tr!("Created {name} · ← → band · ↑ ↓ gain", name = name));
    }
    // Back to the sound of the built-in profile it was made from; it keeps its name.
    pub(super) fn reset_eq(&mut self, name: &str) {
        let mut all = self.eq_presets();
        let Some(p) = all.iter_mut().find(|p| p.name == name && p.custom) else { return };
        p.gains = eq::built_in(&p.base);
        let note = tr!("{name} is {a} again", name = name, a = p.base);
        self.db.set_setting("eq_presets", &eq::save(&all));
        self.sync_eq();
        self.set_note(note);
    }
    pub(super) fn delete_eq(&mut self, name: &str) {
        let mut all = self.eq_presets();
        all.retain(|p| p.name != name);
        self.db.set_setting("eq_presets", &eq::save(&all));
        if self.eq_current().0 != self.db.get_setting("eq").unwrap_or_default() || self.db.get_setting("eq").as_deref() == Some(name) {
            self.db.set_setting("eq", tr!("Flat"));
        }
        self.sync_eq();
        self.open_eq();
        self.set_note(tr!("Deleted {name}", name = name));
    }
}
