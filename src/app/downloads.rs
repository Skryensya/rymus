// Songs kept as files: what b asks for (or every like, in Config), one yt-dlp at a time, and played from disk from
// then on, with or without a network.
use crate::i18n::tr;
use super::*;
use crate::disk;
use std::path::Path;

// What has to be typed to free 90 % or more of the downloads at once.
pub const FREE_ALL_WORDS: &str = "delete my downloads";

#[derive(Default)]
pub struct Downloads {
    pub files: std::collections::HashMap<String, PathBuf>, // song id -> the file
    queue: std::collections::VecDeque<Item>,
    current: Option<String>, // the id being fetched
    pub own: Vec<Item>,      // your own music in the folder, when Config says to read it
    pub used: u64,           // what the folder takes, in bytes
    pub open: bool,          // the Downloads window (B, or Config)
    pub sel: usize,          // its row: SETTINGS, then BUTTONS
}

// The Downloads window: these settings change with ← →, then the buttons run with ⏎.
pub const SETTINGS: [&str; 3] = ["download", "dlformat", "library"];
pub const BUTTONS: [(&str, &str); 4] = [
    ("folder", "where songs are downloaded to, and your own music is read from; what is already there stays"),
    ("now", "the song that plays now; b downloads the highlighted song anywhere, m → Download too"),
    ("liked", "everything you have liked that is not on disk yet, one song at a time"),
    ("free", "deletes the least played share of the songs rymus downloaded (you type the %), after showing what it frees; your own files are never touched"),
];

// A name any file system takes, from text that comes from outside: no separators, no yt-dlp placeholders, not long.
fn file_name(t: &Item) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_control() || "/\\:*?\"<>|%".contains(c) { ' ' } else { c }).collect::<String>();
    let mut name = format!("{} - {}", clean(&t.artist), clean(&t.title)).trim_start_matches([' ', '.', '-']).to_string();
    while name.len() > 150 {
        name.pop();
    }
    let id: String = t.id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).collect();
    format!("{} [{id}]", name.trim())
}

fn songs(n: usize) -> String {
    if n == 1 { tr!("1 song").into() } else { tr!("{n} songs", n = n) }
}

// The lyrics of a downloaded song, saved beside it (Song.lrc or Song.txt) when LRCLIB has them; looked up by the
// file's own length, the surest match. Network work: called off the main thread.
fn save_lyrics(t: &Item, file: &Path) {
    #[cfg(not(test))]
    {
        let length = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
            .arg(file)
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        crate::lyrics::save_beside(file, &crate::lyrics::fetch(t, length));
    }
    #[cfg(test)]
    let _ = (t, file); // no network in tests: the file writing has tests of its own (lyrics.rs)
}

