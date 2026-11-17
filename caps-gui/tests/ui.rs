//! Headless UI tests: the real view and key handler, driven through Rune's `Runtime`.
#![cfg(feature = "ui")]

use caps_gui::cli::Paths;
use caps_gui::ui::edit::Mode;
use caps_gui::ui::{State, on_key, view};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

const CONF: &str = "\
[main]
capslock = overload(nav, capslock)

[nav]
left = home
right = end
p = sysrq
volumedown = C-minus
";

fn state_with(conf: &str) -> State {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf_path = dir.path().join("default.conf");
    std::fs::write(&conf_path, conf).expect("write");
    // The temp dir must outlive the test body; leaking it is fine for a test process.
    std::mem::forget(dir);
    State::load(Paths {
        conf: conf_path.clone(),
        install: conf_path.with_file_name("install.sh"),
        etc: conf_path.with_file_name("etc"),
    })
}

fn runtime_with(conf: &str) -> Runtime<State> {
    let mut rt = Runtime::new(state_with(conf), view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(caps_gui::ui::poll_hook);
    rt.frame(0.0);
    rt
}

fn runtime() -> Runtime<State> {
    runtime_with(CONF)
}

fn press(rt: &mut Runtime<State>, key: Key) {
    rt.key(KeyEvent { key, mods: Mods::default() });
    rt.frame(1.0 / 60.0);
}

fn typ(rt: &mut Runtime<State>, text: &str) {
    press(rt, Key::Text(text.to_string()));
}

#[test]
fn the_table_lists_every_binding() {
    let rt = runtime();
    assert_eq!(rt.state.binds.len(), 4);
    let keys: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|element| element.key.clone())
        .collect();
    for index in 0..4 {
        assert!(keys.contains(&format!("row-{index}")), "row-{index} missing: {keys:?}");
    }
}

#[test]
fn arrows_and_jk_move_the_selection() {
    let mut rt = runtime();
    assert_eq!(rt.state.selected, 0);
    press(&mut rt, Key::Down);
    assert_eq!(rt.state.selected, 1);
    press(&mut rt, Key::Down);
    assert_eq!(rt.state.selected, 2);
    press(&mut rt, Key::Up);
    assert_eq!(rt.state.selected, 1);
    typ(&mut rt, "j");
    assert_eq!(rt.state.selected, 2);
    typ(&mut rt, "k");
    assert_eq!(rt.state.selected, 1);
}

#[test]
fn the_selection_stops_at_the_ends() {
    let mut rt = runtime();
    press(&mut rt, Key::Up);
    assert_eq!(rt.state.selected, 0);
    for _ in 0..10 {
        press(&mut rt, Key::Down);
    }
    assert_eq!(rt.state.selected, 3);
}

#[test]
fn slash_filters_and_escape_clears() {
    let mut rt = runtime();
    typ(&mut rt, "/");
    assert!(rt.state.filtering);
    typ(&mut rt, "le");
    assert_eq!(rt.state.filter, "le");
    assert_eq!(rt.state.visible(), vec![0]);
    // While filtering, "q" is text, not quit.
    typ(&mut rt, "q");
    assert_eq!(rt.state.filter, "leq");
    press(&mut rt, Key::Escape);
    assert!(!rt.state.filtering);
    assert!(rt.state.filter.is_empty());
    assert_eq!(rt.state.visible().len(), 4);
}

#[test]
fn enter_keeps_the_filter() {
    let mut rt = runtime();
    typ(&mut rt, "/");
    typ(&mut rt, "rig");
    press(&mut rt, Key::Enter);
    assert!(!rt.state.filtering);
    assert_eq!(rt.state.filter, "rig");
    assert_eq!(rt.state.visible(), vec![1]);
}

#[test]
fn backspace_edits_the_filter() {
    let mut rt = runtime();
    typ(&mut rt, "/");
    typ(&mut rt, "lef");
    press(&mut rt, Key::Backspace);
    assert_eq!(rt.state.filter, "le");
}

