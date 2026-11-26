// Cover art via the kitty graphics protocol (Ghostty, kitty, WezTerm); ffmpeg converts any image to PNG.
// Other terminals get colored half blocks.
use base64::Engine;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

pub fn supported() -> bool {
    let var = |k| std::env::var(k).unwrap_or_default().to_lowercase();
    let (program, term) = (var("TERM_PROGRAM"), var("TERM"));
    std::env::var_os("KITTY_WINDOW_ID").is_some_and(|v| !v.is_empty())
        || program.contains("ghostty") || program.contains("wezterm")
        || term.contains("kitty") || term.contains("ghostty")
}

pub struct Got {
    pub b64: String,      // PNG
    pub rgb: [u8; 3],     // its most vivid color
    pub palette: Vec<[u8; 3]>, // its main colors, dark to light
    pub image: Vec<u8>,   // as downloaded
}

fn ffmpeg(input: &[u8], args: &[&str]) -> Option<Vec<u8>> {
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i", "pipe:0"])
        .args(args)
        .arg("pipe:1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = child.wait_with_output().ok()?;
    let _ = writer.join();
    out.status.success().then_some(out.stdout)
}

// Pixels grouped into 12 hue buckets, each weighted by saturation x brightness: its total, its most vivid pixel, and
// its weighted color sum. Grays have no hue and are left out.
#[derive(Clone, Copy, Default)]
struct Bucket {
    total: u64,
    top: u64,
    best: Option<[u8; 3]>,
    sum: [u64; 3],
}
fn buckets(px: &[u8]) -> [Bucket; 12] {
    let mut out = [Bucket::default(); 12];
    for p in px.as_chunks::<3>().0 {
        let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        if d == 0 {
            continue;
        }
        let h = if max == r {
            ((g - b) as f64 / d as f64 + 6.0) % 6.0
        } else if max == g {
            (b - r) as f64 / d as f64 + 2.0
        } else {
            (r - g) as f64 / d as f64 + 4.0
        };
        let k = &mut out[(h * 2.0).floor() as usize % 12];
        let s = (d * max) as u64;
        k.total += s;
        for i in 0..3 {
            k.sum[i] += p[i] as u64 * s;
        }
        if k.best.is_none() || s > k.top {
            k.top = s;
            k.best = Some([p[0], p[1], p[2]]);
        }
    }
    out
}

// The cover's dominant color: the most vivid pixel of the heaviest hue bucket.
pub fn vivid(px: &[u8]) -> [u8; 3] {
    let all = buckets(px);
    let heaviest = all.iter().fold(all[0], |a, b| if b.total > a.total { *b } else { a });
    heaviest.best.unwrap_or_else(|| if px.len() >= 3 { [px[0], px[1], px[2]] } else { [0; 3] })
}

// The palette for the visualizer: the average color of its up to 4 heaviest hues (a bucket needs 5% of the weight to
// count), dark to light. A gray cover gives its dark and light grays instead.
pub fn palette(px: &[u8]) -> Vec<[u8; 3]> {
    let all = buckets(px);
    let weight: u64 = all.iter().map(|b| b.total).sum();
    let lum = |c: &[u8; 3]| c[0] as u32 * 30 + c[1] as u32 * 59 + c[2] as u32 * 11;
    let mut main: Vec<&Bucket> = all.iter().filter(|b| b.total * 20 > weight).collect();
    main.sort_by(|a, b| b.total.cmp(&a.total));
    let mut colors: Vec<[u8; 3]> = main.iter().take(4).map(|b| b.sum.map(|v| (v as f64 / b.total as f64).round() as u8)).collect();
    if colors.len() < 2 {
        // too few hues: add the cover's shades
        let mut shades: Vec<[u8; 3]> = px.as_chunks::<3>().0.iter().map(|p| [p[0], p[1], p[2]]).collect();
        shades.sort_by_key(lum);
        for share in [0.2, 0.8] {
            colors.extend(shades.get((shades.len() as f64 * share) as usize));
        }
    }
    colors.sort_by_key(lum);
    colors
}

// The size asked for the full player; lists and the player dock make do with what YouTube links to.
const LARGE: &str = "=w1200-h1200";

// The address of a bigger copy of a cover, for the hosts that take the size in the link ("...=w544-h544-l90-rj").
pub fn large(url: &str) -> Option<String> {
    if !url.contains("googleusercontent.com/") && !url.contains("ggpht.com/") {
        return None;
    }
    let (base, size) = url.rsplit_once("=w")?;
    let (w, rest) = size.split_once("-h")?;
    let after = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (number(w) && after.len() < rest.len()).then(|| format!("{base}{LARGE}{after}"))
}
pub fn is_large(url: &str) -> bool {
    url.contains(LARGE)
}

pub fn load(url: &str) -> Option<Got> {
    let mut image = Vec::new();
    crate::yt::agent().get(url).call().ok()?.into_reader().take(10_000_000).read_to_end(&mut image).ok()?;
    let scale = if is_large(url) { "scale='min(1200,iw)':-1" } else { "scale=400:-1" };
    let png = ffmpeg(&image, &["-vf", scale, "-f", "image2pipe", "-vcodec", "png"])?;
    let px = ffmpeg(&image, &["-vf", "scale=16:16:flags=area", "-f", "rawvideo", "-pix_fmt", "rgb24"])?;
    Some(Got { b64: base64::engine::general_purpose::STANDARD.encode(png), rgb: vivid(&px), palette: palette(&px), image })
}

pub const CLEAR: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

// Removes the image with this id (each spot on screen has its own, so changing one leaves the others).
pub fn delete(id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

// Draw at 1-indexed row/col, `cols` cells wide (height follows the image aspect), as image `id`. q=2: the terminal
// sends no replies.
pub fn place(b64: &str, row: usize, col: usize, cols: usize, id: u32) -> String {
    let mut s = format!("{}\x1b[{row};{col}H", delete(id));
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(4096).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let head = if i == 0 { format!("a=T,f=100,i={id},c={cols},C=1,") } else { String::new() };
        let more = u8::from(i + 1 < chunks.len());
        s.push_str(&format!("\x1b_G{head}q=2,m={more};{}\x1b\\", String::from_utf8_lossy(chunk)));
    }
    s
}

// h rows of w half blocks: foreground = top pixel, background = bottom pixel.
pub fn cells(image: &[u8], w: usize, h: usize) -> Option<Vec<String>> {
    let scale = format!("scale={w}:{}:flags=area", h * 2);
    let px = ffmpeg(image, &["-vf", &scale, "-f", "rawvideo", "-pix_fmt", "rgb24"])?;
    if px.len() < w * h * 6 {
        return None;
    }
    let rgb = |x: usize, y: usize| {
        let i = (y * w + x) * 3;
        format!("{};{};{}", px[i], px[i + 1], px[i + 2])
    };
    Some((0..h)
        .map(|y| (0..w).map(|x| format!("\x1b[38;2;{};48;2;{}m▀", rgb(x, 2 * y), rgb(x, 2 * y + 1))).collect::<String>() + "\x1b[0m")
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigger_covers() {
        let big = large("https://yt3.googleusercontent.com/ab=w120-h120-l90-rj").unwrap();
        assert_eq!(big, "https://yt3.googleusercontent.com/ab=w1200-h1200-l90-rj");
        assert!(is_large(&big));
        assert_eq!(large("https://i.ytimg.com/vi/x/sddefault.jpg"), None);
        assert_eq!(large("https://lh3.googleusercontent.com/ab"), None);
    }

    #[test]
    fn dominant_color() {
        let mut px: Vec<u8> = [90, 150, 70].repeat(20);
        px.extend([250, 20, 20]);
        assert_eq!(vivid(&px), [90, 150, 70]); // one pure red pixel does not beat a green cover
        assert_eq!(vivid(&[128; 6]), [128, 128, 128]);
        let two: Vec<u8> = [[90, 150, 70].repeat(15), [200, 30, 30].repeat(5)].concat();
        assert_eq!(palette(&two), [[200, 30, 30], [90, 150, 70]]); // dark to light
        let gray: Vec<u8> = [[20, 20, 20].repeat(5), [220, 220, 220].repeat(5)].concat();
        assert_eq!(palette(&gray), [[20, 20, 20], [220, 220, 220]]); // a gray cover: its shades
    }
}

#[cfg(test)]
mod image_ids {
    use super::*;

    #[test]
    fn each_image_replaces_only_itself() {
        let one = place("AAAA", 3, 4, 10, 1);
        assert!(one.starts_with(&delete(1)), "the old copy of image 1 goes first");
        assert!(one.contains("a=T,f=100,i=1,c=10,"));
        assert!(!one.contains("d=A") && !one.contains("i=2"), "image 2 is never touched");
        assert_eq!(delete(2), "\x1b_Ga=d,d=I,i=2,q=2\x1b\\");
        let long = place(&"A".repeat(10_000), 1, 1, 5, 2);
        assert_eq!(long.matches("i=2,c=5").count(), 1, "the id is sent once, with the first chunk");
    }
}

