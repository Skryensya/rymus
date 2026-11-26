// New versions of rymus: GitHub's latest release, looked at once a day at most (Config > Updates turns it off), and
// put in place of this binary by `rymus update` or Config's Update row. Only the release's own name is asked for; no
// data of yours goes with it. The download is checked against the release's sha256 before it replaces anything.
use crate::i18n::tr;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "Skryensya/rymus";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Whether release `tag` ("v0.5.0") is newer than version `current` ("0.4.0").
pub fn newer(tag: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(tag) > parts(current)
}

// The name the release's archive has for this machine.
fn target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

// The latest release's tag, e.g. "v0.5.0".
pub fn latest() -> Result<String, String> {
    let v: serde_json::Value = crate::yt::agent()
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("User-Agent", "rymus")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    v["tag_name"].as_str().map(str::to_string).ok_or_else(|| tr!("no release found").into())
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    crate::yt::agent().get(url).set("User-Agent", "rymus").call().map_err(|e| e.to_string())?.into_reader().take(200_000_000).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    std::fs::write(to, bytes).map_err(|e| e.to_string())
}

fn sha256(file: &Path) -> Option<String> {
    // macOS has shasum, Linux sha256sum
    ["shasum -a 256", "sha256sum"].iter().find_map(|tool| {
        let mut words = tool.split(' ');
        let out = Command::new(words.next()?).args(words).arg(file).output().ok().filter(|o| o.status.success())?;
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string)
    })
}

// Downloads release `tag` and puts its binary where this one is. Returns where it went.
pub fn install(tag: &str) -> Result<PathBuf, String> {
    let target = target().ok_or(tr!("there is no ready-made rymus for this computer: build it from source"))?;
    let name = format!("rymus-{tag}-{target}");
    let base = format!("https://github.com/{REPO}/releases/download/{tag}/{name}.tar.gz");
    // /tmp is shared on Linux: a folder made now, readable by you alone, so no one can swap the download after it is checked
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let work = std::env::temp_dir().join(format!("rymus-update-{}-{nanos}", std::process::id()));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(&work).map_err(|e| e.to_string())?; // fails if it exists
    let archive = work.join(format!("{name}.tar.gz"));
    download(&base, &archive)?;
    download(&format!("{base}.sha256"), &work.join("sum"))?;
    let want = std::fs::read_to_string(work.join("sum")).unwrap_or_default().split_whitespace().next().unwrap_or("").to_string();
    if want.is_empty() || sha256(&archive).as_deref() != Some(want.as_str()) {
        return Err(tr!("the download does not match its checksum; nothing was changed").into());
    }
    let ok = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(&work).status().is_ok_and(|s| s.success());
    let new = work.join(&name).join("rymus");
    if !ok || !new.is_file() {
        return Err(tr!("could not unpack the download; nothing was changed").into());
    }
    // the real file behind a link (e.g. Homebrew's bin), replaced in one step: a running rymus keeps its old copy
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| e.to_string())?;
    let staged = exe.with_extension("new");
    std::fs::copy(&new, &staged).map_err(|e| tr!("cannot write next to {a}: {e}", a = exe.display(), e = e))?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    std::fs::rename(&staged, &exe).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(exe)
}

// The release's page on GitHub, where its notes are read in full.
pub fn page(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/{tag}")
}

// What changed in release `tag`: the first section of its notes, as plain lines ("• " for each point).
pub fn notes(tag: &str) -> Result<Vec<String>, String> {
    #[cfg(test)]
    let body = format!("## What's new\n\n- **Credits.** Who made a song (`i`).\n- Share links.\n\n## Install\n\n```sh\ntar -xzf rymus-{tag}.tar.gz\n```");
    #[cfg(not(test))]
    let body = {
        let v: serde_json::Value = crate::yt::agent()
            .get(&format!("https://api.github.com/repos/{REPO}/releases/tags/{tag}"))
            .set("User-Agent", "rymus")
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())?;
        v["body"].as_str().unwrap_or_default().to_string()
    };
    Ok(what_changed(&body))
}
fn what_changed(body: &str) -> Vec<String> {
    let mut lines = vec![];
    let mut headings = 0;
    for line in body.lines().map(str::trim) {
        if line.starts_with("## ") {
            headings += 1;
            if headings > 1 {
                break; // past "What's new": how to install, and the rest
            }
            continue;
        }
        let line = line.strip_prefix("- ").or(line.strip_prefix("* ")).map_or(line.to_string(), |l| format!("• {l}"));
        let plain = crate::item::clean(&line.replace("**", "").replace('`', "").replace('*', ""));
        if !(plain.is_empty() && lines.last().is_none_or(|l: &String| l.is_empty())) {
            lines.push(plain);
        }
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

// `rymus update`
pub fn run() -> Result<(), String> {
    let tag = latest()?;
    if !newer(&tag, VERSION) {
        println!("{}", tr!("rymus {v} is the latest", v = VERSION));
        return Ok(());
    }
    println!("{}", tr!("Updating rymus {v} to {tag}…", v = VERSION, tag = tag));
    let exe = install(&tag)?;
    println!("{}", tr!("Done: {a}. A running rymus picks it up with Cmd Shift R (or Ctrl R).", a = exe.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("v0.5.0", "0.4.0"));
        assert!(newer("v0.10.0", "0.9.3"), "10 is more than 9, not less");
        assert!(newer("v1.0.0", "0.42.7"));
        assert!(!newer("v0.4.0", "0.4.0"));
        assert!(!newer("v0.3.9", "0.4.0"));
        assert!(target().is_some());
    }

    #[test]
    fn notes_keep_what_changed_without_markdown() {
        let lines = notes("v9.0.0").unwrap();
        assert_eq!(lines, ["• Credits. Who made a song (i).", "• Share links."]);
        assert_eq!(page("v9.0.0"), "https://github.com/Skryensya/rymus/releases/tag/v9.0.0");
    }
}
