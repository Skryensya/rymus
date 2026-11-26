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
}