#[test]
fn a_filter_matching_nothing_shows_no_rows() {
    let mut rt = runtime();
    typ(&mut rt, "/");
    typ(&mut rt, "zzz");
    assert!(rt.state.visible().is_empty());
    assert!(rt.state.current().is_none());
}

#[test]
fn the_tap_is_read_from_the_config() {
    let rt = runtime();
    assert_eq!(rt.state.tap.action.as_deref(), Some("capslock"));
}

#[test]
fn the_map_draws_every_cap() {
    let rt = runtime();
    let keys: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|element| element.key.clone())
        .collect();
    for cap in ["cap-capslock", "cap-j", "cap-space", "cap-mute", "cap-up", "cap-f24"] {
        assert!(keys.contains(&cap.to_string()), "{cap} missing");
    }
}

#[test]
fn clicking_a_bound_cap_selects_its_row() {
    let conf = CONF.replace("p = sysrq", "p = sysrq\nj = down");
    let mut rt = runtime_with(&conf);
    let (x, y) = rt.find("cap-j").expect("cap-j");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.current().map(|binding| binding.key.as_str()), Some("j"));
}

#[test]
fn clicking_an_unbound_cap_says_so() {
    let mut rt = runtime();
    let (x, y) = rt.find("cap-z").expect("cap-z");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.edit.key, "z");
    assert_eq!(rt.state.edit.mode, Some(Mode::Picker));
}

#[test]
fn enter_opens_the_picker_for_the_selected_binding() {
    let mut rt = runtime();
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, Some(Mode::Picker));
    assert_eq!(rt.state.edit.key, "left");
}

#[test]
fn the_picker_filters_and_applies_a_catalog_action() {
    let mut rt = runtime();
    press(&mut rt, Key::Enter);
    for c in ["z", "o", "o", "m"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.binds[0].action, "C-minus");
    assert!(rt.state.text.contains("left = C-minus"));
    assert!(rt.state.dirty);
}

#[test]
fn escape_closes_the_picker_without_changing_anything() {
    let mut rt = runtime();
    let before = rt.state.text.clone();
    press(&mut rt, Key::Enter);
    typ(&mut rt, "zoom");
    press(&mut rt, Key::Escape);
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.text, before);
    assert!(!rt.state.dirty);
}

#[test]
fn a_custom_action_can_be_typed() {
    let mut rt = runtime();
    press(&mut rt, Key::Enter);
    // A query that matches nothing leaves Custom… first, then Macro builder…
    for c in ["z", "z", "z"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, Some(Mode::Custom));
    for c in ["C", "-", "m", "i", "n", "u", "s"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.binds[0].action, "C-minus");
}

#[test]
fn the_macro_builder_composes_a_macro() {
    let mut rt = runtime();
    press(&mut rt, Key::Enter);
    for c in ["z", "z", "z"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, Some(Mode::Macro));

    typ(&mut rt, "t");
    for c in ["h", "i"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.draft.steps.len(), 1);

    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.binds[0].action, "macro(hi)");
    assert!(rt.state.text.contains("left = macro(hi)"));
}

#[test]
fn a_macro_step_can_be_removed_and_reordered() {
    let mut rt = runtime();
    press(&mut rt, Key::Enter);
    for c in ["z", "z", "z"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Enter);
    // text "a", then key "C-x"
    typ(&mut rt, "t");
    typ(&mut rt, "a");
    press(&mut rt, Key::Enter);
    typ(&mut rt, "k");
    for c in ["C", "-", "x"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.draft.steps.len(), 2);
    typ(&mut rt, "K");
    assert_eq!(
        rt.state.edit.draft.steps[0],
        caps_gui::keyd::macros::MacroStep::Key("C-x".into())
    );
    typ(&mut rt, "x");
    assert_eq!(rt.state.edit.draft.steps.len(), 1);
}

