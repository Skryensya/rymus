// The pages shown over the lists that any key closes, besides the keys page (help.rs): who made a song or an album,
// and what changed in this version.
use crate::i18n::tr;
use super::*;

// The credits panel: an album's facts first, then each role with its names, and the link it shares.
pub(super) fn credits_rows(c: &crate::app::Credits, w: usize, a: Rgb) -> Vec<String> {
    let tw = w.saturating_sub(6).max(10);
    let mut rows = vec![String::new()];
    // an album's facts (kind and year, songs and length) first; its description, long, after the credits
    let (facts, story) = c.about.split_at(c.about.len().min(2));
    for line in facts {
        rows.extend(crate::lyrics::wrap(line, tw).into_iter().map(|l| format!("   {l}")));
    }
    if !facts.is_empty() {
        rows.push(String::new());
    }
    if c.roles.is_empty() {
        rows.push(format!("   {}", dim(tr!("YouTube Music has no credits for this one"))));
    }
    for (role, names) in &c.roles {
        rows.push(format!("   {}", fg(a, &bold(role))));
        for l in crate::lyrics::wrap(&names.join(" · "), tw) {
            rows.push(format!("     {l}"));
        }
        rows.push(String::new());
    }
    for line in story {
        rows.extend(crate::lyrics::wrap(line, tw).into_iter().map(|l| format!("   {}", dim(&l))));
        rows.push(String::new());
    }
    rows.push(format!("   {}  {}", dim(tr!("Link")), crate::app::credits::share_url(&c.item)));
    rows.into_iter().map(|r| pad_styled(&r, w)).collect()
}

// What changed in this version, from its release notes: each point with an accent bullet, then the release's page.
pub(super) fn news_rows(tag: &str, lines: &[String], w: usize, a: Rgb) -> Vec<String> {
    let tw = w.saturating_sub(6).max(10);
    let mut rows = vec![String::new()];
    for line in lines {
        let (lead, rest) = match line.strip_prefix("• ") {
            Some(point) => (fg(a, "•"), point),
            None => (" ".to_string(), line.as_str()),
        };
        for (k, part) in crate::lyrics::wrap(rest, tw).into_iter().enumerate() {
            rows.push(format!("   {} {part}", if k == 0 { lead.as_str() } else { " " }));
        }
    }
    rows.push(String::new());
    rows.push(format!("   {}", dim(&crate::update::page(tag))));
    rows.into_iter().map(|r| pad_styled(&r, w)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::visible;

    #[test]
    fn help_fits_its_width() {
        for w in [30, 70, 98, 110, 200] {
            let rows = help_rows(w, GREEN);
            assert!(rows.iter().all(|r| width(&visible(r)) == w), "width {w}");
        }
        let wide = help_rows(110, GREEN);
        assert!(wide.len() <= 22, "three columns fit a small screen: {}", wide.len());
        assert!(wide.iter().any(|r| visible(r).contains("add to queue")));
    }
}
