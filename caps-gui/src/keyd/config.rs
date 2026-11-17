//! Pure text parsing, rendering and comparison for keyd configurations.
//!
//! Ported from `keymapper/config.py`; the Python module is the reference until the Rust CLI
//! reaches parity.

use regex::Regex;
use similar::TextDiff;
use std::path::Path;
use std::sync::LazyLock;

use super::data::SECTION;

/// One `[nav]` binding: a keyd key name and the action it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub key: String,
    pub action: String,
}

impl Binding {
    pub fn new(key: impl Into<String>, action: impl Into<String>) -> Self {
        Self { key: key.into(), action: action.into() }
    }

    /// The line this binding becomes inside `[nav]`.
    pub fn line(&self) -> String {
        format!("{} = {}", self.key, self.action)
    }
}

/// What `[main]` says a short Caps press does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tap {
    /// The action, when a single editable `capslock = overload(nav, …)` entry was found.
    pub action: Option<String>,
    /// False when the setting exists but this editor refuses to rewrite it.
    pub editable: bool,
    /// Why the tap is not editable, or why it is unset.
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Tap(String),
}

static TAP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\s*capslock\s*=\s*overload\s*\(\s*nav\s*,\s*)(.+?)(\s*\)\s*(?:#.*)?)$")
        .expect("tap regex")
});

/// Split text into lines, keeping each line's own ending (as Python's `splitlines(keepends=True)`).
fn split_keepends(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            out.push(text[start..=i].to_string());
            start = i + 1;
        }
    }
    if start < text.len() {
        out.push(text[start..].to_string());
    }
    out
}

fn strip_ending(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

/// The `[main]` capslock setting lines, as (index, line).
fn capslock_settings(lines: &[&str], start: usize, end: usize) -> Vec<(usize, String)> {
    (start..end)
        .filter(|&i| {
            let line = lines[i];
            line.contains('=')
                && !line.trim_start().starts_with('#')
                && line
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("capslock"))
        })
        .map(|i| (i, lines[i].to_string()))
        .collect()
}

fn main_bounds(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| line.trim().eq_ignore_ascii_case("[main]"))?;
    let end = (start + 1..lines.len())
        .find(|&i| lines[i].trim().starts_with('['))
        .unwrap_or(lines.len());
    Some((start, end))
}

/// Return the `[nav]` bindings, in file order.
pub fn parse(text: &str) -> Vec<Binding> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') {
            inside = s == SECTION;
        } else if inside && s.contains('=') && !s.starts_with('#') {
            let (key, action) = s.split_once('=').expect("contains =");
            out.push(Binding::new(key.trim(), action.trim()));
        }
    }
    out
}

