// Big text for karaoke, in braille: a 5×7 dot font (capitals, digits, punctuation) where each braille character
// holds 2×4 dots. A letter is 6 dots wide (5 and a space) and 12 tall: 2 rows for its accent, a gap, the 7 rows of
// the letter and 2 rows of space under it; that is 3 columns by 3 rows of the terminal.

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

// The text as rows that fit in w columns, wrapped at spaces: 3 terminal rows per line of text, each exactly w
// columns, centered. None when a character has no big form (the caller shows it small instead).
pub fn render(text: &str, w: usize) -> Option<Vec<String>> {
    let text = text.replace('…', "...");
    let letters: Vec<([&str; 2], char)> = text.chars().map(split).collect::<Option<_>>()?;
    let per_line = (2 * w + 1) / LETTER; // a line of n letters is 6n - 1 dots, two dots per column
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
        let width = dots.div_ceil(2);
        let pad = w.saturating_sub(width) / 2;
        for ty in 0..HEIGHT / 4 {
            let mut s = " ".repeat(pad);
            for tx in 0..width {
                let mut bits = 0;
                for (dx, column) in BITS.iter().enumerate() {
                    for (dy, bit) in column.iter().enumerate() {
                        let x = 2 * tx + dx;
                        if x < dots && dot(ls, x, 4 * ty + dy) {
                            bits |= bit;
                        }
                    }
                }
                s.push(if bits == 0 { ' ' } else { char::from_u32(0x2800 + bits).unwrap_or(' ') });
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
        let rows = render("H", 3).unwrap();
        assert_eq!(rows.len(), 3, "3 terminal rows per line of text");
        assert!(rows.iter().all(|r| width(r) == 3), "5 dots wide fit in 3 columns");
        let p = picture(&rows);
        assert_eq!(&p[0..3], [".....", ".....", "....."].map(|s| format!("{s}.")), "no accent, then the gap");
        assert_eq!(&p[3..10], glyph('H').unwrap().map(|s| format!("{s}.")));
        assert_eq!(&p[10..12], ["......", "......"]);
    }

    #[test]
    fn accents_wrapping_and_fallback() {
        let enye = picture(&render("ñ", 3).unwrap());
        assert_eq!(&enye[0..2], TILDE.map(|s| format!("{s}.")), "the tilde sits above the N");
        assert_eq!(&enye[3..10], glyph('N').unwrap().map(|s| format!("{s}.")));
        assert_eq!(render("Á", 10), render("á", 10), "lower and upper case look the same");
        // 6 letters fit in 18 columns (35 dots); two words wrap onto two lines
        assert_eq!(render("hola mundo", 18).unwrap().len(), 6);
        assert!(render("hola mundo", 18).unwrap().iter().all(|r| width(r) == 18));
        assert_eq!(render("日本", 40), None, "no big form: the caller shows it small");
        assert_eq!(render("x", 1), None, "too narrow for a letter");
        assert!(render("¿qué tal?… ♪ (1, 2; 3)", 80).is_some(), "Spanish punctuation and the note are in the font");
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            assert!(glyph(c).unwrap().iter().all(|r| r.len() == 5), "{c} is 5 dots wide");
        }
    }
}
