// The cover viewer (Shift+click on a cover): the image as large as the terminal can show it, a margin around it, over
// everything else. With kitty graphics it is the real image, sharp; elsewhere half blocks, as big as they go.
use super::*;

const MARGIN: (usize, usize) = (2, 1); // columns, rows left free around the window
const PAD: (usize, usize) = (2, 1); // and inside it, around the image

impl App {
    pub(super) fn viewer_screen(&mut self, w: usize, h: usize) -> String {
        let url = self.viewer.url.clone().unwrap_or_default();
        // the sharpest copy there is; the one already loaded shows until it arrives
        let big = cover::large(&url).unwrap_or_else(|| url.clone());
        let (src, got) = match self.cover(&big) {
            Some(got) => (big, Some(got)),
            None => (url.clone(), self.cover(&url)),
        };
        let a = self.theme.color;
        self.geo = Geo::default(); // nothing under the window answers clicks

        let (ww, wh) = (w.saturating_sub(2 * MARGIN.0), h.saturating_sub(2 * MARGIN.1 + 1)); // the last row: the keys
        let (iw, il) = (ww.saturating_sub(2), wh.saturating_sub(2));
        let (room_c, room_r) = (iw.saturating_sub(2 * PAD.0), il.saturating_sub(2 * PAD.1));
        let kitty = self.kitty;
        let (cols, rows) = match got.as_deref().and_then(png_size) {
            // a kitty image takes the cell size in pixels; half blocks are two pixels a cell, one wide
            Some(px) => fit(px, if kitty { cell_px() } else { (1, 2) }, room_c, room_r),
            None => (0, 0),
        };
        let (x, y) = ((room_c - cols) / 2, (room_r - rows) / 2); // centred in its room
        let art = match &got {
            Some(got) if !kitty && cols > 0 => self.cells(&src, got, cols, rows),
            _ => None,
        };

        let mut inner = vec![spaces(iw); il];
        match (&got, &art) {
            (None, _) => inner[il / 2] = dim(&center(tr!("Loading the cover…"), iw)),
            (Some(_), Some(art)) => {
                for (i, line) in art.iter().enumerate() {
                    if let Some(row) = inner.get_mut(PAD.1 + y + i) {
                        *row = format!("{}{line}{}", spaces(PAD.0 + x), spaces(iw - PAD.0 - x - cols));
                    }
                }
            }
            _ => {} // kitty: the image goes over blank rows
        }
        let window = frame(tr!("Cover · Esc or a click closes"), a, &inner, ww, a);
        let mut lines = vec![spaces(w); MARGIN.1];
        lines.extend(window.into_iter().map(|r| format!("{}{r}{}", spaces(MARGIN.0), spaces(w - MARGIN.0 - ww))));
        lines.resize(h - 1, spaces(w));
        lines.push(hints(&[(tr!("esc"), tr!("close")), (tr!("space"), tr!("play / pause"))], w, a));
        self.theme.paint(&mut lines);

        // the covers of the screen under it go, and come back when it closes
        let mut img = String::new();
        for (id, shown) in [(1, &mut self.shown), (2, &mut self.shown_head)] {
            if !shown.is_empty() {
                img.push_str(&cover::delete(id));
                shown.clear();
            }
        }
        if kitty {
            // 1-indexed: the margin, the frame's border, the padding, then the centring
            let (row, col) = (MARGIN.1 + 2 + PAD.1 + y, MARGIN.0 + 2 + PAD.0 + x);
            let key = format!("{src}|{row}|{col}|{cols}");
            if let Some(got) = got.filter(|_| cols > 0 && key != self.viewer.shown) {
                img.push_str(&cover::place(&got.b64, row, col, cols, 3));
                self.viewer.shown = key;
            }
        }
        format!("\x1b[H{}{img}", lines.join("\r\n"))
    }
}

// Width and height of a PNG, from its header.
fn png_size(got: &Got) -> Option<(usize, usize)> {
    use base64::Engine;
    let head = base64::engine::general_purpose::STANDARD.decode(got.b64.get(..32)?).ok()?;
    let at = |i: usize| head.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    Some((at(16)?, at(20)?)).filter(|&(w, h)| head.starts_with(b"\x89PNG") && w > 0 && h > 0)
}

// A terminal cell in pixels, (width, height); 1:2 when the terminal does not say.
fn cell_px() -> (usize, usize) {
    match crossterm::terminal::window_size() {
        Ok(s) if s.width > 0 && s.height > 0 && s.columns > 0 && s.rows > 0 => ((s.width / s.columns) as usize, (s.height / s.rows) as usize),
        _ => (1, 2),
    }
}

// The most columns and rows an image of `px` pixels takes in `cols` × `rows` cells of `cell` pixels, its shape kept.
fn fit(px: (usize, usize), cell: (usize, usize), cols: usize, rows: usize) -> (usize, usize) {
    let rows_for = |c: usize| (c * cell.0 * px.1).div_ceil(px.0 * cell.1);
    let c = if rows_for(cols) <= rows { cols } else { rows * cell.1 * px.0 / (px.1 * cell.0) };
    (c.min(cols), rows_for(c).min(rows))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_fills_the_room_its_shape_allows() {
        // square image, cells twice as tall as wide: 40 columns take 20 rows
        assert_eq!(fit((1200, 1200), (8, 16), 100, 20), (40, 20));
        assert_eq!(fit((1200, 1200), (8, 16), 30, 40), (30, 15)); // narrow: the width decides
        assert_eq!(fit((1600, 900), (1, 2), 80, 40), (80, 23)); // wide image in half blocks
    }
}
