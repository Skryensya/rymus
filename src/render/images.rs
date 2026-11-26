// Real cover images through the kitty graphics protocol (Ghostty, kitty, WezTerm).
use super::*;

impl App {
    // The real cover images (kitty graphics), sent only when one changes: image 1 is the player's, image 2 the album
    // page's. Returns the escape codes to write after the text.
    pub(super) fn send_images(&mut self, image: Option<(usize, usize, usize)>, head_img: Option<Placed>, shown_url: &str, got: Option<Arc<Got>>) -> String {
    // real cover image: only re-sent when it changes; the old one stays until the new one has loaded
    let mut img = String::new();
    if !self.viewer.shown.is_empty() {
        img.push_str(&cover::delete(3)); // the viewer just closed
        self.viewer.shown.clear();
    }
    if !self.kitty && !self.shown.is_empty() {
        img.push_str(&cover::delete(1)); // pixel art was just turned on
        self.shown.clear();
    }
    // the album page's cover, image 2
    let head_key = head_img.as_ref().map_or(String::new(), |(_, url, row, col, cols)| format!("{url}|{cols}|{row}|{col}"));
    if head_key != self.shown_head {
        img.push_str(&match &head_img {
            Some((got, _, row, col, cols)) => cover::place(&got.b64, *row, *col, *cols, 2),
            None => cover::delete(2),
        });
        self.shown_head = head_key;
    }
    if self.kitty {
                    let key = match image {
            Some((row, col, cols)) if !shown_url.is_empty() => format!("{shown_url}|{cols}|{row}|{col}"),
            _ => String::new(),
        };
        if (key.is_empty() || got.is_some()) && key != self.shown {
            img.push_str(&match (key.is_empty(), image, &got) {
                (false, Some((row, col, cols)), Some(got)) => cover::place(&got.b64, row, col, cols, 1),
                _ => cover::delete(1),
            });
            self.shown = key;
        }
    }
        img
    }
}
