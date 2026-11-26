// The UI's accent color, and how it changes: the new color sweeps across the screen from one side, with a soft front
// and a few drifting dots on the empty cells along it. Text is never covered.
use crate::ui::{Rgb, GREEN};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

const LASTS: Duration = Duration::from_millis(1400);
const SOFT: f64 = 0.13; // half the width of the front, as a share of the screen

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    Left,
    Right,
    Top,
    Bottom,
}

pub struct Theme {
    pub color: Rgb,
    old: Rgb,
    side: From,
    started: Option<Instant>,
    turn: u64,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme { color: GREEN, old: GREEN, side: From::Left, started: None, turn: 0 }
    }
}

impl Theme {
    // Starts a sweep towards `color`, from a side other than the last one.
    pub fn aim(&mut self, color: Rgb) {
        if color == self.color {
            return;
        }
        self.old = self.color;
        self.color = color;
        self.turn = self.turn.wrapping_add(1 + (crate::db::now_ms() as u64 % 3));
        let side = [From::Left, From::Top, From::Right, From::Bottom][(self.turn % 4) as usize];
        self.side = if side == self.side { [From::Right, From::Bottom, From::Left, From::Top][(self.turn % 4) as usize] } else { side };
        self.started = Some(Instant::now());
    }
    // True while the screen has to keep being drawn, including the one clean frame after the sweep.
    pub fn step(&mut self) -> bool {
        match self.started {
            Some(at) => {
                if at.elapsed() >= LASTS {
                    self.started = None;
                }
                true
            }
            None => false,
        }
    }
    pub fn paint(&self, lines: &mut [String]) {
        let Some(at) = self.started else { return };
        let p = at.elapsed().as_secs_f64() / LASTS.as_secs_f64();
        if p < 1.0 {
            sweep(lines, self.old, self.color, p, self.side);
        }
    }
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    std::array::from_fn(|i| (a[i] as f64 + (b[i] as f64 - a[i] as f64) * t).round().clamp(0.0, 255.0) as u8)
}

// The accent and its paler tints (the rows of the spectrum): a color on screen -> (what it was, what it will be).
fn tints(old: Rgb, new: Rgb) -> Vec<(Rgb, Rgb)> {
    let mut out = vec![(new, old)];
    for h in 2..=8 {
        for row in 0..h {
            let light = 0.6 * (1.0 - row as f64 / (h - 1) as f64);
            let tint = |c: Rgb| mix(c, [255; 3], light);
            out.push((tint(new), tint(old)));
        }
    }
    out
}

#[derive(Clone, Default, PartialEq)]
struct Style {
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    attrs: Vec<u8>,
}

impl Style {
    fn apply(&mut self, params: &str) {
        let mut n = params.split(';').map(|x| x.parse::<u8>().unwrap_or(0));
        while let Some(code) = n.next() {
            match code {
                0 => *self = Style::default(),
                38 | 48 => {
                    let _ = n.next(); // 2: true color follows
                    let rgb = Some([n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0)]);
                    if code == 38 {
                        self.fg = rgb;
                    } else {
                        self.bg = rgb;
                    }
                }
                39 => self.fg = None,
                49 => self.bg = None,
                22 => self.attrs.retain(|&a| a != 1 && a != 2),
                other => self.attrs.push(other),
            }
        }
    }
    fn code(&self) -> String {
        let mut s = "\x1b[0".to_string();
        for a in &self.attrs {
            s += &format!(";{a}");
        }
        if let Some([r, g, b]) = self.fg {
            s += &format!(";38;2;{r};{g};{b}");
        }
        if let Some([r, g, b]) = self.bg {
            s += &format!(";48;2;{r};{g};{b}");
        }
        s + "m"
    }
}

