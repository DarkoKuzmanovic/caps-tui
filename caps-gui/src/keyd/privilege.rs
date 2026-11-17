//! Privilege boundary: how the app asks for root from a window with no terminal.
//!
//! The TUI gets root by letting `sudo` prompt in its own terminal. A `.desktop` launch has no
//! TTY, so this goes through two root-owned helpers in `/usr/libexec` authorised by one polkit
//! action, with `sudo -A` as the fallback. `caps-gui-apply` never executes a repo file: the
//! config arrives on stdin and is validated before `/etc/keyd` is touched.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

/// Which root helper to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Helper {
    /// Streams `keyd monitor` to stdout.
    Monitor,
    /// Installs a config arriving on stdin.
    Apply,
}

impl Helper {
    pub fn program(self) -> &'static str {
        match self {
            Helper::Monitor => "caps-gui-monitor",
            Helper::Apply => "caps-gui-apply",
        }
    }

    /// Where `install.sh --install-policy` puts it.
    pub fn path(self) -> PathBuf {
        PathBuf::from("/usr/libexec").join(self.program())
    }
}

/// How this process may obtain root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Priv {
    /// polkit, through the desktop's authentication agent.
    Pkexec,
    /// `sudo -A` with our askpass shim.
    SudoAskpass(PathBuf),
    /// Plain `sudo`: only when stdin is a terminal.
    SudoTty,
    /// A stand-in for tests and headless runs (`CAPS_PRIV_CMD`).
    Stub(PathBuf),
}

/// Decide how to ask for root, from explicit inputs — the form tests use.
pub fn detect_from(
    stub: Option<&Path>,
    desktop_session: bool,
    pkexec: Option<&Path>,
    askpass: Option<&Path>,
) -> Priv {
    if let Some(stub) = stub {
        return Priv::Stub(stub.to_path_buf());
    }
    if desktop_session && pkexec.is_some() {
        return Priv::Pkexec;
    }
    if let Some(askpass) = askpass {
        return Priv::SudoAskpass(askpass.to_path_buf());
    }
    Priv::SudoTty
}

/// Decide how to ask for root in this process.
pub fn detect() -> Priv {
    let stub = std::env::var_os("CAPS_PRIV_CMD").map(PathBuf::from);
    let desktop = std::env::var_os("XDG_CURRENT_DESKTOP").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("DISPLAY").is_some();
    let pkexec = which("pkexec");
    let askpass = Helper::Monitor
        .path()
        .with_file_name("caps-gui-askpass");
    let askpass = askpass.is_file().then_some(askpass);
    detect_from(
        stub.as_deref(),
        desktop,
        pkexec.as_deref(),
        askpass.as_deref(),
    )
}

/// The command that runs a helper as root.
pub fn command(how: &Priv, helper: Helper) -> Command {
    match how {
        Priv::Stub(program) => {
            let mut command = Command::new(program);
            command.arg(helper.program());
            command
        }
        Priv::Pkexec => {
            let mut command = Command::new("pkexec");
            command.arg(helper.path());
            command
        }
        Priv::SudoAskpass(askpass) => {
            let mut command = Command::new("sudo");
            command.arg("-A").arg(helper.path()).env("SUDO_ASKPASS", askpass);
            command
        }
        Priv::SudoTty => {
            let mut command = Command::new("sudo");
            command.arg(helper.path());
            command
        }
    }
}

/// What the apply helper said.
#[derive(Debug)]
pub struct ApplyOutcome {
    pub status: ExitStatus,
    /// The helper's combined stdout and stderr, trimmed — the reason a rejection gives.
    pub message: String,
}

/// Install a config through the apply helper, feeding it on stdin.
///
/// Both streams are piped: a `.desktop` launch has no terminal, so a rejection would otherwise
/// land in the journal and the window would only see a bare exit status.
pub fn apply_text(how: &Priv, config: &str) -> std::io::Result<ApplyOutcome> {
    let mut child = command(how, Helper::Apply)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        stdin.write_all(config.as_bytes())?;
    }
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    let mut message = String::from_utf8_lossy(&output.stdout).to_string();
    message.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(ApplyOutcome { status: output.status, message: message.trim().to_string() })
}

/// Start the monitor helper, reading its output. Its stdin stays open: the helper exits when it
/// closes, which is how the app stops a root process it cannot signal.
pub fn spawn_monitor(how: &Priv) -> std::io::Result<Child> {
    command(how, Helper::Monitor)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn which(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}
