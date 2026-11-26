// Search results: songs, albums and artists, one tab each (← →), more of them loading as you reach the end.
use crate::i18n::tr;
use super::*;

pub const KINDS: [&str; 3] = ["Songs", "Albums", "Artists"];

pub struct Found {
    pub list_id: u64, // the list on screen that these are (an album opened from here is another one)
    pub tab: usize,
    pub lists: [Vec<Item>; 3],
    pub next: [Option<String>; 3], // the token of each tab's next page
    sel: [usize; 3],
}

impl App {
    pub(super) fn run_search(&mut self, text: String) {
        self.db.save_search(&text);
        let yt = self.yt.clone();
        self.run_busy(tr!("Searching…").into(), move || {
            let pages = std::thread::scope(|s| {
                let (yt, text) = (&yt, &text);
                [0, 1, 2].map(|kind| s.spawn(move || yt.page(kind, text, None))).map(|t| t.join().unwrap_or_else(|_| Err(tr!("search thread failed").into())))
            });
            let [songs, albums, artists] = pages;
            let pages = [songs?, albums?, artists?];
            Ok(Box::new(move |app: &mut App| {
                app.query = text.clone();
                app.open_results();
                app.back.clear();
                let next = [pages[0].1.clone(), pages[1].1.clone(), pages[2].1.clone()];
                let lists = pages.map(|p| p.0);
                let tab = lists.iter().position(|l| !l.is_empty());
                if tab.is_none() {
                    app.msg = tr!("Nothing found").into();
                }
                let tab = tab.unwrap_or(0);
                app.set_list(tr!("Results for \"{text}\"", text = text), lists[tab].clone(), false, 0);
                app.result_lists.insert(app.list_id);
                app.found = Some(Found { list_id: app.list_id, tab, lists, next, sel: [0; 3] });
            }) as Apply)
        });
    }
    // The results, when they are what is on screen.
    pub fn results(&self) -> Option<&Found> {
        self.found.as_ref().filter(|f| self.tab == Tab::Search && f.list_id == self.list_id)
    }
    pub(super) fn results_tab(&mut self, to: usize) {
        if self.results().is_none() {
            return;
        }
        let sel = self.sel;
        let Some(f) = &mut self.found else { return };
        f.sel[f.tab] = sel;
        f.tab = to;
        self.items = f.lists[to].clone();
        self.sel = f.sel[to];
        self.clamp_sel();
    }
    pub(super) fn step_results_tab(&mut self, dir: usize) {
        let Some(tab) = self.results().map(|f| f.tab) else { return };
        self.results_tab((tab + dir) % KINDS.len());
    }
    // Near the end of a tab: the next page is asked for, and added under what is there.
    pub(super) fn load_more_results(&mut self) {
        if self.results().is_none() || self.filtered().is_some() || self.sel + 5 < self.items.len() {
            return;
        }
        let Some(f) = &mut self.found else { return };
        let (tab, id) = (f.tab, f.list_id);
        let Some(token) = f.next[tab].take() else { return }; // taken: asked once
        let (yt, tx, query) = (self.yt.clone(), self.tx.clone(), self.query.clone());
        std::thread::spawn(move || {
            let page = yt.page(tab, &query, Some(&token));
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                let Some(f) = app.found.as_mut().filter(|f| f.list_id == id) else { return };
                match page {
                    Ok((rows, next)) => {
                        let have: HashSet<String> = f.lists[tab].iter().map(|t| t.id.clone()).collect();
                        let new: Vec<Item> = rows.into_iter().filter(|t| !have.contains(&t.id)).collect();
                        f.next[tab] = next.filter(|_| !new.is_empty());
                        f.lists[tab].extend(new.clone());
                        if f.tab == tab && app.list_id == id {
                            app.items.extend(new);
                        }
                    }
                    Err(e) => {
                        f.next[tab] = Some(token);
                        app.msg = tr!("Loading more failed: {e}", e = e);
                    }
                }
            })));
        });
    }
}
