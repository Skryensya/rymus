// The EQ tab: the profiles on the left; on the right a slider per band on a dB scale, what range each band is in,
// and what the band (or the profile) under the cursor does to the sound.
use super::*;
use crate::eq::{self, BANDS, FREQS, LIMIT, REGIONS};

const SCALE: usize = 5; // "+12 ┤"

impl App {
    pub(super) fn eq_body(&mut self, v: &View) -> Vec<String> {
        let (w, iw, l, a) = (v.w, v.iw, v.l, v.a);
        let (sel, band) = (self.eq.sel, self.eq.band);
        let all = self.eq_presets();
        let (current, gains) = self.eq_current();
        let custom = all.iter().any(|p| p.name == current && p.custom);

        // left: the profiles
        let lw = if iw < 100 { 18 } else { 24 }.min(iw / 3); // a narrow screen gives the sliders the room
        let mut names: Vec<String> = all.iter().map(|p| format!("{} {}{}", if p.name == current { "●" } else { " " }, p.name, if p.custom { " ✎" } else { "" })).collect();
        names.push("  + New profile".into());
        let n = l.saturating_sub(1); // under the heading
        let top = sel.saturating_sub(n.saturating_sub(1));
        let mut left = vec![dim(&bold(&cell(" PROFILES", lw)))];
        left.extend((0..n).map(|i| match names.get(top + i) {
            Some(name) if top + i == sel && band.is_none() => selected(a, &cell(name, lw.saturating_sub(1))),
            Some(name) if top + i == all.iter().position(|p| p.name == current).unwrap_or(usize::MAX) => fg(a, &cell(&format!(" {name}"), lw)),
            Some(name) => cell(&format!(" {name}"), lw),
            None => spaces(lw),
        }));

        // right: the sliders, then what they mean
        let gw = iw.saturating_sub(lw + 4); // " list │ right"
        let col = gw.saturating_sub(SCALE) / FREQS.len();
        let right: Vec<String> = if col < 4 {
            vec![dim(&cell("Widen the window to see the sliders.", gw))]
        } else {
            let mut r = sliders(&gains, band, col, l.saturating_sub(6), a);
            r.push(format!("{}{}", spaces(SCALE), FREQS.iter().enumerate().map(|(i, f)| mark(&center(&eq::freq_label(*f), col), band == Some(i), a)).collect::<String>()));
            r.push(format!("{}{}", dim(&format!("{:>w$} ", "dB", w = SCALE - 1)), gains.iter().enumerate().map(|(i, g)| {
                let t = center(&if *g == 0 { "0".into() } else { format!("{g:+}") }, col);
                if band == Some(i) { fg(a, &bold(&t)) } else if *g == 0 { dim(&t) } else { t }
            }).collect::<String>()));
            r.push(format!("{}{}", spaces(SCALE), dim(&regions(col))));
            r.push(String::new());
            // the explanation: the band you move, or the profile you hear
            let (title, about) = match band {
                Some(b) => (format!("{} Hz · {} · {:+} dB", eq::freq_label(FREQS[b]), BANDS[b].0, gains[b]), cap(BANDS[b].1)),
                None => (format!("{current}{}", if custom { " · yours" } else { "" }), eq::describe(&gains)),
            };
            r.push(fg(a, &bold(&cut(&title, gw))));
            r.push(cut(&about, gw));
            r
        };
        let rows: Vec<String> = (0..l)
            .map(|i| {
                let side = right.get(i).cloned().unwrap_or_default();
                format!(" {} {} {}", left.get(i).cloned().unwrap_or_else(|| spaces(lw)), dim("│"), pad_visible(&side, gw))
            })
            .collect();
        let (t, c) = v.title(&format!("EQ · {current}"));
        frame(&t, c, &rows, w, a)
    }
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
            let label = format!(" {name} ");
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
