// The Library tab: new releases of the artists you follow (F), the albums you saved (liking one saves it) and the
// artists themselves.
use crate::i18n::tr;
use super::*;

const CHECK_EVERY: i64 = 12 * 3600 * 1000;

fn release(a: &Value) -> Item {
    let text = |k: &str| a[k].as_str().unwrap_or("").to_string();
    Item { kind: Kind::Album, id: text("id"), title: text("title"), author: text("author"), year: text("year"), cover: text("cover"), fresh: true, ..Default::default() }
}

impl App {
    pub(super) fn load_library(&mut self) {
        let follows = self.db.follows();
        let fresh: Vec<Item> = follows.iter().flat_map(|f| f.2.as_array().into_iter().flatten().map(release)).collect();
        self.releases = fresh.len();
        let (downloaded, own) = (self.db.downloaded_tracks(), self.downloads.own.clone());
        self.library = grouped(vec![(tr!("New releases"), fresh), (tr!("Albums"), self.db.saved_albums()), (tr!("Artists"), follows.into_iter().map(|f| f.0).collect()), (tr!("Downloaded"), downloaded), (tr!("Your music"), own)]);
    }
    // F: follow the highlighted artist, or the one whose page you are on; again to stop.
    pub(super) fn toggle_follow(&mut self) {
        let row = self.cur().filter(|i| i.kind == Kind::Artist && !i.id.is_empty()).cloned();
        let page = || self.head.clone().filter(|h| self.tab == Tab::Search && h.kind == Kind::Artist && !h.id.is_empty());
        let Some(artist) = row.or_else(page) else { return self.set_note(tr!("F follows an artist: select one, or open their page").into()) };
        if self.db.follows().iter().any(|f| f.0.id == artist.id) {
            self.db.unfollow(&artist.id);
            self.followed_changed();
            return self.set_note(tr!("No longer following {a}", a = artist.title));
        }
        // what they have now is not news: remember it
        let yt = self.yt.clone();
        self.run_busy(tr!("Following {a}…", a = artist.title), move || {
            let page = yt.artist(&artist.id, &artist.title)?;
            Ok(Box::new(move |app: &mut App| {
                let known: Vec<&String> = page.albums.iter().map(|a| &a.id).collect();
                let cover = if page.picture.is_empty() { &artist.cover } else { &page.picture };
                app.db.follow(&artist.id, &artist.title, cover, &json!(known));
                app.followed_changed();
                app.set_note(tr!("Following {a} · new releases show in Library", a = artist.title));
            }) as Apply)
        });
    }
    pub(super) fn followed_changed(&mut self) {
        let sel = self.sel;
        self.load_library();
        if self.tab == Tab::Library {
            self.sel = sel;
            self.clamp_sel();
        }
    }
    // Looks for albums the followed artists did not have before; at most twice a day unless forced.
    pub(super) fn check_releases(&mut self, force: bool) {
        let last: i64 = self.db.get_setting("follows_checked").and_then(|v| v.parse().ok()).unwrap_or(0);
        let follows = self.db.follows();
        if follows.is_empty() || (!force && now_ms() - last < CHECK_EVERY) {
            return;
        }
        self.db.set_setting("follows_checked", &now_ms().to_string());
        let (yt, tx) = (self.yt.clone(), self.tx.clone());
        std::thread::spawn(move || {
            for (artist, known, fresh) in follows {
                // one that cannot be read is tried again next time
                let Ok(page) = yt.artist(&artist.id, &artist.title) else { continue };
                let had: HashSet<&str> = known.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let new: Vec<Value> = page.albums.iter().filter(|a| !had.contains(a.id.as_str()))
                    .map(|a| json!({"id": a.id, "title": a.title, "author": a.author, "year": a.year, "cover": a.cover})).collect();
                if new.is_empty() {
                    continue;
                }
                let mut all_known: Vec<&str> = had.into_iter().collect();
                all_known.extend(page.albums.iter().map(|a| a.id.as_str()));
                all_known.sort();
                all_known.dedup();
                let known = json!(all_known);
                let fresh: Vec<Value> = new.into_iter().chain(fresh.as_array().cloned().unwrap_or_default()).take(10).collect();
                let id = artist.id.clone();
                let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                    app.db.set_releases(&id, &known, &json!(fresh));
                    app.followed_changed();
                })));
            }
        });
    }
    // Opening a new release takes it off the list of news.
    pub(super) fn saw_release(&mut self, album: &str) {
        for (artist, known, fresh) in self.db.follows() {
            let list = fresh.as_array().cloned().unwrap_or_default();
            let rest: Vec<Value> = list.iter().filter(|a| a["id"] != album).cloned().collect();
            if rest.len() != list.len() {
                self.db.set_releases(&artist.id, &known, &json!(rest));
                self.load_library();
            }
        }
    }
    // d on the Library tab: forget a saved album or stop following, after asking.
    pub(super) fn ask_forget(&mut self) {
        let Some(it) = self.cur().cloned() else { return };
        let (what, action) = match it.kind {
            Kind::Artist => (tr!("Stop following"), Action::Unfollow(it.id.clone())),
            Kind::Album if !it.fresh => (tr!("Remove from library"), Action::DropAlbum(it.id.clone())),
            Kind::Album => (tr!("Dismiss"), Action::DropRelease(it.id.clone())),
            _ => return,
        };
        let items = vec![(tr!("Keep it").to_string(), Action::Nothing), (what.to_string(), action)];
        self.menu = Some(Menu { title: it.title, items, sel: 0 });
    }
    pub(super) fn forget(&mut self, action: Action) {
        match action {
            Action::Unfollow(id) => self.db.unfollow(&id),
            Action::DropAlbum(id) => self.db.drop_album(&id), // its songs stay liked
            Action::DropRelease(id) => self.saw_release(&id),
            _ => return,
        }
        self.followed_changed();
    }
}
