//! Binding rules, presets, keyboard highlights, warnings and editor history.
//!
//! Ported from `keymapper/rules.py`. The Python's `editor_state`/`restore_editor_state` helpers
//! are list-mutation shims for Textual and have no Rust equivalent: the UI owns its state.

use std::collections::BTreeMap;

use super::config::Binding;
use super::data::{KEYBOARD_ROWS, Pack, action_description};

/// keyd's modifier order for chords.
const MONITOR_MODIFIER_ORDER: [&str; 5] = ["C", "M", "A", "S", "G"];

fn modifier_alias(name: &str) -> Option<&'static str> {
    Some(match name.to_lowercase().as_str() {
        "control" | "ctrl" | "leftcontrol" | "rightcontrol" | "leftctrl" | "rightctrl" => "C",
        "meta" | "leftmeta" | "rightmeta" => "M",
        "alt" | "leftalt" => "A",
        "shift" | "leftshift" | "rightshift" => "S",
        "altgr" | "rightalt" => "G",
        _ => return None,
    })
}

/// Format a monitor chord using keyd's C, M, A, S, G modifier order.
pub fn compose_monitor_chord(held_modifier_key_names: &[String], key: &str) -> String {
    let held: Vec<&str> = held_modifier_key_names
        .iter()
        .filter_map(|name| modifier_alias(name))
        .collect();
    let modifiers: Vec<&str> = MONITOR_MODIFIER_ORDER
        .into_iter()
        .filter(|modifier| held.contains(modifier))
        .collect();
    if modifiers.is_empty() {
        key.to_string()
    } else {
        format!("{}-{key}", modifiers.join("-"))
    }
}

/// Compare monitor chords without relying on the order of modifier prefixes.
pub fn monitor_chords_equal(left: &str, right: &str) -> bool {
    fn normalized(chord: &str) -> (Vec<String>, String) {
        let mut parts: Vec<&str> = chord.split('-').collect();
        let mut modifiers: Vec<String> = Vec::new();
        while parts.len() > 1 && MONITOR_MODIFIER_ORDER.contains(&parts[0].to_uppercase().as_str()) {
            modifiers.push(parts.remove(0).to_uppercase());
        }
        if modifiers.is_empty() {
            return (Vec::new(), chord.to_lowercase());
        }
        let ordered: Vec<String> = MONITOR_MODIFIER_ORDER
            .into_iter()
            .filter(|modifier| modifiers.iter().any(|m| m == modifier))
            .map(str::to_string)
            .collect();
        let key = parts.last().map(|part| part.to_lowercase()).unwrap_or_default();
        (ordered, key)
    }
    normalized(left) == normalized(right)
}

/// Bounded undo/redo snapshots of complete editor states.
#[derive(Debug)]
pub struct History<T> {
    limit: usize,
    undo: Vec<(T, String)>,
    redo: Vec<(T, String)>,
}

impl<T: Clone + PartialEq> Default for History<T> {
    fn default() -> Self {
        Self::new(50)
    }
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(limit: usize) -> Self {
        Self { limit: limit.max(1), undo: Vec::new(), redo: Vec::new() }
    }

    /// Record a change; returns false when the snapshots are equal.
    pub fn record(&mut self, before: &T, after: &T, description: &str) -> bool {
        if before == after {
            return false;
        }
        self.undo.push((before.clone(), description.to_string()));
        self.trim();
        self.redo.clear();
        true
    }

    /// Step back, storing `current` for redo.
    pub fn undo(&mut self, current: &T) -> Option<(T, String)> {
        let (snapshot, description) = self.undo.pop()?;
        self.redo.push((current.clone(), description.clone()));
        self.trim_redo();
        Some((snapshot, description))
    }

