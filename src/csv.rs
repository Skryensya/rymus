// CSV as spreadsheets read and write it (RFC 4180): a field with a comma, a quote or a line break is quoted, quotes
// doubled. Used to export your data as files anyone can open, and to read them back.

pub fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// A text a spreadsheet would run as a formula (=, +, @, or - not starting a number) is written with a ' in front,
// which shows it as text; reading it back takes the ' away again.
pub fn defuse(s: &str) -> String {
    let mut c = s.chars();
    let risky = match c.next() {
        Some('=' | '+' | '@' | '\t' | '\r') => true,
        Some('-') => !c.next().is_some_and(|d| d.is_ascii_digit() || d == '.'),
        Some('\'') => defuse(&s[1..]) != s[1..], // already one: kept apart from ours
        _ => false,
    };
    if risky { format!("'{s}") } else { s.to_string() }
}
pub fn revive(s: &str) -> &str {
    match s.strip_prefix('\'') {
        Some(rest) if defuse(rest) != rest => rest,
        _ => s,
    }
}

pub fn line(fields: &[String]) -> String {
    fields.iter().map(|f| field(f)).collect::<Vec<_>>().join(",") + "\r\n"
}

// Every record of a whole file; a quoted field may hold line breaks.
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cur) = (Vec::new(), Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, _) => cur.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cur)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cur));
                rows.push(std::mem::take(&mut row));
            }
            (false, _) => cur.push(c),
        }
    }
    if !cur.is_empty() || !row.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let rows = vec![vec!["id".to_string(), "title".into()], vec!["a1".into(), "Hey, \"you\"\nsecond line".into()], vec!["a2".into(), String::new()]];
        let text: String = rows.iter().map(|r| line(r)).collect();
        assert_eq!(parse(&text), rows);
        assert_eq!(field("plain"), "plain");
        for s in ["=HYPERLINK(\"x\")", "+1", "@SUM(A1)", "-cmd", "'=already", "-12", "-.5", "plain", "", "'quoted"] {
            assert_eq!(revive(&defuse(s)), s, "{s}");
        }
        assert_eq!(defuse("=1+1"), "'=1+1");
        assert_eq!(defuse("-12"), "-12");
    }
}
