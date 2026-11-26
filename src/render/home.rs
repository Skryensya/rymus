// Home's two columns (Listen, Library), each a list of sections that keep their names in sight.
use super::*;

impl App {
    // Home: Listen and Library side by side, or one column when the screen is narrow.
    pub(super) fn home_body(&mut self, v: &View, mq: &mut Marquee, g: &mut Geo) -> Vec<String> {
        let (w, l, a, playing) = (v.w, v.l, v.a, v.playing.as_deref());

        // two columns need room for both lists to read; below that, Home is one column
        let wide = w >= 90;
        if wide != self.home_wide {
            self.home_wide = wide;
            self.load_home();
            self.clamp_sel();
        }
        // a column with nothing in it gives its room to the other one
        let has = [!self.home_left.is_empty(), !self.home_right.is_empty()];
        let shared = (w * 58).div_ceil(100);
        let widths = match has {
            [true, false] | [false, false] => [w, 0],
            [false, true] => [0, w],
            _ => [shared, w - shared],
        };
        let mut columns: Vec<Vec<String>> = Vec::new();
        let mut x0 = 1;
        for (column, id) in [PaneId::Left, PaneId::Right].into_iter().enumerate() {
            let cw = widths[column];
            if cw == 0 {
                continue;
            }
            let focused = self.home_focus == id;
            let sel = if focused { self.sel } else { self.home_sel[column] };
            // the list is lent to the row builder and given back
            let list = std::mem::take(if column == 0 { &mut self.home_left } else { &mut self.home_right });
            let headers: Vec<bool> = list.iter().map(|t| t.kind == Kind::Header).collect();
            let shown = crate::home::window(&headers, l, Some(sel));
            let visible: Vec<Item> = shown.iter().map(|&i| list[i].clone()).collect();
            let at = shown.iter().position(|&i| i == sel).unwrap_or(usize::MAX);
            let mut links = vec![];
            let rows = self.list_rows(mq, &visible, at, 0, cw.saturating_sub(2), l, focused, a, playing, "", &mut links);
            g.links.extend(links.into_iter().map(|(act, a0, a1, y)| (act, a0 + x0 + 1, a1 + x0 + 1, y + g.top)));
            if column == 0 {
                self.home_left = list;
            } else {
                self.home_right = list;
            }
            let name = self.home_titles[column].clone();
            let (t, c) = if focused { v.title(&name) } else { (name, txt()) };
            columns.push(frame(&t, c, &rows, cw, if focused { a } else { gray() }));
            g.panes.push(Pane { rows: Some(shown), ..pane(id, x0, x0 + cw - 1, 0, 0) });
            x0 += cw;
        }
        let first = columns.remove(0);
        match columns.pop() {
            Some(second) => first.into_iter().zip(second).map(|(x, y)| x + &y).collect(),
            None => first,
        }
    }
}
