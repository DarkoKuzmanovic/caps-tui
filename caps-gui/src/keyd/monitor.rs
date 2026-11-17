//! Chord composition and annotations for `keyd monitor` events.
//!
//! Ported from `keymapper/monitor.py`: a modifier held and released alone shows as one
//! `down/up` row, and a key pressed while modifiers are held shows the chord keyd would send,
//! annotated with the binding that produces it.

use std::collections::HashMap;

use super::config::Binding;
use super::rules::{compose_monitor_chord, monitor_chords_equal};
use super::system::parse_monitor_event;

/// Keys that are modifiers, by keyd name.
const MONITOR_MODIFIER_KEYS: [&str; 13] = [
    "control",
    "leftcontrol",
    "rightcontrol",
    "shift",
    "leftshift",
    "rightshift",
    "alt",
    "leftalt",
    "meta",
    "leftmeta",
    "rightmeta",
    "altgr",
    "rightalt",
];

/// One row of the live panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorEvent {
    pub device: String,
    /// The key, or the composed chord.
    pub key: String,
    /// `down`, `up`, or `down/up` for a modifier tapped on its own.
    pub state: String,
    /// Bindings whose action matches what keyd sent.
    pub annotation: Vec<String>,
    /// Whether `key` is a composed chord rather than the raw key.
    pub chord: bool,
}

/// The panel's log, with the modifier bookkeeping that keeps chords readable.
#[derive(Debug, Default)]
pub struct MonitorLog {
    pub events: Vec<MonitorEvent>,
    /// device identity → held modifier key names.
    held: HashMap<String, Vec<String>>,
    /// (device identity, key) → whether a held modifier was consumed by a chord.
    pending: HashMap<(String, String), bool>,
    /// (device identity, key) → index into `events` for the press still open.
    active: HashMap<(String, String), usize>,
}

impl MonitorLog {
    /// Record one monitor line; true when the panel's content changed.
    pub fn record(&mut self, line: &str, binds: &[Binding]) -> bool {
        let Some((device, key, state)) = parse_monitor_event(line) else {
            return false;
        };
        let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
        let raw = fields
            .get(1)
            .map(|field| field.trim().to_string())
            .unwrap_or_else(|| device.clone());
        let identity = format!("{device}\t{raw}");
        let lower = key.to_lowercase();
        let id = (identity.clone(), lower.clone());
        let is_modifier = MONITOR_MODIFIER_KEYS.contains(&lower.as_str());

        if is_modifier {
            if state == "down" {
                self.held.entry(identity).or_default().push(lower);
                self.pending.insert(id, false);
                return false;
            }
            if let Some(held) = self.held.get_mut(&identity) {
                held.retain(|modifier| modifier != &lower);
            }
            let consumed = self.pending.remove(&id).unwrap_or(false);
            if consumed {
                return false;
            }
            self.push(MonitorEvent {
                device,
                key,
                state: "down/up".into(),
                annotation: Vec::new(),
                chord: false,
            });
            return true;
        }

        let held = self.held.get(&identity).cloned().unwrap_or_default();
        let chord_key = compose_monitor_chord(&held, &key);
        let chord = !chord_key.eq_ignore_ascii_case(&key);
        if chord {
            for modifier in &held {
                if let Some(consumed) = self.pending.get_mut(&(identity.clone(), modifier.clone())) {
                    *consumed = true;
                }
            }
        }

        if state == "up"
            && let Some(index) = self.active.remove(&id)
        {
            self.events[index].state = "down/up".into();
            return true;
        }

        let annotation = annotations(binds, &chord_key);
        self.push(MonitorEvent { device, key: chord_key, state: state.clone(), annotation, chord });
        if state == "down" {
            let index = self.events.len() - 1;
            self.active.insert(id, index);
        }
        true
    }

    fn push(&mut self, event: MonitorEvent) {
        self.events.push(event);
        if self.events.len() > 200 {
            let excess = self.events.len() - 200;
            self.events.drain(..excess);
            for index in self.active.values_mut() {
                *index = index.saturating_sub(excess);
            }
        }
    }

    /// The rows to draw, oldest first.
    pub fn rows(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|event| {
                let arrow = if event.chord { "⇢" } else { "·" };
                let note = if event.annotation.is_empty() {
                    String::new()
                } else {
                    format!("   {}", event.annotation.join(", "))
                };
                format!("{arrow} {} {}{note}", event.key, event.state)
            })
            .collect()
    }
}

/// Describe the bindings whose action matches what keyd sent.
fn annotations(binds: &[Binding], output_key: &str) -> Vec<String> {
    const ARROWS: [(&str, &str); 4] = [("left", "←"), ("right", "→"), ("up", "↑"), ("down", "↓")];
    binds
        .iter()
        .filter(|binding| monitor_chords_equal(&binding.action, output_key))
        .map(|binding| {
            let arrow = ARROWS
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&binding.key))
                .or_else(|| ARROWS.iter().find(|(name, _)| name.eq_ignore_ascii_case(&binding.action)));
            match arrow {
                Some((_, symbol)) => format!("{symbol} Caps + {}", binding.key),
                None => format!("Caps + {} → {}", binding.key, binding.action),
            }
        })
        .collect()
}
