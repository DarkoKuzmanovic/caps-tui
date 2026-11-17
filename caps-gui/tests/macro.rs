//! Tests for the macro model. Every expectation was produced by running the Python
//! implementation (`keymapper/macro.py`) on the same steps, so the two stay in step.

use caps_gui::keyd::macros::{MacroError, MacroStep, build_macro, parse_macro};

fn text(value: &str) -> MacroStep {
    MacroStep::Text(value.into())
}

fn key(value: &str) -> MacroStep {
    MacroStep::Key(value.into())
}

#[test]
fn builds_text_that_is_not_a_key_name() {
    assert_eq!(build_macro(&[text("hi")]).unwrap(), "macro(hi)");
}

#[test]
fn represents_spaces_as_the_space_token() {
    assert_eq!(build_macro(&[text("hello world")]).unwrap(), "macro(hello space world)");
}

#[test]
fn splits_text_that_would_read_as_a_key() {
    assert_eq!(build_macro(&[text("esc")]).unwrap(), "macro(e sc)");
}

#[test]
fn splits_text_that_would_read_as_a_delay() {
    assert_eq!(build_macro(&[text("25ms")]).unwrap(), "macro(2 5 m s)");
}

#[test]
fn builds_keys_and_chords_verbatim() {
    assert_eq!(build_macro(&[key("C-a")]).unwrap(), "macro(C-a)");
    assert_eq!(build_macro(&[key("esc")]).unwrap(), "macro(esc)");
}

#[test]
fn builds_delays() {
    assert_eq!(build_macro(&[MacroStep::Delay(100)]).unwrap(), "macro(100ms)");
}

#[test]
fn builds_a_mixed_macro() {
    let steps = [text("hi"), MacroStep::Delay(50), key("enter")];
    assert_eq!(build_macro(&steps).unwrap(), "macro(hi 50ms enter)");
}

#[test]
fn escapes_a_closing_parenthesis() {
    assert_eq!(build_macro(&[text("x)y")]).unwrap(), "macro(x\\)y)");
}

#[test]
fn rejects_an_empty_macro() {
    assert_eq!(build_macro(&[]).unwrap_err(), MacroError::Empty);
}

#[test]
fn rejects_an_out_of_range_delay() {
    assert_eq!(build_macro(&[MacroStep::Delay(1024)]).unwrap_err(), MacroError::Delay);
}

#[test]
fn rejects_an_invalid_key() {
    assert_eq!(build_macro(&[key("bad key")]).unwrap_err(), MacroError::Key);
    assert_eq!(build_macro(&[key("")]).unwrap_err(), MacroError::Key);
}

#[test]
fn parses_a_plain_macro() {
    assert_eq!(
        parse_macro("macro(a b 20ms C-x space)").unwrap(),
        vec![text("a"), text("b"), MacroStep::Delay(20), key("C-x"), text(" ")]
    );
}

#[test]
fn rejects_unsupported_syntax() {
    assert!(parse_macro("macro(a)b)").is_none());
    assert!(parse_macro("nope").is_none());
    assert!(parse_macro("macro()").is_none());
    assert!(parse_macro("macro(9999ms)").is_none());
    // Beyond u32: Python's int() still reads it and rejects it as over 1023; it must not fall
    // through to being typed as text.
    assert!(parse_macro("macro(99999999999ms)").is_none());
}

#[test]
fn round_trips_every_built_macro() {
    let cases: Vec<Vec<MacroStep>> = vec![
        vec![text("hi")],
        vec![text("hello world")],
        vec![text("esc")],
        vec![text("25ms")],
        vec![key("C-a")],
        vec![key("esc")],
        vec![MacroStep::Delay(100)],
        vec![text("hi"), MacroStep::Delay(50), key("enter")],
        vec![text("x)y")],
    ];
    for steps in cases {
        let action = build_macro(&steps).expect("build");
        let parsed = parse_macro(&action).unwrap_or_else(|| panic!("parse {action}"));
        assert_eq!(build_macro(&parsed).expect("rebuild"), action, "round trip {action}");
    }
}