    /// Step forward again, storing `current` for undo.
    pub fn redo(&mut self, current: &T) -> Option<(T, String)> {
        let (snapshot, description) = self.redo.pop()?;
        self.undo.push((current.clone(), description.clone()));
        self.trim();
        Some((snapshot, description))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn trim(&mut self) {
        let excess = self.undo.len().saturating_sub(self.limit);
        if excess > 0 {
            self.undo.drain(..excess);
        }
    }

    fn trim_redo(&mut self) {
        let excess = self.redo.len().saturating_sub(self.limit);
        if excess > 0 {
            self.redo.drain(..excess);
        }
    }
}

/// Catalog entries matching a case-insensitive category/action/description query.
pub fn filter_actions<'a>(query: &str, catalog: &'a [super::data::Action]) -> Vec<&'a super::data::Action> {
    let q = query.trim().to_lowercase();
    catalog
        .iter()
        .filter(|entry| {
            q.is_empty()
                || format!("{} {} {}", entry.category, entry.action, entry.description)
                    .to_lowercase()
                    .contains(&q)
        })
        .collect()
}

/// Binding indexes matching key, action, or the catalog description of the action.
pub fn filter_binding_indices(binds: &[Binding], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..binds.len()).collect();
    }
    binds
        .iter()
        .enumerate()
        .filter(|(_, binding)| {
            binding.key.to_lowercase().contains(&q)
                || binding.action.to_lowercase().contains(&q)
                || action_description(&binding.action)
                    .is_some_and(|description| description.to_lowercase().contains(&q))
        })
        .map(|(index, _)| index)
        .collect()
}

/// What merging a pack did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackMerge {
    pub merged: Vec<Binding>,
    pub added: Vec<Binding>,
    /// (key, old action, new action)
    pub replaced: Vec<(String, String, String)>,
}

/// Merge pack bindings in order, without mutating the input.
pub fn merge_pack(binds: &[Binding], pack: &Pack) -> PackMerge {
    let mut merged = binds.to_vec();
    let mut result = PackMerge { merged: Vec::new(), added: Vec::new(), replaced: Vec::new() };
    for (key, action) in pack.binds {
        match merged.iter().position(|binding| binding.key == *key) {
            None => {
                let binding = Binding::new(*key, *action);
                merged.push(binding.clone());
                result.added.push(binding);
            }
            Some(index) => {
                let old = merged[index].action.clone();
                merged[index] = Binding::new(*key, *action);
                if old != *action {
                    result.replaced.push(((*key).to_string(), old, (*action).to_string()));
                }
            }
        }
    }
    result.merged = merged;
    result
}

/// Add a binding, or replace the row at `index`, dropping other mappings of the same key.
pub fn upsert_binding(binds: &[Binding], key: &str, action: &str, index: Option<usize>) -> Vec<Binding> {
    let Some(index) = index else {
        let mut out: Vec<Binding> = binds
            .iter()
            .filter(|binding| binding.key != key)
            .cloned()
            .collect();
        out.push(Binding::new(key, action));
        return out;
    };
    if index >= binds.len() {
        return binds.to_vec();
    }
    let before = binds[..index].iter().filter(|binding| binding.key != key).count();
    let mut out: Vec<Binding> = binds
        .iter()
        .enumerate()
        .filter(|(i, binding)| *i != index && binding.key != key)
        .map(|(_, binding)| binding.clone())
        .collect();
    out.insert(before.min(out.len()), Binding::new(key, action));
    out
}

/// How a keycap is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Selected,
    Bound,
    Plain,
}

/// Map-key styles for every keycap: the selection beats other bound keys.
pub fn keyboard_highlights(binds: &[Binding], selected_key: Option<&str>) -> BTreeMap<String, Highlight> {
    let bound: Vec<String> = binds.iter().map(|binding| binding.key.to_lowercase()).collect();
    let selected = selected_key.map(str::to_lowercase);
    let mut out = BTreeMap::new();
    for row in KEYBOARD_ROWS {
        for key in *row {
            let lower = key.to_lowercase();
            let style = if Some(&lower) == selected.as_ref() {
                Highlight::Selected
            } else if bound.contains(&lower) {
                Highlight::Bound
            } else {
                Highlight::Plain
            };
            out.insert((*key).to_string(), style);
        }
    }
    out
}