#[test]
fn d_removes_the_selected_binding_and_u_restores_it() {
    let mut rt = runtime();
    typ(&mut rt, "d");
    assert_eq!(rt.state.binds.len(), 3);
    assert!(!rt.state.text.contains("left = home"));
    typ(&mut rt, "u");
    assert_eq!(rt.state.binds.len(), 4);
    assert!(rt.state.text.contains("left = home"));
    typ(&mut rt, "U");
    assert_eq!(rt.state.binds.len(), 3);
}

#[test]
fn a_keyd_rejection_keeps_the_overlay_open() {
    let mut rt = runtime();
    rt.state.checker = |_: &str| {
        Ok(caps_gui::keyd::system::CheckOutcome::Rejected(
            "invalid key or action".into(),
        ))
    };
    press(&mut rt, Key::Enter);
    for c in ["z", "o", "o", "m"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    // The picker stays open, the binding is untouched, and the reason is shown.
    assert_eq!(rt.state.edit.mode, Some(Mode::Picker));
    assert!(rt.state.edit.error.contains("invalid key or action"), "{}", rt.state.edit.error);
    assert_eq!(rt.state.binds[0].action, "home");
    assert!(!rt.state.dirty);
}

#[test]
fn t_sets_the_tap_action() {
    let mut rt = runtime();
    typ(&mut rt, "t");
    assert_eq!(rt.state.edit.mode, Some(Mode::Tap));
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.tap.action.as_deref(), Some("esc"));
    assert!(rt.state.text.contains("capslock = overload(nav, esc)"));
    assert!(rt.state.dirty);
}

#[test]
fn p_merges_a_preset_pack() {
    let mut rt = runtime();
    typ(&mut rt, "p");
    assert_eq!(rt.state.edit.mode, Some(Mode::Packs));
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, None);
    // "Vim arrows" is first: h/j/k/l and u/o.
    assert!(rt.state.text.contains("h = left"));
    assert!(rt.state.text.contains("o = pagedown"));
    assert!(rt.state.status.contains("Pack: Vim arrows"));
}

#[test]
fn b_loads_nav_from_a_backup() {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("default.conf");
    std::fs::write(&conf, CONF).expect("write");
    let etc = dir.path().join("etc");
    std::fs::create_dir_all(&etc).expect("etc");
    std::fs::write(etc.join("default.conf"), CONF).expect("live");
    std::fs::write(
        etc.join("default.conf.bak.2026-11-17-153149"),
        "[main]\ncapslock = overload(nav, capslock)\n\n[nav]\nq = esc\n",
    )
    .expect("backup");

    let state = State::load(Paths {
        conf: conf.clone(),
        install: conf.with_file_name("install.sh"),
        etc: etc.clone(),
    });
    let mut rt = Runtime::new(state, view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);

    typ(&mut rt, "b");
    assert_eq!(rt.state.edit.mode, Some(Mode::Backups));
    assert_eq!(rt.state.edit.backups.len(), 1);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.binds.len(), 1);
    assert_eq!(rt.state.binds[0].key, "q");
    assert!(rt.state.text.contains("q = esc"));
    assert!(!rt.state.text.contains("left = home"));
    assert!(rt.state.dirty);
}

#[test]
fn undo_after_a_tap_change_restores_the_tap() {
    let mut rt = runtime();
    typ(&mut rt, "t");
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.tap.action.as_deref(), Some("esc"));
    typ(&mut rt, "u");
    assert_eq!(rt.state.tap.action.as_deref(), Some("capslock"));
    assert!(rt.state.text.contains("capslock = overload(nav, capslock)"));
}

#[test]
fn the_pack_panel_previews_before_merging() {
    let mut rt = runtime();
    typ(&mut rt, "p");
    // "Knob: tabs" is the sixth pack.
    for _ in 0..5 {
        press(&mut rt, Key::Down);
    }
    rt.frame(1.0 / 60.0);
    assert!(rt.find("preview-0").is_some(), "no preview block");
    let preview: Vec<String> = rt
        .elements()
        .iter()
        .filter(|element| element.key.as_deref().is_some_and(|key| key.starts_with("preview-")))
        .filter_map(|element| element.text.clone())
        .collect();
    assert!(preview.iter().any(|text| text.contains("Pack: Knob: tabs")), "{preview:?}");
    assert!(preview.iter().any(|text| text.contains("volumedown: C-minus → C-S-tab")), "{preview:?}");
    // Nothing merged yet.
    assert!(!rt.state.text.contains("C-S-tab"));
}

