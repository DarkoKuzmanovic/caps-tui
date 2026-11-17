//! Pure model and parser for keyd macro actions.
//!
//! Ported from `keymapper/macro.py`. The token splitting is deliberate, not incidental: a text
//! step that reads like a key name (`esc`) or a delay (`25ms`) is split so keyd types the
//! characters instead of acting on them.

/// One step of a keyd `macro(...)`: text to type, a key or chord, or a pause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroStep {
    Text(String),
    Key(String),
    Delay(u32),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MacroError {
    #[error("a macro must contain at least one step")]
    Empty,
    #[error("text steps require a non-empty string")]
    TextEmpty,
    #[error("text steps may contain only printable characters and spaces")]
    TextCharacters,
    #[error("key steps require a single key or key combo")]
    Key,
    #[error("delay steps require an integer from 0 to 1023 ms")]
    Delay,
}

const MODIFIERS: [&str; 5] = ["C", "M", "A", "S", "G"];

/// Key names keyd knows, sorted for binary search.
const KEY_NAMES: [&str; 89] = [
    "102nd", "apostrophe", "backslash", "backspace", "capslock", "comma", "delete", "dot", "down",
    "end", "enter", "equal", "esc", "escape", "f1", "f10", "f11", "f12", "f13", "f14", "f15", "f16",
    "f17", "f18", "f19", "f2", "f20", "f21", "f22", "f23", "f24", "f3", "f4", "f5", "f6", "f7",
    "f8", "f9", "grave", "home", "insert", "kp0", "kp1", "kp2", "kp3", "kp4", "kp5", "kp6", "kp7",
    "kp8", "kp9", "kpasterisk", "kpcomma", "kpdecimal", "kpdelete", "kpenter", "kpequal", "kpminus",
    "kpplus", "kpslash", "left", "leftalt", "leftbrace", "leftcontrol", "leftmeta", "leftshift",
    "menu", "minus", "mute", "numlock", "pagedown", "pageup", "pause", "print", "right", "rightalt",
    "rightbrace", "rightcontrol", "rightmeta", "rightshift", "scrolllock", "semicolon", "slash",
    "space", "sysrq", "tab", "up", "volumedown", "volumeup",
];

/// Python's `str.isprintable`, close enough for ASCII keyd syntax.
fn is_printable(c: char) -> bool {
    !c.is_control()
}

fn is_delay(token: &str) -> bool {
    token
        .strip_suffix("ms")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn delay_value(token: &str) -> Option<u32> {
    if !is_delay(token) {
        return None;
    }
    token[..token.len() - 2].parse().ok()
}

fn valid_key_atom(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return false;
    }
    let word = chars.iter().all(|c| c.is_ascii_alphanumeric() || *c == '_');
    let single = chars.len() == 1
        && !chars[0].is_alphanumeric()
        && chars[0] != '_'
        && !chars[0].is_whitespace()
        && !" +()\\".contains(chars[0]);
    (word || single) && chars.iter().all(|c| is_printable(*c))
}

fn looks_like_key_part(value: &str, allow_char: bool) -> bool {
    let mut parts: Vec<&str> = value.split('-').collect();
    let mut saw_modifier = false;
    while parts.len() > 1 && MODIFIERS.contains(&parts[0]) {
        parts.remove(0);
        saw_modifier = true;
    }
    if saw_modifier {
        return parts.len() == 1 && valid_key_atom(parts[0]);
    }
    parts.len() == 1
        && (KEY_NAMES.binary_search(&parts[0].to_lowercase().as_str()).is_ok()
            || (allow_char && parts[0].chars().count() == 1 && valid_key_atom(parts[0])))
}

/// Common key names and syntactically clear combos — not prose.
fn looks_like_key(value: &str) -> bool {
    if value.contains('+') {
        let parts: Vec<&str> = value.split('+').collect();
        return parts.len() > 1 && parts.iter().all(|part| looks_like_key_part(part, true));
    }
    looks_like_key_part(value, false)
}

