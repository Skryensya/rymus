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
    Playlist,
    NewPlaylist,
    Query,     // a past search
    Setting,   // a row of the Config tab
    Button,    // play (id "play") or shuffle (id "shuffle") the songs of the list it is in
    Header,
}

// How an entry got into the queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    #[default]
    List, // part of what was played: an album, a playlist, a mix
    Added, // put there on purpose: plays before the rest, and survives playing something else
    Radio, // similar songs that keep the queue going
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
    pub likes: i64,
    pub seconds: i64,
    pub rank: usize,
    pub fresh: bool,
    pub spec: Option<Spec>,
    pub uid: u64, // identity of a queue entry: the same song can be queued twice
    pub origin: Origin,
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
