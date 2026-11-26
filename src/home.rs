// Home's columns are lists of sections, each a header and its rows (built in App::load_home).

// Which rows of a column to draw in a height of h, so that every block stays in sight. `headers` says which rows are
// block names (each starts a block). Short blocks take what they need, the rest share what is left, and each one
// scrolls on its own, keeping its name on top and the selected row in view.
pub fn window(headers: &[bool], h: usize, sel: Option<usize>) -> Vec<usize> {
    let n = headers.len();
    let mut starts: Vec<usize> = (0..n).filter(|&i| i == 0 || headers[i]).collect();
    starts.push(n);
    let blocks: Vec<(usize, usize)> = starts.windows(2).map(|w| (w[0], w[1])).collect();
    // everything fits, or the screen is too small to give every block two rows: one list, scrolled as a whole
    if n <= h || h < 2 * blocks.len() {
        let top = sel.unwrap_or(0).saturating_sub(h / 2).min(n.saturating_sub(h));
        return (top..n.min(top + h)).collect();
    }
    let mut heights = vec![0; blocks.len()];
    let mut open: Vec<usize> = (0..blocks.len()).collect();
    let mut left = h;
    while !open.is_empty() {
        let share = left / open.len();
        let small: Vec<usize> = open.iter().copied().filter(|&b| blocks[b].1 - blocks[b].0 <= share).collect();
        if small.is_empty() {
            for (k, &b) in open.iter().enumerate() {
                heights[b] = share + usize::from(k < left % open.len());
            }
            break;
        }
        for &b in &small {
            heights[b] = blocks[b].1 - blocks[b].0;
            left -= heights[b];
        }
        open.retain(|b| !small.contains(b));
    }
    let mut out = Vec::new();
    for (&(start, end), &height) in blocks.iter().zip(&heights) {
        let head = usize::from(headers[start]);
        out.extend(start..start + head);
        let (first, room) = (start + head, height - head);
        let at = sel.filter(|&s| s >= first && s < end).map_or(0, |s| s - first);
        let top = first + at.saturating_sub(room / 2).min((end - first).saturating_sub(room));
        out.extend(top..end.min(top + room));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_share_the_height() {
        // a list of 20 under the title, then a named list of 3 (rows 20..24)
        let headers: Vec<bool> = (0..24).map(|i| i == 20).collect();
        let shown = window(&headers, 10, Some(0));
        assert_eq!(shown, [0, 1, 2, 3, 4, 5, 20, 21, 22, 23]); // the short one is whole, the long one gets the rest
        assert!(window(&headers, 10, Some(19)).starts_with(&[14, 15, 16, 17, 18, 19, 20])); // it scrolls alone
        // two long lists split the height, and the second keeps its name while scrolling to the selection
        let headers: Vec<bool> = (0..40).map(|i| i == 20).collect();
        let shown = window(&headers, 10, Some(39));
        assert_eq!(shown, [0, 1, 2, 3, 4, 20, 36, 37, 38, 39]);
        assert_eq!(window(&headers, 100, None).len(), 40);
        assert_eq!(window(&headers, 3, Some(30)).len(), 3); // too small: one list
        assert!(window(&[], 5, None).is_empty());
    }
}
