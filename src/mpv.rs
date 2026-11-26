// mpv runs headless and is driven over its JSON IPC channel; the visualizer's measurements come back through a named
// pipe. It lives apart from the terminal, so it can keep playing after rymus is gone and be found again by the next one.
use crate::platform::{self, Input, Output};
use serde_json::Value;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct Mpv {
    child: Option<Child>, // None: started by an earlier run
    writer: Option<Output>,
    reader: Option<Input>, // handed out by connect()
    pub path: String,
    pub viz: String, // the named pipe the visualizer's filter writes to
}

impl Mpv {
    // Returns the player and its measurements, which must always be drained or the audio filter blocks.
    pub fn spawn(volume: i64) -> std::io::Result<(Mpv, Input)> {
        let (path, viz) = platform::player_paths();
        let measurements = platform::make_viz(&viz)?;
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
        platform::detached(&mut cmd);
        let child = cmd.spawn()?;
        Ok((Mpv { child: Some(child), writer: None, reader: None, path, viz }, measurements))
    }

    // The crossfade's other half: a short-lived mpv that waits, paused on `from`, to play the end of a song fading out
    // while the main one starts the next. `filters` sound it like the main one ("lavfi=[...]," each). Not detached:
    // it goes when rymus does.
    pub fn tail(url: &str, format: &str, from: f64, secs: f64, volume: i64, filters: &str) -> std::io::Result<Mpv> {
        if cfg!(test) {
            return Ok(Mpv { child: None, writer: Some(Box::new(std::io::sink())), reader: None, path: String::new(), viz: String::new() });
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed); // one waits while the last one still plays
        let path = platform::player_paths().0.replace(".sock", &format!("-tail{n}.sock"));
        let mut cmd = Command::new("mpv");
        cmd.args(["--pause", "--no-video", "--no-terminal", "--hr-seek=yes"])
            .arg(format!("--input-ipc-server={path}"))
            .arg(format!("--ytdl-format={format}"))
            .arg(format!("--start={from:.2}"))
            .arg(format!("--length={:.2}", secs + 0.5))
            .arg(format!("--volume={volume}"))
            .arg(format!("--af={filters}lavfi=[afade=t=out:st={from:.2}:d={secs}]"));
        if let Ok(extra) = std::env::var("YM_MPV_ARGS") {
            cmd.args(extra.split_whitespace());
        }
        cmd.arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn()?;
        Ok(Mpv { child: Some(child), writer: None, reader: None, path, viz: String::new() })
    }

    // Connects without waiting: false when the player is not listening (yet, or any more).
    pub fn try_connect(&mut self) -> bool {
        if self.writer.is_none() {
            self.writer = platform::connect(&self.path).ok().map(|(_, writer)| writer);
        }
        self.writer.is_some()
    }

    // The player an earlier run left playing, if it is still there.
    pub fn attach(path: &str, viz: &str) -> Option<(Mpv, Input)> {
        if !platform::left_by_us(path, viz) {
            return None;
        }
        let (reader, writer) = platform::connect(path).ok()?;
        let measurements = platform::open_viz(viz).ok()?;
        Some((Mpv { child: None, writer: Some(writer), reader: Some(reader), path: path.into(), viz: viz.into() }, measurements))
    }

    // For tests: no mpv at all, the commands are collected in the returned buffer.
    #[cfg(test)]
    pub fn fake() -> (Mpv, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        struct Record(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Record {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let told = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (Mpv { child: None, writer: Some(Box::new(Record(told.clone()))), reader: None, path: String::new(), viz: String::new() }, told)
    }

    // What mpv says; waits for it to start when it was just spawned.
    pub fn connect(&mut self) -> std::io::Result<Input> {
        if let Some(reader) = self.reader.take() {
            return Ok(reader);
        }
        let mut last = None;
        for _ in 0..100 {
            match platform::connect(&self.path) {
                Ok((reader, writer)) => {
                    self.writer = Some(writer);
                    return Ok(reader);
                }
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("mpv did not start")))
    }

    pub fn cmd(&mut self, command: Value) {
        if let Some(w) = &mut self.writer {
            // one write, so a line is never split on the way
            let _ = w.write_all(format!("{}\n", serde_json::json!({ "command": command })).as_bytes());
        }
    }

    // Leaving the player to itself: someone has to keep reading its measurements.
    pub fn keep_measurements_read(&self) -> bool {
        cfg!(test) || platform::start_drain(&self.viz, &self.path)
    }

    pub fn stop(&mut self) {
        self.cmd(serde_json::json!(["quit"]));
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        platform::remove(&self.path, &self.viz);
    }
}
