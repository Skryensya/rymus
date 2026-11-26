// The Config tab: each group under its heading, every setting with all its choices in view (the chosen one
// marked), and below, what the setting under the cursor does, in full.
use crate::i18n::tr;
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
        let mut clicks: Vec<(usize, usize, usize, Action)> = vec![]; // (line, x0, x1, what): each choice is clickable
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
                let (chips, spans) = chips(&it.id, &it.author, on, iw.saturating_sub(lw + 6), a);
                for (x0, x1, v) in spans {
                    clicks.push((lines.len(), 5 + lw + x0, 5 + lw + x1, Action::Choose(it.id.clone(), v)));
                }
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
            let head = if it.kind == Kind::Button { it.title.trim_end_matches('…').to_string() } else { format!("{} · {}", it.title, crate::i18n::t(&it.author)) };
            about.push(fg(if it.id == "wipe" { RED } else { a }, &bold(&head)));
            let hint = it.detail.clone().unwrap_or_default();
            let mut chars = hint.chars();
            let hint: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default(); // capitalised
            let mut text = crate::lyrics::wrap(&hint, pw);
            text.truncate(if side { 4 } else { 2 });
            about.extend(text);
            // the list scrolls, so the explanation may take what is left after a dozen rows of it
            let room = if side { l } else { l.saturating_sub(lines.len().min(12)) };
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
        for (line, x0, x1, what) in clicks.into_iter().filter(|c| c.0 >= top && c.0 < top + n) {
            g.links.push((what, x0, x1, g.top + line - top));
        }
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
        let (t, c) = v.title(tr!("Config · ↑ ↓ choose · ← → change"));
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
            format!("{} {}", dim(tr!("this song")), fg(a, &"█".repeat(half.saturating_sub(10)))),
            format!("{} {}{}", dim(tr!("next song")), spaces(half.saturating_sub(10)), "█".repeat(half.saturating_sub(10))),
            dim(tr!("one ends, the next starts: no overlap")),
        ];
    }
    let fade = ((secs / 12.0) * 16.0).round().max(2.0) as usize; // columns the overlap takes, up to 16
    let solid = w.saturating_sub(fade + 12) / 2;
    let down: String = (0..fade).map(|i| RAMP[i * RAMP.len() / fade]).collect();
    let up: String = (0..fade).map(|i| RAMP[RAMP.len() - 1 - i * RAMP.len() / fade]).collect();
    let pad = solid;
    vec![
        format!("{} {}{}", dim(tr!("this song")), fg(a, &"█".repeat(solid)), fg(a, &down)),
        format!("{} {}{up}{}", dim(tr!("next song")), spaces(pad), "█".repeat(solid)),
        format!("{}{}", spaces(10 + pad), dim(&tr!("╰{a}╯ {b} s together", a = "─".repeat(fade.saturating_sub(2)), b = secs))),
    ]
}

fn pad_to(s: &str, w: usize) -> String {
    let n = width(&plain(s));
    if n > w { cut(&plain(s), w) } else { format!("{s}{}", spaces(w - n)) }
}

// A setting's choices on one line of at most `room` columns, the chosen one marked: colors with a dot of their color,
// borders with a corner of theirs. When they do not all fit, the ones around the chosen one, with ‹ › for the rest.
// Returns the line and where each choice landed (x0, x1 from the line's start, its value).
fn chips(key: &str, chosen: &str, on: bool, room: usize, a: Rgb) -> (String, Vec<(usize, usize, &'static str)>) {
    let all = config::choices(key);
    let at = all.iter().position(|c| *c == chosen).unwrap_or(0);
    let shown: Vec<(String, String)> = all
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let name = if key == "language" { capital(c) } else { crate::i18n::t(c).to_string() };
            let mark = match key {
                "accent" => config::accent(c).map_or_else(|| "◐ ".to_string(), |rgb| format!("{} ", fg(readable(rgb), "●"))),
                "border" => crate::ui::BORDERS.get(i).map_or(String::new(), |b| format!("{}{} ", b.corners[0], b.top.chars().next().unwrap_or(' '))),
                _ => String::new(),
            };
            let plain_text = format!("{}{name}", plain(&mark));
            let text = if i == at { fg(a, &bold(&format!("[{mark}{name}]"))) } else if on { format!(" {mark}{name} ") } else { dim(&format!(" {} ", plain_text)) };
            (text, plain_text)
        })
        .collect();
    let w = |i: usize| width(&shown[i].1) + 3; // the brackets or spaces, and the gap after
    // grow around the chosen one while it fits, two columns kept for ‹ ›
    let (mut lo, mut hi, mut used) = (at, at + 1, w(at));
    loop {
        let grew = hi < shown.len() && used + w(hi) + 4 <= room && { used += w(hi); hi += 1; true };
        let grew_left = lo > 0 && used + w(lo - 1) + 4 <= room && { lo -= 1; used += w(lo); true };
        if !grew && !grew_left {
            break;
        }
    }
    let (mut line, mut x, mut spans) = (String::new(), 0, vec![]);
    if lo > 0 {
        line += &dim("‹ ");
        x += 2;
    }
    for (i, (text, plain_text)) in shown.iter().enumerate().take(hi).skip(lo) {
        spans.push((x, x + width(plain_text) + 1, all[i]));
        line += text;
        line += " ";
        x += width(plain_text) + 3;
    }
    if hi < shown.len() {
        line += &dim("›");
    }
    (line, spans)
}

fn capital(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
}
