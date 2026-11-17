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
    .expect("load")
}

fn state() -> State {
    state_with(CONF)
}

fn runtime_with(conf: &str) -> Runtime<State> {
    let mut rt = Runtime::new(state_with(conf), view, (980.0, 760.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
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
