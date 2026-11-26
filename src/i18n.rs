// Translations. The English text is the key: lang/translations.csv (built in) and ~/.rymus/translations.csv (yours, on
// top of it) have an "english" column and one per language. An empty cell, or a text that is in neither, stays English.
use std::collections::HashMap;
use std::path::Path;

pub const ENGLISH: &str = "english";
const BUILT_IN: &str = include_str!("../lang/translations.csv");

// The words of one language, by their English text.
type Table = HashMap<&'static str, &'static str>;

#[derive(Default)]
struct All {
    languages: Vec<&'static str>, // english first
    keys: Vec<&'static str>,      // in the order they were met
    tables: HashMap<&'static str, Table>,
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

impl All {
    // One file's rows over what is already there.
    fn add(&mut self, csv: &str) {
        let mut rows = crate::csv::parse(csv.trim_start_matches('\u{feff}')).into_iter();
        let Some(head) = rows.next() else { return };
        let Some(english) = head.iter().position(|h| h.trim().eq_ignore_ascii_case(ENGLISH)) else { return };
        let columns: Vec<Option<&'static str>> = head
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let name = crate::item::clean(name.trim()).to_lowercase();
                (i != english && !name.is_empty()).then(|| match self.languages.iter().find(|l| **l == name) {
                    Some(known) => *known,
                    None => {
                        let new = leak(&name);
                        self.languages.push(new);
                        new
                    }
                })
            })
            .collect();
        for row in rows {
            let Some(text) = row.get(english).filter(|t| !t.is_empty()) else { continue };
            let key = match self.keys.iter().find(|k| **k == text.as_str()) {
                Some(k) => *k,
                None => {
                    let k = leak(text);
                    self.keys.push(k);
                    k
                }
            };
            for (cell, language) in row.iter().zip(&columns) {
                if let Some(language) = language.filter(|_| !cell.trim().is_empty()) {
                    // text from a file: nothing in it may steer the terminal
                    self.tables.entry(language).or_default().insert(key, leak(&cell.chars().filter(|c| !c.is_control()).collect::<String>()));
                }
            }
        }
    }
    fn read(yours: Option<&str>) -> All {
        let mut all = All { languages: vec![ENGLISH], ..Default::default() };
        all.add(BUILT_IN);
        if let Some(csv) = yours {
            all.add(csv);
        }
        all
    }
}

struct State {
    all: All,
    listed: &'static [&'static str],
    chosen: &'static str,
}

// One for the whole program (threads compose messages too); in tests, one per test, so they do not speak over each other.
#[cfg(not(test))]
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    use std::sync::{Mutex, OnceLock};
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    let state = STATE.get_or_init(|| Mutex::new(State { all: All::read(None), listed: &[], chosen: ENGLISH }));
    f(&mut state.lock().unwrap_or_else(|e| e.into_inner()))
}
#[cfg(test)]
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    thread_local!(static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State { all: All::read(None), listed: &[], chosen: ENGLISH }));
    STATE.with(|s| f(&mut s.borrow_mut()))
}

// Reads your file (next to the database) over the built in one.
pub fn load(folder: &Path) {
    let yours = std::fs::read_to_string(folder.join("translations.csv")).ok();
    with(|s| s.all = All::read(yours.as_deref()));
}
// Every language there is a column for, english first.
pub fn languages() -> &'static [&'static str] {
    with(|s| {
        // made once per reading of the files (it is asked for on every frame of Config)
        if s.listed.len() != s.all.languages.len() {
            s.listed = Box::leak(s.all.languages.clone().into_boxed_slice());
        }
        s.listed
    })
}
pub fn set(language: &str) {
    with(|s| s.chosen = s.all.languages.iter().find(|l| **l == language).copied().unwrap_or(ENGLISH));
}

