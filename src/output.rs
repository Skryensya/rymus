// Which device the sound goes to, so that playback can pause the moment it changes (headphones unplugged, Bluetooth
// gone, another output chosen). Each system is asked in its own way.

// Something that is different when the sound goes somewhere else. None: unknown.
#[cfg(target_os = "macos")]
pub fn current() -> Option<String> {
    use std::ffi::c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }
    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(object: u32, address: *const Address, qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    }
    const fn code(c: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*c)
    }
    const SYSTEM: u32 = 1;

    fn read(object: u32, selector: u32, scope: u32) -> Option<u32> {
        let address = Address { selector, scope, element: 0 };
        let (mut value, mut size) = (0u32, 4u32);
        // SAFETY: both properties read here are a single 32 bit number, and `size` says that is all there is room for
        let status = unsafe { AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, (&mut value as *mut u32).cast()) };
        (status == 0).then_some(value)
    }

    let device = read(SYSTEM, code(b"dOut"), code(b"glob"))?; // the default output device
    let source = read(device, code(b"ssrc"), code(b"outp")).unwrap_or(0); // not every device has sources
    Some(format!("{device}:{source}"))
}

// PulseAudio, or PipeWire speaking it: the default sink and its active port (unplugging headphones changes the port).
#[cfg(target_os = "linux")]
pub fn current() -> Option<String> {
    let pactl = |args: &[&str]| {
        let out = std::process::Command::new("pactl").args(args).env("LC_ALL", "C").stderr(std::process::Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    fingerprint(&pactl(&["info"])?, &pactl(&["list", "sinks"])?)
}

#[cfg(any(target_os = "linux", test))]
fn fingerprint(info: &str, sinks: &str) -> Option<String> {
    let sink = info.lines().find_map(|l| l.strip_prefix("Default Sink:"))?.trim();
    let mut inside = false;
    let mut port = "";
    for line in sinks.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("Name:") {
            inside = name.trim() == sink;
        } else if inside {
            if let Some(p) = line.strip_prefix("Active Port:") {
                port = p.trim();
            }
        }
    }
    Some(format!("{sink}:{port}"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn current() -> Option<String> {
    None
}

// Reports the output to `changed` when it is not what it was. False when `changed` wants no more.
fn compare(last: &mut Option<String>, changed: &impl Fn() -> bool) -> bool {
    let now = current();
    if now.is_some() && now != *last {
        if last.is_some() && !changed() {
            return false;
        }
        *last = now;
    }
    true
}

// Calls `changed` each time the output becomes another one. Runs until it returns false.
#[cfg(not(target_os = "linux"))]
pub fn watch(changed: impl Fn() -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let mut last = current();
        // shortcut: polled, ten times a second is quick enough to feel immediate; a system listener if it is not
        while compare(&mut last, &changed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

// The sound server announces changes, so nothing is polled. Without pactl nothing is detected.
#[cfg(target_os = "linux")]
pub fn watch(changed: impl Fn() -> bool + Send + 'static) {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    std::thread::spawn(move || {
        let mut command = std::process::Command::new("pactl");
        command.arg("subscribe").env("LC_ALL", "C").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        // SAFETY: prctl is safe between fork and exec; the listener ends with us
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let Ok(mut child) = command.spawn() else { return };
        let Some(events) = child.stdout.take() else { return };
        let mut last = current();
        for line in BufReader::new(events).lines().map_while(Result::ok) {
            // "Event 'change' on sink #52", "… on server", "… on card #48"
            let about_outputs = ["on sink #", "on server", "on card #"].iter().any(|what| line.contains(what));
            if about_outputs && !compare(&mut last, &changed) {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn the_system_names_an_output() {
        // a machine with no audio device at all answers device 0
        if let Some(output) = super::current() {
            assert!(!output.starts_with("0:"));
        }
    }

    #[test]
    fn pulse_sink_and_port() {
        let info = "Server Name: PulseAudio\nDefault Sink: alsa.analog\n";
        let sinks = "Sink #1\n\tName: other\n\tActive Port: hdmi\nSink #2\n\tName: alsa.analog\n\tPorts:\n\t\tanalog-output-headphones: x\n\tActive Port: analog-output-speaker\n";
        assert_eq!(super::fingerprint(info, sinks).as_deref(), Some("alsa.analog:analog-output-speaker"));
        let plugged = sinks.replace("Active Port: analog-output-speaker", "Active Port: analog-output-headphones");
        assert_ne!(super::fingerprint(info, sinks), super::fingerprint(info, &plugged));
        assert_eq!(super::fingerprint("nothing here", sinks), None);
    }
}
