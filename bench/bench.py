#!/usr/bin/env python3
"""Compare rymus builds: startup, memory, CPU and terminal output. Silent: mpv gets --ao=null, the database is temporary.

    python3 bench/bench.py ymus ./target/release/rymus

Only the app process is measured; mpv, yt-dlp and ffmpeg are the same programs for every build. Playback streams one
song from YouTube, so it needs the network.
"""
import fcntl, json, os, pty, select, signal, sqlite3, statistics, struct, subprocess, sys, tempfile, termios, time

ROWS, COLS = 34, 120
STARTS, SECONDS = 15, 20
PATIENCE = 90  # seconds to wait for a screen before giving up (the network can make a start slow)
SONG = {"id": "5NV6Rdv1a3I", "title": "Get Lucky", "artist": "Daft Punk", "album": "Random Access Memories", "cover": None}


class App:
    def __init__(self, command, db):
        self.buf = bytearray()
        self.total = 0
        self.started = time.perf_counter()
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ.update(YM_DB=db, YM_MPV_ARGS="--ao=null", TERM_PROGRAM="ghostty", TERM="xterm-256color")
            os.execvp(command, [command])
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    def pump(self, seconds, until=None):
        end = time.perf_counter() + seconds
        while time.perf_counter() < end:
            if select.select([self.fd], [], [], 0.002)[0]:
                try:
                    data = os.read(self.fd, 1 << 16)
                except OSError:
                    return False
                self.total += len(data)
                self.buf += data
                if until and until in self.buf:
                    return True
                del self.buf[:-4096]
        return False

    def cpu(self):  # seconds of CPU used by the app process so far
        text = subprocess.check_output(["ps", "-o", "time=", "-p", str(self.pid)], text=True).strip()
        total = 0.0
        for part in text.split(":"):
            total = total * 60 + float(part)
        return total

    def rss_mb(self):
        return int(subprocess.check_output(["ps", "-o", "rss=", "-p", str(self.pid)], text=True)) / 1024

    def close(self):
        os.write(self.fd, b"q")
        self.pump(1.0)
        try:
            if os.waitpid(self.pid, os.WNOHANG)[0] == 0:
                os.kill(self.pid, signal.SIGTERM)
                self.pump(0.5)
                os.waitpid(self.pid, 0)
        except ChildProcessError:
            pass
        # an app stopped while still starting leaves its player behind
        subprocess.run(["pkill", "-f", f"rymus-{self.pid}.sock"], check=False)

    def ready(self, text=b"Discover"):  # always on Home
        if not self.pump(PATIENCE, text):
            self.close()
            sys.exit(f"no screen after {PATIENCE} s")


def database(command, session):
    path = os.path.join(tempfile.mkdtemp(), "bench.db")
    first = App(command, path)  # lets the app create its schema
    first.ready()
    first.close()
    if session:
        with sqlite3.connect(path) as c:
            c.execute("INSERT OR REPLACE INTO settings VALUES('session', ?)", [json.dumps({"queue": [SONG], "pos": 0, "time": 0})])
    return path


def window(app, seconds=SECONDS):
    cpu, total, start = app.cpu(), app.total, time.perf_counter()
    app.pump(seconds)
    wall = time.perf_counter() - start
    return {"cpu %": 100 * (app.cpu() - cpu) / wall, "memory MB": app.rss_mb(), "output KB/s": (app.total - total) / wall / 1024}


def measure(command):
    result = {}
    db = database(command, False)
    times = []
    for _ in range(STARTS):
        app = App(command, db)
        app.ready()
        times.append(1000 * (time.perf_counter() - app.started))
        app.close()
    result["startup ms (median)"] = statistics.median(times)
    result["startup ms (fastest)"] = min(times)
    result["startup ms (slowest)"] = max(times)

    app = App(command, database(command, False))
    app.ready()
    app.pump(3)
    for k, v in window(app).items():
        result[f"idle: {k}"] = v
    app.close()

    app = App(command, database(command, True))
    app.ready()
    app.pump(3)
    os.write(app.fd, b"\r")  # the Resume row
    app.ready("▶".encode())
    app.pump(10)
    for k, v in window(app).items():
        result[f"playing + visualizer: {k}"] = v
    os.write(app.fd, b"V")
    app.pump(2)
    for k, v in window(app).items():
        result[f"trip mode: {k}"] = v
    app.close()
    return result


if __name__ == "__main__":
    commands = sys.argv[1:] or ["./target/release/rymus"]
    results = []
    for c in commands:
        print(f"measuring {c} …", file=sys.stderr, flush=True)
        results.append(measure(c))
    print("| metric | " + " | ".join(commands) + " |")
    print("|---|" + "---|" * len(commands))
    for key in results[0]:
        print(f"| {key} | " + " | ".join(f"{r[key]:.1f}" for r in results) + " |")
