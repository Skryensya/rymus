// Zellij-like look: powerline ribbons for the tab bar, rounded panes with the title in the border,
// a status bar of key hints. Pure string builders, no state. 24-bit color.
use std::sync::OnceLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub type Rgb = [u8; 3];
pub const RED: Rgb = [232, 36, 36];
pub const GRAY: Rgb = [90, 90, 90];
pub const BAR: Rgb = [28, 28, 28];
pub const RIB: Rgb = [48, 48, 52];
pub const TXT: Rgb = [208, 208, 208];
pub const BLACK: Rgb = [0, 0, 0];
pub const GREEN: Rgb = [166, 209, 137];
pub const RESET: &str = "\x1b[0m";

pub struct Glyphs {
    pub sep: &'static str,
    pub like: &'static str,
    pub unliked: &'static str,
    pub dislike: &'static str,
}
// Powerline and Nerd Font glyphs (Ghostty ships them); plain symbols with YM_PLAIN=1.
pub fn glyphs() -> &'static Glyphs {
    static G: OnceLock<Glyphs> = OnceLock::new();
    G.get_or_init(|| {
        if std::env::var_os("YM_PLAIN").is_some_and(|v| !v.is_empty()) {
            Glyphs { sep: " ", like: "♥", unliked: "♡", dislike: "✗" }
        } else {
            Glyphs { sep: "\u{e0b0}", like: "\u{f004}", unliked: "\u{f08a}", dislike: "\u{f165}" }
        }
    })
}

pub fn fgc([r, g, b]: Rgb) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}
pub fn bgc([r, g, b]: Rgb) -> String {
    format!("\x1b[48;2;{r};{g};{b}m")
}
pub fn fg(a: Rgb, s: &str) -> String {
    format!("{}{s}\x1b[39m", fgc(a))
}
pub fn dim(s: &str) -> String {
    format!("\x1b[2m{s}\x1b[0m")
}
pub fn bold(s: &str) -> String {
    format!("\x1b[1m{s}\x1b[0m")
}

pub fn width(s: &str) -> usize {
    s.width()
}
pub fn spaces(n: usize) -> String {
    " ".repeat(n)
}
// The first characters of s that fit in n columns.
pub fn take(s: &str, n: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        used += c.width().unwrap_or(0);
        if used > n {
            return &s[..i];
        }
    }
    s
}
// The last characters of s that fit in n columns.
fn tail(s: &str, n: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices().rev() {
        used += c.width().unwrap_or(0);
        if used > n {
            return &s[i + c.len_utf8()..];
        }
    }
    s
}
pub fn cut(s: &str, n: usize) -> String {
    if width(s) > n {
        format!("{}…", take(s, n.saturating_sub(1)))
    } else {
        s.to_string()
    }
}
pub fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", spaces(w.saturating_sub(width(s))))
}
// Text padded or cut to exactly w visible columns.
pub fn cell(s: &str, w: usize) -> String {
    pad(&cut(s, w), w)
}

#[cfg(test)]
pub fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// Tab bar: [ymus][tab][tab]... as ribbons. Returns the line and the column span of each tab (for the mouse).
pub fn tab_bar(w: usize, tabs: &[(String, &str)], active: usize, accent: Rgb) -> (String, Vec<(usize, usize, usize)>) {
    let sep = glyphs().sep;
    let build = |short: bool| {
        let mut items = vec![(" ♫ rymus ".to_string(), BAR, accent, true)];
        for (i, (full, small)) in tabs.iter().enumerate() {
            let label = format!(" {} ", if short { small } else { full.as_str() });
            items.push(if i == active { (label, accent, BLACK, true) } else { (label, RIB, TXT, false) });
        }
        let (mut s, mut col, mut last) = (String::new(), 1usize, None::<Rgb>);
        let mut spans = Vec::new();
        for (i, (t, bg, color, strong)) in items.iter().enumerate() {
            match last {
                Some(l) => {
                    s += &format!("{}{}{sep}", fgc(l), bgc(*bg));
                    col += 1;
                }
                None => s += &bgc(*bg),
            }
            s += &format!("{}{}{t}\x1b[22m", fgc(*color), if *strong { "\x1b[1m" } else { "" });
            if i > 0 {
                spans.push((i - 1, (col - 1).max(1), col + width(t) - 1));
            }
            col += width(t);
            last = Some(*bg);
        }
        s += &format!("{}{}{sep}", bgc(BAR), fgc(last.unwrap_or(BAR)));
        (s, col, spans)
    };
    let mut b = build(false);
    if b.1 > w {
        b = build(true);
    }
    (format!("{}{}{}{RESET}", b.0, bgc(BAR), spaces(w.saturating_sub(b.1))), b.2)
}

