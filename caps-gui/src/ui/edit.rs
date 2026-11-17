//! Editing: the action picker, the custom-action field and the macro builder.
//!
//! One overlay at a time, drawn in place of the table. Every commit goes through
//! [`apply_binding`], which renders the config, records undo history and marks the state dirty.

use rune::tok::{self, radius, type_};
use rune::{El, Key, KeyEvent, col, el, row, text};

use super::State;
use crate::keyd::config::{Binding, render};
use crate::keyd::data::{CATALOG, CUSTOM, action_description};
use crate::keyd::macros::{MacroStep, build_macro, parse_macro};
use crate::keyd::rules::{filter_actions, upsert_binding};

/// What the overlay is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Picker,
    Custom,
    Macro,
}

/// A macro being composed.
#[derive(Debug, Default, Clone)]
pub struct MacroDraft {
    pub steps: Vec<MacroStep>,
    /// Which step kind is being typed, if any.
    pub typing: Option<Kind>,
    pub buffer: String,
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Key,
    Delay,
}

/// The overlay's state.
#[derive(Debug, Default, Clone)]
pub struct Edit {
    pub mode: Option<Mode>,
    /// The key the chosen action will be bound to.
    pub key: String,
    pub query: String,
    pub picker: usize,
    pub custom: String,
    pub draft: MacroDraft,
    pub error: String,
}

impl Edit {
    pub fn is_open(&self) -> bool {
        self.mode.is_some()
    }

    pub fn close(&mut self) {
        *self = Edit::default();
    }
}

/// A snapshot of everything an undo restores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub binds: Vec<Binding>,
    pub tap: Option<String>,
}

fn snapshot(s: &State) -> Snapshot {
    Snapshot { binds: s.binds.clone(), tap: s.tap.action.clone() }
}

/// Replace or add a binding, re-render, and record it for undo.
pub fn apply_binding(s: &mut State, key: &str, action: &str) {
    let index = s.binds.iter().position(|binding| binding.key.eq_ignore_ascii_case(key));
    let before = snapshot(s);
    let binds = upsert_binding(&s.binds, key, action, index);
    s.text = render(&s.text, &binds);
    s.binds = binds;
    s.history.record(&before, &snapshot(s), &format!("{key} → {action}"));
    s.dirty = true;
    s.edit.close();
    s.status = format!("{key} → {action}");
}

/// Remove a binding, re-render, and record it for undo.
pub fn remove_binding(s: &mut State, key: &str) {
    let Some(index) = s.binds.iter().position(|binding| binding.key.eq_ignore_ascii_case(key)) else {
        s.status = format!("{key} is not bound.");
        return;
    };
    let before = snapshot(s);
    let mut binds = s.binds.clone();
    binds.remove(index);
    s.text = render(&s.text, &binds);
    s.binds = binds;
    s.history.record(&before, &snapshot(s), &format!("removed {key}"));
    s.dirty = true;
    s.status = format!("Removed {key}.");
}

/// Undo the last edit.
pub fn undo(s: &mut State) {
    let current = snapshot(s);
    match s.history.undo(&current) {
        Some((previous, description)) => {
            restore(s, previous);
            s.status = format!("Undid {description}.");
        }
        None => s.status = "Nothing to undo.".into(),
    }
}

/// Redo the last undone edit.
pub fn redo(s: &mut State) {
    let current = snapshot(s);
    match s.history.redo(&current) {
        Some((next, description)) => {
            restore(s, next);
            s.status = format!("Redid {description}.");
        }
        None => s.status = "Nothing to redo.".into(),
    }
}

fn restore(s: &mut State, snapshot: Snapshot) {
    s.text = render(&s.text, &snapshot.binds);
    s.binds = snapshot.binds;
    s.dirty = true;
}

/// Open the picker for `key`.
pub fn open_picker(s: &mut State, key: &str) {
    s.edit = Edit {
        mode: Some(Mode::Picker),
        key: key.to_string(),
        ..Edit::default()
    };
}

/// The picker's rows: the matching catalog entries, then Custom… and Macro builder…
fn picker_rows(query: &str) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = filter_actions(query, CATALOG)
        .into_iter()
        .map(|entry| (entry.action.to_string(), entry.description.to_string()))
        .collect();
    rows.push((CUSTOM.to_string(), "Custom… type any keyd action".to_string()));
    rows.push(("__macro__".to_string(), "Macro builder…".to_string()));
    rows
}

fn choose(s: &mut State) {
    let rows = picker_rows(&s.edit.query);
    let Some((action, _)) = rows.get(s.edit.picker) else {
        return;
    };
    match action.as_str() {
        CUSTOM => {
            s.edit.mode = Some(Mode::Custom);
            s.edit.custom.clear();
        }
        "__macro__" => {
            s.edit.mode = Some(Mode::Macro);
            s.edit.draft = MacroDraft::default();
        }
        action => {
            let action = action.to_string();
            apply_binding(s, &s.edit.key.clone(), &action);
        }
    }
}