// The cover beside a downloaded song: "Song.jpg" large (1200 px when YouTube has it) and "Song.thumb.jpg", the
// small one lists use. The extension follows what was sent (jpg, png or webp). Their paths, where saved.
pub(super) fn save_covers(t: &Item, file: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    #[cfg(test)]
    {
        let _ = (t, file); // no network in tests
        (None, None)
    }
    #[cfg(not(test))]
    {
        if t.cover.is_empty() {
            return (None, None);
        }
        let (dir, stem) = (file.parent().unwrap_or(Path::new(".")), file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        let get = |url: &str, name: &dyn Fn(&str) -> String| -> Option<PathBuf> {
            use std::io::Read;
            let mut bytes = Vec::new();
            crate::yt::agent().get(url).call().ok()?.into_reader().take(20_000_000).read_to_end(&mut bytes).ok()?;
            let ext = match bytes.get(..12)? {
                [0x89, b'P', b'N', b'G', ..] => "png",
                [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P'] => "webp",
                [0xFF, 0xD8, ..] => "jpg",
                _ => return None, // not an image
            };
            let to = dir.join(name(ext));
            std::fs::write(&to, bytes).ok().map(|_| to)
        };
        let large = crate::cover::large(&t.cover).unwrap_or_else(|| t.cover.clone());
        (get(&large, &|ext| format!("{stem}.{ext}")), get(&t.cover, &|ext| format!("{stem}.thumb.{ext}")))
    }
}
fn has_cover(file: &Path) -> bool {
    ["jpg", "png", "webp"].iter().any(|ext| file.with_extension(ext).is_file())
}

// Runs yt-dlp; the file it wrote, or why not.
fn fetch(t: &Item, folder: &Path, format: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| tr!("cannot make {a}: {e}", a = folder.display(), e = e))?;
    let template = format!("{}.%(ext)s", folder.to_string_lossy().replace('%', "%%") + "/" + &file_name(t));
    #[cfg(test)]
    {
        let ext = if format == "original" { "webm" } else { format };
        let file = PathBuf::from(template.replace("%%", "%").replace("%(ext)s", ext));
        std::fs::write(&file, "audio").map_err(|e| e.to_string())?;
        Ok(file)
    }
    #[cfg(not(test))]
    {
        let mut cmd = std::process::Command::new("yt-dlp");
        cmd.args(["--no-playlist", "--no-progress", "--embed-metadata", "-f", "bestaudio/best", "--print", "after_move:filepath", "-o", &template]);
        if format != "original" {
            cmd.args(["-x", "--audio-format", format, "--audio-quality", "0"]);
        }
        let out = cmd.arg(track_url(t)).stdin(std::process::Stdio::null()).output().map_err(|e| format!("yt-dlp: {e}"))?;
        let file = String::from_utf8_lossy(&out.stdout).lines().last().map(PathBuf::from);
        match file.filter(|f| out.status.success() && f.is_file()) {
            Some(file) => Ok(file),
            None => Err(String::from_utf8_lossy(&out.stderr).lines().rev().find(|l| l.starts_with("ERROR")).unwrap_or(tr!("yt-dlp failed")).to_string()),
        }
    }
}

impl Downloads {
    // Songs still to come: the one on its way and the ones after it.
    pub fn waiting(&self) -> usize {
        self.queue.len() + usize::from(self.current.is_some())
    }
}

impl App {
    pub(super) fn load_downloads(&mut self) {
        self.downloads.files = self.db.downloads().into_iter().filter(|(_, f)| f.is_file()).collect();
        // songs downloaded before lyrics were saved with them get theirs, one at a time, in the background
        // songs downloaded before their lyrics and cover were saved with them get them, one at a time, in the background
        let missing: Vec<(Item, PathBuf)> = self.db.download_files().into_iter().filter(|(_, f)| f.is_file() && (!crate::lyrics::has_beside(f) || !has_cover(f))).collect();
        if !missing.is_empty() {
            std::thread::spawn(move || {
                for (t, f) in &missing {
                    if !crate::lyrics::has_beside(f) {
                        save_lyrics(t, f);
                    }
                    if !has_cover(f) {
                        save_covers(t, f);
                    }
                }
            });
        }
    }
    pub fn downloaded(&self, id: &str) -> bool {
        self.downloads.files.contains_key(id)
    }
    // Where songs are kept: your folder, else Music/rymus.
    pub fn download_folder(&self) -> PathBuf {
        match self.db.get_setting("dlfolder").filter(|f| !f.is_empty()) {
            Some(folder) => data::expand(&folder),
            None if cfg!(test) => self.db_path.parent().map(|d| d.join("music")).unwrap_or_default(),
            None => data::expand("~/Music/rymus"),
        }
    }
    // b: the highlighted song (every song of an album's page from its buttons), or what plays.
    pub(super) fn download_here(&mut self) {
        let hidden = self.full || self.lyrics;
        let tracks = match self.cur().filter(|_| !hidden) {
            Some(t) if t.kind == Kind::Track => vec![t.clone()],
            Some(t) if t.kind == Kind::Button => self.tracks_from(0),
            _ => self.now().cloned().into_iter().collect(),
        };
        self.download(tracks, true);
    }
    pub(super) fn download_liked(&mut self) {
        let liked = self.db.liked();
        self.download(liked, true);
    }
    // After a like, when Config says to keep every like.
    pub(super) fn download_like(&mut self, t: &Item) {
        if config::value(&self.db, "download") == "liked songs" {
            self.download(vec![t.clone()], false);
        }
    }
    pub(super) fn download(&mut self, tracks: Vec<Item>, tell: bool) {
        let before = self.downloads.queue.len();
        for t in tracks {
            let waiting = self.downloads.queue.iter().any(|q| q.id == t.id) || self.downloads.current.as_deref() == Some(t.id.as_str());
            if t.kind == Kind::Track && !t.id.is_empty() && !waiting && !self.downloads.files.get(&t.id).is_some_and(|f| f.is_file()) {
                self.downloads.queue.push_back(t);
            }
        }
        let added = self.downloads.queue.len() - before;
        if tell {
            self.set_note(match added {
                0 => tr!("Already downloaded (or on its way)").into(),
                1 => tr!("Downloading 1 song").into(),
                n => tr!("Downloading {n} songs", n = n),
            });
        }
        self.next_download();
    }
    fn next_download(&mut self) {
        if self.downloads.current.is_some() {
            return;
        }
        let Some(t) = self.downloads.queue.pop_front() else { return };
        self.downloads.current = Some(t.id.clone());
        let (tx, folder, format) = (self.tx.clone(), self.download_folder(), config::value(&self.db, "dlformat"));
        self.hook("download-started", &t, json!({"folder": folder.to_string_lossy(), "format": format, "waiting": self.downloads.queue.len()}));
        std::thread::spawn(move || {
            let got = fetch(&t, &folder, format);
            // what goes with the song: its lyrics and its cover, both beside it
            let extras = got.as_ref().ok().map(|file| {
                save_lyrics(&t, file);
                let (cover, thumbnail) = save_covers(&t, file);
                let lyrics = ["lrc", "txt"].iter().map(|ext| file.with_extension(ext)).find(|f| f.is_file());
                (cover, thumbnail, lyrics)
            });
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                app.downloads.current = None;
                match got {
                    Ok(file) => {
                        app.db.add_download(&t, &file);
                        app.downloads.files.insert(t.id.clone(), file.clone());
                        let (cover, thumbnail, lyrics) = extras.unwrap_or_default();
                        let path = |f: Option<PathBuf>| f.map(|f| f.to_string_lossy().into_owned());
                        app.hook("downloaded", &t, json!({"path": file.to_string_lossy(), "format": format, "cover": path(cover), "thumbnail": path(thumbnail), "lyrics": path(lyrics)}));
                        let left = app.downloads.queue.len();
                        let more = if left > 0 { tr!(" · {left} to go", left = left) } else { String::new() };
                        app.set_note(tr!("Downloaded: {a}{more}", a = t.title, more = more));
                        app.scan_library(); // the folder's size
                    }
                    Err(e) => {
                        app.hook("download-failed", &t, json!({"error": e}));
                        app.msg = tr!("Could not download {a}: {e}", a = t.title, e = e);
                    }
                }
                app.next_download();
            })));
        });
    }
    pub(super) fn ask_delete_download(&mut self, t: Item) {
        let items = vec![(tr!("Keep it").to_string(), Action::Nothing), (tr!("Delete the file").to_string(), Action::DeleteDownload(t.id.clone()))];
        self.menu = Some(Menu { title: tr!("delete the download of {a}?", a = t.title), items, sel: 0 });
    }
    pub(crate) fn delete_download(&mut self, id: &str) {
        if id.starts_with(disk::OWN) {
            return; // your own music: rymus never deletes it
        }
        self.remove_download(id, "you");
        self.set_note(tr!("Download deleted").into());
    }
    // Deletes the file of a song rymus downloaded and forgets it; the bytes it took. `reason`: "you" or "free up space".
    fn remove_download(&mut self, id: &str, reason: &str) -> u64 {
        let song = self.db.downloaded_tracks().into_iter().find(|t| t.id == id);
        let mut bytes = 0;
        if let Some(file) = self.downloads.files.remove(id) {
            // the song and what was saved with it: lyrics, cover, thumbnail
            let mut gone = vec![file.clone()];
            gone.extend(["lrc", "txt", "jpg", "png", "webp", "thumb.jpg", "thumb.png", "thumb.webp"].iter().map(|ext| file.with_extension(ext)).filter(|f| f.is_file()));
            for f in &gone {
                bytes += std::fs::metadata(f).map_or(0, |m| m.len());
                let _ = std::fs::remove_file(f);
            }
            if let Some(t) = &song {
                let files: Vec<String> = gone.iter().map(|f| f.to_string_lossy().into_owned()).collect();
                self.hook("download-deleted", t, json!({"path": file.to_string_lossy(), "reason": reason, "bytes": bytes, "files": files}));
            }
        }
        self.db.drop_download(id);
        bytes
    }

    // Reads your own music from the folder (in the background), when Config says to: every audio file rymus did not
    // download plays from disk and shows in Library.
    pub(super) fn scan_library(&mut self) {
        let on = config::value(&self.db, "library") == "on";
        let (tx, folder) = (self.tx.clone(), self.download_folder());
        let ours: HashSet<PathBuf> = self.db.downloads().into_iter().map(|d| d.1).collect();
        std::thread::spawn(move || {
            let all = disk::files(&folder);
            let used = all.iter().map(|f| f.1).sum();
            let own: Vec<Item> = if on { all.into_iter().filter(|f| disk::is_audio(&f.0) && !ours.contains(&f.0)).map(|f| disk::own_track(&f.0)).collect() } else { vec![] };
            let _ = tx.send(Msg::Apply(Box::new(move |app: &mut App| {
                app.downloads.used = used;
                app.downloads.files.retain(|id, _| !id.starts_with(disk::OWN));
                for t in &own {
                    app.downloads.files.insert(t.id.clone(), PathBuf::from(&t.id[disk::OWN.len()..]));
                }
                app.downloads.own = own;
                app.followed_changed();
            })));
        });
    }
    // Free up space: first how much, typed (the last one offered again).
    pub(super) fn ask_free_cut(&mut self) {
        let last = self.db.get_setting("freecut").unwrap_or_else(|| "25".into());
        self.start_naming(Naming::FreeCut, last.trim_end_matches([' ', '%']).to_string());
    }
    pub(crate) fn free_cut(&mut self, text: &str) {
        match text.trim().trim_end_matches('%').trim().parse::<usize>() {
            Ok(pct) if (1..=100).contains(&pct) => {
                self.db.set_setting("freecut", &pct.to_string());
                self.ask_free_space(pct);
            }
            _ => self.set_note(tr!("Free up space takes a share from 1 to 100 %").into()),
        }
    }
    // Then what goes: the least played `pct` % of rymus's downloads, said before anything is deleted.
    pub(crate) fn ask_free_space(&mut self, pct: usize) {
        let gone = disk::least_played(&self.db.download_plays(), pct);
        if gone.is_empty() {
            return self.set_note(tr!("Nothing to free: {pct} % of your downloads is not a single song", pct = pct));
        }
        let bytes: u64 = gone.iter().filter_map(|g| std::fs::metadata(&g.1).ok()).map(|m| m.len()).sum();
        let most = gone.iter().map(|g| g.2).max().unwrap_or(0);
        let ids: Vec<String> = gone.iter().map(|g| g.0.clone()).collect();
        let what = tr!("Delete {a}, {b} (played {most} times at most)", a = songs(ids.len()), b = disk::human(bytes), most = most);
        // nearly all of them: one Enter is not enough, the phrase has to be typed (see FREE_ALL_WORDS)
        let delete = if pct >= 90 { Action::ConfirmFreeSpace(ids) } else { Action::FreeSpace(ids) };
        let items = vec![(tr!("Keep them").to_string(), Action::Nothing), (what, delete)];
        self.menu = Some(Menu { title: tr!("free up space: the least played {pct} % of your downloads?", pct = pct), items, sel: 0 });
    }
    // The typed confirmation of a Free up space of 90 % or more.
    pub(super) fn free_all(&mut self, ids: Vec<String>, typed: &str) {
        if typed.trim().to_lowercase() == FREE_ALL_WORDS {
            self.free_space(ids);
        } else {
            self.set_note(tr!("Nothing was deleted: type \"{w}\" to free that much", w = FREE_ALL_WORDS));
        }
    }
    pub(super) fn free_space(&mut self, ids: Vec<String>) {
        let mut freed = 0;
        for id in ids.iter().filter(|id| !id.starts_with(disk::OWN)) {
            freed += self.remove_download(id, tr!("free up space"));
        }
        self.scan_library(); // the folder's size
        self.followed_changed();
        self.set_note(tr!("Freed {a} · {b} deleted", a = disk::human(freed), b = songs(ids.len())));
    }

    pub(super) fn open_downloads(&mut self) {
        self.show_lists();
        self.typing = false;
        self.eq.open = false;
        self.downloads.open = true;
        self.downloads.sel = 0;
        self.scan_library(); // the size, fresh
    }
    pub(super) fn close_downloads(&mut self) {
        self.downloads.open = false;
    }
    // The window takes every key but play/pause, volume and the key list (false: left to the app).
    pub(super) fn downloads_key(&mut self, code: KeyCode) -> bool {
        use KeyCode as K;
        let sel = self.downloads.sel;
        let last = SETTINGS.len() + BUTTONS.len() - 1;
        match code {
            K::Char(' ' | '+' | '-' | '=' | '?') => return false,
            K::Up | K::Char('k') => self.downloads.sel = sel.saturating_sub(1),
            K::Down | K::Char('j') => self.downloads.sel = (sel + 1).min(last),
            K::Left | K::Char('h') | K::Right | K::Char('l') if sel < SETTINGS.len() => {
                let dir = if matches!(code, K::Left | K::Char('h')) { -1 } else { 1 };
                let v = config::step(&self.db, SETTINGS[sel], dir);
                self.apply_setting(SETTINGS[sel], v);
            }
            K::Char('b') => self.run_download_button("now"),
            K::Enter if sel >= SETTINGS.len() => self.run_download_button(BUTTONS[sel - SETTINGS.len()].0),
            K::Esc | K::Backspace | K::Char('q' | 'B') => self.close_downloads(),
            _ => {}
        }
        true
    }
    fn run_download_button(&mut self, id: &str) {
        match id {
            "folder" => self.run(Action::DownloadFolder),
            "now" => match self.now().cloned() {
                Some(t) => self.download(vec![t], true),
                None => self.set_note(tr!("Nothing plays · b on any song downloads it").into()),
            },
            "liked" => self.download_liked(),
            _ => self.ask_free_cut(),
        }
    }
}
