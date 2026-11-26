// Zellij-like look: powerline ribbons for the tab bar, rounded panes with the title in the border,
// a status bar of key hints. Pure string builders, no state. 24-bit color.
use std::sync::OnceLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub type Rgb = [u8; 3];
pub const RED: Rgb = [232, 36, 36];
pub const GREEN: Rgb = [166, 209, 137];

// Dark or light terminal (Config > Look > Mode): every color but the accent has one of each. Per thread, like the
// border style: the app draws on one, and each test runs on its own.
thread_local!(static LIGHT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
pub fn set_light(on: bool) {
    LIGHT.set(on);
}
fn pick(dark: Rgb, light: Rgb) -> Rgb {
    if LIGHT.get() { light } else { dark }
}
pub fn gray() -> Rgb {
    pick([90, 90, 90], [178, 178, 178]) // frames not in focus
}
pub fn bar() -> Rgb {
    pick([28, 28, 28], [232, 232, 234]) // the tab bar
}
pub fn rib() -> Rgb {
    pick([48, 48, 52], [216, 216, 222]) // tabs, labels and the selected row
}
pub fn txt() -> Rgb {
    pick([208, 208, 208], [45, 45, 48])
}
pub fn on_accent() -> Rgb {
    pick([0, 0, 0], [255, 255, 255]) // text on the accent
}
pub fn strong() -> Rgb {
    pick([255, 255, 255], [0, 0, 0]) // the selected row's text, and what the spectrum's tints fade towards
}
// The accent as it is drawn: on a light background a pale one is darkened until it reads well.
pub fn readable(c: Rgb) -> Rgb {
    let lum = 0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64;
    if !LIGHT.get() || lum <= 110.0 {
        return c;
    }
    c.map(|v| (v as f64 * 110.0 / lum).round() as u8)
}
pub const RESET: &str = "\x1b[0m";

pub struct Glyphs {
    pub sep: &'static str,
    pub like: &'static str,
    pub unliked: &'static str,
    pub dislike: &'static str,
    pub play: &'static str,
    pub pause: &'static str,
    pub prev: &'static str,
    pub next: &'static str,
    pub repeat: &'static str,
    pub repeat_one: &'static str,
}
// Powerline and Nerd Font glyphs (Ghostty ships them); plain symbols with YM_PLAIN=1.
pub fn glyphs() -> &'static Glyphs {
    static G: OnceLock<Glyphs> = OnceLock::new();
    G.get_or_init(|| {
        if std::env::var_os("YM_PLAIN").is_some_and(|v| !v.is_empty()) {
            Glyphs { sep: " ", like: "♥", unliked: "♡", dislike: "✗", play: "▶", pause: "‖", prev: "«", next: "»", repeat: "↻", repeat_one: "1" }
        } else {
            Glyphs { sep: "\u{e0b0}", like: "\u{f004}", unliked: "\u{f08a}", dislike: "\u{f165}", play: "\u{f04b}", pause: "\u{f04c}", prev: "\u{f048}", next: "\u{f051}", repeat: "\u{f0456}", repeat_one: "\u{f0458}" }
        }
    })
}

