// Your data as a whole (Config > Data): export it, import it back, or wipe it out. An export is a folder of CSV files,
// one per table, that any spreadsheet opens; importing one replaces everything with what it holds.
use crate::i18n::tr;
use super::*;
use std::path::Path;

// What has to be typed, exactly, to wipe everything out: a sentence, so it is never typed by accident.
pub const WIPE_WORD: &str = "delete all my data";

// "~/x" means the home folder.
pub(super) fn expand(path: &str) -> PathBuf {
    crate::platform::expand(path)
}

impl App {
    // Enter on Export, Import or Wipe: each asks first, in the text field.
    pub(super) fn start_data(&mut self, what: &str) {
        match what {
            "export" => {
                let t = crate::platform::local_time(crate::db::now_ms() / 1000);
                let day = format!("{}-{:02}-{:02}", t.tm_year + 1900, t.tm_mon + 1, t.tm_mday);
                self.start_naming(Naming::Export, format!("~/rymus-export-{day}"));
            }
            "import" => self.start_naming(Naming::Import, "~/".into()),
            _ => self.start_naming(Naming::Wipe, String::new()),
        }
    }

    pub(super) fn export_data(&mut self, to: &str) {
        let path = expand(to);
        match self.db.export_csv(&path) {
            Ok(()) => self.set_note(tr!("Exported to {to}", to = to)),
            Err(e) => self.msg = tr!("Could not export: {e}", e = e),
        }
    }

    pub(super) fn import_data(&mut self, from: &str) {
        let source = expand(from);
        // read into a new file first: a folder that cannot be read leaves your data as it was
        let staged = self.db_path.with_extension("importing");
        if let Err(e) = crate::db::Db::import_csv(&source, &staged) {
            let _ = std::fs::remove_file(&staged);
            self.msg = tr!("Not a rymus export: {e}", e = e);
            return;
        }
        self.replace_db(Some(&staged));
        self.set_note(tr!("Imported {from}", from = from));
    }

    // Only the exact word wipes; anything else (or Esc) leaves everything as it was.
    pub(crate) fn wipe_data(&mut self, typed: &str) {
        if typed.to_lowercase() != WIPE_WORD {
            self.set_note(tr!("Nothing was deleted: type \"{wipe_word}\" to delete all your data", wipe_word = WIPE_WORD));
            return;
        }
        self.replace_db(None);
        self.set_note(tr!("All your data was deleted · rymus starts fresh").into());
    }

    // Closes the database, deletes its files, puts `with` in its place (or nothing) and opens it again: an empty one
    // is made then, as on the first run. Everything read from it is read again.
    fn replace_db(&mut self, with: Option<&Path>) {
        drop(std::mem::replace(&mut self.db, crate::db::Db::memory()));
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", self.db_path.display()));
        }
        if let Some(file) = with {
            let _ = std::fs::rename(file, &self.db_path);
        }
        match crate::db::Db::open(&self.db_path) {
            Ok(db) => self.db = db,
            Err(e) => {
                self.msg = tr!("Could not open the database: {e}", e = e);
                return;
            }
        }
        // the song playing was being counted in the old database: from here it counts in this one, from zero
        self.heard = 0.0;
        self.play_id = self.sounding.as_ref().map(|t| self.db.start_play(t, crate::db::now_ms()));
        self.likes = self.db.liked_ids();
        self.dislikes = self.db.disliked_ids();
        self.session = load_session(&self.db);
        for s in &config::SETTINGS {
            let v = config::value(&self.db, s.key);
            self.apply_setting(s.key, v);
        }
        self.settings = config::rows(&self.db);
        self.start_eq();
        self.playlists_changed();
    }

    // A newer rymus, told with a note once a run; GitHub is asked at most once a day (Config > Updates can say never).
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
    fn tell_update(&mut self, tag: &str) {
        self.set_note(tr!("rymus {tag} is out · Config > Updates, or rymus update", tag = tag));
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
