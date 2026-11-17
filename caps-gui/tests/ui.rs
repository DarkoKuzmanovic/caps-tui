//! Headless UI tests: the real view and key handler, driven through Rune's `Runtime`.

use caps_gui::cli::Paths;
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

fn state() -> State {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("default.conf");
    std::fs::write(&conf, CONF).expect("write");
    // The temp dir must outlive the test body; leaking it is fine for a test process.
    std::mem::forget(dir);
    State::load(Paths {
        conf: conf.clone(),
        install: conf.with_file_name("install.sh"),
        etc: conf.with_file_name("etc"),
    })
    .expect("load")
}

fn runtime() -> Runtime<State> {
    let mut rt = Runtime::new(state(), view, (900.0, 560.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt
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