// The screen drawn in `new`, shown `p` (0..1) of the way through the change from `old`.
pub fn sweep(lines: &mut [String], old: Rgb, new: Rgb, p: f64, side: From) {
    let tints = tints(old, new);
    let rows = lines.len();
    let columns = lines.first().map_or(0, |l| crate::ui::width(&strip(l)));
    let eased = p * p * (3.0 - 2.0 * p);
    // starts before the first cell and ends past the last, so both ends are clean
    let front = eased * (1.0 + 4.0 * SOFT) - 2.0 * SOFT;
    for (y, line) in lines.iter_mut().enumerate() {
        let mut out = String::new();
        let plain: Vec<char> = strip(line).chars().collect();
        // only inside empty room: never the space between two words
        let open = |i: usize| i > 0 && plain.get(i - 1) == Some(&' ') && plain.get(i + 1) == Some(&' ');
        let mut index = 0;
        let (mut style, mut written, mut x) = (Style::default(), None::<Style>, 0usize);
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                let sequence: String = chars.by_ref().take_while(|c| !c.is_ascii_alphabetic()).collect();
                style.apply(sequence.trim_start_matches('['));
                continue;
            }
            let across = x as f64 / columns.saturating_sub(1).max(1) as f64;
            let down = y as f64 / rows.saturating_sub(1).max(1) as f64;
            let (u, v) = match side {
                From::Left => (across, down),
                From::Right => (1.0 - across, down),
                From::Top => (down, across),
                From::Bottom => (1.0 - down, across),
            };
            let u = u + 0.035 * (v * 9.0 + p * 7.0).sin(); // the front ripples a little
            let arrived = ((front + SOFT - u) / (2.0 * SOFT)).clamp(0.0, 1.0); // 0: still old, 1: new
            let recolor = |color: Option<Rgb>| color.map(|c| tints.iter().find(|t| t.0 == c).map_or(c, |t| mix(t.1, t.0, arrived)));
            let mut shown = Style { fg: recolor(style.fg), bg: recolor(style.bg), attrs: style.attrs.clone() };
            let mut glyph = c;
            let near = (u - front).abs() / SOFT;
            // a sparse, fixed speckle: the same cells light up as the front passes, no flicker
            let speck = (x.wrapping_mul(73) ^ y.wrapping_mul(151)) % 7;
            if c == ' ' && open(index) && style.bg.is_none() && near < 1.0 && speck < 2 {
                glyph = if near < 0.3 { '∘' } else { '·' };
                shown = Style { fg: Some(mix(mix(old, new, arrived), [0; 3], 0.55 * near)), bg: None, attrs: vec![] };
            }
            if written.as_ref() != Some(&shown) {
                out += &shown.code();
                written = Some(shown);
            }
            out.push(glyph);
            index += 1;
            x += c.width().unwrap_or(0);
        }
        out += "\x1b[0m";
        *line = out;
    }
}

fn strip(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
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
    use crate::ui::{fg, width};

    const OLD: Rgb = [10, 200, 10];
    const NEW: Rgb = [200, 10, 10];

    fn screen() -> Vec<String> {
        (0..10).map(|_| format!("{}{}", fg(NEW, "title ♫ 日本"), " ".repeat(30))).collect()
    }
    fn at(p: f64, side: From) -> Vec<String> {
        let mut lines = screen();
        sweep(&mut lines, OLD, NEW, p, side);
        lines
    }

    #[test]
    fn sweeps_from_old_to_new_without_covering_text() {
        let code = |c: Rgb| format!("38;2;{};{};{}", c[0], c[1], c[2]);
        let wide = width(&strip(&screen()[0]));
        for side in [From::Left, From::Right, From::Top, From::Bottom] {
            for p in [0.0, 0.3, 0.5, 0.8, 1.0] {
                for line in at(p, side) {
                    assert_eq!(width(&strip(&line)), wide, "{side:?} {p}");
                    assert!(strip(&line).starts_with("title ♫ 日本"), "text stays");
                }
            }
            assert!(at(0.0, side).iter().all(|l| l.contains(&code(OLD)) && !l.contains(&code(NEW))), "{side:?} starts old");
            assert!(at(1.0, side).iter().all(|l| l.contains(&code(NEW)) && !l.contains(&code(OLD))), "{side:?} ends new");
        }
        // half way from the top, the first row has changed and the last has not
        let half = at(0.5, From::Top);
        assert!(half[0].contains(&code(NEW)) && half[9].contains(&code(OLD)));
        assert!(at(0.5, From::Left).iter().any(|l| strip(l).contains('·') || strip(l).contains('∘')), "the front is drawn");
    }

    #[test]
    fn a_new_color_starts_a_sweep_and_the_same_one_does_not() {
        let mut theme = Theme::default();
        theme.aim(GREEN);
        assert!(!theme.step());
        theme.aim(NEW);
        assert!(theme.step() && theme.color == NEW);
    }
}