/// Read the `[main]` capslock overload's tap action.
pub fn read_tap(text: &str) -> Tap {
    let lines: Vec<&str> = text.lines().collect();
    let Some((start, end)) = main_bounds(&lines) else {
        return Tap {
            action: None,
            editable: true,
            message: "Tap action is not configured ([main] section missing).".into(),
        };
    };
    let settings = capslock_settings(&lines, start + 1, end);
    match settings.len() {
        0 => Tap {
            action: None,
            editable: true,
            message: "Tap action is not configured (missing [main] capslock setting).".into(),
        },
        1 => {
            let line = &settings[0].1;
            match TAP_RE.captures(line) {
                Some(caps) if !caps[2].trim().is_empty() => Tap {
                    action: Some(caps[2].trim().to_string()),
                    editable: true,
                    message: String::new(),
                },
                _ => Tap {
                    action: None,
                    editable: false,
                    message: format!(
                        "Read-only [main] capslock setting: {} (expected overload(nav, action)); refusing to edit.",
                        line.trim()
                    ),
                },
            }
        }
        _ => Tap {
            action: None,
            editable: false,
            message: format!(
                "Read-only [main] capslock settings: {}; refusing to edit.",
                settings
                    .iter()
                    .map(|(_, line)| line.trim())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        },
    }
}

/// Set the tap action, preserving every unrelated line and the file's line endings.
pub fn write_tap(text: &str, action: &str) -> Result<String, ConfigError> {
    let requested = action.trim();
    if requested.is_empty() || action.contains('\n') || action.contains('\r') {
        return Err(ConfigError::Tap(
            "Tap action must be a non-empty single-line keyd action.".into(),
        ));
    }
    let current = read_tap(text);
    if !current.editable {
        return Err(ConfigError::Tap(current.message));
    }
    if current.action.as_deref() == Some(requested) {
        return Ok(text.to_string());
    }

    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines = split_keepends(text);
    let plain: Vec<&str> = lines.iter().map(|l| strip_ending(l)).collect();

    let Some((start, end)) = main_bounds(&plain) else {
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with(['\n', '\r']) {
            out.push_str(newline);
        }
        if !out.is_empty() && !out.ends_with(&format!("{newline}{newline}")) {
            out.push_str(newline);
        }
        out.push_str("[main]");
        out.push_str(newline);
        out.push_str(&format!("capslock = overload(nav, {requested})"));
        out.push_str(newline);
        return Ok(out);
    };

    let settings = capslock_settings(&plain, start + 1, end);
    if let Some((index, _)) = settings.first() {
        let original = lines[*index].clone();
        let body = strip_ending(&original);
        let ending = &original[body.len()..];
        if let Some(caps) = TAP_RE.captures(body) {
            lines[*index] = format!("{}{}{}{}", &caps[1], requested, &caps[3], ending);
        }
    } else {
        if end == lines.len() && !lines.is_empty() && !lines[end - 1].ends_with(['\n', '\r']) {
            let last = lines.len() - 1;
            lines[last].push_str(newline);
        }
        lines.insert(end, format!("capslock = overload(nav, {requested}){newline}"));
    }
    Ok(lines.concat())
}

/// Replace the `[nav]` body with `binds`, keeping every other section as it is.
pub fn render(text: &str, binds: &[Binding]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') {
            inside = s == SECTION;
            out.push(line.to_string());
            if inside {
                out.extend(binds.iter().map(Binding::line));
            }
            continue;
        }
        if !inside {
            out.push(line.to_string());
        }
    }
    if !text.lines().any(|l| l.trim() == SECTION) {
        out.push(String::new());
        out.push(SECTION.to_string());
        out.extend(binds.iter().map(Binding::line));
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Render the bindings, then the tap action when it changed.
pub fn render_editor(
    text: &str,
    binds: &[Binding],
    tap: &str,
    original_tap: &str,
) -> Result<String, ConfigError> {
    let rendered = render(text, binds);
    if tap != original_tap {
        write_tap(&rendered, tap)
    } else {
        Ok(rendered)
    }
}

/// The name shown for a tap action, or why there is none.
pub fn tap_label(tap: &Tap) -> String {
    if let Some(action) = &tap.action {
        return match action.to_ascii_lowercase().as_str() {
            "capslock" => "Caps Lock".into(),
            "esc" => "Esc".into(),
            "backspace" => "Backspace".into(),
            _ => action.clone(),
        };
    }
    if tap.editable {
        "not configured".into()
    } else {
        tap.message
            .strip_prefix("Read-only: ")
            .unwrap_or(&tap.message)
            .to_string()
    }
}

/// A unified diff, empty when the texts match.
pub fn diff_lines(old_text: &str, new_text: &str, old_name: &str, new_name: &str) -> Vec<String> {
    if old_text == new_text {
        return Vec::new();
    }
    let diff = TextDiff::from_lines(old_text, new_text);
    diff.unified_diff()
        .context_radius(3)
        .header(old_name, new_name)
        .to_string()
        .lines()
        .map(|line| line.trim_end_matches(['\r', '\n']).to_string())
        .collect()
}

/// The `YYYY-MM-DD-HHMMSS` stamp of a `default.conf.bak.*` path, if it has one.
pub fn backup_timestamp(path: &str) -> Option<String> {
    const PREFIX: &str = "default.conf.bak.";
    let name = Path::new(path).file_name()?.to_str()?;
    let stamp = name.strip_prefix(PREFIX)?;
    let bytes = stamp.as_bytes();
    let shaped = bytes.len() == 17
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'-'
        && bytes[11..].iter().all(u8::is_ascii_digit)
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit);
    if !shaped {
        return None;
    }
    let number = |range: std::ops::Range<usize>| stamp[range].parse::<u32>().ok();
    let (month, day) = (number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(13..15)?, number(15..17)?);
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    valid.then(|| stamp.to_string())
}

/// Backup paths that carry a stamp, newest first.
pub fn sorted_backups(paths: &[String]) -> Vec<String> {
    let mut stamped: Vec<(String, String)> = paths
        .iter()
        .filter_map(|p| backup_timestamp(p).map(|stamp| (stamp, p.clone())))
        .collect();
    stamped.sort_by(|a, b| b.0.cmp(&a.0));
    stamped.into_iter().map(|(_, path)| path).collect()
}

/// A backup preview: the diff lines, and whether anything outside `[nav]` differs.
pub fn backup_preview(current_text: &str, backup_text: &str) -> (Vec<String>, bool) {
    let mut lines = diff_lines(current_text, backup_text, "current editor", "backup");
    let outside_nav = without_nav(current_text) != without_nav(backup_text);
    let current = read_tap(current_text);
    let backup = read_tap(backup_text);
    let signature = |tap: &Tap| {
        if tap.editable {
            tap.action.clone()
        } else {
            Some(tap.message.clone())
        }
    };
    let tap_differs = signature(&current) != signature(&backup);
    if lines.is_empty() {
        return (
            vec!["No differences; the backup has the same configuration.".into()],
            outside_nav,
        );
    }
    if tap_differs {
        let name = |tap: &Tap| match (&tap.action, tap.editable) {
            (Some(action), true) if !action.is_empty() => action.clone(),
            _ => "not configured/read-only".into(),
        };
        lines.insert(
            0,
            format!(
                "Tap action differs: editor={}, backup={}; restore changes [nav] only.",
                name(&current),
                name(&backup)
            ),
        );
    }
    if outside_nav {
        lines.insert(0, "Other sections differ; [nav] only".into());
    }
    (lines, outside_nav)
}

/// The config with every `[nav]` binding removed, for safe backup comparisons.
pub fn without_nav(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            inside = stripped == SECTION;
            out.push(line);
        } else if !inside {
            out.push(line);
        }
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}
