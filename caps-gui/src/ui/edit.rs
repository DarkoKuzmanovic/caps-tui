//! Editing: the action picker, the custom-action field and the macro builder.
//!
//! One overlay at a time, drawn in place of the table. Every commit goes through
//! [`apply_binding`], which renders the config, records undo history and marks the state dirty.

use rune::tok::{self, radius, type_};
use rune::{El, Key, KeyEvent, col, el, row, text};

use super::State;
use crate::keyd::config::{Binding, parse, render};
use crate::keyd::data::{CATALOG, CUSTOM, PACKS, action_description};
use crate::keyd::macros::{MacroStep, build_macro, parse_macro};
use crate::keyd::rules::{filter_actions, upsert_binding};
use crate::keyd::system::CheckOutcome;

/// What the overlay is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Picker,
    Custom,
    Macro,
    Tap,
    Packs,
    Backups,
    Monitor,
    Apply,
    Capture,
}

/// Where a typed or chosen action lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    #[default]
    Binding,
    Tap,
}

/// The tap actions the editor offers; the last row opens the custom field.
pub const TAP_CHOICES: [(&str, &str); 4] = [
    ("capslock", "Caps Lock"),
    ("esc", "Esc"),
    ("backspace", "Backspace"),
    (CUSTOM, "Custom…"),
];

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
    pub target: Target,
    pub query: String,
    pub picker: usize,
    pub custom: String,
    pub draft: MacroDraft,
    pub error: String,
    pub tap_choice: usize,
    pub pack: usize,
    pub backup: usize,
    pub backups: Vec<crate::keyd::system::BackupEntry>,
    /// The diff and warnings shown before an apply.
    pub apply_lines: Vec<String>,
    /// How far the review is scrolled.
    pub apply_offset: usize,
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

/// Replace or add a binding, re-render, validate with keyd, and record it for undo. A rejection
/// keeps the overlay open with the reason.
pub fn apply_binding(s: &mut State, key: &str, action: &str) {
    let index = s.binds.iter().position(|binding| binding.key.eq_ignore_ascii_case(key));
    let before = snapshot(s);
    let binds = upsert_binding(&s.binds, key, action, index);
    let candidate = render(&s.text, &binds);
    if let Err(reason) = validate(s, &candidate) {
        s.edit.error = reason;
        return;
    }
    s.text = candidate;
    s.binds = binds;
    s.history.record(&before, &snapshot(s), &format!("{key} → {action}"));
    s.dirty = true;
    s.edit.close();
    s.status = format!("{key} → {action}");
}

/// Ask keyd about a candidate config; `Err` carries the message to show.
fn validate(s: &State, candidate: &str) -> Result<(), String> {
    match (s.checker)(candidate) {
        Ok(CheckOutcome::Accepted) | Ok(CheckOutcome::KeydMissing) => Ok(()),
        Ok(CheckOutcome::Rejected(message)) => Err(format!("keyd rejects this: {message}")),
        Err(error) => Err(format!("could not run keyd check: {error}")),
    }
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
    let mut text = render(&s.text, &snapshot.binds);
    // The tap lives in [main], so it needs its own write; an uneditable [main] keeps the text.
    if let Some(tap) = &snapshot.tap
        && let Ok(updated) = crate::keyd::config::write_tap(&text, tap)
    {
        text = updated;
    }
    s.text = text;
    s.binds = snapshot.binds;
    s.tap = crate::keyd::config::read_tap(&s.text);
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
    match s.edit.target {
        Target::Binding => {
            let key = s.edit.key.clone();
            apply_binding(s, &key, &action);
        }
        Target::Tap => commit_tap(s, &action),
    }
}

/// Set the `[main]` tap action.
fn commit_tap(s: &mut State, action: &str) {
    let candidate = match crate::keyd::config::write_tap(&s.text, action) {
        Ok(candidate) => candidate,
        Err(error) => {
            s.edit.error = error.to_string();
            return;
        }
    };
    if let Err(reason) = validate(s, &candidate) {
        s.edit.error = reason;
        return;
    }
    let before = snapshot(s);
    s.text = candidate;
    s.tap = crate::keyd::config::read_tap(&s.text);
    s.history.record(&before, &snapshot(s), &format!("tap {action}"));
    s.dirty = true;
    s.edit.close();
    s.status = format!("Tap: {}", crate::keyd::config::tap_label(&s.tap));
}

