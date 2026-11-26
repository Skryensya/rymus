// The Config tab: each group under its heading, every setting with all its choices in view (the chosen one
// marked), and below, what the setting under the cursor does, in full.
use super::*;
use crate::config;
use super::eq::plain;

impl App {
    pub(super) fn config_body(&mut self, v: &View, g: &mut Geo) -> Vec<String> {
        let (w, iw, l, a) = (v.w, v.iw, v.l, v.a);
        let data = std::mem::take(&mut self.settings);
        let sel = self.sel;
        let lw = data.iter().filter(|r| r.kind == Kind::Setting).map(|r| width(&r.title)).max().unwrap_or(0) + 4;

        // the list: a blank line before each heading but the first; `index` is the item each row stands for
        let (mut lines, mut index) = (Vec::new(), Vec::new());
        for (i, it) in data.iter().enumerate() {
            if it.kind == Kind::Header {
                if i > 0 {
                    lines.push(String::new());
                    index.push(i);
                }
                lines.push(dim(&bold(&format!("  {}", it.title.to_uppercase()))));
            } else {
                let on = i == sel;
                if it.kind == Kind::Button {
                    // an action, not a choice: wipe is in red
                    let color = if it.id == "wipe" { RED } else { a };
                    lines.push(if on { format!("{} {}", fg(color, "▌"), fg(color, &bold(&it.title))) } else if it.id == "wipe" { format!("  {}", fg(RED, &it.title)) } else { format!("  {}", it.title) });
                    index.push(i);
                    continue;
                }
                let label = cell(&it.title, lw);
                let chips: String = config::choices(&it.id)
                    .iter()
                    .map(|c| if *c == it.author { fg(a, &bold(&format!("[{c}]"))) } else if on { format!(" {c} ") } else { dim(&format!(" {c} ")) })
                    .collect::<Vec<_>>()
                    .join(" ");
                // too narrow for them all: only the chosen one, still changed with ← →
                let chips = if 4 + lw + width(&plain(&chips)) > iw { fg(a, &bold(&format!("‹ {} ›", it.author))) } else { chips };
                lines.push(if on { format!("{} {}{chips}", fg(a, "▌"), fg(a, &bold(&label))) } else { format!("  {label}{chips}") });
            }
            index.push(i);
        }

        // the one under the cursor, in full: beside the list when there is room, else under it
        let list_w = lines.iter().map(|r| width(&plain(r))).max().unwrap_or(0) + 3;
        let side = iw >= list_w + 36;
        let pw = if side { iw - list_w - 3 } else { iw.saturating_sub(4) };
        let mut about = vec![];
        if let Some(it) = data.get(sel).filter(|it| matches!(it.kind, Kind::Setting | Kind::Button)) {
            if !side {
                about.push(dim(&"─".repeat(pw)));
            }
            let head = if it.kind == Kind::Button { it.title.trim_end_matches('…').to_string() } else { format!("{} · {}", it.title, it.author) };
            about.push(fg(if it.id == "wipe" { RED } else { a }, &bold(&head)));
            let hint = it.detail.clone().unwrap_or_default();
            let mut text = crate::lyrics::wrap(&format!("{}{}", hint[..1].to_uppercase(), &hint[1..]), pw);
            text.truncate(if side { 4 } else { 2 });
            about.extend(text);
            let room = if side { l } else { l.saturating_sub(lines.len()) };
            if it.id == "fade" && room >= about.len() + 4 + usize::from(!side) {
                about.push(String::new());
                about.extend(crossfade_picture(config::fade_secs(&it.author), pw, a));
            }
        }

        // the list scrolls to keep the cursor in sight
        let n = if side { l } else { l.saturating_sub(about.len() + 1).max(1) };
        let at = index.iter().position(|&i| i == sel).unwrap_or(0);
        let top = at.saturating_sub(n / 2).min(lines.len().saturating_sub(n));
        let mut rows: Vec<String> = lines.iter().skip(top).take(n).map(|r| format!(" {r}")).collect();
        let mut map: Vec<usize> = index.iter().skip(top).take(n).copied().collect();
        if side {
            rows.resize(l, String::new());
            for (i, line) in about.into_iter().enumerate() {
                rows[i] = format!("{}{line}", pad_to(&rows[i], list_w + 1));
            }
        } else {
            rows.resize(l.saturating_sub(about.len()), String::new());
            rows.extend(about.into_iter().map(|r| format!("  {r}")));
        }
        map.resize(rows.len(), usize::MAX);
        let rows: Vec<String> = rows.into_iter().take(l).map(|r| pad_to(&r, iw)).collect();
        self.settings = data;
        g.panes.push(Pane { rows: Some(map), ..pane(PaneId::List, 1, if side { list_w + 1 } else { w }, 0, 0) }); // the explanation is not a row
        let (t, c) = v.title("Config · ↑ ↓ choose · ← → change");
        frame(&t, c, &rows, w, a)
    }
}

// Two songs overlapping for the chosen seconds: one fading out over the start of the next.
fn crossfade_picture(secs: f64, w: usize, a: Rgb) -> Vec<String> {
    const RAMP: [&str; 8] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁"];
    let w = w.min(60);
    if secs <= 0.0 {
        let half = w / 2;
        return vec![
            format!("{} {}", dim("this song"), fg(a, &"█".repeat(half.saturating_sub(10)))),
            format!("{} {}{}", dim("next song"), spaces(half.saturating_sub(10)), "█".repeat(half.saturating_sub(10))),
            dim("one ends, the next starts: no overlap"),
        ];
    }
    let fade = ((secs / 12.0) * 16.0).round().max(2.0) as usize; // columns the overlap takes, up to 16
    let solid = w.saturating_sub(fade + 12) / 2;
    let down: String = (0..fade).map(|i| RAMP[i * RAMP.len() / fade]).collect();
    let up: String = (0..fade).map(|i| RAMP[RAMP.len() - 1 - i * RAMP.len() / fade]).collect();
    let pad = solid;
    vec![
        format!("{} {}{}", dim("this song"), fg(a, &"█".repeat(solid)), fg(a, &down)),
        format!("{} {}{up}{}", dim("next song"), spaces(pad), "█".repeat(solid)),
        format!("{}{}", spaces(10 + pad), dim(&format!("╰{}╯ {} s together", "─".repeat(fade.saturating_sub(2)), secs))),
    ]
}

fn pad_to(s: &str, w: usize) -> String {
    let n = width(&plain(s));
    if n > w { cut(&plain(s), w) } else { format!("{s}{}", spaces(w - n)) }
}