// The text in the chosen language.
// (a translation lives for the whole run, so it outlives whatever text it stands for)
pub fn t(english: &str) -> &str {
    with(|s| s.all.tables.get(s.chosen).and_then(|table| table.get(english)).copied().unwrap_or(english))
}
// For a text only known when it runs (a stored name): translated if it is one of ours.
pub fn any(text: &str) -> String {
    with(|s| s.all.tables.get(s.chosen).and_then(|table| table.get(text)).map_or_else(|| text.to_string(), |v| v.to_string()))
}
// The same, with each {name} replaced by its value.
pub fn fill(english: &'static str, values: &[(&str, String)]) -> String {
    let mut out = String::new();
    let mut rest = t(english);
    // one pass, so a song titled "{n}" stays that
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let name = &rest[open + 1..open + close];
        match values.iter().find(|v| v.0 == name) {
            Some(value) => {
                out.push_str(&rest[..open]);
                out.push_str(&value.1);
            }
            None => out.push_str(&rest[..open + close + 1]),
        }
        rest = &rest[open + close + 1..];
    }
    out + rest
}

// `rymus translations`: every text with what each language has for it, to fill in and save as yours.
pub fn export() -> String {
    with(|s| {
        let head: Vec<String> = s.all.languages.iter().map(|l| l.to_string()).collect();
        let mut out = crate::csv::line(&head);
        for key in &s.all.keys {
            let row: Vec<String> = s.all.languages.iter().map(|l| if *l == ENGLISH { key.to_string() } else { s.all.tables.get(l).and_then(|t| t.get(key)).map_or(String::new(), |v| v.to_string()) }).collect();
            out.push_str(&crate::csv::line(&row));
        }
        out
    })
}

// tr!("Quit") is the text in the chosen language; tr!("{n} songs", n = 3) also fills it in.
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::t($s)
    };
    ($s:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill($s, &[$((stringify!($name), $value.to_string())),+])
    };
}
pub(crate) use tr;

#[cfg(test)]
mod tests {
    use super::*;

    // Every tr!("…") in the code, read from the source files.
    fn used() -> Vec<String> {
        let mut out = vec![];
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![dir.clone()];
        while let Some(f) = files.pop() {
            if f.is_dir() {
                files.extend(std::fs::read_dir(&f).unwrap().flatten().map(|e| e.path()));
                continue;
            }
            if f.ends_with("i18n.rs") {
                continue; // its own examples
            }
            let text = std::fs::read_to_string(&f).unwrap();
            let code = text.split("#[cfg(test)]\nmod tests").next().unwrap();
            for part in code.split("tr!(\"").skip(1) {
                let (mut s, mut chars) = (String::new(), part.chars());
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some(other) => s.push(other),
                            None => {}
                        },
                        '"' => break,
                        c => s.push(c),
                    }
                }
                out.push(s);
            }
        }
        out
    }

    #[test]
    fn every_text_is_in_the_translations_and_spanish_has_them_all() {
        let all = All::read(None);
        let missing: Vec<String> = used().into_iter().filter(|s| !all.keys.contains(&s.as_str())).collect();
        assert!(missing.is_empty(), "add these to lang/translations.csv: {missing:#?}");
        let es = &all.tables["español"];
        // left in English on purpose: commands, file names, hook names
        let untranslated: Vec<&&str> = all.keys.iter().filter(|k| !es.contains_key(*k) && !k.starts_with(['~', 'Y']) && !k.starts_with("rymus") && !k.starts_with("ctrl") && !k.contains('-') && k.chars().any(char::is_uppercase)).collect();
        assert!(untranslated.is_empty(), "{untranslated:?}");
    }

    #[test]
    fn a_language_is_chosen_filled_in_and_falls_back_to_english() {
        set("español");
        assert_eq!(t("Searching…"), "Buscando…");
        assert_eq!(tr!("Added {n} songs to {name}", n = 3, name = "{n}"), "Se agregaron 3 canciones a {n}", "a value is never read as a placeholder");
        assert_eq!(t("A text nobody wrote"), "A text nobody wrote");
        // yours over the built in ones: a new language, and a better word
        with(|s| s.all = All::read(Some("english,español,français\r\nSearching…,Busco…,Recherche…\r\n")));
        assert_eq!(languages(), ["english", "español", "français"]);
        assert_eq!(t("Searching…"), "Busco…");
        assert_eq!(t("Searching…\x1b"), "Searching…\x1b");
        set("français");
        assert_eq!((t("Searching…"), t("Nothing found")), ("Recherche…", "Nothing found"));
        set("klingon");
        assert_eq!(t("Searching…"), "Searching…");
        assert!(export().starts_with("english,español,français\r\n"));
    }
}
