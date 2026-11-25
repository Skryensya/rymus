// One row of any list. Every row says what it is; fields that do not apply stay empty.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Track,
    Artist,
    Album,
    ArtistStat,
    AlbumStat,
    Discover,
    Mix,
    Resume,
    Header,
}

#[derive(Clone, Debug)]
pub enum Spec {
    All,
    Style(Vec<String>),
    RadioTrack(Box<Item>),
    RadioArtist(String),
}

#[derive(Clone, Debug, Default)]
pub struct Item {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: String,
    pub author: String,
    pub year: String,
    pub detail: Option<String>,
    pub plays: i64,
    pub likes: i64,
    pub seconds: i64,
    pub rank: usize,
    pub fresh: bool,
    pub spec: Option<Spec>,
    pub uid: u64, // identity of a queue entry: the same song can be queued twice
}

impl Item {
    pub fn header(title: &str) -> Item {
        Item { kind: Kind::Header, title: title.into(), ..Default::default() }
    }
    pub fn main_artist(&self) -> &str {
        self.artist.split(", ").next().unwrap_or("")
    }
}

// Text from outside ends up on the terminal: drop control characters so it cannot send escape sequences.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}
