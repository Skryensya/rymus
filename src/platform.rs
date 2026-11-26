// What leans on the system (macOS and Linux), except the audio output (output.rs): how to talk to mpv, where its
// measurements arrive, signals, local time and folders.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Input = Box<dyn Read + Send>;
pub const BACK: &str = "rymus-back"; // the message that ends the drain
pub type Output = Box<dyn Write + Send>;

pub use unix::*;

#[cfg(target_os = "macos")]
pub const INSTALL_HINT: &str = "mpv not found: brew install mpv yt-dlp ffmpeg";
#[cfg(target_os = "linux")]
pub const INSTALL_HINT: &str = "mpv not found: install mpv, yt-dlp and ffmpeg with your package manager";

// "~/x" means the home folder.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home_dir().join(rest),
        None => PathBuf::from(path),
    }
}

static QUIT: AtomicBool = AtomicBool::new(false);
// True once the system asked the program to end (signal, terminal closed).
pub fn asked_to_quit() -> bool {
    QUIT.load(Ordering::Relaxed)
}

mod unix {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;

    // (mpv's control socket, the visualizer's named pipe) for this process.
    pub fn player_paths() -> (String, String) {
        // shortcut: unix socket paths are capped at ~104 characters on macOS, so /tmp, not the temp dir
        let base = format!("/tmp/rymus-{}", std::process::id());
        (format!("{base}.sock"), format!("{base}.viz"))
    }

    // No controlling terminal: closing the window does not take the player down behind our back.
    pub fn detached(command: &mut Command) {
        // SAFETY: setsid is async-signal-safe, which is all that may run between fork and exec
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    pub fn connect(path: &str) -> std::io::Result<(Input, Output)> {
        let socket = UnixStream::connect(path)?;
        Ok((Box::new(socket.try_clone()?), Box::new(socket)))
    }

    // Opened for reading and writing so that neither end ever waits for the other, nor sees the pipe close.
    pub fn open_viz(path: &str) -> std::io::Result<Input> {
        Ok(Box::new(OpenOptions::new().read(true).write(true).open(path)?) as Box<File>)
    }
    pub fn make_viz(path: &str) -> std::io::Result<Input> {
        let _ = std::fs::remove_file(path);
        let c = std::ffi::CString::new(path).map_err(std::io::Error::other)?;
        // SAFETY: a valid, nul terminated path
        if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        open_viz(path)
    }

    // Only talk to something that is ours: /tmp is shared.
    pub fn left_by_us(socket: &str, viz: &str) -> bool {
        ours(socket, false) && ours(viz, true)
    }
    // A socket (or with `fifo`, a named pipe) that is yours, not a link, not someone else's.
    pub fn ours(path: &str, fifo: bool) -> bool {
        // SAFETY: getuid cannot fail
        let me = unsafe { libc::getuid() };
        std::fs::symlink_metadata(path).is_ok_and(|m| m.uid() == me && if fifo { m.file_type().is_fifo() } else { m.file_type().is_socket() })
    }

    pub fn remove(socket: &str, viz: &str) {
        let _ = std::fs::remove_file(socket);
        let _ = std::fs::remove_file(viz);
    }

    // The program to start again: the path it was started from, which a new build overwrites.
    pub fn this_program() -> Option<PathBuf> {
        let path = std::env::current_exe().ok()?;
        // Linux marks a replaced binary; the new one is at the same place
        Some(path.to_str().and_then(|p| p.strip_suffix(" (deleted)")).map_or(path.clone(), Into::into))
    }

    // Keeps the visualizer's pipe read while rymus is away, so the player's audio filter can stay as it is: taking it
    // out and putting it back stops the sound for about a second. False if that could not be arranged.
    pub fn start_drain(viz: &str, socket: &str) -> bool {
        // handed over already open, so there is no moment without a reader
        let (Ok(pipe), Some(program)) = (OpenOptions::new().read(true).write(true).open(viz), this_program()) else { return false };
        let mut command = Command::new(program);
        command.args(["--drain", socket]).stdin(pipe).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        detached(&mut command);
        command.spawn().is_ok()
    }
    // What that process does (`rymus --drain <socket>`): throw the measurements away until the player ends or a new
    // rymus says it is back.
    pub fn drain(socket: &str) {
        std::thread::spawn(|| {
            urgent_thread();
            let mut buf = [0u8; 16 * 1024];
            let mut input = std::io::stdin().lock();
            while matches!(input.read(&mut buf), Ok(1..)) {}
        });
        let Ok(player) = UnixStream::connect(socket) else { return };
        for line in std::io::BufRead::lines(std::io::BufReader::new(player)).map_while(Result::ok) {
            if line.contains("client-message") && line.contains(BACK) {
                break;
            }
        }
    }

    // The thread that reads the visualizer's measurements goes first when the machine is busy: if it falls behind, the
    // pipe fills and mpv's audio waits for it.
    pub fn urgent_thread() {
        #[cfg(target_os = "macos")]
        // SAFETY: only changes how this thread is scheduled
        unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
        }
    }
    pub fn watch_for_quit() {
        extern "C" fn on_signal(_: libc::c_int) {
            QUIT.store(true, Ordering::Relaxed);
        }
        for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            // SAFETY: the handler only stores to an atomic
            unsafe { libc::signal(signal, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t) };
        }
    }

    pub fn local_time(secs: i64) -> libc::tm {
        let secs = secs as libc::time_t;
        // SAFETY: tm is plain data, and localtime_r only writes into it
        unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&secs, &mut tm);
            tm
        }
    }

    pub fn home_dir() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
    }
    // Readable by you alone.
    pub fn private_dir(dir: &Path) {
        let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir);
    }
}
