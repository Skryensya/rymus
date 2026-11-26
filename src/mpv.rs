// mpv runs headless and is driven over its JSON IPC channel; the visualizer's measurements come back through a named
// pipe. It lives apart from the terminal, so it can keep playing after rymus is gone and be found again by the next one.
use crate::platform::{self, Input, Output};
use serde_json::Value;
use std::io::Write;
use std::process::{Child, Command, Stdio};

// How far ahead of what you hear the audio is decoded and filtered. mpv's default (0.2 s) runs dry when the machine is
// busy, because the visualizer's analyser runs in the same audio thread: measured silently with 16 to 32 busy cores,
// 0.2 s stuttered and 0.5 s did not. Larger would make equalizer changes slower to be heard.
const AUDIO_BUFFER: &str = "--audio-buffer=0.5";

pub struct Mpv {
    child: Option<Child>, // None: started by an earlier run
    writer: Option<Output>,
    reader: Option<Input>, // handed out by connect()
    pending: Vec<u8>,      // commands given before the player was listening, sent once it is
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
            .arg(AUDIO_BUFFER)
            .arg(format!("--volume={volume}"));
        if let Ok(extra) = std::env::var("YM_MPV_ARGS") {
            cmd.args(extra.split_whitespace()); // e.g. YM_MPV_ARGS=--ao=null for silent runs
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        platform::detached(&mut cmd);
        let child = cmd.spawn()?;
        Ok((Mpv { child: Some(child), writer: None, reader: None, path, viz, pending: vec![] }, measurements))
    }

    // The crossfade's other half: a short-lived mpv that waits, paused on `from`, to play the end of a song fading out
    // while the main one starts the next. `filters` sound it like the main one ("lavfi=[...]," each). Not detached:
    // it goes when rymus does.
    pub fn tail(url: &str, format: &str, from: f64, secs: f64, volume: i64, filters: &str) -> std::io::Result<Mpv> {
        if cfg!(test) {
            return Ok(Mpv { child: None, writer: Some(Box::new(std::io::sink())), reader: None, path: String::new(), viz: String::new(), pending: vec![] });
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed); // one waits while the last one still plays
        let path = platform::player_paths().0.replace(".sock", &format!("-tail{n}.sock"));
        let mut cmd = Command::new("mpv");
        // the media keys and the system's Now Playing stay with the main player, not with this end fading out
        cmd.args(["--pause", "--no-video", "--no-terminal", "--hr-seek=yes", "--media-controls=no", "--input-media-keys=no"])
            .arg(format!("--input-ipc-server={path}"))
            .arg(format!("--ytdl-format={format}"))
            .arg(AUDIO_BUFFER)
            .arg(format!("--start={from:.2}"))
            .arg(format!("--length={:.2}", secs + 0.5))
            .arg(format!("--volume={volume}"))
            .arg(format!("--af={filters}lavfi=[afade=t=out:st={from:.2}:d={secs}]"));
        if let Ok(extra) = std::env::var("YM_MPV_ARGS") {
            cmd.args(extra.split_whitespace());
        }
        cmd.arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn()?;
        Ok(Mpv { child: Some(child), writer: None, reader: None, path, viz: String::new(), pending: vec![] })
    }

    // Connects without waiting: false when the player is not listening (yet, or any more).
    pub fn try_connect(&mut self) -> bool {
        if self.writer.is_none() {
            self.writer = platform::connect(&self.path).ok().map(|(_, mut writer)| {
                let _ = writer.write_all(&std::mem::take(&mut self.pending));
                writer
            });
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
        Some((Mpv { child: None, writer: Some(writer), reader: Some(reader), path: path.into(), viz: viz.into(), pending: vec![] }, measurements))
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
        (Mpv { child: None, writer: Some(Box::new(Record(told.clone()))), reader: None, path: String::new(), viz: String::new(), pending: vec![] }, told)
    }

    // What mpv says; waits for it to start when it was just spawned.
    // One try at reaching the player, for a caller that keeps going meanwhile: its events once it listens. What was
    // told to it before goes first.
    pub fn connect_now(&mut self) -> Option<Input> {
        if let Some(reader) = self.reader.take() {
            return Some(reader);
        }
        let (reader, mut writer) = platform::connect(&self.path).ok()?;
        let _ = writer.write_all(&std::mem::take(&mut self.pending));
        self.writer = Some(writer);
        Some(reader)
    }

    pub fn cmd(&mut self, command: Value) {
        let line = format!("{}\n", serde_json::json!({ "command": command }));
        match &mut self.writer {
            // one write, so a line is never split on the way
            Some(w) => {
                let _ = w.write_all(line.as_bytes());
            }
            None => self.pending.extend_from_slice(line.as_bytes()),
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