fn commit_custom(s: &mut State) {
    let action = s.edit.custom.trim().to_string();
    if action.is_empty() || action.contains('\n') || action.contains('\r') {
        s.edit.error = "An action must be a non-empty single line.".into();
        return;
    }
    let key = s.edit.key.clone();
    apply_binding(s, &key, &action);
}

fn commit_macro(s: &mut State) {
    let draft = s.edit.draft.clone();
    if draft.steps.is_empty() {
        s.edit.error = "A macro needs at least one step.".into();
        return;
    }
    match build_macro(&draft.steps) {
        Ok(action) => {
            let key = s.edit.key.clone();
            apply_binding(s, &key, &action);
        }
        Err(error) => s.edit.error = error.to_string(),
    }
}

fn commit_step(s: &mut State) {
    let Some(kind) = s.edit.draft.typing else {
        return;
    };
    let buffer = s.edit.draft.buffer.trim().to_string();
    let step = match kind {
        Kind::Text => MacroStep::Text(buffer),
        Kind::Key => MacroStep::Key(buffer),
        Kind::Delay => match buffer.parse::<u32>() {
            Ok(ms) => MacroStep::Delay(ms),
            Err(_) => {
                s.edit.error = "A delay is a whole number of milliseconds.".into();
                return;
            }
        },
    };
    // Validate the step by building the macro it belongs to.
    let mut steps = s.edit.draft.steps.clone();
    steps.push(step.clone());
    if let Err(error) = build_macro(&steps) {
        s.edit.error = error.to_string();
        return;
    }
    s.edit.error.clear();
    s.edit.draft.steps = steps;
    s.edit.draft.typing = None;
    s.edit.draft.buffer.clear();
    s.edit.draft.selected = s.edit.draft.steps.len().saturating_sub(1);
}

/// Keys while an overlay is open. Returns true when the key was consumed.
pub fn on_key(s: &mut State, ev: &KeyEvent) -> bool {
    let Some(mode) = s.edit.mode else {
        return false;
    };
    match mode {
        Mode::Picker => match &ev.key {
            Key::Text(typed) => {
                s.edit.query.push_str(typed);
                s.edit.picker = 0;
                true
            }
            Key::Backspace => {
                s.edit.query.pop();
                s.edit.picker = 0;
                true
            }
            Key::Up => {
                s.edit.picker = s.edit.picker.saturating_sub(1);
                true
            }
            Key::Down => {
                let count = picker_rows(&s.edit.query).len();
                s.edit.picker = (s.edit.picker + 1).min(count.saturating_sub(1));
                true
            }
            Key::Enter => {
                choose(s);
                true
            }
            Key::Escape => {
                s.edit.close();
                true
            }
            _ => false,
        },
        Mode::Custom => match &ev.key {
            Key::Text(typed) => {
                s.edit.custom.push_str(typed);
                true
            }
            Key::Backspace => {
                s.edit.custom.pop();
                true
            }
            Key::Enter => {
                commit_custom(s);
                true
            }
            Key::Escape => {
                s.edit.close();
                true
            }
            _ => false,
        },
        Mode::Macro => macro_keys(s, ev),
        Mode::Normal => false,
    }
}

fn macro_keys(s: &mut State, ev: &KeyEvent) -> bool {
    if s.edit.draft.typing.is_some() {
        return match &ev.key {
            Key::Text(typed) => {
                s.edit.draft.buffer.push_str(typed);
                true
            }
            Key::Backspace => {
                s.edit.draft.buffer.pop();
                true
            }
            Key::Enter => {
                commit_step(s);
                true
            }
            Key::Escape => {
                s.edit.draft.typing = None;
                s.edit.draft.buffer.clear();
                true
            }
            _ => false,
        };
    }
    match &ev.key {
        Key::Text(typed) => match typed.as_str() {
            "t" => {
                s.edit.draft.typing = Some(Kind::Text);
                s.edit.draft.buffer.clear();
                true
            }
            "k" => {
                s.edit.draft.typing = Some(Kind::Key);
                s.edit.draft.buffer.clear();
                true
            }
            "d" => {
                s.edit.draft.typing = Some(Kind::Delay);
                s.edit.draft.buffer.clear();
                true
            }
            "x" => {
                let index = s.edit.draft.selected;
                if index < s.edit.draft.steps.len() {
                    s.edit.draft.steps.remove(index);
                    s.edit.draft.selected = s.edit.draft.selected.saturating_sub(1);
                }
                true
            }
            "J" => {
                let index = s.edit.draft.selected;
                if index + 1 < s.edit.draft.steps.len() {
                    s.edit.draft.steps.swap(index, index + 1);
                    s.edit.draft.selected = index + 1;
                }
                true
            }
            "K" => {
                let index = s.edit.draft.selected;
                if index > 0 {
                    s.edit.draft.steps.swap(index, index - 1);
                    s.edit.draft.selected = index - 1;
                }
                true
            }
            _ => false,
        },
        Key::Up => {
            s.edit.draft.selected = s.edit.draft.selected.saturating_sub(1);
            true
        }
        Key::Down => {
            let last = s.edit.draft.steps.len().saturating_sub(1);
            s.edit.draft.selected = (s.edit.draft.selected + 1).min(last);
            true
        }
        Key::Enter => {
            commit_macro(s);
            true
        }
        Key::Escape => {
            s.edit.close();
            true
        }
        _ => false,
    }
}