#[test]
fn the_backup_panel_previews_before_loading() {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("default.conf");
    std::fs::write(&conf, CONF).expect("write");
    let etc = dir.path().join("etc");
    std::fs::create_dir_all(&etc).expect("etc");
    std::fs::write(etc.join("default.conf"), CONF).expect("live");
    std::fs::write(
        etc.join("default.conf.bak.2026-11-17-153149"),
        "[main]\ncapslock = overload(nav, capslock)\n\n[nav]\nleft = end\n",
    )
    .expect("backup");

    let state = State::load(Paths {
        conf: conf.clone(),
        install: conf.with_file_name("install.sh"),
        etc: etc.clone(),
    });
    let mut rt = Runtime::new(state, view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    typ(&mut rt, "b");
    rt.frame(1.0 / 60.0);

    let preview: Vec<String> = rt
        .elements()
        .iter()
        .filter(|element| element.key.as_deref().is_some_and(|key| key.starts_with("preview-")))
        .filter_map(|element| element.text.clone())
        .collect();
    assert!(preview.iter().any(|text| text.contains("-left = home")), "{preview:?}");
    assert!(preview.iter().any(|text| text.contains("+left = end")), "{preview:?}");
    assert!(rt.state.text.contains("left = home"), "nothing loaded yet");
}

#[test]
fn the_monitor_panel_shows_streamed_keys() {
    let mut rt = runtime();
    let command = vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf 'kbd\\tDaKai\\tleftcontrol down\\nkbd\\tDaKai\\tleft down\\n'; sleep 0.1".to_string(),
    ];
    rt.state.monitor_stream =
        Some(caps_gui::keyd::system::MonitorStream::spawn(&command).expect("spawn"));
    rt.state.edit.mode = Some(Mode::Monitor);

    // The poll hook drains the stream and asks for a redraw, exactly as the window does.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline && rt.state.monitor.events.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(20));
        rt.frame(1.0 / 60.0);
    }

    assert_eq!(rt.state.monitor.events.len(), 1);
    assert_eq!(rt.state.monitor.events[0].key, "C-left");
    let keys: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|element| element.key.clone())
        .collect();
    assert!(keys.iter().any(|key| key.starts_with("monitor-")), "{keys:?}");
}

#[test]
fn l_opens_the_monitor_panel() {
    let mut rt = runtime();
    rt.state.monitor_command = Some(vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf 'kbd\tDaKai\tleftcontrol down\nkbd\tDaKai\tleft down\n'; sleep 1".to_string(),
    ]);
    typ(&mut rt, "l");
    assert_eq!(rt.state.edit.mode, Some(Mode::Monitor));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline && rt.state.monitor.events.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(20));
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.monitor.events.len(), 1);
    assert_eq!(rt.state.monitor.events[0].key, "C-left");
}

#[test]
fn escape_stops_the_monitor() {
    let mut rt = runtime();
    let command = vec!["sh".to_string(), "-c".to_string(), "sleep 5".to_string()];
    rt.state.monitor_stream =
        Some(caps_gui::keyd::system::MonitorStream::spawn(&command).expect("spawn"));
    rt.state.edit.mode = Some(Mode::Monitor);
    press(&mut rt, Key::Escape);
    assert!(rt.state.monitor_stream.is_none());
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.monitor_status, "stopped");
}

