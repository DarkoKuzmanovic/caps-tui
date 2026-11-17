//! Tests for the privilege boundary: which command runs a helper, and that the apply helper
//! receives the config on stdin. No root, no polkit, no /etc/keyd.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use caps_gui::keyd::privilege::{Helper, Priv, apply_text, command, detect_from};

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write");
    let mut permissions = std::fs::metadata(&path).expect("stat").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    path
}

#[test]
fn a_stub_wins_over_everything() {
    let dir = temp_dir();
    let stub_path = stub(dir.path(), "stub", "#!/bin/sh\nexit 0\n");
    let priv_kind = detect_from(
        Some(&stub_path),
        true,
        Some(Path::new("/usr/bin/pkexec")),
        Some(Path::new("/usr/libexec/caps-gui-askpass")),
    );
    assert_eq!(priv_kind, Priv::Stub(stub_path));
}

#[test]
fn a_desktop_session_with_pkexec_uses_polkit() {
    let priv_kind = detect_from(None, true, Some(Path::new("/usr/bin/pkexec")), None);
    assert_eq!(priv_kind, Priv::Pkexec);
}

#[test]
fn without_pkexec_the_askpass_shim_is_used() {
    let askpass = Path::new("/usr/libexec/caps-gui-askpass");
    assert_eq!(
        detect_from(None, true, None, Some(askpass)),
        Priv::SudoAskpass(askpass.to_path_buf())
    );
}

#[test]
fn a_terminal_is_the_last_resort() {
    assert_eq!(detect_from(None, false, None, None), Priv::SudoTty);
}

#[test]
fn helpers_live_in_usr_libexec() {
    assert_eq!(Helper::Apply.path(), PathBuf::from("/usr/libexec/caps-gui-apply"));
    assert_eq!(Helper::Monitor.path(), PathBuf::from("/usr/libexec/caps-gui-monitor"));
}

#[test]
fn polkit_runs_the_installed_helper() {
    let cmd = command(&Priv::Pkexec, Helper::Apply);
    assert_eq!(cmd.get_program(), "pkexec");
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, vec!["/usr/libexec/caps-gui-apply"]);
}

#[test]
fn askpass_sets_sudo_askpass() {
    let askpass = PathBuf::from("/usr/libexec/caps-gui-askpass");
    let cmd = command(&Priv::SudoAskpass(askpass.clone()), Helper::Monitor);
    assert_eq!(cmd.get_program(), "sudo");
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, vec!["-A", "/usr/libexec/caps-gui-monitor"]);
    let env: Vec<_> = cmd
        .get_envs()
        .filter(|(key, _)| *key == "SUDO_ASKPASS")
        .collect();
    assert_eq!(env.len(), 1);
    assert_eq!(env[0].1, Some(askpass.as_os_str()));
}

#[test]
fn a_terminal_runs_plain_sudo() {
    let cmd = command(&Priv::SudoTty, Helper::Apply);
    assert_eq!(cmd.get_program(), "sudo");
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, vec!["/usr/libexec/caps-gui-apply"]);
}

#[test]
fn the_stub_receives_the_helper_name() {
    let dir = temp_dir();
    let stub_path = stub(dir.path(), "stub", "#!/bin/sh\nexit 0\n");
    let cmd = command(&Priv::Stub(stub_path.clone()), Helper::Apply);
    assert_eq!(cmd.get_program(), stub_path.as_os_str());
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, vec!["caps-gui-apply"]);
}

#[test]
fn apply_feeds_the_config_on_stdin() {
    let dir = temp_dir();
    let received = dir.path().join("received.conf");
    let script = stub(
        dir.path(),
        "stub",
        &format!("#!/bin/sh\ncat > '{}'\nexit 0\n", received.display()),
    );
    let status = apply_text(&Priv::Stub(script), "[nav]\na = b\n").expect("apply");
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(&received).expect("read"),
        "[nav]\na = b\n"
    );
}