fn escape_token(value: &str) -> String {
    value.replace('\\', "\\\\").replace(')', "\\)")
}

/// Represent spaces and reserved words without changing their text meaning.
fn text_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let pieces: Vec<&str> = value.split(' ').collect();
    for (index, part) in pieces.iter().enumerate() {
        if !part.is_empty() {
            if is_delay(part) {
                // Typed literally: keyd would otherwise read it as a delay.
                tokens.extend(part.chars().map(String::from));
            } else if looks_like_key(part) && part.chars().count() > 1 {
                if part.contains('+') || part.contains('-') {
                    tokens.push((*part).to_string());
                } else {
                    let mut chars = part.chars();
                    tokens.push(chars.next().expect("non-empty").to_string());
                    tokens.push(chars.as_str().to_string());
                }
            } else {
                tokens.push((*part).to_string());
            }
        }
        if index + 1 < pieces.len() {
            tokens.push("space".to_string());
        }
    }
    tokens
}

fn valid_key(value: &str) -> bool {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || "()\\".contains(c) || !is_printable(c))
    {
        return false;
    }
    let parts: Vec<&str> = value.split('+').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return false;
    }
    for part in parts {
        let mut components: Vec<&str> = part.split('-').collect();
        while components.len() > 1 && MODIFIERS.contains(&components[0]) {
            components.remove(0);
        }
        if components.len() != 1 || !valid_key_atom(components[0]) {
            return false;
        }
    }
    true
}

/// Build a keyd `macro(...)` action from validated steps.
pub fn build_macro(steps: &[MacroStep]) -> Result<String, MacroError> {
    if steps.is_empty() {
        return Err(MacroError::Empty);
    }
    let mut tokens: Vec<String> = Vec::new();
    for step in steps {
        match step {
            MacroStep::Text(value) => {
                if value.is_empty() {
                    return Err(MacroError::TextEmpty);
                }
                if value
                    .chars()
                    .any(|c| (c.is_whitespace() && c != ' ') || !is_printable(c))
                {
                    return Err(MacroError::TextCharacters);
                }
                tokens.extend(text_tokens(value).into_iter().map(|token| escape_token(&token)));
            }
            MacroStep::Key(value) => {
                if !valid_key(value) {
                    return Err(MacroError::Key);
                }
                tokens.push(escape_token(value));
            }
            MacroStep::Delay(ms) => {
                if *ms > 1023 {
                    return Err(MacroError::Delay);
                }
                tokens.push(format!("{ms}ms"));
            }
        }
    }
    Ok(format!("macro({})", tokens.join(" ")))
}

fn decode_tokens(body: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = body.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '\\' {
            index += 1;
            let next = *chars.get(index)?;
            if next != '\\' && next != ')' {
                return None;
            }
            current.push(next);
        } else if c == ')' {
            return None;
        } else if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
        index += 1;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Some(tokens)
}

/// Best-effort parse of a keyd macro action; unsupported syntax returns `None`.
pub fn parse_macro(action: &str) -> Option<Vec<MacroStep>> {
    let body = action.strip_prefix("macro(")?.strip_suffix(')')?;
    let tokens = decode_tokens(body)?;
    if tokens.is_empty()
        || tokens
            .iter()
            .any(|token| !token.chars().all(is_printable))
    {
        return None;
    }
    let mut steps = Vec::new();
    for token in tokens {
        if let Some(ms) = delay_value(&token) {
            if ms > 1023 {
                return None;
            }
            steps.push(MacroStep::Delay(ms));
        } else if token.eq_ignore_ascii_case("space") {
            steps.push(MacroStep::Text(" ".into()));
        } else if looks_like_key(&token) {
            steps.push(MacroStep::Key(token));
        } else {
            steps.push(MacroStep::Text(token));
        }
    }
    Some(steps)
}