// The frames' style, chosen in Config. Each edge is a pattern repeated along it (one column per character), so a
// border can be ornate: corners, the top and bottom edges, and the sides, which go down a row at a time.
// Per thread: the app draws on one, and each test runs on its own.
pub struct Border {
    pub corners: [char; 4], // top left, top right, bottom left, bottom right
    pub top: &'static str,
    pub bottom: &'static str,
    pub left: &'static str,
    pub right: &'static str,
}
// In the order of Config's choices.
pub const BORDERS: [Border; 10] = [
    Border { corners: ['╭', '╮', '╰', '╯'], top: "─", bottom: "─", left: "│", right: "│" }, // rounded
    Border { corners: ['┌', '┐', '└', '┘'], top: "─", bottom: "─", left: "│", right: "│" }, // square
    Border { corners: ['┏', '┓', '┗', '┛'], top: "━", bottom: "━", left: "┃", right: "┃" }, // heavy
    Border { corners: ['╔', '╗', '╚', '╝'], top: "═", bottom: "═", left: "║", right: "║" }, // double
    Border { corners: ['╭', '╮', '╰', '╯'], top: "┄", bottom: "┄", left: "┆", right: "┆" }, // dashed
    Border { corners: ['·', '·', '·', '·'], top: "┈", bottom: "┈", left: "┊", right: "┊" }, // dotted
    Border { corners: ['╒', '╕', '╘', '╛'], top: "═", bottom: "═", left: "│", right: "│" }, // deco
    Border { corners: ['▛', '▜', '▙', '▟'], top: "▀", bottom: "▄", left: "▌", right: "▐" }, // block
    Border { corners: ['+', '+', '+', '+'], top: "-", bottom: "-", left: "|", right: "|" }, // ascii
    // ornate, in plain ASCII: scrolls along the edges and curls down the sides
    Border { corners: ['@', '@', '@', '@'], top: "-~=*=~", bottom: "-~=*=~", left: "{}", right: "}{" },
];
thread_local!(static BORDER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) });
pub fn set_border(style: usize) {
    BORDER.set(style.min(BORDERS.len() - 1));
}
pub fn border() -> &'static Border {
    &BORDERS[BORDER.get()]
}
impl Border {
    // `n` columns of an edge's pattern, starting `from` columns in, so the pattern runs on past the title.
    fn edge(pattern: &str, from: usize, n: usize) -> String {
        pattern.chars().cycle().skip(from).take(n).collect()
    }
    fn side(pattern: &str, row: usize) -> char {
        pattern.chars().nth(row % pattern.chars().count()).unwrap_or('|')
    }
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
    if n == 0 {
        return String::new(); // not even room for the …
    }
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

// The text without its colour codes.
pub fn plain(s: &str) -> String {
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
pub use plain as visible;
// Styled text padded to `w` visible columns; cut, without its colours, when it is longer.
pub fn pad_styled(s: &str, w: usize) -> String {
    let n = width(&plain(s));
    if n > w { cut(&plain(s), w) } else { format!("{s}{}", spaces(w - n)) }
}
// The text with its first letter in capitals.
pub fn capital(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
}
// Text centred in `w` columns.
pub fn center(s: &str, w: usize) -> String {
    let n = width(s).min(w);
    let l = (w - n) / 2;
    format!("{}{}{}", spaces(l), cut(s, w), spaces(w - n - l))
}

// Tab bar: [rymus][tab][tab]... as ribbons. Returns the line and the column span of each tab (for the mouse).
pub fn tab_bar(w: usize, tabs: &[(String, &str)], active: usize, accent: Rgb) -> (String, Vec<(usize, usize, usize)>) {
    let sep = glyphs().sep;
    // brand: the " ♫ rymus " label first, left out when even the short tabs do not fit with it; on the narrowest
    // screens only the tab you are on is shown
    let build = |short: bool, brand: bool, all: bool| {
        let mut items = if brand { vec![(" ♫ rymus ".to_string(), bar(), accent, true)] } else { vec![] };
        let mut index = Vec::new(); // tab number of each item after the brand
        for (i, (full, small)) in tabs.iter().enumerate().filter(|(i, _)| all || *i == active) {
            index.push(i);
            let label = format!(" {} ", if short { small } else { full.as_str() });
            items.push(if i == active { (label, accent, on_accent(), true) } else { (label, rib(), txt(), false) });
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
            if !brand || i > 0 {
                spans.push((index[i - usize::from(brand)], (col - 1).max(1), col + width(t) - 1));
            }
            col += width(t);
            last = Some(*bg);
        }
        s += &format!("{}{}{sep}", bgc(bar()), fgc(last.unwrap_or(bar())));
        (s, col, spans)
    };
    let mut b = build(false, true, true);
    for (short, brand, all) in [(true, true, true), (true, false, true), (false, false, false), (true, false, false)] {
        if b.1 > w {
            b = build(short, brand, all);
        }
    }
    (format!("{}{}{}{RESET}", b.0, bgc(bar()), spaces(w.saturating_sub(b.1))), b.2)
}

// Pane frame (inner rows must already be exactly w-2 visible columns). Title sits in the top border.
pub fn frame(title: &str, tcol: Rgb, inner: &[String], w: usize, color: Rgb) -> Vec<String> {
    let t = cut(title, w.saturating_sub(5));
    let b = |s: String| fg(color, &s);
    let x = border();
    let [tl, tr, bl, br] = x.corners;
    let tw = width(&t);
    let top = format!(
        "{}{}\x1b[1m{t}\x1b[22m\x1b[39m{}",
        b(format!("{tl}{} ", Border::edge(x.top, 0, 1))),
        fgc(tcol),
        b(format!(" {}{tr}", Border::edge(x.top, 3 + tw, w.saturating_sub(5 + tw))))
    );
    let mut out = vec![top];
    out.extend(inner.iter().enumerate().map(|(row, l)| format!("{}{l}{}", b(Border::side(x.left, row).into()), b(Border::side(x.right, row).into()))));
    out.push(b(format!("{bl}{}{br}", Border::edge(x.bottom, 0, w.saturating_sub(2)))));
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
// A paler (on a light background, darker) shade of a color, `t` of the way to white (or black).
pub fn tint(c: Rgb, t: f64) -> Rgb {
    let to = strong();
    std::array::from_fn(|i| (c[i] as f64 + (to[i] as f64 - c[i] as f64) * t).round() as u8)
}

// The player's visualizer: one column per terminal column, `h` rows. Styles (Config): 0 dots that rise from the
// bottom, 1 solid bars in eighth blocks, 2 a line of single dots at each level, 3 a mirror growing up and down from the
// middle. Dots use the left braille column only, which leaves a thin gap between columns.
const LEFT: [u32; 4] = [0x01, 0x02, 0x04, 0x40]; // the left braille dots, top to bottom
const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub fn spectrum(bands: &[f64], w: usize, h: usize, accent: Rgb, style: usize) -> Vec<String> {
    let n = bands.len();
    let level: Vec<f64> = (0..w)
        .map(|i| {
            let x = if w > 1 { (i * (n - 1)) as f64 / (w - 1) as f64 } else { 0.0 };
            let a = x.floor() as usize;
            let b = (a + 1).min(n - 1);
            (bands[a] + (bands[b] - bands[a]) * (x - a as f64)).clamp(0.0, 1.0)
        })
        .collect();
    let dots = 4 * h as i64; // dot rows, 0 at the top
    // the dot rows each column lights, first to last; never empty, so silence still shows a baseline
    let span = |f: f64| -> (i64, i64) {
        let e = ((f * dots as f64).round() as i64).max(1);
        match style {
            2 => (dots - e, dots - e),
            3 => {
                let half = ((f * dots as f64 / 2.0).round() as i64).max(1);
                (dots / 2 - half, dots / 2 + half - 1)
            }
            _ => (dots - e, dots - 1),
        }
    };
    (0..h)
        .map(|row| {
            let s: String = level
                .iter()
                .map(|&f| {
                    if style == 1 {
                        let e = ((f * 8.0 * h as f64).round() as i64).max(1);
                        EIGHTHS[(e - (h - 1 - row) as i64 * 8).clamp(0, 8) as usize]
                    } else {
                        let (lo, hi) = span(f);
                        let bits: u32 = (0..4).filter(|k| (lo..=hi).contains(&(row as i64 * 4 + k))).map(|k| LEFT[k as usize]).sum();
                        char::from_u32(0x2800 + bits).unwrap_or(' ')
                    }
                })
                .collect();
            // paler towards the top, or towards the edges for the mirror
            let t = if h < 2 { 0.0 } else if style == 3 { (row as f64 - (h - 1) as f64 / 2.0).abs() / ((h - 1) as f64 / 2.0) } else { 1.0 - row as f64 / (h - 1) as f64 };
            let light = 0.6 * t;
            fg(tint(accent, light), &s)
        })
        .collect()
}

// Always-visible search field under the tab bar: three rows in its own box, or one plain row on a short screen.
pub fn search_bar(w: usize, title: &str, typing: bool, text: &str, placeholder: &str, accent: Rgb, boxed: bool) -> Vec<String> {
    let label = format!(" {title} ");
    // inside the box: a space on each side; on one line: the label and a space
    let field_w = w.saturating_sub(if boxed { 4 } else { width(&label) + 1 });
    let field = if typing {
        let body = format!("{text}█");
        pad(tail(&body, field_w), field_w)
    } else {
        let shown = cut(if text.is_empty() { placeholder } else { text }, field_w);
        format!("\x1b[2m{shown}\x1b[22m{}", spaces(field_w.saturating_sub(width(&shown))))
    };
    if boxed {
        let color = if typing { accent } else { gray() };
        return frame(title, if typing { accent } else { txt() }, &[format!(" {field} ")], w, color);
    }
    let lab = if typing {
        format!("{}{}\x1b[1m{label}\x1b[22m{RESET}", bgc(accent), fgc(on_accent()))
    } else {
        format!("{}{}{label}{RESET}", bgc(rib()), fgc(txt()))
    };
    vec![format!("{lab} {field}")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_border_style_keeps_the_width() {
        for style in 0..BORDERS.len() {
            set_border(style);
            let rows = frame("Title", [255; 3], &[spaces(20), spaces(20), spaces(20)], 22, [255; 3]);
            assert!(rows.iter().all(|r| width(&visible(r)) == 22), "style {style}: {:?}", rows.iter().map(|r| visible(r)).collect::<Vec<_>>());
        }
        set_border(9); // ornate
        let rows: Vec<String> = frame("Hi", [255; 3], &[spaces(10), spaces(10)], 12, [255; 3]).iter().map(|r| visible(r)).collect();
        assert_eq!(rows, ["@- Hi ~-~=*@", "{          }", "}          {", "@-~=*=~-~=*@"]);
        set_border(0);
    }

    #[test]
    fn visualizer_styles() {
        let bands = [0.0, 0.5, 1.0, 0.5, 0.0];
        for style in 0..4 {
            let rows: Vec<String> = spectrum(&bands, 9, 3, [255, 255, 255], style).iter().map(|r| visible(r)).collect();
            assert_eq!(rows.len(), 3);
            assert!(rows.iter().all(|r| r.chars().count() == 9), "style {style}: {rows:?}");
        }
        let col = |rows: &[String], i: usize| rows.iter().map(|r| r.chars().nth(i).unwrap()).collect::<String>();
        let bars: Vec<String> = spectrum(&bands, 5, 2, [0, 0, 0], 1).iter().map(|r| visible(r)).collect();
        assert_eq!((col(&bars, 2), col(&bars, 0)), ("██".into(), " ▁".into()), "loud is full, silence a baseline");
        // the mirror lights the middle of a quiet column, and a loud one top to bottom
        let mirror: Vec<String> = spectrum(&bands, 5, 2, [0, 0, 0], 3).iter().map(|r| visible(r)).collect();
        assert_eq!(col(&mirror, 0), "\u{2840}\u{2801}");
        assert_eq!(col(&mirror, 2), "\u{2847}\u{2847}");
        // the line is one dot per column
        let line: Vec<String> = spectrum(&bands, 5, 2, [0, 0, 0], 2).iter().map(|r| visible(r)).collect();
        assert_eq!(col(&line, 2), "\u{2801}\u{2800}");
    }

    #[test]
    fn search_rows_are_exact() {
        for (typing, boxed) in [(false, false), (true, false), (false, true), (true, true)] {
            let rows = search_bar(60, "/ Search", typing, "canción larga ".repeat(9).as_str(), "…", GREEN, boxed);
            assert_eq!(rows.len(), if boxed { 3 } else { 1 });
            assert!(rows.iter().all(|r| width(&visible(r)) == 60), "typing {typing} boxed {boxed}");
        }
    }
}
