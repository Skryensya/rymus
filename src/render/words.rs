// The lyrics view (y) and karaoke (Y).
use super::*;

impl App {
    // Karaoke: the line being sung in big braille letters, the one before and the one after small around it.
    #[allow(clippy::too_many_arguments)]
    // Under it, 4 dots fill up on the way to the next line (see lyrics::dots_filled). All of it is centred on the same
    // middle column.
    pub(super) fn karaoke_rows(&self, lines: &[String], current: Option<usize>, synced: bool, dots: Option<usize>, w: usize, l: usize, a: Rgb, center: impl Fn(&str, String) -> String) -> Vec<String> {
        if !synced {
            let text = cut("Karaoke needs synced lyrics · y shows the words", w);
            return (0..l).map(|i| if i == l / 2 { center(&text, dim(&text)) } else { spaces(w) }).collect();
        }
        // 5 columns of margin on each side (1 on a very narrow screen), so the words never touch the frame
        let margin = if w >= 40 { 5 } else { 1 };
        let tw = w.saturating_sub(2 * margin);
        let small = |n: Option<usize>| -> Vec<String> {
            n.and_then(|n| lines.get(n)).map_or(vec![], |t| lyrics::wrap(t, tw).into_iter().take(2).collect())
        };
        // before the first line: a note, with the first line coming next
        let (sung, before, after) = match current {
            Some(c) => (lines[c].clone(), small(c.checked_sub(1)), small(Some(c + 1))),
            None => ("♪".to_string(), vec![], small(Some(0))),
        };
        // the last row is the countdown's, against the bottom border; the rest is centred in what is left above it
        let body = l.saturating_sub(1);
        let room = body.saturating_sub(before.len() + after.len() + 2); // a blank row on each side of the big line
        // drawn inside the margins, then given them back on each side: centred like everything else
        let side = spaces(margin);
        let big = crate::bigtext::render(&sung, tw)
            .map(|rows| rows.into_iter().map(|r| format!("{side}{r}{side}")).collect::<Vec<_>>())
            .filter(|r| r.len() <= room)
            .unwrap_or_else(|| lyrics::wrap(&sung, tw).into_iter().map(|t| center(&t, t.clone())).collect());
        let mut rows: Vec<String> = before.iter().map(|t| center(t, dim(t))).collect();
        rows.push(spaces(w));
        rows.extend(big.into_iter().map(|r| fg(a, &bold(&pad(&r, w)))));
        rows.push(spaces(w));
        rows.extend(after.iter().map(|t| center(t, t.clone())));
        rows.truncate(body);
        let top = (body - rows.len()) / 2;
        let mut out = vec![spaces(w); top];
        out.extend(rows);
        out.resize(body, spaces(w));
        // the countdown: filled dots in the accent colour, the empty ones dim
        out.push(match dots {
            Some(filled) => {
                let plain = (0..lyrics::DOTS).map(|i| if i < filled { "●" } else { "○" }).collect::<Vec<_>>().join(" ");
                let styled = (0..lyrics::DOTS).map(|i| if i < filled { fg(a, "●") } else { dim("○") }).collect::<Vec<_>>().join(" ");
                center(&plain, styled)
            }
            None => spaces(w),
        });
        out.truncate(l);
        out
    }

    // The l rows of the lyrics view. Synced lyrics follow the song (unless you just scrolled); plain ones are scrolled
    // by hand. Hands over to karaoke_rows when karaoke is on.
    pub(super) fn lyric_rows(&mut self, w: usize, l: usize, a: Rgb) -> Vec<String> {
        let center = |text: &str, styled: String| {
            let n = width(text);
            let p = w.saturating_sub(n) / 2;
            format!("{}{styled}{}", spaces(p), spaces(w.saturating_sub(p + n)))
        };
        let ready = self.want_lyrics().is_some();
        let message = if self.now().is_none() {
            "Nothing playing".to_string()
        } else {
            match self.lyrics_ready().filter(|_| ready) {
                None => format!("{} Looking for lyrics…", spin()),
                Some(Lyrics::Error) => "Could not reach LRCLIB. Press y twice to retry.".into(),
                Some(Lyrics::Instrumental) => "♪ Instrumental ♪".into(),
                Some(Lyrics::Synced(v)) if !v.is_empty() => String::new(),
                Some(Lyrics::Plain(_)) => String::new(),
                _ => "No lyrics found on LRCLIB.".into(),
            }
        };
        if !message.is_empty() {
            let text = cut(&message, w);
            return (0..l).map(|i| if i == l / 2 { center(&text, dim(&text)) } else { spaces(w) }).collect();
        }
        let (current, synced, lines): (Option<usize>, bool, Vec<String>) = match self.lyrics_ready() {
            Some(Lyrics::Synced(v)) => {
                (self.line_at(v), true, v.iter().map(|x| if x.1.is_empty() { "♪".to_string() } else { x.1.clone() }).collect())
            }
            Some(Lyrics::Plain(v)) => (None, false, v.clone()),
            _ => (None, false, vec![]),
        };
        self.lyric_line = current;
        if self.karaoke {
            self.lyric_dots = match self.lyrics_ready() {
                Some(Lyrics::Synced(v)) => self.dots_at(v),
                _ => None,
            };
            return self.karaoke_rows(&lines, current, synced, self.lyric_dots, w, l, a, center);
        }
        let rows: Vec<(String, usize)> =
            lines.iter().enumerate().flat_map(|(n, t)| lyrics::wrap(t, w.saturating_sub(if w >= 40 { 10 } else { 4 })).into_iter().map(move |p| (p, n))).collect(); // 5 columns of margin a side
        // synced words follow the song, unless you just scrolled them; scrolling starts from what is on screen
        let top = if synced && self.lyrics_follow() {
            let top = rows.iter().position(|x| x.1 == current.unwrap_or(0)).unwrap_or(0).saturating_sub(l / 2);
            self.lyric_scroll = top;
            top
        } else {
            self.lyric_scroll = self.lyric_scroll.min(rows.len().saturating_sub(l));
            self.lyric_scroll
        };
        (0..l)
            .map(|i| match rows.get(top + i) {
                None => spaces(w),
                Some((part, n)) => {
                    let styled = if !synced {
                        part.clone()
                    } else if current == Some(*n) {
                        fg(a, &bold(part))
                    } else {
                        dim(part)
                    };
                    center(part, styled)
                }
            })
            .collect()
    }
}
