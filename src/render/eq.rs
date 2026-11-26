// The equalizer's window: the profiles on the left (built in, then yours); on the right a slider per band on a dB
// scale, what range each band is in, and what the band (or the profile) under the cursor does; its keys at the foot.
use crate::i18n::tr;
use super::*;
use crate::app::Action;
use crate::eq::{self, BANDS, FREQS, LIMIT, REGIONS};

const SCALE: usize = 5; // "+12 ┤"

impl App {
    // The window drawn over `behind` (the body frame as it was), which stays dimmed around it.
    pub(super) fn eq_window(&mut self, v: &View, g: &mut Geo, behind: Vec<String>) -> Vec<String> {
        let a = v.a;
        let (x, y, ww, wh, screen_row) = window_at(v, g, behind.len(), 110);
        let (iw, l) = (ww.saturating_sub(2), wh.saturating_sub(2));

        let (sel, band) = (self.eq.sel, self.eq.band);
        let all = self.eq_presets();
        let (current, gains) = self.eq_current();
        let mine = all.iter().find(|p| p.name == current && p.custom);
        let list_l = l.saturating_sub(1); // the keys take the last row

        // left: the profiles under two headings; None marks a row that is not a profile
        let lw = if iw < 100 { 18 } else { 24 }.min(iw / 3); // a narrow screen gives the sliders the room
        let mut entries: Vec<(String, Option<usize>)> = vec![(tr!(" BUILT IN").into(), None)];
        for (i, p) in all.iter().enumerate() {
            if p.custom && (i == 0 || !all[i - 1].custom) {
                entries.push((tr!(" YOURS").into(), None));
            }
            entries.push((format!("{} {}", if p.name == current { "●" } else { " " }, crate::i18n::any(&p.name)), Some(i)));
        }
        entries.push((tr!("  + New profile").into(), Some(all.len())));
        let at = entries.iter().position(|e| e.1 == Some(sel)).unwrap_or(0);
        let top = (at + 1).saturating_sub(list_l).min(entries.len().saturating_sub(list_l));
        let left: Vec<String> = (0..list_l)
            .map(|i| match entries.get(top + i) {
                None => spaces(lw),
                Some((name, None)) => dim(&bold(&cell(name, lw))),
                Some((name, Some(n))) => {
                    g.links.push((Action::PickEq(*n), x + 3, x + 2 + lw, screen_row + 1 + i));
                    if *n == sel && band.is_none() {
                        selected(a, &cell(name, lw.saturating_sub(1)))
                    } else if all.get(*n).is_some_and(|p| p.name == current) {
                        fg(a, &cell(&format!(" {name}"), lw))
                    } else {
                        cell(&format!(" {name}"), lw)
                    }
                }
            })
            .collect();

        // right: the sliders, then what they mean
        let gw = iw.saturating_sub(lw + 4);
        let col = gw.saturating_sub(SCALE) / FREQS.len();
        let right: Vec<String> = if col < 4 {
            vec![dim(&cell(tr!("Widen the window to see the sliders."), gw))]
        } else {
            let mut r = sliders(&gains, band, col, list_l.saturating_sub(6), a);
            r.push(format!("{}{}", spaces(SCALE), FREQS.iter().enumerate().map(|(i, f)| mark(&center(&eq::freq_label(*f), col), band == Some(i), a)).collect::<String>()));
            r.push(format!("{}{}", dim(&format!("{:>w$} ", "dB", w = SCALE - 1)), gains.iter().enumerate().map(|(i, g)| {
                let t = center(&if *g == 0 { "0".into() } else { format!("{g:+}") }, col);
                if band == Some(i) { fg(a, &bold(&t)) } else if *g == 0 { dim(&t) } else { t }
            }).collect::<String>()));
            r.push(format!("{}{}", spaces(SCALE), dim(&regions(col))));
            r.push(String::new());
            // the explanation: the band you move, or the profile you hear
            let (title, about) = match band {
                Some(b) => (tr!("{a} Hz · {b} · {c} dB", a = eq::freq_label(FREQS[b]), b = crate::i18n::t(BANDS[b].0), c = format!("{:+}", gains[b])), cap(crate::i18n::t(BANDS[b].1))),
                None => {
                    let whose = mine.map_or(tr!("built in").to_string(), |p| {
                        let edited = if p.gains == eq::built_in(&p.base) { tr!("same as") } else { tr!("edited from") };
                        tr!("yours · {edited} {a}", edited = edited, a = p.base)
                    });
                    (format!("{current} · {whose}"), eq::describe(&gains))
                }
            };
            r.push(fg(a, &bold(&cut(&title, gw))));
            r.push(cut(&about, gw));
            r
        };
        let mut rows: Vec<String> = (0..list_l)
            .map(|i| {
                let side = right.get(i).cloned().unwrap_or_default();
                format!(" {} {} {}", left.get(i).cloned().unwrap_or_else(|| spaces(lw)), dim("│"), pad_visible(&side, gw))
            })
            .collect();
        // what works here, said where you are looking
        let keys: &[(&str, &str)] = match (band, mine.is_some()) {
            (None, true) => &[("⏎", "edit"), ("r", "reset"), ("d", "delete"), ("n", "new"), ("0", "flat"), ("esc", "close")],
            (None, false) => &[("↑↓", "choose"), ("n", tr!("new from this")), ("0", "flat"), ("esc", "close")],
            (Some(_), true) => &[("←→", "band"), ("↑↓", "gain"), ("0", "zero"), ("r", "reset"), ("d", "delete"), ("esc", "back")],
            (Some(_), false) => &[("←→", "band"), ("n", tr!("new from this to edit it")), ("esc", "back")],
        };
        rows.push(keys_line(keys, iw, a));
        rows.truncate(l);

        let (title, color) = v.title(&tr!("EQ · {current}", current = current)); // notes and errors show here too
        overlay(behind, &frame(&title, color, &rows, ww, a), x, y, v.w)
    }
}