/// Bindings whose key is not on the drawn map (the F-row and arrows are drawn separately).
pub fn keyboard_unshown_bindings(binds: &[Binding]) -> Vec<String> {
    let shown: Vec<String> = KEYBOARD_ROWS
        .iter()
        .flat_map(|row| row.iter().map(|key| key.to_lowercase()))
        .collect();
    binds
        .iter()
        .filter(|binding| !shown.contains(&binding.key.to_lowercase()))
        .map(|binding| binding.key.clone())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Warn,
    Info,
}

impl Level {
    pub fn upper(&self) -> &'static str {
        match self {
            Level::Warn => "WARN",
            Level::Info => "INFO",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub key: String,
    pub level: Level,
    pub message: String,
}

/// Risks worth showing before an apply.
pub fn binding_warnings(binds: &[Binding], tap: Option<&str>) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let mut push = |key: &str, level: Level, message: String| {
        warnings.push(Warning { key: key.to_string(), level, message });
    };
    for binding in binds {
        let key_lower = binding.key.to_lowercase();
        let action_lower = binding.action.to_lowercase();
        if matches!(action_lower.as_str(), "a-f4" | "c-q" | "c-w") {
            push(
                &binding.key,
                Level::Warn,
                format!("{} may close a window or application.", binding.action),
            );
        }
        if action_lower.starts_with("command(") {
            push(
                &binding.key,
                Level::Warn,
                "command(...) runs as root; review its contents carefully.".to_string(),
            );
        }
        if key_lower == action_lower {
            push(&binding.key, Level::Warn, format!("No-op: {} sends itself.", binding.key));
        }
        if key_lower == "capslock" {
            let tap_text = tap
                .filter(|value| !value.is_empty())
                .map(|value| format!(" (tap sends {value})"))
                .unwrap_or_default();
            push(
                &binding.key,
                Level::Warn,
                format!("Caps Lock is the layer key{tap_text}; binding it inside [nav] is hard to reach."),
            );
        }
        if action_lower.starts_with("m-") || action_lower.starts_with("super-") {
            push(
                &binding.key,
                Level::Info,
                format!("{} may be intercepted by the desktop.", binding.action),
            );
        }
    }
    warnings
}

/// The warnings attached to one key.
pub fn warnings_for_key<'a>(warnings: &'a [Warning], key: &str) -> Vec<(&'a Level, &'a str)> {
    warnings
        .iter()
        .filter(|warning| warning.key.to_lowercase() == key.to_lowercase())
        .map(|warning| (&warning.level, warning.message.as_str()))
        .collect()
}

/// The warning block shown in a preview screen.
pub fn warning_preview_lines(binds: &[Binding], tap: Option<&str>) -> Vec<String> {
    let warnings = binding_warnings(binds, tap);
    if warnings.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["Warnings:".to_string()];
    lines.extend(
        warnings
            .iter()
            .map(|warning| format!("  {} {}: {}", warning.level.upper(), warning.key, warning.message)),
    );
    lines
}

/// The lines a pack preview shows.
pub fn pack_summary(pack: &Pack, merge: &PackMerge, tap: Option<&str>) -> Vec<String> {
    let mut lines = vec![format!("Pack: {}", pack.name)];
    if !merge.added.is_empty() {
        lines.push("Added:".to_string());
        lines.extend(merge.added.iter().map(|binding| format!("  {} → {}", binding.key, binding.action)));
        let warnings = binding_warnings(&merge.added, tap);
        if !warnings.is_empty() {
            lines.push("Warnings for added bindings:".to_string());
            lines.extend(
                warnings
                    .iter()
                    .map(|warning| format!("  {} {}: {}", warning.level.upper(), warning.key, warning.message)),
            );
        }
    }
    if !merge.replaced.is_empty() {
        lines.push("Replaced:".to_string());
        lines.extend(
            merge
                .replaced
                .iter()
                .map(|(key, old, new)| format!("  {key}: {old} → {new}")),
        );
    }
    if merge.added.is_empty() && merge.replaced.is_empty() {
        lines.push("No bindings changed; all pack actions were already present.".to_string());
    }
    lines
}
