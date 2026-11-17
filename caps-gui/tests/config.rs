//! Tests for the keyd config model. Fixtures are inline so a developer's uncommitted
//! `default.conf` can never change what these assert.

use caps_gui::keyd::config::{
    Binding, backup_preview, backup_timestamp, diff_lines, parse, read_tap, render, render_editor,
    sorted_backups, tap_label, without_nav, write_tap,
};

const CONF: &str = "\
# Example: Caps Lock nav layer
[ids]
0e6a:02c0

[main]
# tap = Caps Lock, hold = nav layer
capslock = overload(nav, capslock)

[nav]
left = home
right = end
p = sysrq
volumedown = C-minus
volumeup = C-equal
up = pageup
down = pagedown
mute = C-0
";

#[test]
fn parses_the_nav_bindings_in_file_order() {
    let binds = parse(CONF);
    assert_eq!(binds.len(), 8);
    assert_eq!(binds[0], Binding::new("left", "home"));
    assert_eq!(binds[7], Binding::new("mute", "C-0"));
}

#[test]
fn parse_ignores_comments_and_other_sections() {
    let text = "\
[main]
capslock = overload(nav, capslock)

[nav]
# comment = not a binding
a = b

[other]
c = d
";
    assert_eq!(parse(text), vec![Binding::new("a", "b")]);
}

#[test]
fn reads_the_tap_action() {
    let tap = read_tap(CONF);
    assert_eq!(tap.action.as_deref(), Some("capslock"));
    assert!(tap.editable);
    assert_eq!(tap_label(&tap), "Caps Lock");
}

#[test]
fn reports_a_missing_main_section() {
    let tap = read_tap("[nav]\na = b\n");
    assert!(tap.action.is_none());
    assert!(tap.editable);
    assert_eq!(tap.message, "Tap action is not configured ([main] section missing).");
    assert_eq!(tap_label(&tap), "not configured");
}

#[test]
fn refuses_two_capslock_settings() {
    let text = "\
[main]
capslock = overload(nav, capslock)
capslock = esc

[nav]
a = b
";
    let tap = read_tap(text);
    assert!(!tap.editable);
    assert!(tap.message.starts_with("Read-only [main] capslock settings:"));
}

#[test]
fn refuses_an_unrecognised_overload() {
    let text = "[main]\ncapslock = overload(other, capslock)\n\n[nav]\na = b\n";
    let tap = read_tap(text);
    assert!(!tap.editable);
    assert!(tap.message.contains("expected overload(nav, action)"));
}

#[test]
fn writes_the_tap_and_keeps_everything_else() {
    let out = write_tap(CONF, "esc").expect("write");
    assert!(out.contains("capslock = overload(nav, esc)"));
    assert!(out.contains("left = home"));
    assert!(out.contains("# tap = Caps Lock, hold = nav layer"));
    assert_eq!(read_tap(&out).action.as_deref(), Some("esc"));
}

#[test]
fn writing_the_same_tap_returns_the_text_unchanged() {
    assert_eq!(write_tap(CONF, "capslock").expect("write"), CONF);
}

#[test]
fn preserves_crlf_line_endings() {
    let crlf = CONF.replace('\n', "\r\n");
    let out = write_tap(&crlf, "esc").expect("write");
    assert!(out.contains("capslock = overload(nav, esc)\r\n"));
    assert!(!out.contains("\n\n") || out.contains("\r\n\r\n"));
    assert_eq!(out.matches("\r\n").count(), crlf.matches("\r\n").count());
}

#[test]
fn creates_a_main_section_when_missing() {
    let out = write_tap("[nav]\na = b\n", "esc").expect("write");
    assert!(out.contains("[main]\ncapslock = overload(nav, esc)"));
    assert_eq!(read_tap(&out).action.as_deref(), Some("esc"));
}

#[test]
fn rejects_a_multi_line_action() {
    assert!(write_tap(CONF, "esc\nrm -rf /").is_err());
    assert!(write_tap(CONF, "   ").is_err());
}

