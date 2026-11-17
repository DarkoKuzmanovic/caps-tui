//! Tests for the keyd system glue. No sudo, no `/etc/keyd`, no real keyd: every process is a
//! stub and every directory is a temp dir.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use caps_gui::keyd::system::{
    CheckOutcome, MonitorStream, backup_entries, check_with, monitor_capture, monitor_command_from,
    parse_monitor_event, parse_monitor_line, read_live_config_in, split_command,
};

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write script");
    let mut permissions = std::fs::metadata(&path).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    path
}

#[test]
fn splits_a_command_line_with_quotes() {
    assert_eq!(split_command("sudo -n keyd monitor"), vec!["sudo", "-n", "keyd", "monitor"]);
    assert_eq!(
        split_command("sh -c 'printf hello world'"),
        vec!["sh", "-c", "printf hello world"]
    );
    assert_eq!(split_command("  a   b  "), vec!["a", "b"]);
    assert!(split_command("   ").is_empty());
}

#[test]
fn parses_a_monitor_key_down() {
    assert_eq!(parse_monitor_line("kbd\tleft\tleft down").as_deref(), Some("left"));
    assert_eq!(parse_monitor_line("kbd\tleft\tleft up"), None);
    assert_eq!(parse_monitor_line("kbd\tcapslock\tcapslock down"), None);
    assert_eq!(parse_monitor_line("garbage"), None);
    assert_eq!(parse_monitor_line("\t\t"), None);
}

#[test]
fn parses_monitor_events_of_both_states() {
    assert_eq!(
        parse_monitor_event("kbd\tleft\tleft down"),
        Some(("kbd".into(), "left".into(), "down".into()))
    );
    assert_eq!(
        parse_monitor_event("kbd\tleft\tleft up"),
        Some(("kbd".into(), "left".into(), "up".into()))
    );
    assert_eq!(parse_monitor_event("kbd\tleft\tleft sideways"), None);
}

#[test]
fn builds_the_monitor_command_from_its_inputs() {
    let (argv, sudo) = monitor_command_from(Some("fake --flag"), None);
    assert_eq!(argv, vec!["fake", "--flag"]);
    assert!(!sudo);

    let (argv, sudo) = monitor_command_from(None, Some(Path::new("/usr/bin/keyd")));
    assert_eq!(argv, vec!["sudo", "-n", "/usr/bin/keyd", "monitor"]);
    assert!(sudo);

    let (argv, sudo) = monitor_command_from(None, None);
    assert!(argv.is_empty());
    assert!(!sudo);
}

#[test]
fn check_reports_keyd_errors_with_the_temp_path_hidden() {
    let dir = temp_dir();
    let failing = write_executable(
        dir.path(),
        "keyd-fail",
        "#!/bin/sh\necho 'invalid key or action'\nexit 1\n",
    );
    match check_with(Some(&failing), "[nav]\nBROKEN = nonsense\n").expect("check ran") {
        CheckOutcome::Rejected(message) => {
            assert!(message.contains("invalid key or action"), "{message}");
            assert!(!message.contains("tmp"), "the temp path must be replaced: {message}");
        }
        other => panic!("expected a rejection, got {other:?}"),
    }

    let passing = write_executable(dir.path(), "keyd-ok", "#!/bin/sh\nexit 0\n");
    assert_eq!(
        check_with(Some(&passing), "[nav]\na = b\n").expect("check ran"),
        CheckOutcome::Accepted
    );
    assert_eq!(
        check_with(None, "[nav]\na = b\n").expect("no binary is not an error"),
        CheckOutcome::KeydMissing
    );
}

#[test]
fn a_missing_keyd_binary_is_an_error_not_an_acceptance() {
    let dir = temp_dir();
    // A path that exists but cannot be executed: the spawn fails, and that must not read as
    // "keyd accepted it".
    let broken = dir.path().join("not-executable");
    std::fs::write(&broken, "not a program").expect("write");
    assert!(check_with(Some(&broken), "[nav]\na = b\n").is_err());
}

#[test]
fn reads_the_live_config_or_falls_back() {
    let dir = temp_dir();
    let (text, unreadable) = read_live_config_in(dir.path(), "fallback");
    assert_eq!(text, "fallback");
    assert!(unreadable);

    std::fs::write(dir.path().join("default.conf"), "[nav]\na = b\n").expect("write");
    let (text, unreadable) = read_live_config_in(dir.path(), "fallback");
    assert_eq!(text, "[nav]\na = b\n");
    assert!(!unreadable);
}

#[test]
fn lists_backups_newest_first_with_their_binding_counts() {
    let dir = temp_dir();
    std::fs::write(dir.path().join("default.conf"), "[nav]\na = b\n").expect("write");
    std::fs::write(dir.path().join("default.conf.bak.2026-11-15-090000"), "[nav]\na = b\nc = d\n")
        .expect("write");
    std::fs::write(dir.path().join("default.conf.bak.2026-11-17-153149"), "[nav]\na = b\n")
        .expect("write");
    std::fs::write(dir.path().join("unrelated.txt"), "x").expect("write");

    let entries = backup_entries(Some(dir.path()));
    assert_eq!(entries.len(), 2);
    assert!(entries[0].path.ends_with("default.conf.bak.2026-11-17-153149"));
    assert_eq!(entries[0].bindings, Some(1));
    assert_eq!(entries[1].bindings, Some(2));
    assert_eq!(entries[0].stamp.as_deref(), Some("2026-11-17-153149"));
}

#[test]
fn captures_the_first_key_down() {
    let command = vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf 'kbd\\tleft\\tleft down\\n'; sleep 5".to_string(),
    ];
    let started = Instant::now();
    let key = monitor_capture(&command, Duration::from_secs(5)).expect("capture");
    assert_eq!(key, "left");
    assert!(started.elapsed() < Duration::from_secs(4), "capture must return at once");
}

#[test]
fn capture_times_out_when_nothing_arrives() {
    let command = vec!["sh".to_string(), "-c".to_string(), "sleep 5".to_string()];
    let error = monitor_capture(&command, Duration::from_millis(150)).expect_err("timeout");
    assert!(error.to_string().contains("timed out"), "{error}");
}

#[test]
fn streams_monitor_lines_and_reports_exit() {
    let command = vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf 'one\\ntwo\\n'; sleep 0.1".to_string(),
    ];
    let mut stream = MonitorStream::spawn(&command).expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut seen: Vec<String> = Vec::new();
    while Instant::now() < deadline {
        let (lines, status) = stream.poll();
        seen.extend(lines);
        if seen.len() >= 2 && status.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stream.close();
    assert_eq!(seen, vec!["one", "two"]);
}