// Where a window over the body sits: centred, at most `max` wide. (x, y, width, height, its top row on screen)
pub(super) fn window_at(v: &View, g: &mut Geo, rows: usize, max: usize) -> (usize, usize, usize, usize, usize) {
    let w = v.w;
    let ww = if w < 80 { w } else { (w - 8).min(max) };
    let wh = rows.min(26);
    let (x, y) = ((w - ww) / 2, (rows - wh) / 2);
    let screen_row = g.top - 1 + y; // the body frame's border is the row above its first one
    g.window = Some((x + 1, x + ww, screen_row, screen_row + wh - 1));
    (x, y, ww, wh, screen_row)
}

// The window drawn over what is behind it, which dims.
pub(super) fn overlay(behind: Vec<String>, window: &[String], x: usize, y: usize, w: usize) -> Vec<String> {
    let ww = window.first().map_or(0, |r| width(&plain(r)));
    behind
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let under = plain(&row);
            match i.checked_sub(y).and_then(|j| window.get(j)) {
                None => dim(&under),
                Some(win) => {
                    let before = pad_visible(take(&under, x), x);
                    let rest: String = under.chars().skip(take(&under, x + ww).chars().count()).collect();
                    format!("{}{win}{}", dim(&before), dim(&pad_visible(&rest, w.saturating_sub(x + ww))))
                }
            }
        })
        .collect()
}

// The keys that work, at the foot of a window.
pub(super) fn keys_line(keys: &[(&str, &str)], iw: usize, a: Rgb) -> String {
    let (mut foot, mut used) = (" ".to_string(), 1);
    for (key, what) in keys.iter().map(|(key, what)| (crate::i18n::t(key), crate::i18n::t(what))) {
        let n = width(key) + width(what) + 4;
        if used + n > iw {
            break;
        }
        foot += &format!("{} {}   ", fg(a, &bold(key)), dim(what));
        used += n;
    }
    format!("{foot}{}", spaces(iw.saturating_sub(used)))
}

