//! The keyboard map: the same rows, arrows, knob and F-row the TUI draws, as clickable caps.
//!
//! Every cap is an element with a key (`cap-<keyd name>`), so a headless script can click one and
//! `--tree` can prove it is there.

use rune::tok::{self, type_};
use rune::{El, col, el, row, text};

use super::State;
use crate::keyd::data::{KEYBOARD_ROWS, KNOB_KEYS, function_keys, key_label, keycap_cells};
use crate::keyd::rules::Highlight;

/// Points per terminal cell — the TUI's widths, scaled for a window.
const CELL: f32 = 4.2;
const CAP_H: f32 = 26.0;
const GAP: f32 = 4.0;

fn style(s: &State, key: &str) -> Highlight {
    let selected = s.current().map(|binding| binding.key.to_lowercase());
    if selected.as_deref() == Some(key.to_lowercase().as_str()) {
        return Highlight::Selected;
    }
    if s.binds.iter().any(|binding| binding.key.eq_ignore_ascii_case(key)) {
        Highlight::Bound
    } else {
        Highlight::Plain
    }
}

fn cap(s: &State, t: &tok::Theme, key: &str, cells: u32) -> El {
    let (fill, ink) = match style(s, key) {
        Highlight::Selected => (t.text, t.bg),
        Highlight::Bound => (tok::accent(0, true), t.bg),
        Highlight::Plain => (t.control_off, t.text),
    };
    let owned = key.to_string();
    el().key(format!("cap-{key}"))
        .size(cells as f32 * CELL, CAP_H)
        .radius(5.0)
        .fill(fill)
        .center()
        .child(text(key_label(key)).font(type_::SECTION).color(ink))
        .on_press(move |s: &mut State| super::select_key(s, &owned))
}

fn spacer(cells: u32) -> El {
    el().size(cells as f32 * CELL, CAP_H)
}

/// The whole map: rows on the left, arrows + knob + F-row on the right.
pub fn map(s: &State, t: &tok::Theme) -> El {
    let mut board = col().key("map").gap(GAP);
    for keys in KEYBOARD_ROWS {
        let mut line = row().gap(GAP);
        for key in *keys {
            line = line.child(cap(s, t, key, keycap_cells(key)));
        }
        board = board.child(line);
    }

    let arrows = col()
        .key("map-arrows")
        .gap(GAP)
        .child(
            row()
                .gap(GAP)
                .child(spacer(keycap_cells("up")))
                .child(cap(s, t, "up", keycap_cells("up")))
                .child(spacer(keycap_cells("up"))),
        )
        .child(
            row()
                .gap(GAP)
                .child(cap(s, t, "left", keycap_cells("left")))
                .child(cap(s, t, "down", keycap_cells("down")))
                .child(cap(s, t, "right", keycap_cells("right"))),
        );

    let mut knob = row().key("map-knob").gap(GAP);
    for key in KNOB_KEYS {
        knob = knob.child(cap(s, t, key, keycap_cells(key)));
    }

    let mut function_rows = col().key("map-f-row").gap(GAP);
    let functions = function_keys();
    for chunk in functions.chunks(6) {
        let mut line = row().gap(GAP);
        for key in chunk {
            line = line.child(cap(s, t, key, keycap_cells(key)));
        }
        function_rows = function_rows.child(line);
    }

    let side = col().gap(GAP + 4.0).child(arrows).child(knob).child(function_rows);

    row()
        .w_full()
        .gap(20.0)
        .child(board)
        .child(side)
        .child(
            text("Filled = bound · white = selected · click a cap to select it")
                .font(type_::SECTION)
                .grow(1.0)
                .color(t.text_secondary),
        )
}
