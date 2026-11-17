//! Tests for the binding rules. Expectations mirror `keymapper/rules.py`.

use caps_gui::keyd::config::Binding;
use caps_gui::keyd::data::{CATALOG, PACKS, action_description, key_label, keycap_cells};
use caps_gui::keyd::rules::{
    Highlight, History, Level, binding_warnings, compose_monitor_chord, filter_actions,
    filter_binding_indices, keyboard_highlights, keyboard_unshown_bindings, merge_pack,
    monitor_chords_equal, pack_summary, upsert_binding, warning_preview_lines, warnings_for_key,
};

fn binds(pairs: &[(&str, &str)]) -> Vec<Binding> {
    pairs.iter().map(|(k, a)| Binding::new(*k, *a)).collect()
}

#[test]
fn composes_chords_in_keyd_modifier_order() {
    let held = ["leftshift".to_string(), "leftcontrol".to_string()];
    assert_eq!(compose_monitor_chord(&held, "minus"), "C-S-minus");
    assert_eq!(compose_monitor_chord(&[], "minus"), "minus");
    assert_eq!(compose_monitor_chord(&["rightalt".to_string()], "a"), "G-a");
}

#[test]
fn compares_chords_ignoring_modifier_order() {
    assert!(monitor_chords_equal("S-C-minus", "C-S-minus"));
    assert!(monitor_chords_equal("minus", "MINUS"));
    assert!(!monitor_chords_equal("C-minus", "C-equal"));
}

#[test]
fn history_records_undoes_and_redoes() {
    let mut history: History<Vec<Binding>> = History::new(2);
    let first = binds(&[("a", "b")]);
    let second = binds(&[("a", "c")]);
    assert!(history.record(&first, &second, "edit a"));
    assert!(!history.record(&second, &second, "no change"));
    assert!(history.can_undo());

    let (snapshot, description) = history.undo(&second).expect("undo");
    assert_eq!(snapshot, first);
    assert_eq!(description, "edit a");
    assert!(history.can_redo());

    let (again, _) = history.redo(&first).expect("redo");
    assert_eq!(again, second);
}

#[test]
fn history_drops_the_oldest_beyond_its_limit() {
    let mut history: History<Vec<Binding>> = History::new(2);
    let states: Vec<Vec<Binding>> = (0..4).map(|n| binds(&[("a", &format!("{n}"))])).collect();
    for pair in states.windows(2) {
        history.record(&pair[0], &pair[1], "step");
    }
    // Two undos are possible, then the history is empty.
    assert!(history.undo(&states[3]).is_some());
    assert!(history.undo(&states[2]).is_some());
    assert!(history.undo(&states[1]).is_none());
}

#[test]
fn filters_actions_by_category_action_and_description() {
    let zoom = filter_actions("zoom", CATALOG);
    assert!(zoom.iter().any(|entry| entry.action == "C-minus"));
    assert!(zoom.iter().all(|entry| {
        format!("{} {} {}", entry.category, entry.action, entry.description)
            .to_lowercase()
            .contains("zoom")
    }));
    assert_eq!(filter_actions("", CATALOG).len(), CATALOG.len());
    assert!(filter_actions("no such action", CATALOG).is_empty());
}

#[test]
fn filters_bindings_by_key_action_and_description() {
    let bindings = binds(&[("left", "home"), ("p", "sysrq")]);
    assert_eq!(filter_binding_indices(&bindings, ""), vec![0, 1]);
    assert_eq!(filter_binding_indices(&bindings, "left"), vec![0]);
    assert_eq!(filter_binding_indices(&bindings, "sysrq"), vec![1]);
    // "Print Screen" is the catalog description of sysrq, not text in the binding.
    assert_eq!(filter_binding_indices(&bindings, "print"), vec![1]);
}

#[test]
fn merges_a_pack_reporting_added_and_replaced() {
    let bindings = binds(&[("left", "home")]);
    let pack = PACKS.iter().find(|pack| pack.name == "Knob: zoom").expect("pack");
    let merge = merge_pack(&bindings, pack);
    assert_eq!(merge.added.len(), 3);
    assert!(merge.replaced.is_empty());
    assert_eq!(merge.merged.len(), 4);

    let again = merge_pack(&merge.merged, pack);
    assert!(again.added.is_empty());
    assert!(again.replaced.is_empty());
    assert_eq!(again.merged, merge.merged);
}