/// Merge a preset pack into `[nav]`.
fn commit_pack(s: &mut State, pack: &crate::keyd::data::Pack) {
    let merge = crate::keyd::rules::merge_pack(&s.binds, pack);
    let candidate = render(&s.text, &merge.merged);
    if let Err(reason) = validate(s, &candidate) {
        s.edit.error = reason;
        return;
    }
    let before = snapshot(s);
    let summary = crate::keyd::rules::pack_summary(pack, &merge, s.tap.action.as_deref()).join(" · ");
    s.text = candidate;
    s.binds = merge.merged;
    s.history.record(&before, &snapshot(s), &format!("pack {}", pack.name));
    s.dirty = true;
    s.edit.close();
    s.status = summary;
}

/// Load `[nav]` bindings from a timestamped backup, leaving every other section alone.
fn commit_backup(s: &mut State, path: &std::path::Path) {
    let backup = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            s.edit.error = format!("cannot read {}: {error}", path.display());
            return;
        }
    };
    let (lines, outside) = crate::keyd::config::backup_preview(&s.text, &backup);
    let binds = parse(&backup);
    let candidate = render(&s.text, &binds);
    if let Err(reason) = validate(s, &candidate) {
        s.edit.error = reason;
        return;
    }
    let before = snapshot(s);
    s.text = candidate;
    s.binds = binds;
    s.history.record(&before, &snapshot(s), "load backup");
    s.dirty = true;
    s.edit.close();
    s.status = lines
        .first()
        .cloned()
        .unwrap_or_else(|| "Loaded [nav] from the backup.".to_string());
    if outside {
        s.status = format!("{} (other sections differ; [nav] only)", s.status);
    }
}

/// Open the review: the live/editor diff plus the risk warnings.
pub fn open_apply(s: &mut State) {
    if let Some(error) = &s.load_error {
        s.status = format!("not applying: {error}");
        return;
    }
    // The fallback is the *saved* file, not the buffer: with an unreadable /etc the diff would
    // otherwise be empty and the apply refused for the wrong reason.
    let saved = std::fs::read_to_string(&s.paths.conf).unwrap_or_default();
    let live = crate::keyd::system::read_live_config_in(&s.paths.etc, &saved).0;
    let diff = crate::keyd::config::diff_lines(&live, &s.text, "live default.conf", "editor");
    if diff.is_empty() {
        s.status = "No differences; nothing to apply.".to_string();
        return;
    }
    let mut lines = crate::keyd::rules::warning_preview_lines(&s.binds, s.tap.action.as_deref());
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.extend(diff);
    s.edit = Edit {
        mode: Some(Mode::Apply),
        apply_lines: lines,
        ..Edit::default()
    };
}

/// Save the buffer, then send it to the apply helper and report what it said.
fn apply_now(s: &mut State) {
    if let Some(error) = &s.load_error {
        s.edit.error = format!("not applying: {error}");
        return;
    }
    // keyd gets the last word before anything touches /etc.
    if let Err(reason) = validate(s, &s.text) {
        s.edit.error = reason;
        return;
    }
    // The helper installs what it is handed, so the file must match it: otherwise the next
    // launch loads the old bindings and the next apply pushes them back.
    if let Err(error) = std::fs::write(&s.paths.conf, &s.text) {
        s.edit.error = format!("cannot write {}: {error}", s.paths.conf.display());
        return;
    }
    s.dirty = false;
    let how = s.privilege.clone();
    match crate::keyd::privilege::apply_text(&how, &s.text) {
        Ok(outcome) if outcome.status.success() => {
            s.edit.close();
            s.status = "Saved and applied.".to_string();
        }
        Ok(outcome) => {
            s.edit.error = if outcome.message.is_empty() {
                format!("apply failed: {}", outcome.status)
            } else {
                outcome.message
            };
        }
        Err(error) => {
            s.edit.error = format!("could not run the apply helper: {error}");
        }
    }
}

/// Open the tap-action chooser.
pub fn open_tap(s: &mut State) {
    s.edit = Edit {
        mode: Some(Mode::Tap),
        target: Target::Tap,
        ..Edit::default()
    };
}

/// Open the preset pack list.
pub fn open_packs(s: &mut State) {
    s.edit = Edit { mode: Some(Mode::Packs), ..Edit::default() };
}

