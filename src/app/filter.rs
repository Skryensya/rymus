// f narrows the list you are looking at as you type; : (or Ctrl K) finds any command by name.
use crate::i18n::t;
use crate::i18n::tr;
use super::*;

pub struct Filter {
    pub text: String,
    pub tab: Tab,
    list_id: u64, // which page of results, on the Search tab
    pub rows: Vec<Item>,
}

// What each command is called, and the key that does it.
const COMMANDS: [(&str, char); 26] = [
    ("Play / pause", ' '), ("Next song", 'n'), ("Previous song", 'p'), ("Search", '/'), ("Filter this list", 'f'), ("Like", 'l'), ("Dislike", 'x'),
    ("Add to queue", 'a'), ("Save to a playlist", 's'), ("Radio from this song", 'R'), ("Discover", 'D'), ("Lyrics", 'y'), ("Karaoke", 'Y'),
    ("Big player", 'v'), ("Trip visualizer", 'V'), ("Equalizer", 'E'), ("Repeat: queue, song, off", 'r'), ("Sleep timer", 'z'), ("Download this song", 'b'), ("Follow this artist", 'F'), ("Undo", 'u'),
    ("Album of what plays", 'o'), ("Artist of what plays", 'O'), ("Visualizer on / off", 'w'), ("All the keys", '?'), ("Quit, music stays", 'Q'),
];
pub const PALETTE: &str = "commands";

impl App {
    fn filterable(&self) -> bool {
        matches!(self.tab, Tab::Liked | Tab::Library | Tab::Queue | Tab::Playlists | Tab::History | Tab::Search)
    }
    // The filter, when it is on the list on screen.
    pub fn filtered(&self) -> Option<&Filter> {
        self.filter.as_ref().filter(|f| f.tab == self.tab && (f.tab != Tab::Search || f.list_id == self.list_id))
    }
    pub(super) fn start_filter(&mut self) {
        if !self.filterable() || self.full || self.lyrics {
            return self.set_note(tr!("f filters Liked, the queue, playlists, history and pages").into());
        }
        let text = self.filtered().map_or(String::new(), |f| f.text.clone());
        self.filter = Some(Filter { text: text.clone(), tab: self.tab, list_id: self.list_id, rows: vec![] });
        self.refilter();
        self.start_naming(Naming::Filter, text);
    }
    // As the text changes.
    pub(super) fn filter_typed(&mut self) {
        let text = self.buf.clone();
        if let Some(f) = &mut self.filter {
            f.text = text;
        }
        self.refilter();
        self.sel = 0;
        self.clamp_sel();
    }
    pub(super) fn drop_filter(&mut self) {
        // stay on the song you had found
        let found = self.filtered().and_then(|f| f.rows.get(self.sel)).map(|t| (t.uid, t.id.clone()));
        self.filter = None;
        if let Some((uid, id)) = found {
            self.sel = self.data().iter().position(|t| t.uid == uid && t.id == id).unwrap_or(0);
        }
        self.clamp_sel();
    }
    // The rows that hold every word typed, from the list as it is now (it changes under the filter).
    pub fn refilter(&mut self) {
        if self.filtered().is_none() {
            return;
        }
        let Some(mut f) = self.filter.take() else { return };
        let wanted = crate::yt::words(&f.text);
        f.rows = self
            .data() // the filter is out of the way: the whole list
            .iter()
            .filter(|t| !matches!(t.kind, Kind::Header | Kind::Button))
            .filter(|t| {
                let have = crate::yt::words(&format!("{} {} {} {}", t.title, t.artist, t.album, t.author)).join(" ");
                wanted.iter().all(|w| have.contains(w.as_str()))
            })
            .cloned()
            .collect();
        self.filter = Some(f);
        self.sel = self.sel.min(self.data().len().saturating_sub(1));
    }

    pub(super) fn open_palette(&mut self) {
        self.palette = Some(String::new());
        self.fill_palette();
    }
    fn fill_palette(&mut self) {
        let Some(query) = self.palette.clone() else { return };
        let wanted = query.to_lowercase();
        let places = [(tr!("Go to Home"), Tab::Home), (tr!("Go to Playlists"), Tab::Playlists), (tr!("Go to Liked"), Tab::Liked), (tr!("Go to Library"), Tab::Library), (tr!("Go to Queue"), Tab::Queue),
            (tr!("Go to History"), Tab::History), (tr!("Go to Stats"), Tab::Stats), (tr!("Go to Config"), Tab::Config)];
        let items: Vec<(String, Action)> = COMMANDS
            .iter()
            .map(|&(name, key)| (format!("{}  ({})", t(name), if key == ' ' { t("space").into() } else { key.to_string() }), Action::Press(key)))
            .chain(places.iter().map(|&(name, tab)| (t(name).to_string(), Action::GoTab(tab))))
            .filter(|(name, _)| wanted.split_whitespace().all(|w| name.to_lowercase().contains(w)))
            .collect();
        let items = if items.is_empty() { vec![(tr!("No such command").to_string(), Action::Nothing)] } else { items };
        let title = if query.is_empty() { tr!("{palette} · type to find", palette = t(PALETTE)) } else { format!("{PALETTE}: {query}") };
        self.menu = Some(Menu { title, items, sel: 0 });
    }
    // The palette takes the letters; true when it used the key.
    pub(super) fn palette_key(&mut self, code: KeyCode) -> bool {
        if self.menu.as_ref().is_none_or(|m| !m.title.starts_with(t(PALETTE))) {
            self.palette = None;
        }
        let Some(query) = &mut self.palette else { return false };
        match code {
            KeyCode::Char(c) => query.push(c),
            KeyCode::Backspace => {
                query.pop();
            }
            KeyCode::Esc => {
                self.palette = None;
                self.menu = None;
                return true;
            }
            _ => return false,
        }
        self.fill_palette();
        true
    }
    // A command picked in the palette does what its key does.
    pub(super) fn press_key(&mut self, key: char) {
        match key {
            '?' => self.help = true,
            ' ' => self.toggle_pause(),
            _ => self.command_key(KeyCode::Char(key), Some(key), key.is_uppercase()),
        }
    }
}