fn live_runtime(live: &str) -> (tempfile::TempDir, std::path::PathBuf, Runtime<State>) {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("default.conf");
    std::fs::write(&conf, CONF).expect("write");
    let etc = dir.path().join("etc");
    std::fs::create_dir_all(&etc).expect("etc");
    std::fs::write(etc.join("default.conf"), live).expect("live");
    let state = State::load(Paths {
        conf: conf.clone(),
        install: conf.with_file_name("install.sh"),
        etc,
    });
    let mut rt = Runtime::new(state, view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(caps_gui::ui::poll_hook);
    rt.frame(0.0);
    (dir, conf, rt)
}

fn script(path: &std::path::Path, body: &str) {
    std::fs::write(path, body).expect("write script");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).expect("stat").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod");
}

#[test]
fn a_opens_the_apply_review_with_the_diff() {
    let (_dir, _conf, mut rt) = live_runtime(&CONF.replace("left = home", "left = end"));
    typ(&mut rt, "A");
    assert_eq!(rt.state.edit.mode, Some(Mode::Apply));
    let texts: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|element| element.text.clone())
        .collect();
    assert!(texts.iter().any(|text| text.contains("-left = end")), "{texts:?}");
    assert!(texts.iter().any(|text| text.contains("+left = home")), "{texts:?}");
}

#[test]
fn a_with_nothing_to_apply_says_so() {
    let (_dir, _conf, mut rt) = live_runtime(CONF);
    typ(&mut rt, "A");
    assert_eq!(rt.state.edit.mode, None);
    assert_eq!(rt.state.status, "No differences; nothing to apply.");
}

#[test]
fn enter_applies_through_the_helper() {
    let (dir, conf, mut rt) = live_runtime(&CONF.replace("left = home", "left = end"));
    let received = dir.path().join("received.conf");
    let stub = dir.path().join("apply-stub");
    script(
        &stub,
        &format!("#!/bin/sh\ncat > '{}'\nexit 0\n", received.display()),
    );
    rt.state.privilege = caps_gui::keyd::privilege::Priv::Stub(stub);

    typ(&mut rt, "A");
    press(&mut rt, Key::Enter);

    assert_eq!(rt.state.status, "Saved and applied.");
    assert!(!rt.state.dirty);
    // The helper received the buffer, and the file on disk matches it, so the next launch
    // cannot load the old bindings and push them back.
    assert_eq!(std::fs::read_to_string(&received).expect("received"), rt.state.text);
    assert_eq!(std::fs::read_to_string(&conf).expect("conf"), rt.state.text);
}

#[test]
fn a_rejected_apply_shows_the_reason() {
    let (dir, _conf, mut rt) = live_runtime(&CONF.replace("left = home", "left = end"));
    let stub = dir.path().join("apply-stub");
    script(&stub, "#!/bin/sh\ncat > /dev/null\necho 'invalid key or action' >&2\nexit 1\n");
    rt.state.privilege = caps_gui::keyd::privilege::Priv::Stub(stub);

    typ(&mut rt, "A");
    press(&mut rt, Key::Enter);

    assert_eq!(rt.state.edit.mode, Some(Mode::Apply));
    assert!(rt.state.edit.error.contains("invalid key or action"), "{}", rt.state.edit.error);
}

#[test]
fn a_long_review_keeps_the_warnings_first_and_marks_the_overflow() {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("default.conf");
    // The buffer has a root-running binding, so the review carries a warning.
    std::fs::write(
        &conf,
        "[main]\ncapslock = overload(nav, capslock)\n\n[nav]\nspace = command(runuser -u quzma -- /bin/true)\n",
    )
    .expect("write");
    let etc = dir.path().join("etc");
    std::fs::create_dir_all(&etc).expect("etc");
    // The live config differs in forty bindings, so the diff is far longer than the panel.
    let mut live = String::from("[main]\ncapslock = overload(nav, capslock)\n\n[nav]\n");
    for index in 0..40 {
        live.push_str(&format!("k{index} = esc\n"));
    }
    std::fs::write(etc.join("default.conf"), live).expect("live");

    let state = State::load(Paths {
        conf: conf.clone(),
        install: conf.with_file_name("install.sh"),
        etc,
    });
    let mut rt = Runtime::new(state, view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(caps_gui::ui::poll_hook);
    rt.frame(0.0);

    typ(&mut rt, "A");
    assert_eq!(rt.state.edit.mode, Some(Mode::Apply));
    let texts: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|element| element.text.clone())
        .collect();
    // The warning is the first row, so no diff length can push it off the panel.
    assert!(texts.iter().any(|text| text.contains("WARN space")), "{texts:?}");
    assert_eq!(rt.state.edit.apply_lines[0], "Warnings:");
    // And the panel says how much it is not showing.
    let marker = texts.iter().find(|text| text.contains("more lines"));
    assert!(marker.is_some(), "{texts:?}");
}

