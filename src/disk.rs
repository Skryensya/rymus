// The music folder on disk: the audio files in it, the room they take, and which downloads go first to free some.
use crate::item::{Item, Kind};
use std::path::{Path, PathBuf};

const AUDIO: [&str; 11] = ["mp3", "m4a", "opus", "flac", "ogg", "oga", "wav", "webm", "aac", "aiff", "wma"];

// The id a file of your own goes by: never a YouTube id, so nothing looks it up there, and it is never deleted.
pub const OWN: &str = "file:";

// Every file under `dir` and its size, subfolders included (not through links, so a loop cannot trap it).
pub fn files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = vec![];
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                todo.push(e.path());
            } else if kind.is_file() {
                out.push((e.path(), e.metadata().map_or(0, |m| m.len())));
            }
        }
    }
    out
}

pub fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{} MB", b >> 20),
        b if b >= 1 << 10 => format!("{} KB", b >> 10),
        b => format!("{b} B"),
    }
}

// A song from a file of your own: "Artist - Title.flac" when it is named so, else the file name, with its folder
// as the album.
pub fn own_track(p: &Path) -> Item {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (artist, title) = match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), stem.clone()),
    };
    let album = p.parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    Item { kind: Kind::Track, id: format!("{OWN}{}", p.to_string_lossy()), title, artist, album, ..Default::default() }
}

// The downloads to delete for `pct` percent of them, least played first (the oldest first among equals).
// `plays` comes sorted that way already.
pub fn least_played<T: Clone>(plays: &[T], pct: usize) -> Vec<T> {
    plays[..plays.len() * pct / 100].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_its_songs_and_what_goes_first() {
        let dir = std::env::temp_dir().join(format!("rymus-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Album")).unwrap();
        std::fs::write(dir.join("Album/Band - Song.FLAC"), "1234").unwrap();
        std::fs::write(dir.join("cover.jpg"), "12").unwrap();
        let all = files(&dir);
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().map(|f| f.1).sum::<u64>(), 6);
        let songs: Vec<_> = all.iter().filter(|f| is_audio(&f.0)).collect();
        assert_eq!(songs.len(), 1);
        let t = own_track(&songs[0].0);
        assert_eq!((t.artist.as_str(), t.title.as_str(), t.album.as_str()), ("Band", "Song", "Album"));
        assert!(t.id.starts_with(OWN));
        assert_eq!(human(1536 << 20), "1.5 GB");
        assert_eq!(least_played(&[1, 2, 3, 4, 5, 6, 7, 8], 25), [1, 2]);
        assert_eq!(least_played(&[1, 2, 3], 10), Vec::<i32>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