fn step_label(step: &MacroStep) -> String {
    match step {
        MacroStep::Text(value) => format!("text  {value:?}"),
        MacroStep::Key(value) => format!("key   {value}"),
        MacroStep::Delay(ms) => format!("delay {ms} ms"),
    }
}

/// The overlay panel, or nothing when no overlay is open.
pub fn panel(s: &State, t: &tok::Theme) -> El {
    let Some(mode) = s.edit.mode else {
        return el().size(0.0, 0.0);
    };
    let mut body = col().key("overlay").w_full().gap(6.0).padding(16.0).radius(radius::MD).fill(t.card);
    let title = match mode {
        Mode::Picker => format!("Action for {}", s.edit.key),
        Mode::Custom => format!("Custom action for {}", s.edit.key),
        Mode::Macro => format!("Macro for {}", s.edit.key),
        Mode::Normal => String::new(),
    };
    body = body.child(text(title).font(type_::TITLE).color(t.text));

    match mode {
        Mode::Picker => {
            if !s.edit.query.is_empty() {
                body = body.child(text(format!("/{}", s.edit.query)).font(type_::LABEL).color(t.text_secondary));
            }
            let rows = picker_rows(&s.edit.query);
            let start = s.edit.picker.saturating_sub(6);
            for (offset, (action, description)) in rows.iter().enumerate().skip(start).take(9) {
                let selected = offset == s.edit.picker;
                let fill = if selected { tok::accent(0, true) } else { t.bg };
                let ink = if selected { t.bg } else { t.text };
                let detail = if *action == CUSTOM || action == "__macro__" {
                    description.clone()
                } else {
                    action_description(action).unwrap_or(description).to_string()
                };
                body = body.child(
                    row()
                        .key(format!("pick-{offset}"))
                        .h(26.0)
                        .px(8.0)
                        .gap(10.0)
                        .center_y()
                        .radius(radius::SM)
                        .fill(fill)
                        .child(text(action.clone()).font(type_::LABEL).w(120.0).color(ink))
                        .child(text(detail).font(type_::SECTION).grow(1.0).color(ink)),
                );
            }
        }
        Mode::Custom => {
            body = body.child(
                row()
                    .key("custom-field")
                    .h(30.0)
                    .px(10.0)
                    .center_y()
                    .radius(radius::SM)
                    .fill(t.bg)
                    .child(text(s.edit.custom.clone()).font(type_::BODY).color(t.text)),
            );
            body = body.child(
                text("type a keyd action · Enter accepts · Esc cancels")
                    .font(type_::SECTION)
                    .color(t.text_secondary),
            );
        }
        Mode::Macro => {
            let draft = &s.edit.draft;
            if let Some(kind) = draft.typing {
                let kind = match kind {
                    Kind::Text => "text",
                    Kind::Key => "key or chord",
                    Kind::Delay => "delay in ms",
                };
                body = body.child(
                    row()
                        .key("macro-field")
                        .h(30.0)
                        .px(10.0)
                        .center_y()
                        .radius(radius::SM)
                        .fill(t.bg)
                        .child(text(format!("{kind}: {}", draft.buffer)).font(type_::BODY).color(t.text)),
                );
            }
            if draft.steps.is_empty() {
                body = body.child(
                    text("t text · k key · d delay · Enter builds the macro")
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                );
            }
            for (index, step) in draft.steps.iter().enumerate() {
                let selected = index == draft.selected;
                let fill = if selected { t.control_off } else { t.bg };
                body = body.child(
                    row()
                        .key(format!("step-{index}"))
                        .h(24.0)
                        .px(8.0)
                        .center_y()
                        .radius(radius::SM)
                        .fill(fill)
                        .child(text(step_label(step)).font(type_::SECTION).color(t.text)),
                );
            }
            if !draft.steps.is_empty() {
                let preview = build_macro(&draft.steps).unwrap_or_else(|error| error.to_string());
                body = body.child(text(preview).font(type_::SECTION).color(t.text_secondary));
                body = body.child(
                    text("x removes · J/K reorder · Enter accepts · Esc cancels")
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                );
            }
        }
        Mode::Normal => {}
    }

    if !s.edit.error.is_empty() {
        body = body.child(text(s.edit.error.clone()).font(type_::SECTION).color(t.text));
    }
    body
}

/// Whether the macro draft parses (used by tests and the preview).
pub fn draft_action(draft: &MacroDraft) -> Option<String> {
    build_macro(&draft.steps).ok()
}

/// Parse a macro back into steps, for editing an existing binding.
pub fn steps_of(action: &str) -> Option<Vec<MacroStep>> {
    parse_macro(action)
}