#[test]
fn a_captures_a_key_from_the_monitor() {
    let mut rt = runtime();
    rt.state.monitor_command = Some(vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf 'kbd\\tDaKai\\tcapslock down\\nkbd\\tDaKai\\tcapslock up\\nkbd\\tDaKai\\tq down\\n'; sleep 1"
            .to_string(),
    ]);
    typ(&mut rt, "a");
    assert_eq!(rt.state.edit.mode, Some(Mode::Capture));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline && rt.state.edit.mode == Some(Mode::Capture) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        rt.frame(1.0 / 60.0);
    }
    // Caps Lock is filtered (it is the layer key); the next key opens the picker for it.
    assert_eq!(rt.state.edit.mode, Some(Mode::Picker));
    assert_eq!(rt.state.edit.key, "q");
    assert!(rt.state.monitor_stream.is_none(), "the stream stops after one key");
}

#[test]
fn a_capture_gives_up_after_ten_seconds() {
    let mut rt = runtime();
    rt.state.monitor_command = Some(vec![
        "sh".to_string(),
        "-c".to_string(),
        "sleep 30".to_string(),
    ]);
    typ(&mut rt, "a");
    assert_eq!(rt.state.edit.mode, Some(Mode::Capture));

    // A silent stream never wakes the window, so the timeout must ride the clock, not a poll.
    for _ in 0..30 {
        rt.frame(0.5);
    }
    assert_eq!(rt.state.edit.mode, None);
    assert!(rt.state.status.contains("timed out"), "{}", rt.state.status);
    assert!(rt.state.monitor_stream.is_none());
}

#[test]
fn a_missing_config_refuses_to_apply() {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("missing.conf");
    let state = State::load(Paths {
        conf: conf.clone(),
        install: conf.with_file_name("install.sh"),
        etc: dir.path().join("etc"),
    });
    assert!(state.load_error.is_some());
    let mut rt = Runtime::new(state, view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(caps_gui::ui::poll_hook);
    rt.frame(0.0);

    typ(&mut rt, "A");
    assert_eq!(rt.state.edit.mode, None);
    assert!(rt.state.status.starts_with("not applying:"), "{}", rt.state.status);
    assert!(!conf.exists(), "nothing may be written over an unreadable config");
}

#[test]
fn q_warns_again_after_more_editing() {
    let mut rt = runtime();
    // Make the buffer dirty without leaving the table.
    typ(&mut rt, "d");
    assert!(rt.state.dirty);

    typ(&mut rt, "q");
    assert!(rt.state.quit_armed);
    assert!(rt.state.status.starts_with("Unsaved changes"));

    // Any other key means work continues, so the warning must come back.
    press(&mut rt, Key::Down);
    assert!(!rt.state.quit_armed);
    typ(&mut rt, "q");
    assert!(rt.state.quit_armed);
    assert!(rt.state.status.starts_with("Unsaved changes"));
}

#[test]
fn s_saves_the_file() {
    let mut rt = runtime();
    press(&mut rt, Key::Enter);
    for c in ["z", "o", "o", "m"] {
        typ(&mut rt, c);
    }
    press(&mut rt, Key::Enter);
    assert!(rt.state.dirty);
    typ(&mut rt, "s");
    assert!(!rt.state.dirty);
    let saved = std::fs::read_to_string(&rt.state.paths.conf).expect("read");
    assert!(saved.contains("left = C-minus"));
}