#[test]
fn merging_replaces_an_existing_action() {
    let bindings = binds(&[("volumedown", "left")]);
    let pack = PACKS.iter().find(|pack| pack.name == "Knob: zoom").expect("pack");
    let merge = merge_pack(&bindings, pack);
    assert_eq!(
        merge.replaced,
        vec![("volumedown".to_string(), "left".to_string(), "C-minus".to_string())]
    );
}

#[test]
fn upsert_appends_and_drops_duplicates() {
    let bindings = binds(&[("a", "b"), ("c", "d"), ("a", "e")]);
    let out = upsert_binding(&bindings, "a", "f", None);
    assert_eq!(out, binds(&[("c", "d"), ("a", "f")]));
}

#[test]
fn upsert_replaces_the_selected_row_in_place() {
    let bindings = binds(&[("a", "b"), ("c", "d")]);
    let out = upsert_binding(&bindings, "x", "y", Some(1));
    assert_eq!(out, binds(&[("a", "b"), ("x", "y")]));
    // An out-of-range index leaves the list alone.
    assert_eq!(upsert_binding(&bindings, "x", "y", Some(9)), bindings);
}

#[test]
fn highlights_selected_over_bound_over_plain() {
    let bindings = binds(&[("j", "home"), ("p", "sysrq")]);
    let map = keyboard_highlights(&bindings, Some("p"));
    assert_eq!(map["p"], Highlight::Selected);
    assert_eq!(map["j"], Highlight::Bound);
    assert_eq!(map["z"], Highlight::Plain);
    // The arrows are drawn as their own cluster, so they are not in the row map.
    assert!(!map.contains_key("left"));
}

#[test]
fn lists_bindings_that_are_not_on_the_drawn_map() {
    let bindings = binds(&[("left", "home"), ("volumedown", "C-minus"), ("f13", "esc")]);
    // Arrows, the knob and the F-row are drawn as their own clusters, not in the rows.
    assert_eq!(keyboard_unshown_bindings(&bindings), vec!["left", "volumedown", "f13"]);
}

#[test]
fn warns_about_risky_bindings() {
    let bindings = binds(&[
        ("q", "A-f4"),
        ("space", "command(runuser -u quzma -- /bin/true)"),
        ("x", "x"),
        ("capslock", "esc"),
        ("leftbrace", "M-left"),
    ]);
    let warnings = binding_warnings(&bindings, Some("capslock"));
    assert!(warnings.iter().any(|w| w.level == Level::Warn && w.message.contains("close a window")));
    assert!(warnings.iter().any(|w| w.message.contains("runs as root")));
    assert!(warnings.iter().any(|w| w.message.contains("No-op")));
    assert!(warnings.iter().any(|w| w.message.contains("layer key (tap sends capslock)")));
    assert!(warnings.iter().any(|w| w.level == Level::Info && w.message.contains("desktop")));
}

#[test]
fn warning_helpers_filter_and_format() {
    let bindings = binds(&[("q", "A-f4"), ("x", "x")]);
    let warnings = binding_warnings(&bindings, None);
    assert_eq!(warnings_for_key(&warnings, "X").len(), 1);
    let lines = warning_preview_lines(&bindings, None);
    assert_eq!(lines[0], "Warnings:");
    assert!(lines.iter().any(|line| line.starts_with("  WARN x:")));
    assert!(warning_preview_lines(&[], None).is_empty());
}

#[test]
fn pack_summary_lists_added_and_replaced() {
    let pack = PACKS.iter().find(|pack| pack.name == "Knob: tabs").expect("pack");
    let merge = merge_pack(&binds(&[("volumedown", "left")]), pack);
    let lines = pack_summary(pack, &merge, None);
    assert_eq!(lines[0], "Pack: Knob: tabs");
    assert!(lines.iter().any(|line| line == "Added:"));
    assert!(lines.iter().any(|line| line == "Replaced:"));
    assert!(lines.iter().any(|line| line.contains("volumedown: left → C-S-tab")));
}

#[test]
fn data_tables_agree_with_the_catalog() {
    assert_eq!(action_description("C-minus"), Some("Zoom out (Ctrl+Minus)"));
    assert_eq!(action_description("nope"), None);
    assert_eq!(key_label("capslock"), "Caps");
    assert_eq!(key_label("f13"), "F13");
    assert_eq!(key_label("q"), "Q");
    assert_eq!(keycap_cells("space"), 27);
    assert_eq!(keycap_cells("a"), 6);
    // Every pack binding names an action the catalog knows or a plain keyd key.
    for pack in PACKS {
        for (_, action) in pack.binds {
            assert!(
                action_description(action).is_some() || action.contains('-') || action.len() <= 8,
                "pack {} uses {action}",
                pack.name
            );
        }
    }
}
