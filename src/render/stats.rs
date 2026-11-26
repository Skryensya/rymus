// The Stats tab: totals, pickers, charts and the ranked list.
use crate::i18n::tr;
use super::*;

impl App {
    // Stats: minutes listened, the period and category pickers, the charts (when they fit) and the ranked list.
    pub(super) fn stats_body(&mut self, v: &View, mq: &mut Marquee, g: &mut Geo) -> Vec<String> {
        let (w, iw, l, a, playing) = (v.w, v.iw, v.l, v.a, v.playing.as_deref());

        // charts between the summary and the list, when there is room for both
        let (tl, labels) = &self.timeline;
        let side = iw >= 80; // the hours chart beside the timeline
        let lw = if side { iw * 3 / 5 } else { iw.saturating_sub(2) };
        let hw = iw.saturating_sub(lw + 5);
        let chart = if l >= 16 && lw >= tl.len() && tl.iter().any(|&s| s > 0) {
            let h = if l >= 30 { 5 } else { 3 };
            let left = charts::bars(tl, labels, lw, h, a);
            let right = if side && hw >= 24 { Some(charts::bars(&self.hours, &charts::hour_labels(), hw, h, a)) } else { None };
            let per = if PERIODS[self.period].1 > 31 { tr!("Per month") } else { tr!("Per day") };
            let mut rows = vec![if right.is_some() { format!(" {}   {} ", dim(&cell(per, lw)), dim(&cell(tr!("By hour of day"), hw))) } else { format!(" {} ", dim(&cell(per, iw - 2))) }];
            for (i, row) in left.into_iter().enumerate() {
                rows.push(match &right {
                    Some(r) => format!(" {row}   {} ", r[i]),
                    None => format!(" {row} "),
                });
            }
            rows.push(spaces(iw));
            rows
        } else {
            vec![]
        };
        let n = l.saturating_sub(2 + chart.len()).max(1);
        let data = std::mem::take(&mut self.stat_rows);
        let top = self.sel.saturating_sub(n / 2).min(data.len().saturating_sub(n));
        let label = if iw < 70 { tr!("Listened") } else { tr!("Minutes listened") };
        let totals: Vec<String> = PERIODS.iter().enumerate().map(|(i, p)| format!("{} {}", crate::i18n::t(p.0), minutes(self.totals[i]))).collect();
        let total = cell(&format!(" {label} · {}", totals.join(" · ")), iw);
        let pick = |names: &[&str], on: usize| {
            names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let t = capital_t(name);
                    if i == on { format!("[{t}]") } else { format!(" {t} ") }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        // the keys sit next to what they change
        let periods: Vec<&str> = PERIODS.iter().map(|p| p.0).collect();
        let left = format!(" {}  ←→", pick(&CATS, self.stat_cat));
        let right = format!("t  {} ", pick(&periods, self.period));
        let mut selector = cell(&format!("{left}{}{right}", spaces(iw.saturating_sub(width(&left) + width(&right)).max(2))), iw);
        for (names, on) in [(&CATS[..], self.stat_cat), (&periods[..], self.period)] {
            let name = names[on];
            let chosen = format!("[{}]", capital_t(name));
            selector = selector.replacen(&chosen, &fg(a, &bold(&chosen)), 1);
        }
        let (t, c) = v.title(&tr!("Stats · top 100 {a} · last {b} days", a = crate::i18n::t(CATS[self.stat_cat]), b = PERIODS[self.period].1));
        let sel = self.sel;
        let mut rows = vec![bold(&total), selector];
        rows.extend(chart.iter().cloned());
        let mut links = vec![];
        rows.extend(self.list_rows(mq, &data, sel, top, iw, n, true, a, playing, tr!("Nothing listened in this period yet."), &mut links));
        let first = g.top + 2 + chart.len();
        g.links.extend(links.into_iter().filter(|link| 2 + chart.len() + link.3 < l).map(|(act, x0, x1, y)| (act, x0 + 2, x1 + 2, y + first)));
        rows.truncate(l);
        self.stat_rows = data;
        let above = 2 + chart.len(); // summary rows and charts above the list
        g.panes.push(pane(PaneId::List, 1, w, top as i64 - above as i64, above));
        frame(&t, c, &rows, w, a)
    }
}

// A name in the chosen language, with its first letter in capitals.
fn capital_t(name: &str) -> String {
    capital(crate::i18n::t(name))
}
