// The album page's right column: the cover with the album's details under it.
use super::*;

impl App {
    // The album page's right column, l rows of exactly c + 2 columns: the cover (c columns, r rows) and the album's
    // title, artist, year and song count under it. With kitty the cover is a real image at (row, col), set in img,
    // and its rows stay blank.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn album_side(&mut self, al: &Item, songs: usize, l: usize, r: usize, row: usize, col: usize, img: &mut Option<Placed>) -> Vec<String> {
        let c = 2 * r;
        let art = match self.cover(&al.cover).filter(|_| !al.cover.is_empty()) {
            Some(got) if self.kitty => {
                *img = Some((got, al.cover.clone(), row, col, c));
                None
            }
            Some(got) => self.cells(&al.cover, &got, c, r),
            None => None,
        };
        let mut side: Vec<String> = (0..r).map(|i| art.as_ref().and_then(|a| a.get(i)).cloned().unwrap_or_else(|| spaces(c))).collect();
        side.push(spaces(c));
        side.extend(crate::lyrics::wrap(&al.title, c).into_iter().take(2).map(|t| bold(&cell(&t, c))));
        if al.kind == Kind::Artist {
            side.push(dim(&cell("Artist", c)));
        } else {
            side.push(cell(&al.author, c));
            let year = if al.year.is_empty() { String::new() } else { format!("{} · ", al.year) };
            side.push(dim(&cell(&format!("{year}{songs} songs"), c)));
        }
        side.resize(l, spaces(c));
        side.truncate(l);
        side.into_iter().map(|s| format!(" {s} ")).collect()
    }
}
