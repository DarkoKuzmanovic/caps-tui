//! keyd validation, monitoring, backups and apply — the framework-neutral glue.
//!
//! Ported from `keymapper/system.py`. The Python's `select`-based polling becomes a reader
//! thread per process: Rust's std has no portable non-blocking pipe, and a thread keeps the
//! UI free to draw.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use super::config::{backup_timestamp, parse, sorted_backups};

/// The live keyd config directory, honouring `KEYMAP_ETC`.
pub fn keyd_etc_dir() -> PathBuf {
    std::env::var_os("KEYMAP_ETC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/keyd"))
}

/// The `keyd` binary: `PATH` first, then `KEYD_BIN`.
pub fn keyd_bin() -> Option<PathBuf> {
    which("keyd").or_else(|| std::env::var_os("KEYD_BIN").map(PathBuf::from))
}

fn which(binary: &str) -> Option<PathBuf> {
    if binary.contains('/') {
        let path = PathBuf::from(binary);
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

/// Split a command line the way `shlex.split` does for the cases we use.
pub fn split_command(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What `keyd check` said about a config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Accepted,
    Rejected(String),
    /// keyd is not installed, so nothing was validated.
    KeydMissing,
}

/// Validate a config with keyd.
///
/// `Err` means the check itself could not run (no temp file, no process): callers must not read
/// that as "accepted". Python let these raise; returning `Option` would fail open.
pub fn check(text: &str) -> std::io::Result<CheckOutcome> {
    check_with(keyd_bin().as_deref(), text)
}

/// `check`, against an explicit binary — the form tests use.
pub fn check_with(binary: Option<&Path>, text: &str) -> std::io::Result<CheckOutcome> {
    let Some(binary) = binary else {
        return Ok(CheckOutcome::KeydMissing);
    };
    let mut file = tempfile::Builder::new().suffix(".conf").tempfile()?;
    file.write_all(text.as_bytes())?;
    let path = file.path().to_path_buf();
    let output = Command::new(binary).arg("check").arg(&path).output()?;
    if output.status.success() {
        return Ok(CheckOutcome::Accepted);
    }
    let mut message = String::from_utf8_lossy(&output.stdout).to_string();
    message.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(CheckOutcome::Rejected(
        message
            .replace(&path.display().to_string(), "config")
            .trim()
            .to_string(),
    ))
}

/// The command that streams keyd's monitor output, and whether it needs sudo.
pub fn monitor_command() -> (Vec<String>, bool) {
    let fake = std::env::var("KEYMAP_MONITOR_CMD").ok();
    monitor_command_from(fake.as_deref(), keyd_bin().as_deref())
}

/// `monitor_command`, from explicit inputs — the form tests use.
pub fn monitor_command_from(fake: Option<&str>, binary: Option<&Path>) -> (Vec<String>, bool) {
    if let Some(fake) = fake
        && !fake.trim().is_empty()
    {
        return (split_command(fake), false);
    }
    match binary {
        Some(binary) => (
            vec![
                "sudo".to_string(),
                "-n".to_string(),
                binary.display().to_string(),
                "monitor".to_string(),
            ],
            true,
        ),
        None => (Vec::new(), false),
    }
}

/// Make sure sudo has credentials, prompting in the terminal when it has to.
pub fn authenticate_monitor() -> bool {
    let cached = Command::new("sudo")
        .args(["-n", "-v"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if cached {
        return true;
    }
    println!("keyd monitor needs sudo access; authenticate to capture a key.");
    Command::new("sudo")
        .arg("-v")
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Capture timed out after {0:?}; use Ctrl+T to type a key name.")]
    Timeout(Duration),
    #[error("keyd monitor unavailable; use terminal key capture.")]
    Unavailable,
    #[error("keyd monitor unavailable: {0}")]
    Spawn(String),
}

/// Wait for the next key-down from a monitor command.
pub fn monitor_capture(command: &[String], timeout: Duration) -> Result<String, CaptureError> {
    let Some((program, args)) = command.split_first() else {
        return Err(CaptureError::Unavailable);
    };
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| CaptureError::Spawn(error.to_string()))?;
    let stdout = child.stdout.take().expect("piped");
    let (sender, receiver) = channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let result = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Err(CaptureError::Timeout(timeout));
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(line) => {
                if let Some(key) = parse_monitor_line(&line) {
                    break Ok(key);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if child.try_wait().ok().flatten().is_some() {
                    break Err(CaptureError::Unavailable);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break Err(CaptureError::Unavailable);
            }
        }
    };
    stop(&mut child);
    result
}

fn stop(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let pid = child.id() as i32;
    // The child leads its own process group (`process_group(0)` at spawn), so this reaches
    // sudo/pkexec and whatever they started. A direct SIGKILL would leave a root `keyd monitor`
    // alive until its next write.
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether keyd is running, so a plain reload is enough.
pub fn quick_apply_available() -> bool {
    if keyd_bin().is_none() {
        return false;
    }
    Command::new("systemctl")
        .args(["is-active", "--quiet", "keyd"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("could not write {path}: {source}")]
    Write { path: String, source: std::io::Error },
    #[error("could not run the installer: {0}")]
    Spawn(std::io::Error),
}

/// Write the config beside the checkout and run `install.sh` (the CLI's path).
pub fn run_apply(text: &str, conf_path: &Path, install_script: &Path) -> Result<i32, ApplyError> {
    std::fs::write(conf_path, text).map_err(|source| ApplyError::Write {
        path: conf_path.display().to_string(),
        source,
    })?;
    let status = if quick_apply_available() {
        Command::new("sudo")
            .arg("env")
            .arg(format!("KEYMAP_ETC={}", keyd_etc_dir().display()))
            .arg("bash")
            .arg(install_script)
            .arg("--reload")
            .status()
    } else {
        Command::new("bash").arg(install_script).status()
    }
    .map_err(ApplyError::Spawn)?;
    Ok(status.code().unwrap_or(1))
}

/// The key name of a monitor key-down, ignoring releases and Caps Lock.
pub fn parse_monitor_line(line: &str) -> Option<String> {
    let (_, key, state) = parse_monitor_event(line)?;
    if state != "down" || key.eq_ignore_ascii_case("capslock") {
        return None;
    }
    Some(key)
}

/// A monitor row as (device, key, state), accepting down and up.
pub fn parse_monitor_event(line: &str) -> Option<(String, String, String)> {
    let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
    if fields.len() != 3 || fields[0].trim().is_empty() || fields[1].trim().is_empty() {
        return None;
    }
    let (key, state) = fields[2].rsplit_once(' ')?;
    if key.trim().is_empty() || !matches!(state, "down" | "up") {
        return None;
    }
    Some((fields[0].trim().to_string(), key.trim().to_string(), state.to_string()))
}

/// The live config, or the fallback when it cannot be read (with `true` for "unreadable").
pub fn read_live_config(fallback: &str) -> (String, bool) {
    read_live_config_in(&keyd_etc_dir(), fallback)
}

/// `read_live_config`, against an explicit directory — the form tests use.
pub fn read_live_config_in(etc: &Path, fallback: &str) -> (String, bool) {
    match std::fs::read_to_string(etc.join("default.conf")) {
        Ok(text) => (text, false),
        Err(_) => (fallback.to_string(), true),
    }
}

/// One backup on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    pub path: PathBuf,
    pub stamp: Option<String>,
    /// `[nav]` bindings in the backup, or `None` when it cannot be read.
    pub bindings: Option<usize>,
}

/// Timestamped backups in `directory`, newest first.
pub fn backup_entries(directory: Option<&Path>) -> Vec<BackupEntry> {
    let directory = directory.map(Path::to_path_buf).unwrap_or_else(keyd_etc_dir);
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let paths: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().display().to_string())
        .filter(|path| Path::new(path).file_name().is_some_and(|name| {
            name.to_string_lossy().starts_with("default.conf.bak.")
        }))
        .collect();
    sorted_backups(&paths)
        .into_iter()
        .map(|path| {
            let bindings = std::fs::read_to_string(&path)
                .ok()
                .map(|text| parse(&text).len());
            BackupEntry {
                stamp: backup_timestamp(&path),
                path: PathBuf::from(path),
                bindings,
            }
        })
        .collect()
}

/// Called from a reader thread when new output arrives. Rune's `Waker` is this same type, so the
/// UI can hand its own in without the core depending on Rune.
pub type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;

/// A live `keyd monitor` process read on background threads.
pub struct MonitorStream {
    child: Child,
    /// Kept open: the helper exits when this closes, which is how a root process is stopped.
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    closed: bool,
}

impl MonitorStream {
    pub fn spawn(command: &[String]) -> std::io::Result<Self> {
        Self::spawn_with(command, None)
    }

    pub fn spawn_with(command: &[String], waker: Option<Waker>) -> std::io::Result<Self> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| std::io::Error::other("empty monitor command"))?;
        let mut command = Command::new(program);
        command.args(args);
        Self::from_command_with(command, waker)
    }

    /// Wrap an already-built command: same pipes, same process group, same bounded stop, so the
    /// privileged monitor path cannot drift from the unprivileged one.
    pub fn from_command_with(mut command: Command, waker: Option<Waker>) -> std::io::Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let stdin = child.stdin.take();
        let (sender, lines) = channel::<String>();
        // stderr goes to the same channel, as Python's `stderr=STDOUT`: "sudo: a password is
        // required" and pkexec denials belong in the panel, and an unread pipe would stall the
        // child once its buffer filled.
        let mut readers: Vec<Box<dyn std::io::Read + Send>> = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(Box::new(stdout));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(Box::new(stderr));
        }
        for stream in readers {
            let sender = sender.clone();
            let waker = waker.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if sender.send(line).is_err() {
                        break;
                    }
                    // An idle window draws no frames, so the panel would freeze exactly when a
                    // key arrives from another app. Ask the shell for one.
                    if let Some(wake) = &waker {
                        wake();
                    }
                }
            });
        }
        drop(sender);
        Ok(Self { child, stdin, lines, closed: false })
    }

    /// Complete lines that have arrived, and the exit status once the process has ended.
    pub fn poll(&mut self) -> (Vec<String>, Option<ExitStatus>) {
        let mut lines = Vec::new();
        if !self.closed {
            loop {
                match self.lines.try_recv() {
                    Ok(line) => lines.push(line),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.closed = true;
                        break;
                    }
                }
            }
        }
        (lines, self.child.try_wait().ok().flatten())
    }

    /// Stop the monitor: close its stdin first (a root helper exits on EOF), then signal it.
    pub fn close(&mut self) {
        drop(self.stdin.take());
        stop(&mut self.child);
        self.closed = true;
    }
}

impl Drop for MonitorStream {
    fn drop(&mut self) {
        if !self.closed {
            drop(self.stdin.take());
            stop(&mut self.child);
        }
    }
}
