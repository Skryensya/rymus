// New versions of rymus, inside the app: the daily check and its note, Config's Update row, and what changed, shown
// once on the first run of a new version. The work itself (GitHub, checksums, the binary) is in update.rs.
use crate::i18n::tr;
use super::*;

impl App {
    // A newer rymus, told with a note once a run; GitHub is asked at most once a day (Config > General can say never).
    pub fn check_for_update(&mut self) {
        if config::value(&self.db, "updates") == "off" {
            return;
        }
        let known = self.db.get_setting("update_latest").unwrap_or_default();
        if crate::update::newer(&known, crate::update::VERSION) {
            self.tell_update(&known);
        }
        let checked: i64 = self.db.get_setting("update_checked").and_then(|v| v.parse().ok()).unwrap_or(0);
        if crate::db::now_ms() - checked > 86_400_000 {
            self.ask_github(false);
        }
    }
    // The first run of a newer rymus than the last one shows what changed, from its release notes. A first run ever,
    // or one after the data was deleted, shows nothing: there is no "before" to compare with.
    pub fn show_news(&mut self) {
        let seen = self.db.get_setting("seen_version");
        self.db.set_setting("seen_version", crate::update::VERSION);
        let tag = format!("v{}", crate::update::VERSION);
        if !seen.is_some_and(|v| crate::update::newer(&tag, &v)) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let Ok(lines) = crate::update::notes(&tag) else { return }; // offline: the page is still a click away in Config
            if lines.is_empty() {
                return;
            }
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                app.panel_scroll = 0;
                app.news = Some((tag, lines));
            })));
        });
    }
    fn tell_update(&mut self, tag: &str) {
        self.set_note(tr!("rymus {tag} is out · Config > General, or rymus update", tag = tag));
    }
    // Looks the latest release up in the background; `install`: and puts it in place if it is newer.
    fn ask_github(&mut self, install: bool) {
        if cfg!(test) {
            return; // no network in tests
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let found = crate::update::latest();
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| match found {
                Ok(tag) => {
                    app.db.set_setting("update_latest", &tag);
                    app.db.set_setting("update_checked", &crate::db::now_ms().to_string());
                    app.settings = config::rows(&app.db);
                    match (crate::update::newer(&tag, crate::update::VERSION), install) {
                        (true, true) => app.install_update(tag),
                        (true, false) => app.tell_update(&tag),
                        (false, true) => app.set_note(tr!("rymus {v} is the latest", v = crate::update::VERSION)),
                        (false, false) => {}
                    }
                }
                Err(e) if install => app.msg = tr!("Could not look for updates: {e}", e = e),
                Err(_) => {} // a daily check that fails just waits for the next run
            })));
        });
    }
    // Config's Update row: install what is known to be newer, or look first.
    pub(super) fn update_now(&mut self) {
        let known = self.db.get_setting("update_latest").unwrap_or_default();
        if crate::update::newer(&known, crate::update::VERSION) {
            self.install_update(known);
        } else {
            self.set_note(tr!("Looking for updates…").into());
            self.ask_github(true);
        }
    }
    fn install_update(&mut self, tag: String) {
        self.busy = tr!("Updating to {tag}…", tag = tag);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let done = crate::update::install(&tag);
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                app.busy.clear();
                match done {
                    Ok(_) => app.set_note(tr!("Updated to {tag} · Cmd Shift R restarts into it", tag = tag)),
                    Err(e) => app.msg = tr!("Could not update: {e}", e = e),
                }
            })));
        });
    }
}
