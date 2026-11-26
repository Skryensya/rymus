// Big text for karaoke: a 5×7 dot font (capitals, digits, punctuation). A letter is 6 dots wide (5 and a space) and
// 12 tall: 2 rows for its accent, a gap, the 7 rows of the letter and 2 rows of space under it. Each style puts a
// different number of those dots in one character of the terminal.

// Each glyph: 7 rows of 5 dots, '#' on.
fn glyph(c: char) -> Option<[&'static str; 7]> {
    Some(match c {
        'A' => [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'B' => ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
        'C' => [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
        'D' => ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
        'E' => ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'F' => ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
        'G' => [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
        'H' => ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'I' => [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."],
        'J' => ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."],
        'K' => ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
        'L' => ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        'M' => ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
        'N' => ["#...#", "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#"],
        'O' => [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'P' => ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
        'Q' => [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
        'R' => ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        'S' => [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
        'U' => ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'V' => ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
        'W' => ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#."],
        'X' => ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
        'Y' => ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
        'Z' => ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
        '0' => [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
        '1' => ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
        '2' => [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
        '3' => ["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
        '4' => ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
        '5' => ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
        '6' => ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
        '7' => ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
        '8' => [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
        '9' => [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
        ' ' => [".....", ".....", ".....", ".....", ".....", ".....", "....."],
        '.' => [".....", ".....", ".....", ".....", ".....", ".....", "..#.."],
        ',' => [".....", ".....", ".....", ".....", ".....", "..#..", ".#..."],
        '!' => ["..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#.."],
        '?' => [".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#.."],
        '¡' => ["..#..", ".....", "..#..", "..#..", "..#..", "..#..", "..#.."],
        '¿' => ["..#..", ".....", "..#..", ".#...", "#....", "#...#", ".###."],
        '\'' => ["..#..", "..#..", ".#...", ".....", ".....", ".....", "....."],
        '"' => [".#.#.", ".#.#.", ".....", ".....", ".....", ".....", "....."],
        '-' => [".....", ".....", ".....", ".###.", ".....", ".....", "....."],
        ':' => [".....", ".##..", ".##..", ".....", ".##..", ".##..", "....."],
        ';' => [".....", ".##..", ".##..", ".....", ".##..", "..#..", ".#..."],
        '(' => ["...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#."],
        ')' => [".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#..."],
        '&' => [".##..", "#..#.", "#.#..", ".#...", "#.#.#", "#..#.", ".##.#"],
        '/' => [".....", "....#", "...#.", "..#..", ".#...", "#....", "....."],
        // the note fills the whole box (head on the left, flag on the right), so it sits centred like a letter
        '♪' => ["...#.", "...##", "...#.", "...#.", ".###.", "####.", ".##.."],
        _ => return None,
    })
}

// Accents: two rows of dots above the letter.
const NONE: [&str; 2] = [".....", "....."];
const ACUTE: [&str; 2] = ["...#.", "..#.."];
const GRAVE: [&str; 2] = [".#...", "..#.."];
const CIRCUMFLEX: [&str; 2] = ["..#..", ".#.#."];
const DIAERESIS: [&str; 2] = [".....", ".#.#."];
const TILDE: [&str; 2] = [".##.#", "#..#."];

// A letter as (accent, base glyph): accented letters keep their mark above the capital; lower case is drawn upper.
fn split(c: char) -> Option<([&'static str; 2], char)> {
    let up = c.to_uppercase().next().unwrap_or(c);
    let (accent, base) = match up {
        'Á' => (ACUTE, 'A'),
        'É' => (ACUTE, 'E'),
        'Í' => (ACUTE, 'I'),
        'Ó' => (ACUTE, 'O'),
        'Ú' => (ACUTE, 'U'),
        'À' => (GRAVE, 'A'),
        'È' => (GRAVE, 'E'),
        'Ì' => (GRAVE, 'I'),
        'Ò' => (GRAVE, 'O'),
        'Ù' => (GRAVE, 'U'),
        'Â' => (CIRCUMFLEX, 'A'),
        'Ê' => (CIRCUMFLEX, 'E'),
        'Î' => (CIRCUMFLEX, 'I'),
        'Ô' => (CIRCUMFLEX, 'O'),
        'Û' => (CIRCUMFLEX, 'U'),
        'Ä' => (DIAERESIS, 'A'),
        'Ë' => (DIAERESIS, 'E'),
        'Ï' => (DIAERESIS, 'I'),
        'Ö' => (DIAERESIS, 'O'),
        'Ü' => (DIAERESIS, 'U'),
        'Ñ' => (TILDE, 'N'),
        'Ç' => (NONE, 'C'),
        '’' | '‘' | '`' | '´' => (NONE, '\''),
        '“' | '”' => (NONE, '"'),
        '–' | '—' => (NONE, '-'),
        other => (NONE, other),
    };
    glyph(base).map(|_| (accent, base))
}

const LETTER: usize = 6; // dots per letter, with the space after it
const HEIGHT: usize = 12; // dots per line of text: 3 terminal rows of braille

// Dot (x, y) of a line of letters: y 0–1 the accent, 2 a gap, 3–9 the glyph, 10–11 the space under it.
fn dot(letters: &[([&str; 2], char)], x: usize, y: usize) -> bool {
    let (accent, base) = letters[x / LETTER];
    let col = x % LETTER;
    let row = match y {
        _ if col == 5 => return false,
        0 | 1 => accent[y],
        3..=9 => glyph(base).map_or(".....", |g| g[y - 3]),
        _ => return false,
    };
    row.as_bytes()[col] == b'#'
}

// The bits of the 8 dots in a braille character: left column top to bottom, then the right one.
const BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

// How the dots are drawn: braille holds 2×4 per character (3 columns by 3 rows a letter), sextants 2×3 (3 by 4,
// solid) and half blocks 1×2 (6 by 6, the biggest).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Dots,
    Solid,
    Blocks,
}
pub const STYLES: [(Style, &str); 3] = [(Style::Dots, "dots"), (Style::Solid, "solid"), (Style::Blocks, "blocks")];

impl Style {
    fn cell(self) -> (usize, usize) {
        match self {
            Style::Dots => (2, 4),
            Style::Solid => (2, 3),
            Style::Blocks => (1, 2),
        }
    }
    // The character with these dots on: bit `y * columns + x`, except braille, which has its own order.
    fn glyph(self, bits: u32) -> char {
        let code = match self {
            _ if bits == 0 => return ' ',
            Style::Dots => 0x2800 + (0..8).filter(|i| bits & (1 << i) != 0).map(|i| BITS[i % 2][i / 2]).sum::<u32>(),
            // the sextants leave out the two that already existed as half blocks, and the full one
            Style::Solid => match bits {
                21 => 0x258C,
                42 => 0x2590,
                63 => 0x2588,
                v => 0x1FB00 + v - 1 - u32::from(v > 21) - u32::from(v > 42),
            },
            Style::Blocks => [0x2580, 0x2584, 0x2588][bits as usize - 1],
        };
        char::from_u32(code).unwrap_or(' ')
    }
}

// The text as rows that fit in w columns, wrapped at spaces: 12 dots of height per line of text, each row exactly w
// columns, centered. None when a character has no big form (the caller shows it small instead).
pub fn render(text: &str, w: usize, style: Style) -> Option<Vec<String>> {
    let (cw, ch) = style.cell();
    let text = text.replace('…', "...");
    let letters: Vec<([&str; 2], char)> = text.chars().map(split).collect::<Option<_>>()?;
    let per_line = (cw * w + 1) / LETTER; // a line of n letters is 6n - 1 dots
    if per_line == 0 {
        return None;
    }
    let plain: String = letters.iter().map(|l| l.1).collect();
    let mut out = Vec::new();
    let mut at = 0; // index into letters, in step with the wrapped lines
    for line in crate::lyrics::wrap(&plain, per_line) {
        let n = line.chars().count();
        let ls = &letters[at..at + n];
        at += n;
        while letters.get(at).is_some_and(|l| l.1 == ' ') {
            at += 1; // wrap dropped the space it broke at
        }
        let dots = (n * LETTER).saturating_sub(1);
        let width = dots.div_ceil(cw);
        let pad = w.saturating_sub(width) / 2;
        for ty in 0..HEIGHT / ch {
            let mut s = " ".repeat(pad);
            for tx in 0..width {
                let mut bits = 0;
                for i in 0..cw * ch {
                    let x = cw * tx + i % cw;
                    if x < dots && dot(ls, x, ch * ty + i / cw) {
                        bits |= 1 << i;
                    }
                }
                s.push(style.glyph(bits));
            }
            s.push_str(&" ".repeat(w.saturating_sub(pad + width)));
            out.push(s);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::width;

    // The dots of rendered rows back as a picture: '#' on, '.' off (2 columns and 4 rows per character).
    fn picture(rows: &[String]) -> Vec<String> {
        let mut out = vec![String::new(); rows.len() * 4];
        for (r, row) in rows.iter().enumerate() {
            for c in row.chars() {
                let bits = if c == ' ' { 0 } else { c as u32 - 0x2800 };
                for dy in 0..4 {
                    for column in &BITS {
                        out[4 * r + dy].push(if bits & column[dy] != 0 { '#' } else { '.' });
                    }
                }
            }
        }
        out
    }

    #[test]
    fn a_letter_is_its_glyph_in_dots() {
        let rows = render("H", 3, Style::Dots).unwrap();
        assert_eq!(rows.len(), 3, "3 terminal rows per line of text");
        assert!(rows.iter().all(|r| width(r) == 3), "5 dots wide fit in 3 columns");
        let p = picture(&rows);
        assert_eq!(&p[0..3], [".....", ".....", "....."].map(|s| format!("{s}.")), "no accent, then the gap");
        assert_eq!(&p[3..10], glyph('H').unwrap().map(|s| format!("{s}.")));
        assert_eq!(&p[10..12], ["......", "......"]);
    }

    #[test]
    fn the_solid_styles_draw_the_same_letter() {
        // "I" in half blocks: a character per dot across, two down
        let rows = render("I", 5, Style::Blocks).unwrap();
        assert_eq!(rows, ["     ", " ▄▄▄ ", "  █  ", "  █  ", " ▄█▄ ", "     "]);
        let solid = render("hola", 40, Style::Solid).unwrap();
        assert_eq!(solid.len(), 4);
        assert!(solid.iter().all(|r| width(r) == 40), "{solid:?}");
        assert_eq!([Style::Solid.glyph(1), Style::Solid.glyph(22), Style::Solid.glyph(62), Style::Solid.glyph(63)], ['\u{1FB00}', '\u{1FB14}', '\u{1FB3B}', '█']);
    }

    #[test]
    fn accents_wrapping_and_fallback() {
        let enye = picture(&render("ñ", 3, Style::Dots).unwrap());
        assert_eq!(&enye[0..2], TILDE.map(|s| format!("{s}.")), "the tilde sits above the N");
        assert_eq!(&enye[3..10], glyph('N').unwrap().map(|s| format!("{s}.")));
        assert_eq!(render("Á", 10, Style::Dots), render("á", 10, Style::Dots), "lower and upper case look the same");
        // 6 letters fit in 18 columns (35 dots); two words wrap onto two lines
        assert_eq!(render("hola mundo", 18, Style::Dots).unwrap().len(), 6);
        assert!(render("hola mundo", 18, Style::Dots).unwrap().iter().all(|r| width(r) == 18));
        assert_eq!(render("日本", 40, Style::Dots), None, "no big form: the caller shows it small");
        assert_eq!(render("x", 1, Style::Dots), None, "too narrow for a letter");
        assert!(render("¿qué tal?… ♪ (1, 2; 3)", 80, Style::Dots).is_some(), "Spanish punctuation and the note are in the font");
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            assert!(glyph(c).unwrap().iter().all(|r| r.len() == 5), "{c} is 5 dots wide");
        }
    }
}