// One slider per band: a track from -LIMIT to +LIMIT dB, filled from 0 to the knob. `h` rows at most, kept odd so
// 0 dB has a row of its own in the middle.
fn sliders(gains: &[i64], band: Option<usize>, col: usize, h: usize, a: Rgb) -> Vec<String> {
    let h = (h.max(5) - 1) / 2 * 2 + 1;
    let mid = h / 2;
    let per_row = LIMIT as f64 / mid as f64;
    let kw = if col >= 7 { 3 } else { 1 }; // the knob's width
    let (lpad, rpad) = ((col - kw) / 2, col - kw - (col - kw) / 2);
    let knob_row = |g: i64| (mid as f64 - g as f64 / per_row).round().clamp(0.0, (h - 1) as f64) as usize;
    (0..h)
        .map(|r| {
            let level = (mid as f64 - r as f64) * per_row;
            let labelled = r == 0 || r == mid || r == h - 1 || (h >= 9 && (r == mid / 2 || r == mid + (mid + 1) / 2));
            let axis = if r == mid { "┼" } else if labelled { "┤" } else { "│" };
            let num = if !labelled { String::new() } else if r == mid { "0".into() } else { format!("{:+.0}", level) };
            let mut line = dim(&format!("{num:>3} {axis}"));
            for (i, g) in gains.iter().enumerate() {
                let (k, on) = (knob_row(*g), band == Some(i));
                let filled = (r > k && r < mid) || (r < k && r > mid);
                let (fill, side) = if r == mid { ("─", "─") } else { (" ", " ") };
                let piece = if r == k {
                    let knob = "━".repeat(kw);
                    if on { fg(a, &bold(&format!("{}{knob}{}", arrow(lpad, "▸", side), arrow(rpad, "◂", side)))) } else { format!("{}{}{}", dim(&side.repeat(lpad)), bold(&knob), dim(&side.repeat(rpad))) }
                } else if filled {
                    format!("{}{}{}", spaces(lpad), fg(a, &"█".repeat(kw)), spaces(rpad))
                } else {
                    let track = if kw == 3 { format!("{fill}{}{fill}", if r == mid { "┼" } else { "│" }) } else { (if r == mid { "┼" } else { "│" }).to_string() };
                    let t = format!("{}{track}{}", side.repeat(lpad), side.repeat(rpad));
                    if on { fg(a, &t) } else { dim(&t) }
                };
                line += &piece;
            }
            line
        })
        .collect()
}

// The pointers either side of the knob being moved, in the room beside it.
fn arrow(room: usize, glyph: &str, fill: &str) -> String {
    if room == 0 { String::new() } else if glyph == "▸" { format!("{}{glyph}", fill.repeat(room - 1)) } else { format!("{glyph}{}", fill.repeat(room - 1)) }
}

// "╰── bass ──╯╰─ mids ─╯╰ treble ╯" under the bands each range takes.
fn regions(col: usize) -> String {
    REGIONS
        .iter()
        .map(|(name, n)| {
            let inner = (n * col).saturating_sub(2);
            let label = format!(" {} ", crate::i18n::t(name));
            if width(&label) > inner {
                return format!("╰{}╯", "─".repeat(inner));
            }
            let l = (inner - width(&label)) / 2;
            format!("╰{}{label}{}╯", "─".repeat(l), "─".repeat(inner - width(&label) - l))
        })
        .collect()
}

fn center(s: &str, w: usize) -> String {
    let n = width(s).min(w);
    let l = (w - n) / 2;
    format!("{}{}{}", spaces(l), cut(s, w), spaces(w - n - l))
}
fn mark(s: &str, on: bool, a: Rgb) -> String {
    if on { fg(a, &bold(s)) } else { dim(s) }
}
fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

// Pads or cuts styled text to `w` visible columns.
fn pad_visible(s: &str, w: usize) -> String {
    let n = width(&plain(s));
    format!("{s}{}", spaces(w.saturating_sub(n)))
}

// The text without its colour codes.
pub(super) fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            let _ = chars.by_ref().find(|c| c.is_ascii_alphabetic());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sliders_line_up() {
        let rows = sliders(&[12, 0, -12, 6, 0, 0, 0, 0, 0, 0], Some(0), 8, 13, [200, 100, 50]);
        assert_eq!(rows.len(), 13);
        let plain: Vec<String> = rows.iter().map(|r| plain(r)).collect();
        assert!(plain.iter().all(|r| width(r) == SCALE + 80), "every row as wide as the bands: {plain:#?}");
        assert!(plain[0].starts_with("+12 ┤") && plain[6].starts_with("  0 ┼") && plain[12].starts_with("-12 ┤"));
        assert!(plain[0][..].contains("▸━━━◂"), "the band being moved has its knob at +12: {}", plain[0]);
        assert_eq!(width(&regions(8)), 80);
    }
}
