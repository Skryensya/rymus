// The Config tab: its sections on the left, the settings of the one holding the cursor on the right, and under the
// setting being changed, all its choices and what it does.
use crate::i18n::tr;
use super::*;
use crate::config;

impl App {
    pub(super) fn config_body(&mut self, v: &View, g: &mut Geo) -> Vec<String> {
        let (w, iw, l, a) = (v.w, v.iw, v.l, v.a);
        let data = std::mem::take(&mut self.settings);
        let (sel, in_menu) = (self.sel, self.config_menu);
        // the sections: (heading row, its title); the one holding the cursor is shown
        let sections: Vec<(usize, &str)> = data.iter().enumerate().filter(|(_, r)| r.kind == Kind::Header).map(|(i, r)| (i, r.title.as_str())).collect();
        let cur = sections.iter().rposition(|&(i, _)| i <= sel).unwrap_or(0);
        let first = sections.get(cur).map_or(0, |s| s.0 + 1);
        let last = sections.get(cur + 1).map_or(data.len(), |s| s.0);

        // the menu on the left, or one line over the settings when the window is narrow
        let mw = sections.iter().map(|s| width(s.1)).max().unwrap_or(0) + 4;
        let side = iw >= mw + 44;
        let (x0, rw) = if side { (mw + 2, iw - mw - 1) } else { (2, iw) };
        let ls = if side { mw + 4 } else { 3 }; // the screen column each line starts at
        let mut menu: Vec<String> = vec![];
        let mut menu_map: Vec<usize> = vec![];
        if side {
            for (k, &(i, title)) in sections.iter().enumerate() {
                // the cursor is here while ↑ ↓ go through the sections; inside one, its name stays lit
                menu.push(match (k == cur, in_menu) {
                    (true, true) => format!("{} {}", fg(a, "▌"), fg(a, &bold(title))),
                    (true, false) => format!("  {}", fg(a, title)),
                    _ => format!("  {title}"),
                });
                menu_map.push(i + 1); // a click opens it at its first row
            }
        }
        let lw = data.iter().filter(|r| r.kind == Kind::Setting).map(|r| width(&r.title)).max().unwrap_or(0) + 3;

        // the section's rows, with what the one under the cursor does right below it
        let (mut lines, mut index) = (Vec::new(), Vec::new());
        if !side {
            let names: Vec<String> = sections.iter().enumerate().map(|(k, s)| if k == cur { fg(a, &bold(s.1)) } else { dim(s.1) }).collect();
            lines.push(names.join(&dim(" · ")));
            index.push(usize::MAX);
            lines.push(String::new());
            index.push(usize::MAX);
        }
        let mut clicks: Vec<(usize, usize, usize, Action)> = vec![]; // (line, x0, x1, what): each choice is clickable
        let mut at = 0;
        for (i, it) in data.iter().enumerate().take(last).skip(first) {
            let on = i == sel && !in_menu;
            let color = if it.id == "wipe" { RED } else { a };
            if on {
                at = lines.len();
            }
            if it.kind == Kind::Button {
                // an action, not a choice: ▸ says it opens or does something
                let text = format!("▸ {}", it.title);
                lines.push(if on { format!("{} {}", fg(color, "▌"), fg(color, &bold(&text))) } else if it.id == "wipe" { format!("  {}", fg(RED, &text)) } else { format!("  {text}") });
            } else {
                let label = cell(&it.title, lw);
                let all = config::choices(&it.id);
                let toggle = all.len() == 2 && all.contains(&"on") && all.contains(&"off");
                let value = if toggle {
                    // a switch: ● on, ○ off; a click flips it
                    let yes = it.author == "on";
                    let text = if yes { format!("● {}", crate::i18n::t("on")) } else { format!("○ {}", crate::i18n::t("off")) };
                    clicks.push((lines.len(), ls + 2 + lw, ls + 2 + lw + width(&text), Action::Choose(it.id.clone(), if yes { "off" } else { "on" })));
                    if yes { fg(a, &bold(&text)) } else { dim(&text) }
                } else if on {
                    // the one being changed shows every choice
                    let (chips, spans) = chips(&it.id, &it.author, true, rw.saturating_sub(lw + 4), a);
                    for (c0, c1, v) in spans {
                        clicks.push((lines.len(), ls + 2 + lw + c0, ls + 2 + lw + c1, Action::Choose(it.id.clone(), v)));
                    }
                    chips
                } else {
                    shown(&it.id, &it.author)
                };
                lines.push(if on { format!("{} {}{value}", fg(a, "▌"), fg(a, &bold(&label))) } else { format!("  {label}{value}") });
            }
            index.push(i);
            if on {
                let hint = it.detail.clone().unwrap_or_default();
                let mut text = crate::lyrics::wrap(&capital(&hint), rw.saturating_sub(6).max(10));
                text.truncate(3);
                for t in text {
                    lines.push(format!("    {}", dim(&t)));
                    index.push(i);
                }
                if it.id == "fade" && l >= last - first + 8 {
                    for t in crossfade_picture(config::fade_secs(&it.author), rw.saturating_sub(6), a) {
                        lines.push(format!("    {t}"));
                        index.push(i);
                    }
                }
            }
        }

        // scrolls only when the section is taller than the window
        let top = at.saturating_sub(l / 2).min(lines.len().saturating_sub(l));
        for (line, c0, c1, what) in clicks.into_iter().filter(|c| c.0 >= top && c.0 < top + l) {
            g.links.push((what, c0, c1, g.top + line - top));
        }
        let mut map: Vec<usize> = index.into_iter().skip(top).take(l).collect();
        map.resize(l, usize::MAX);
        let mut rows: Vec<String> = lines.into_iter().skip(top).take(l).collect();
        rows.resize(l, String::new());
        let rows: Vec<String> = rows
            .into_iter()
            .enumerate()
            .map(|(r, line)| {
                let left = if side { pad_styled(&format!(" {}", menu.get(r).map_or("", |m| m.as_str())), mw) + &dim("│") } else { String::new() };
                pad_styled(&format!("{left} {line}"), iw)
            })
            .collect();
        self.settings = data;
        if side {
            menu_map.resize(l, usize::MAX);
            g.panes.push(Pane { rows: Some(menu_map), ..pane(PaneId::List, 1, mw + 1, 0, 0) });
        }
        g.panes.push(Pane { rows: Some(map), ..pane(PaneId::List, x0, w, 0, 0) });
        let (t, c) = v.title(if in_menu { tr!("Config · ↑ ↓ section · → open it") } else { tr!("Config · ↑ ↓ choose · ← → change · esc sections") });
        frame(&t, c, &rows, w, a)
    }
}

// A setting's chosen value as text, with its color dot or border corner.
fn shown(key: &str, v: &str) -> String {
    let i = config::choices(key).iter().position(|c| *c == v).unwrap_or(0);
    format!("{}{}", mark(key, v, i), if key == "language" { capital(v) } else { crate::i18n::t(v).to_string() })
}
fn mark(key: &str, c: &str, i: usize) -> String {
    match key {
        "accent" => config::accent(c).map_or_else(|| "◐ ".to_string(), |rgb| format!("{} ", fg(readable(rgb), "●"))),
        "border" => crate::ui::BORDERS.get(i).map_or(String::new(), |b| format!("{}{} ", b.corners[0], b.top.chars().next().unwrap_or(' '))),
        _ => String::new(),
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
            let mark = mark(key, c, i);
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

