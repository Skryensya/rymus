// The Stats tab's charts: time listened over the period (per day, or per month for a year) and by hour of day.
use crate::ui::{dim, fg, Rgb};

const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn local(at_ms: i64) -> libc::tm {
    crate::platform::local_time(at_ms / 1000)
}
// Days since the epoch on the local calendar, so a bucket runs midnight to midnight.
fn day(tm: &libc::tm) -> i64 {
    // days from civil (Howard Hinnant), proleptic Gregorian
    let (y, m, d) = (tm.tm_year as i64 + 1900, tm.tm_mon as i64 + 1, tm.tm_mday as i64);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

// Seconds listened per bucket, oldest first, with a label for each: the last `days` days (named by weekday for a
// week, by date for longer), or the last 12 months for a year.
pub fn timeline(plays: &[(i64, i64)], now: i64, days: i64) -> (Vec<i64>, Vec<String>) {
    let today = local(now);
    if days > 31 {
        let month = |tm: &libc::tm| tm.tm_year as i64 * 12 + tm.tm_mon as i64;
        let mut v = vec![0; 12];
        for &(at, s) in plays {
            let ago = month(&today) - month(&local(at));
            if (0..12).contains(&ago) {
                v[11 - ago as usize] += s;
            }
        }
        let labels = (0..12).map(|i| MONTHS[(today.tm_mon as usize + 1 + i) % 12].to_string()).collect();
        return (v, labels);
    }
    let n = days as usize;
    let mut v = vec![0; n];
    for &(at, s) in plays {
        let ago = day(&today) - day(&local(at));
        if (0..days).contains(&ago) {
            v[n - 1 - ago as usize] += s;
        }
    }
    let labels = (0..n)
        .map(|i| {
            let tm = local(now - (n - 1 - i) as i64 * 86_400_000);
            if days <= 7 { DAY_NAMES[tm.tm_wday as usize].to_string() } else { tm.tm_mday.to_string() }
        })
        .collect();
    (v, labels)
}

pub fn by_hour(plays: &[(i64, i64)]) -> Vec<i64> {
    let mut v = vec![0; 24];
    for &(at, s) in plays {
        v[local(at).tm_hour as usize] += s;
    }
    v
}
pub fn hour_labels() -> Vec<String> {
    (0..24).map(|h| h.to_string()).collect()
}

// h rows of vertical bars (eighth blocks), then an axis row of labels: exactly w columns each. Labels that would
// touch the previous one are left out. Needs w >= values.len().
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub fn bars(values: &[i64], labels: &[String], w: usize, h: usize, accent: Rgb) -> Vec<String> {
    let n = values.len().max(1);
    let col = (w / n).max(1);
    let bar = if col >= 3 { col - 1 } else { col }; // a gap between wide bars
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let eighths: Vec<i64> = values.iter().map(|&v| if v > 0 { (v * 8 * h as i64 / max).max(1) } else { 0 }).collect();
    let pad = " ".repeat(w.saturating_sub(col * n));
    let mut rows: Vec<String> = (0..h)
        .map(|r| {
            let floor = (h - 1 - r) as i64 * 8;
            let s: String = eighths
                .iter()
                .map(|e| {
                    let c = BLOCKS[(e - floor).clamp(0, 8) as usize];
                    format!("{}{}", c.to_string().repeat(bar), " ".repeat(col - bar))
                })
                .collect();
            fg(accent, &s) + &pad
        })
        .collect();
    let mut axis = vec![' '; w];
    let mut free = 0; // first column a label may start at
    for (i, label) in labels.iter().enumerate() {
        let at = i * col;
        if at >= free && at + label.chars().count() <= w {
            for (k, ch) in label.chars().enumerate() {
                axis[at + k] = ch;
            }
            free = at + label.chars().count() + 1;
        }
    }
    rows.push(dim(&axis.into_iter().collect::<String>()));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut esc = false;
        for c in s.chars() {
            match (esc, c) {
                (false, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (false, _) => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn buckets() {
        let now = 1_764_000_000_000; // a fixed moment
        let plays = [(now, 60), (now - 86_400_000, 30), (now - 86_400_000, 30), (now - 40 * 86_400_000, 999)];
        let (week, labels) = timeline(&plays, now, 7);
        assert_eq!(week.len(), 7);
        assert_eq!((week[6], week[5], week.iter().sum::<i64>()), (60, 60, 120)); // 40 days ago is outside
        assert_eq!(labels[6], DAY_NAMES[local(now).tm_wday as usize]);
        let (year, labels) = timeline(&plays, now, 365);
        assert_eq!((year.len(), year.iter().sum::<i64>(), year[11] >= 60), (12, 1119, true));
        assert_eq!(labels[11], MONTHS[local(now).tm_mon as usize]);
        assert_eq!(by_hour(&plays).iter().sum::<i64>(), 1119);
        assert_eq!(day(&local(now)) - day(&local(now - 86_400_000)), 1);
    }

    #[test]
    fn drawing() {
        let rows = bars(&[0, 4, 8], &["a".into(), "bb".into(), "c".into()], 10, 2, [255, 255, 255]);
        let plain: Vec<String> = rows.iter().map(|r| strip(r)).collect();
        assert!(plain.iter().all(|r| r.chars().count() == 10));
        assert_eq!(plain[0], "      ██  ");
        assert_eq!(plain[1], "   ██ ██  ");
        assert_eq!(plain[2], "a  bb c   ");
        // labels that would touch are dropped
        assert_eq!(strip(&bars(&[1; 4], &["10".into(), "11".into(), "12".into(), "13".into()], 4, 1, [0, 0, 0])[1]), "10  ");
    }
}