#[test]
fn render_replaces_only_the_nav_body() {
    let binds = vec![Binding::new("left", "end"), Binding::new("q", "esc")];
    let out = render(CONF, &binds);
    assert!(out.contains("[nav]\nleft = end\nq = esc\n"));
    assert!(!out.contains("p = sysrq"));
    assert!(out.contains("[ids]\n0e6a:02c0"));
    assert!(out.contains("capslock = overload(nav, capslock)"));
}

#[test]
fn render_appends_a_nav_section_when_missing() {
    let out = render("[main]\ncapslock = overload(nav, capslock)\n", &[Binding::new("a", "b")]);
    assert!(out.contains("\n[nav]\na = b\n"));
}

#[test]
fn render_editor_applies_both_edits() {
    let out = render_editor(CONF, &[Binding::new("a", "b")], "esc", "capslock").expect("render");
    assert!(out.contains("a = b"));
    assert!(out.contains("capslock = overload(nav, esc)"));
}

#[test]
fn diff_is_empty_for_identical_text() {
    assert!(diff_lines(CONF, CONF, "live", "new").is_empty());
}

#[test]
fn diff_shows_the_changed_binding() {
    let changed = CONF.replace("left = home", "left = end");
    let lines = diff_lines(CONF, &changed, "live default.conf", "new default.conf");
    assert!(lines[0].starts_with("--- live default.conf"));
    assert!(lines[1].starts_with("+++ new default.conf"));
    assert!(lines.iter().any(|l| l == "-left = home"));
    assert!(lines.iter().any(|l| l == "+left = end"));
}

#[test]
fn parses_backup_timestamps() {
    assert_eq!(
        backup_timestamp("/etc/keyd/default.conf.bak.2026-11-17-153149").as_deref(),
        Some("2026-11-17-153149")
    );
    assert_eq!(backup_timestamp("/etc/keyd/default.conf"), None);
    assert_eq!(backup_timestamp("/etc/keyd/default.conf.bak.nonsense"), None);
    assert_eq!(backup_timestamp("/etc/keyd/default.conf.bak.2026-13-17-153149"), None);
    assert_eq!(backup_timestamp("/etc/keyd/other.bak.2026-11-17-153149"), None);
}

#[test]
fn sorts_backups_newest_first() {
    let paths = vec![
        "/k/default.conf.bak.2026-11-15-090000".to_string(),
        "/k/default.conf".to_string(),
        "/k/default.conf.bak.2026-11-17-153149".to_string(),
        "/k/default.conf.bak.2026-11-16-120000".to_string(),
    ];
    assert_eq!(
        sorted_backups(&paths),
        vec![
            "/k/default.conf.bak.2026-11-17-153149".to_string(),
            "/k/default.conf.bak.2026-11-16-120000".to_string(),
            "/k/default.conf.bak.2026-11-15-090000".to_string(),
        ]
    );
}

#[test]
fn backup_preview_flags_changes_outside_nav() {
    let backup = CONF.replace("0e6a:02c0", "1234:5678");
    let (lines, outside) = backup_preview(CONF, &backup);
    assert!(outside);
    assert_eq!(lines[0], "Other sections differ; [nav] only");
}

#[test]
fn backup_preview_flags_a_tap_difference() {
    let backup = CONF.replace("overload(nav, capslock)", "overload(nav, esc)");
    let (lines, outside) = backup_preview(CONF, &backup);
    // The tap lives in [main], so it counts as "outside [nav]" too — same order as the Python.
    assert!(outside);
    assert_eq!(lines[0], "Other sections differ; [nav] only");
    assert!(lines[1].starts_with("Tap action differs: editor=capslock, backup=esc"));
}

#[test]
fn backup_preview_says_when_nothing_differs() {
    let (lines, outside) = backup_preview(CONF, CONF);
    assert!(!outside);
    assert_eq!(lines, vec!["No differences; the backup has the same configuration."]);
}

#[test]
fn without_nav_drops_only_the_bindings() {
    let stripped = without_nav(CONF);
    assert!(!stripped.contains("left = home"));
    assert!(stripped.contains("[nav]"));
    assert!(stripped.contains("capslock = overload(nav, capslock)"));
}