// Pane frame (inner rows must already be exactly w-2 visible columns). Title sits in the top border.
pub fn frame(title: &str, tcol: Rgb, inner: &[String], w: usize, color: Rgb) -> Vec<String> {
    let t = cut(title, w.saturating_sub(5));
    let b = |s: String| fg(color, &s);
    let top = format!(
        "{}{}\x1b[1m{t}\x1b[22m\x1b[39m{}",
        b("╭─ ".into()),
        fgc(tcol),
        b(format!(" {}╮", "─".repeat(w.saturating_sub(5 + width(&t)))))
    );
    let mut out = vec![top];
    out.extend(inner.iter().map(|l| format!("{}{l}{}", b("│".into()), b("│".into()))));
    out.push(b(format!("╰{}╯", "─".repeat(w.saturating_sub(2)))));
    out
}

// Bottom bar: key + label; as many as fit.
pub fn hints(list: &[(&str, &str)], w: usize, accent: Rgb) -> String {
    let (mut s, mut len) = (" ".to_string(), 1);
    for (k, l) in list {
        let n = width(k) + width(l) + 4;
        if len + n > w {
            break;
        }
        s += &format!("{}\x1b[1m{k}\x1b[22m {}{l}   ", fgc(accent), fgc([130, 130, 130]));
        len += n;
    }
    s + &spaces(w.saturating_sub(len)) + RESET
}

// Spectrum in braille, bass on the left: h rows by exactly w columns, one thin line per column (4 dots of height per
// row). bands are 0..1; columns in between are interpolated.
const DOTS: [u32; 5] = [0, 0x40, 0x44, 0x46, 0x47]; // 0 to 4 dots of the left braille column, filled from the bottom
pub fn spectrum(bands: &[f64], w: usize, h: usize, accent: Rgb) -> Vec<String> {
    let n = bands.len();
    let heights: Vec<i64> = (0..w)
        .map(|i| {
            let x = if w > 1 { (i * (n - 1)) as f64 / (w - 1) as f64 } else { 0.0 };
            let a = x.floor() as usize;
            let b = (a + 1).min(n - 1);
            (((bands[a] + (bands[b] - bands[a]) * (x - a as f64)) * 4.0 * h as f64).round() as i64).max(1) // baseline
        })
        .collect();
    (0..h)
        .map(|row| {
            let floor = (h - 1 - row) as i64 * 4;
            let s: String = heights.iter().filter_map(|e| char::from_u32(0x2800 + DOTS[(e - floor).clamp(0, 4) as usize])).collect();
            let light = if h > 1 { 0.6 * (1.0 - row as f64 / (h - 1) as f64) } else { 0.0 }; // top rows are paler
            fg(accent.map(|c| (c as f64 + (255.0 - c as f64) * light).round() as u8), &s)
        })
        .collect()
}

// Always-visible search field under the tab bar.
pub fn search_bar(w: usize, typing: bool, text: &str, placeholder: &str, accent: Rgb) -> String {
    let label = " / Search ";
    let field_w = w.saturating_sub(label.len() + 1);
    let field = if typing {
        let body = format!("{text}█");
        pad(tail(&body, field_w), field_w)
    } else {
        let shown = cut(if text.is_empty() { placeholder } else { text }, field_w);
        format!("\x1b[2m{shown}\x1b[22m{}", spaces(field_w.saturating_sub(width(&shown))))
    };
    let lab = if typing {
        format!("{}{}\x1b[1m{label}\x1b[22m{RESET}", bgc(accent), fgc(BLACK))
    } else {
        format!("{}{}{label}{RESET}", bgc(RIB), fgc(TXT))
    };
    format!("{lab} {field}")
}
