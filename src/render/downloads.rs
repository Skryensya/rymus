// The Downloads window, over whatever is on screen (B, or Config): where songs go and how much room they take, the
// settings, and what to do: download, read your own music, free up space.
use crate::i18n::t;
use crate::i18n::tr;
use super::eq::{keys_line, overlay, plain, window_at};
use super::*;
use crate::app::downloads::{BUTTONS, SETTINGS};
use crate::{config, disk};

impl App {
    pub(super) fn downloads_window(&mut self, v: &View, g: &mut Geo, behind: Vec<String>) -> Vec<String> {
        let a = v.a;
        let (x, y, ww, wh, screen_row) = window_at(v, g, behind.len(), 90);
        let (iw, l) = (ww.saturating_sub(2), wh.saturating_sub(2));
        let sel = self.downloads.sel;
        let folder = self.db.get_setting("dlfolder").filter(|f| !f.is_empty()).unwrap_or_else(|| "~/Music/rymus".into());
        let ours = self.downloads.files.keys().filter(|id| !id.starts_with(disk::OWN)).count();
        let own = self.downloads.own.len();

        // what is there now
        let mut rows = vec![
            format!(" {} {}", dim(tr!("Folder")), bold(&cut(&format!("{folder} · {}", disk::human(self.downloads.used)), iw.saturating_sub(9)))),
            format!(" {}", dim(&cut(&self.downloads_status(ours, own), iw.saturating_sub(2)))),
            String::new(),
        ];
        // the rows you move through: settings with their choices, then the buttons
        let lw = SETTINGS.iter().map(|k| width(config_label(k))).max().unwrap_or(0) + 3;
        let mut about = (String::new(), String::new());
        for (i, key) in SETTINGS.iter().enumerate() {
            let value = config::value(&self.db, key);
            let on = i == sel;
            let chips: String = config::choices(key)
                .iter()
                .map(|c| {
                    let (chosen, c) = (*c == value, t(c));
                    if chosen { fg(a, &bold(&format!("[{c}]"))) } else if on { format!(" {c} ") } else { dim(&format!(" {c} ")) }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let label = cell(config_label(key), lw);
            g.links.push((Action::PickDownloads(i), x + 1, x + ww, screen_row + 1 + rows.len()));
            rows.push(if on { format!("{} {}{chips}", fg(a, "▌"), fg(a, &bold(&label))) } else { format!("  {label}{chips}") });
            if on {
                about = (format!("{} · {}", config_label(key), t(value)), config_hint(key).to_string());
            }
        }
        rows.push(String::new());
        for (j, (id, hint)) in BUTTONS.iter().enumerate() {
            let i = SETTINGS.len() + j;
            let title = match *id {
                "folder" => tr!("Change folder…").to_string(),
                "now" => tr!("Download what plays{a}", a = self.now().map(|t| format!(": {}", t.title)).unwrap_or_default()),
                "liked" => tr!("Download all liked songs").to_string(),
                _ => tr!("Free up space…").to_string(),
            };
            g.links.push((Action::PickDownloads(i), x + 1, x + ww, screen_row + 1 + rows.len()));
            let title = cut(&title, iw.saturating_sub(3));
            rows.push(if i == sel { format!("{} {}", fg(a, "▌"), fg(a, &bold(&title))) } else { format!("  {title}") });
            if i == sel {
                about = (title.trim_end_matches('…').to_string(), t(hint).to_string());
            }
        }
        // what the row under the cursor does
        rows.push(String::new());
        rows.push(format!(" {}", fg(a, &bold(&cut(&about.0, iw.saturating_sub(2))))));
        let hint = about.1;
        let text = crate::lyrics::wrap(&capital(&hint), iw.saturating_sub(2));
        rows.extend(text.into_iter().take(2).map(|t| format!(" {t}")));

        let keys: &[(&str, &str)] = if sel < SETTINGS.len() { &[("↑↓", "choose"), ("←→", "change"), ("b", tr!("download what plays")), ("esc", "close")] } else { &[("↑↓", "choose"), ("⏎", "run"), ("b", tr!("download what plays")), ("esc", "close")] };
        rows.resize(l.saturating_sub(1), String::new());
        rows.truncate(l.saturating_sub(1));
        let mut rows: Vec<String> = rows.into_iter().map(|r| pad(&r, iw)).collect();
        rows.push(keys_line(keys, iw, a));
        let (title, color) = v.title(tr!("Downloads")); // notes and errors show here too
        overlay(behind, &frame(&title, color, &rows, ww, a), x, y, v.w)
    }
    fn downloads_status(&self, ours: usize, own: usize) -> String {
        let mut s = tr!("{a} downloaded", a = if ours == 1 { tr!("1 song").into() } else { tr!("{ours} songs", ours = ours) });
        if config::value(&self.db, "library") == "on" {
            s += &tr!(" · {own} of your own", own = own);
        }
        match self.downloads.waiting() {
            0 => {}
            n => s += &tr!(" · downloading, {n} to go", n = n),
        }
        s
    }
}

fn capital(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
}
fn config_label(key: &str) -> &'static str {
    config::SETTINGS.iter().find(|s| s.key == key).map_or("", |s| t(s.label))
}
fn config_hint(key: &str) -> &'static str {
    config::SETTINGS.iter().find(|s| s.key == key).map_or("", |s| t(s.hint))
}
fn pad(s: &str, w: usize) -> String {
    let n = width(&plain(s));
    if n > w { cut(&plain(s), w) } else { format!("{s}{}", spaces(w - n)) }
}
