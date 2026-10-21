"""Framework-neutral keyd validation, monitoring, backup, and apply helpers."""

import glob
import os
import queue
import select
import shlex
import shutil
import signal
import subprocess
import tempfile
import time

from .config import parse, parse_backup_timestamp, sorted_backups
from .paths import CONF, HERE


def keyd_bin():
    return shutil.which("keyd") or os.environ.get("KEYD_BIN")


def check(text):
    """None if keyd accepts the config (or keyd is missing), else its error text."""
    binary = keyd_bin()
    if not binary:
        return None
    with tempfile.NamedTemporaryFile("w", suffix=".conf", delete=False) as config:
        config.write(text)
    try:
        result = subprocess.run([binary, "check", config.name], capture_output=True, text=True)
        return None if result.returncode == 0 else (
            result.stdout + result.stderr).replace(config.name, "config").strip()
    finally:
        os.unlink(config.name)


def monitor_command():
    """Return (argv, needs_sudo) for keyd monitor, honoring the fake-monitor override."""
    fake = os.environ.get("KEYMAP_MONITOR_CMD")
    if fake:
        return shlex.split(fake), False
    binary = keyd_bin()
    return (["sudo", "-n", binary, "monitor"], True) if binary else ([], False)


def authenticate_monitor():
    """Authenticate sudo for monitoring without assuming any terminal framework."""
    try:
        if subprocess.run(["sudo", "-n", "-v"], stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL, timeout=2).returncode == 0:
            return True
    except (OSError, subprocess.TimeoutExpired):
        pass
    print("keyd monitor needs sudo access; authenticate to capture a key.", flush=True)
    try:
        return subprocess.call(["sudo", "-v"]) == 0
    except OSError:
        return False


def monitor_capture(command, timeout=10, *, cancel_event=None, typed_queue=None):
    """Wait for a monitor key-down, typed name, cancellation, or timeout.

    UI frameworks own key input and terminal suspension. ``typed_queue`` may receive
    ``("type", name)`` or ``("cancel", None)`` from that UI while this function
    runs in a worker thread.
    """
    process = None
    deadline = time.monotonic() + timeout
    try:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   text=True, bufsize=1, start_new_session=True)
        while True:
            if cancel_event is not None and cancel_event.is_set():
                return None, "Cancelled."
            if typed_queue is not None:
                try:
                    kind, value = typed_queue.get_nowait()
                except queue.Empty:
                    pass
                else:
                    return (value, "") if kind == "type" and value else (None, "Cancelled.")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None, (f"Capture timed out after {timeout:g}s; use Ctrl+T to type a key name.")
            try:
                readable, _, _ = select.select([process.stdout], [], [], min(remaining, 0.1))
            except (OSError, ValueError):
                return None, "keyd monitor unavailable; use terminal key capture."
            if readable:
                line = process.stdout.readline()
                if not line and process.poll() is not None:
                    return None, "keyd monitor unavailable; use terminal key capture."
                key = parse_monitor_line(line)
                if key:
                    return key, ""
    except (OSError, ValueError) as exc:
        return None, f"keyd monitor unavailable: {exc}"
    finally:
        if process is not None:
            _stop_process(process)


def _stop_process(process):
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            try:
                process.terminate()
            except (ProcessLookupError, PermissionError):
                pass
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                try:
                    process.kill()
                except (ProcessLookupError, PermissionError):
                    pass
            process.wait()
    if process.stdout is not None:
        process.stdout.close()


def quick_apply_available():
    if not keyd_bin():
        return False
    try:
        result = subprocess.run(["systemctl", "is-active", "--quiet", "keyd"],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3)
        return result.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def run_apply(text, *, announce=True, conf_path=None):
    """Write the selected config and run the installer; return its exit code."""
    destination = conf_path or CONF
    with open(destination, "w") as config:
        config.write(text)
    quick = quick_apply_available()
    script = os.path.join(HERE, "install.sh")
    if quick:
        command = ["sudo", "env", f"KEYMAP_ETC={keyd_etc_dir()}", "bash", script, "--reload"]
        description = "Applying with keyd reload"
    else:
        command = ["bash", script]
        description = "Running full install"
    prefix = f"Saved {destination}. " if announce else ""
    print(f"{prefix}{description} (sudo)...\n", flush=True)
    try:
        return subprocess.call(command)
    except KeyboardInterrupt:
        return 130


def parse_monitor_line(line):
    """Return a keyd key name for a down event; ignore releases and Caps Lock."""
    fields = line.rstrip("\r\n").split("\t")
    if len(fields) != 3 or not fields[0].strip() or not fields[1].strip():
        return None
    event = fields[2].rsplit(" ", 1)
    if len(event) != 2 or not event[0] or event[1] != "down":
        return None
    key = event[0].strip()
    return None if not key or key.casefold() == "capslock" else key


def keyd_etc_dir():
    return os.environ.get("KEYMAP_ETC", "/etc/keyd")


def read_live_config(fallback):
    try:
        with open(os.path.join(keyd_etc_dir(), "default.conf")) as config:
            return config.read(), False
    except OSError:
        return fallback, True


def backup_entries(directory=None):
    directory = directory or keyd_etc_dir()
    paths = sorted_backups(glob.glob(os.path.join(directory, "default.conf.bak.*")))
    entries = []
    for path in paths:
        try:
            with open(path) as config:
                count = len(parse(config.read()))
        except OSError:
            count = None
        entries.append((path, parse_backup_timestamp(path), count))
    return entries


def parse_monitor_event(line):
    """Parse a monitor row as (device, key, state), accepting down and up."""
    fields = line.rstrip("\r\n").split("\t")
    if len(fields) != 3 or not fields[0].strip() or not fields[1].strip():
        return None
    event = fields[2].rsplit(" ", 1)
    if len(event) != 2 or not event[0].strip() or event[1] not in {"down", "up"}:
        return None
    return fields[0].strip(), event[0].strip(), event[1]


class MonitorStream:
    """Non-blocking reader for a live keyd monitor process."""

    def __init__(self, command):
        self.process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                        bufsize=0)
        self.pending = bytearray()
        self.closed = False

    def poll(self):
        """Return currently available complete lines and the exit code, without waiting."""
        lines = []
        stream = self.process.stdout
        if self.closed or stream is None:
            return lines, self.process.poll()
        fd = stream.fileno()
        while True:
            try:
                readable, _, _ = select.select([fd], [], [], 0)
                if not readable:
                    break
                chunk = os.read(fd, 4096)
            except OSError:
                self.closed = True
                break
            if not chunk:
                if self.pending:
                    lines.append(self.pending.decode("utf-8", errors="replace").rstrip("\r"))
                    self.pending.clear()
                stream.close()
                self.closed = True
                break
            self.pending.extend(chunk)
            while True:
                newline = self.pending.find(b"\n")
                if newline < 0:
                    break
                lines.append(self.pending[:newline].decode("utf-8", errors="replace").rstrip("\r"))
                del self.pending[:newline + 1]
        return lines, self.process.poll()

    def close(self):
        """Terminate and reap the monitor, escalating if it does not exit promptly."""
        _stop_process(self.process)
        self.closed = True
