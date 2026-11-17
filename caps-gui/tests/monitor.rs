//! Tests for the live monitor log: chord composition, annotations and the modifier bookkeeping.

use caps_gui::keyd::config::Binding;
use caps_gui::keyd::monitor::MonitorLog;

fn binds(pairs: &[(&str, &str)]) -> Vec<Binding> {
    pairs.iter().map(|(k, a)| Binding::new(*k, *a)).collect()
}

#[test]
fn a_modifier_tapped_alone_is_one_row() {
    let mut log = MonitorLog::default();
    assert!(!log.record("kbd\tDaKai\tleftcontrol down", &[]));
    assert!(log.record("kbd\tDaKai\tleftcontrol up", &[]));
    assert_eq!(log.events.len(), 1);
    assert_eq!(log.events[0].key, "leftcontrol");
    assert_eq!(log.events[0].state, "down/up");
}

#[test]
fn a_held_modifier_composes_a_chord() {
    let mut log = MonitorLog::default();
    let bindings = binds(&[("j", "C-left")]);
    assert!(!log.record("kbd\tDaKai\tleftcontrol down", &bindings));
    assert!(log.record("kbd\tDaKai\tleft down", &bindings));
    assert_eq!(log.events.len(), 1);
    assert_eq!(log.events[0].key, "C-left");
    assert!(log.events[0].chord);
    assert_eq!(log.events[0].annotation, vec!["Caps + j → C-left"]);
}

#[test]
fn a_consumed_modifier_does_not_get_its_own_row() {
    let mut log = MonitorLog::default();
    let bindings = binds(&[("j", "C-left")]);
    log.record("kbd\tDaKai\tleftcontrol down", &bindings);
    log.record("kbd\tDaKai\tleft down", &bindings);
    // The modifier is released after the chord: it was consumed, so no extra row.
    assert!(!log.record("kbd\tDaKai\tleftcontrol up", &bindings));
    assert_eq!(log.events.len(), 1);
}

#[test]
fn a_key_release_closes_its_row() {
    let mut log = MonitorLog::default();
    assert!(log.record("kbd\tDaKai\tleft down", &[]));
    assert!(log.record("kbd\tDaKai\tleft up", &[]));
    assert_eq!(log.events.len(), 1);
    assert_eq!(log.events[0].state, "down/up");
}

#[test]
fn arrows_get_their_symbol_in_the_annotation() {
    let mut log = MonitorLog::default();
    let bindings = binds(&[("volumedown", "left")]);
    log.record("kbd\tDaKai\tleft down", &bindings);
    assert_eq!(log.events[0].annotation, vec!["← Caps + volumedown"]);
}

#[test]
fn garbage_lines_are_ignored() {
    let mut log = MonitorLog::default();
    assert!(!log.record("not a monitor line", &[]));
    assert!(!log.record("kbd\tDaKai\tleft sideways", &[]));
    assert!(log.events.is_empty());
}

#[test]
fn rows_render_the_state_and_annotation() {
    let mut log = MonitorLog::default();
    let bindings = binds(&[("j", "C-left")]);
    log.record("kbd\tDaKai\tleftcontrol down", &bindings);
    log.record("kbd\tDaKai\tleft down", &bindings);
    let rows = log.rows();
    assert_eq!(rows, vec!["⇢ C-left down   Caps + j → C-left"]);
}