/// Open the backup list for the live keyd directory.
pub fn open_backups(s: &mut State) {
    let backups = crate::keyd::system::backup_entries(Some(&s.paths.etc));
    s.edit = Edit {
        mode: Some(Mode::Backups),
        backups,
        ..Edit::default()
    };
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
        Mode::Tap => match &ev.key {
            Key::Up => {
                s.edit.tap_choice = s.edit.tap_choice.saturating_sub(1);
                true
            }
            Key::Down => {
                s.edit.tap_choice = (s.edit.tap_choice + 1).min(TAP_CHOICES.len() - 1);
                true
            }
            Key::Enter => {
                let (action, _) = TAP_CHOICES[s.edit.tap_choice.min(TAP_CHOICES.len() - 1)];
                if action == CUSTOM {
                    s.edit.mode = Some(Mode::Custom);
                    s.edit.custom.clear();
                } else {
                    commit_tap(s, action);
                }
                true
            }
            Key::Escape => {
                s.edit.close();
                true
            }
            _ => false,
        },
        Mode::Packs => match &ev.key {
            Key::Up => {
                s.edit.pack = s.edit.pack.saturating_sub(1);
                true
            }
            Key::Down => {
                s.edit.pack = (s.edit.pack + 1).min(PACKS.len() - 1);
                true
            }
            Key::Enter => {
                let pack = &PACKS[s.edit.pack.min(PACKS.len() - 1)];
                commit_pack(s, pack);
                true
            }
            Key::Escape => {
                s.edit.close();
                true
            }
            _ => false,
        },
        Mode::Backups => match &ev.key {
            Key::Up => {
                s.edit.backup = s.edit.backup.saturating_sub(1);
                true
            }
            Key::Down => {
                let last = s.edit.backups.len().saturating_sub(1);
                s.edit.backup = (s.edit.backup + 1).min(last);
                true
            }
            Key::Enter => {
                if let Some(entry) = s.edit.backups.get(s.edit.backup) {
                    let path = entry.path.clone();
                    commit_backup(s, &path);
                }
                true
            }
            Key::Escape => {
                s.edit.close();
                true
            }
            _ => false,
        },
        Mode::Monitor => match &ev.key {
            Key::Escape => {
                // Dropping the stream closes the helper's stdin, which is how a root process
                // is stopped.
                s.monitor_stream = None;
                s.monitor_status = "stopped".to_string();
                s.edit.close();
                true
            }
            Key::Text(typed) if typed == "c" => {
                s.monitor = crate::keyd::monitor::MonitorLog::default();
                true
            }
            _ => false,
        },
        Mode::Apply => match &ev.key {
            Key::Up => {
                s.edit.apply_offset = s.edit.apply_offset.saturating_sub(1);
                true
            }
            Key::Down => {
                let last = s.edit.apply_lines.len().saturating_sub(1);
                s.edit.apply_offset = (s.edit.apply_offset + 1).min(last);
                true
            }
            Key::Enter => {
                apply_now(s);
                true
            }
            Key::Escape => {
                s.edit.close();
                true
            }
            _ => false,
        },
        Mode::Capture => match &ev.key {
            Key::Escape => {
                s.monitor_stream = None;
                s.edit.close();
                s.status = "Capture cancelled.".to_string();
                true
            }
            _ => false,
        },
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
        Mode::Tap => "Short Caps press".to_string(),
        Mode::Packs => "Preset packs".to_string(),
        Mode::Backups => "Backups".to_string(),
        Mode::Monitor => "Live keyd output".to_string(),
        Mode::Apply => "Review and apply".to_string(),
        Mode::Capture => "Capture a key".to_string(),
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
        Mode::Tap => {
            for (index, (action, label)) in TAP_CHOICES.iter().enumerate() {
                let selected = index == s.edit.tap_choice;
                let fill = if selected { tok::accent(0, true) } else { t.bg };
                let ink = if selected { t.bg } else { t.text };
                body = body.child(
                    row()
                        .key(format!("tap-{index}"))
                        .h(26.0)
                        .px(8.0)
                        .gap(10.0)
                        .center_y()
                        .radius(radius::SM)
                        .fill(fill)
                        .child(text((*label).to_string()).font(type_::BODY).w(160.0).color(ink))
                        .child(text((*action).to_string()).font(type_::SECTION).grow(1.0).color(ink)),
                );
            }
            body = body.child(
                text("Enter chooses · Esc cancels").font(type_::SECTION).color(t.text_secondary),
            );
        }
        Mode::Packs => {
            for (index, pack) in crate::keyd::data::PACKS.iter().enumerate() {
                let selected = index == s.edit.pack;
                let fill = if selected { tok::accent(0, true) } else { t.bg };
                let ink = if selected { t.bg } else { t.text };
                body = body.child(
                    row()
                        .key(format!("pack-{index}"))
                        .h(26.0)
                        .px(8.0)
                        .gap(10.0)
                        .center_y()
                        .radius(radius::SM)
                        .fill(fill)
                        .child(text(pack.name.to_string()).font(type_::BODY).w(180.0).color(ink))
                        .child(text(pack.description.to_string()).font(type_::SECTION).grow(1.0).color(ink)),
                );
            }
            body = body.child(
                text("Enter merges the pack · Esc cancels").font(type_::SECTION).color(t.text_secondary),
            );
            if let Some(pack) = PACKS.get(s.edit.pack) {
                let merge = crate::keyd::rules::merge_pack(&s.binds, pack);
                for (index, line) in crate::keyd::rules::pack_summary(pack, &merge, s.tap.action.as_deref())
                    .iter()
                    .enumerate()
                {
                    body = body.child(
                        text(line.clone())
                            .key(format!("preview-{index}"))
                            .font(type_::SECTION)
                            .color(t.text_secondary),
                    );
                }
            }
        }
        Mode::Backups => {
            if s.edit.backups.is_empty() {
                body = body.child(
                    text(format!("No timestamped backups in {}", s.paths.etc.display()))
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                );
            }
            for (index, entry) in s.edit.backups.iter().enumerate() {
                let selected = index == s.edit.backup;
                let fill = if selected { tok::accent(0, true) } else { t.bg };
                let ink = if selected { t.bg } else { t.text };
                let stamp = entry.stamp.clone().unwrap_or_else(|| "?".into());
                let count = entry
                    .bindings
                    .map(|n| format!("{n} bindings"))
                    .unwrap_or_else(|| "unreadable".into());
                body = body.child(
                    row()
                        .key(format!("backup-{index}"))
                        .h(26.0)
                        .px(8.0)
                        .gap(10.0)
                        .center_y()
                        .radius(radius::SM)
                        .fill(fill)
                        .child(text(stamp).font(type_::BODY).w(180.0).color(ink))
                        .child(text(count).font(type_::SECTION).grow(1.0).color(ink)),
                );
            }
            body = body.child(
                text("Enter loads [nav] from the backup · Esc cancels")
                    .font(type_::SECTION)
                    .color(t.text_secondary),
            );
            if let Some(entry) = s.edit.backups.get(s.edit.backup) {
                match std::fs::read_to_string(&entry.path) {
                    Ok(backup) => {
                        let (lines, _) = crate::keyd::config::backup_preview(&s.text, &backup);
                        for (index, line) in lines.iter().take(20).enumerate() {
                            body = body.child(
                                text(line.clone())
                                    .key(format!("preview-{index}"))
                                    .font(type_::SECTION)
                                    .color(t.text_secondary),
                            );
                        }
                    }
                    Err(error) => {
                        body = body.child(
                            text(format!("cannot read {}: {error}", entry.path.display()))
                                .font(type_::SECTION)
                                .color(t.text_secondary),
                        );
                    }
                }
            }
        }
        Mode::Monitor => {
            let note = if s.dirty {
                format!("{} · unsaved edits", s.live_state.label())
            } else {
                s.live_state.label().to_string()
            };
            body = body.child(
                text(note)
                    .key("monitor-live")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            );
            body = body.child(
                text(s.monitor_status.clone())
                    .key("monitor-status")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            );
            let rows = s.monitor.rows();
            for (index, row_text) in rows.iter().rev().take(14).rev().enumerate() {
                body = body.child(
                    text(row_text.clone())
                        .key(format!("monitor-{index}"))
                        .font(type_::SECTION)
                        .color(t.text),
                );
            }
            body = body.child(
                text("c clears · Esc stops").font(type_::SECTION).color(t.text_secondary),
            );
        }
        Mode::Apply => {
            let total = s.edit.apply_lines.len();
            let offset = s.edit.apply_offset.min(total.saturating_sub(1));
            for (index, line) in s.edit.apply_lines.iter().skip(offset).take(12).enumerate() {
                body = body.child(
                    text(line.clone())
                        .key(format!("apply-{index}"))
                        .font(type_::SECTION)
                        .color(t.text),
                );
            }
            if offset + 12 < total {
                body = body.child(
                    text(format!("… {} more lines", total - offset - 12))
                        .key("apply-more")
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                );
            }
            body = body.child(
                text("↑↓ scroll · Enter applies with root · Esc cancels")
                    .font(type_::SECTION)
                    .color(t.text_secondary),
            );
        }
        Mode::Capture => {
            body = body.child(
                text("Press the key by itself — do not hold Caps.")
                    .key("capture-hint")
                    .font(type_::BODY)
                    .color(t.text),
            );
            body = body.child(
                text("keyd reports its own output, so the key arrives as keyd sees it · Esc cancels")
                    .font(type_::SECTION)
                    .color(t.text_secondary),
            );
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
