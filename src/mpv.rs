// mpv runs headless and is driven over its JSON IPC socket; the visualizer's measurements come back through fd 3.
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct Mpv {
    child: Child,
    sock: Option<UnixStream>,
    pub path: String,
}

fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    for fd in fds {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

impl Mpv {
    // Returns the player and the read end of its fd 3, which must always be drained or the audio filter blocks.
    pub fn spawn(volume: i64) -> std::io::Result<(Mpv, File)> {
        // shortcut: macOS caps unix socket paths at ~104 chars, so /tmp, not the temp dir
        let path = format!("/tmp/ymus-{}.sock", std::process::id());
        let (read, write) = pipe()?;
        let mut cmd = Command::new("mpv");
        cmd.args(["--idle=yes", "--no-video", "--no-terminal", "--ytdl-format=bestaudio/best"])
            .arg(format!("--input-ipc-server={path}"))
            // open the next entry early and buffer its first seconds
            .args(["--prefetch-playlist=yes", "--demuxer-readahead-secs=30"])
            .arg(format!("--volume={volume}"));
        if let Ok(extra) = std::env::var("YM_MPV_ARGS") {
            cmd.args(extra.split_whitespace()); // e.g. YM_MPV_ARGS=--ao=null for silent runs
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let raw = write.as_raw_fd();
        unsafe {
            cmd.pre_exec(move || {
                // dup2 clears close-on-exec on the copy; if the pipe already is fd 3, clear it by hand
                let ok = if raw == 3 { libc::fcntl(3, libc::F_SETFD, 0) } else { libc::dup2(raw, 3) };
                if ok < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let child = cmd.spawn()?;
        drop(write);
        Ok((Mpv { child, sock: None, path }, File::from(read)))
    }

    pub fn connect(&mut self) -> std::io::Result<UnixStream> {
        let mut last = None;
        for _ in 0..100 {
            match UnixStream::connect(&self.path) {
                Ok(s) => {
                    let reader = s.try_clone()?;
                    self.sock = Some(s);
                    return Ok(reader);
                }
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("mpv did not start")))
    }

    pub fn cmd(&mut self, command: Value) {
        if let Some(s) = &mut self.sock {
            let _ = writeln!(s, "{}", serde_json::json!({ "command": command }));
        }
    }

    pub fn stop(&mut self) {
        self.cmd(serde_json::json!(["quit"]));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}
