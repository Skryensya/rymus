// The EQ tab: choose a profile and hear it at once, move any band's slider, keep profiles of your own.
use super::*;
use crate::eq::{self, Gains, Preset, FREQS, LIMIT};

#[derive(Default)]
pub struct EqView {
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
        for c in eq::commands(&self.eq_current().1) {
            self.mpv.cmd(c);
        }
    }
    // Coming to the tab: the cursor on the profile you hear.
    pub(super) fn enter_eq(&mut self) {
        let name = self.eq_current().0;
        let sel = self.eq_presets().iter().position(|p| p.name == name).unwrap_or(0);
        self.eq = EqView { sel, band: None };
    }
    fn choose_eq(&mut self, i: usize) {
        if let Some(p) = self.eq_presets().get(i) {
            self.db.set_setting("eq", &p.name);
            self.sync_eq();
        }
    }
    // The tab's own keys. False for a key it leaves to the rest of the app (tab, space, volume…).
    pub(super) fn eq_key(&mut self, code: KeyCode) -> bool {
        use KeyCode as K;
        let (sel, band) = (self.eq.sel, self.eq.band);
        let all = self.eq_presets();
        let last = all.len(); // the "+ New profile" row
        if let Some(b) = band {
            // on the sliders: every change is kept and heard at once
            match code {
                K::Up | K::Char('k') => self.move_band(b, 1),
                K::Down | K::Char('j') => self.move_band(b, -1),
                K::Char('0') => self.move_band(b, -self.eq_current().1[b]),
                K::Left | K::Char('h') if b == 0 => self.eq.band = None, // back to the profiles
                K::Left | K::Char('h') => self.eq.band = Some(b - 1),
                K::Right | K::Char('l') => self.eq.band = Some((b + 1).min(FREQS.len() - 1)),
                K::Esc | K::Enter => self.eq.band = None,
                _ => return false,
            }
            return true;
        }
        match code {
            K::Up | K::Char('k') => self.eq_move(sel.saturating_sub(1), last),
            K::Down | K::Char('j') => self.eq_move((sel + 1).min(last), last),
            K::Enter if sel == last => self.start_naming(Naming::EqPreset, String::new()),
            K::Enter | K::Right | K::Char('l') if sel < last => self.eq.band = Some(0),
            K::Char('n') => self.start_naming(Naming::EqPreset, String::new()),
            K::Char('d') if sel < last && all[sel].custom => {
                let name = all[sel].name.clone();
                let items = vec![("Keep it".to_string(), Action::Nothing), ("Delete".to_string(), Action::DeleteEq(name.clone()))];
                self.menu = Some(Menu { title: format!("delete {name}?"), items, sel: 0 });
            }
            _ => return false,
        }
        true
    }
    fn eq_move(&mut self, to: usize, last: usize) {
        self.eq.sel = to;
        if to < last {
            self.choose_eq(to); // heard while moving, like flipping a switch
        }
    }
    // One band of the profile you hear. A built-in one stays as it is: the change goes to a copy that is yours.
    fn move_band(&mut self, b: usize, step: i64) {
        let mut all = self.eq_presets();
        let Some(mut at) = all.iter().position(|p| p.name == self.eq_current().0) else { return };
        if !all[at].custom {
            let base = format!("My {}", all[at].name);
            let free = |n: &String| !all.iter().any(|p| p.name.eq_ignore_ascii_case(n));
            let name = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base} {n}") }).find(free).expect("a free name");
            all.push(Preset { name: name.clone(), gains: all[at].gains, custom: true });
            at = all.len() - 1;
            self.db.set_setting("eq", &name);
            self.set_note(format!("Saved as {name}: built-in profiles stay as they are"));
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
            return self.set_note(format!("There is already a profile called {name}"));
        }
        let gains = self.eq_current().1;
        all.push(Preset { name: name.clone(), gains, custom: true });
        self.db.set_setting("eq_presets", &eq::save(&all));
        self.db.set_setting("eq", &name);
        self.eq = EqView { sel: all.len() - 1, band: Some(0) };
        self.set_note(format!("Created {name} · ← → band · ↑ ↓ gain"));
    }
    pub(super) fn delete_eq(&mut self, name: &str) {
        let mut all = self.eq_presets();
        all.retain(|p| p.name != name);
        self.db.set_setting("eq_presets", &eq::save(&all));
        if self.eq_current().0 != self.db.get_setting("eq").unwrap_or_default() || self.db.get_setting("eq").as_deref() == Some(name) {
            self.db.set_setting("eq", "Flat");
        }
        self.sync_eq();
        self.enter_eq();
        self.set_note(format!("Deleted {name}"));
    }
}
