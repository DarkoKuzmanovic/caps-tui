//! End-to-end CLI tests: the real binary, a temp config, a stub keyd and a stub installer.
//! Nothing here reads the checkout's own `default.conf` or touches `/etc/keyd`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CONF: &str = "\
[ids]
0e6a:02c0

[main]
capslock = overload(nav, capslock)

[nav]
left = home
p = sysrq
";

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("bin")).expect("bin");
        std::fs::create_dir_all(dir.path().join("etc")).expect("etc");
        std::fs::write(dir.path().join("default.conf"), CONF).expect("conf");
        // A keyd that accepts everything, and an installer that does nothing.
        write_script(
            &dir.path().join("bin/keyd"),
            "#!/bin/sh\nexit 0\n",
        );
        write_script(
            &dir.path().join("install.sh"),
            "#!/bin/sh\nexit 0\n",
        );
        Self { dir }
    }

    fn conf(&self) -> PathBuf {
        self.dir.path().join("default.conf")
    }

    fn etc(&self) -> PathBuf {
        self.dir.path().join("etc")
    }

    fn text(&self) -> String {
        std::fs::read_to_string(self.conf()).expect("read conf")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_caps-gui"))
            .args(args)
            .env("CAPS_CONF", self.conf())
            .env("CAPS_INSTALL", self.dir.path().join("install.sh"))
            .env("KEYMAP_ETC", self.etc())
            .env("PATH", self.dir.path().join("bin"))
            .output()
            .expect("run caps-gui")
    }
}

fn write_script(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write script");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).expect("stat").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod");
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn list_prints_bindings_and_the_tap() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["list"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Caps + left → home  — Move to the start of the line"), "{text}");
    assert!(text.contains("Caps + p → sysrq"), "{text}");
    assert!(text.contains("Tap: Caps Lock"), "{text}");
}

#[test]
fn add_writes_the_binding_and_reports_it() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["add", "h", "left"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Added: Caps + h → left"));
    assert!(sandbox.text().contains("h = left"));
}

#[test]
fn adding_the_same_binding_again_says_so() {
    let sandbox = Sandbox::new();
    sandbox.run(&["add", "h", "left"]);
    let output = sandbox.run(&["add", "h", "left"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Already bound: Caps + h → left"));
}

#[test]
fn adding_a_bound_key_replaces_it() {
    let sandbox = Sandbox::new();
    sandbox.run(&["add", "h", "left"]);
    let output = sandbox.run(&["add", "h", "right"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Replaced: Caps + h → right (was left)"));
    assert!(sandbox.text().contains("h = right"));
    assert!(!sandbox.text().contains("h = left"));
}

#[test]
fn rm_removes_a_binding_and_refuses_an_unknown_key() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["rm", "p"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Removed: Caps + p → sysrq"));
    assert!(!sandbox.text().contains("p = sysrq"));

    let output = sandbox.run(&["rm", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("not bound: nope"));
}

#[test]
fn tap_sets_the_caps_tap_action() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["tap", "esc"]);
    assert!(output.status.success());
    assert!(sandbox.text().contains("capslock = overload(nav, esc)"));
    assert!(stdout(&output).contains("Tap action: Esc"));
}

#[test]
fn packs_lists_every_pack() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["packs"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Vim arrows"), "{text}");
    assert!(text.contains("Knob: undo/redo"), "{text}");
}

#[test]
fn pack_merges_by_unique_prefix() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["pack", "knob: zoom"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Pack: Knob: zoom"));
    assert!(sandbox.text().contains("volumedown = C-minus"));
}

#[test]
fn pack_refuses_an_ambiguous_prefix() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["pack", "knob"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("ambiguous"));
}

#[test]
fn diff_reports_identical_different_and_unreadable() {
    let sandbox = Sandbox::new();
    // Unreadable: no live config yet.
    let output = sandbox.run(&["diff"]);
    assert_eq!(output.status.code(), Some(2));

    std::fs::write(sandbox.etc().join("default.conf"), CONF).expect("live");
    let output = sandbox.run(&["diff"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("No differences"));

    std::fs::write(sandbox.etc().join("default.conf"), CONF.replace("left = home", "left = end"))
        .expect("live");
    let output = sandbox.run(&["diff"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    // The live config is the "old" side, the checkout is the "new" one.
    assert!(text.contains("-left = end"), "{text}");
    assert!(text.contains("+left = home"), "{text}");
}

#[test]
fn selftest_passes_against_the_fixture() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["--selftest"]);
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).contains("selftest ok"));
}
