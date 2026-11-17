//! Static keyd data: the section we edit, the action catalog, preset packs and the keyboard map.
//!
//! Ported from `keymapper/data.py` and the label/width tables in `keymapper/tui/screens.py` +
//! `app.tcss`. Widths are terminal cells (a plain keycap is 6), which the UI scales.

/// The section holding the Caps Lock layer bindings.
pub const SECTION: &str = "[nav]";

/// Sentinel the UI uses for "type a raw keyd action".
pub const CUSTOM: &str = "__custom__";

/// One entry of the action picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    pub category: &'static str,
    pub action: &'static str,
    pub description: &'static str,
}

pub const CATALOG: &[Action] = &[
    Action { category: "Navigation", action: "home", description: "Move to the start of the line" },
    Action { category: "Navigation", action: "end", description: "Move to the end of the line" },
    Action { category: "Navigation", action: "pageup", description: "Move up one page" },
    Action { category: "Navigation", action: "pagedown", description: "Move down one page" },
    Action { category: "Navigation", action: "up", description: "Move up" },
    Action { category: "Navigation", action: "down", description: "Move down" },
    Action { category: "Navigation", action: "left", description: "Move left" },
    Action { category: "Navigation", action: "right", description: "Move right" },
    Action { category: "Word / line editing", action: "C-left", description: "Jump one word left (Ctrl+Left)" },
    Action { category: "Word / line editing", action: "C-right", description: "Jump one word right (Ctrl+Right)" },
    Action { category: "Word / line editing", action: "C-backspace", description: "Delete the previous word" },
    Action { category: "Word / line editing", action: "C-delete", description: "Delete the next word" },
    Action { category: "Word / line editing", action: "delete", description: "Delete the character under the cursor" },
    Action { category: "Word / line editing", action: "backspace", description: "Delete the character before the cursor" },
    Action { category: "Word / line editing", action: "insert", description: "Toggle insert/overwrite mode" },
    Action { category: "Word / line editing", action: "S-home", description: "Select to the start of the line" },
    Action { category: "Word / line editing", action: "S-end", description: "Select to the end of the line" },
    Action { category: "Clipboard / editing", action: "C-c", description: "Copy selection (Ctrl+C)" },
    Action { category: "Clipboard / editing", action: "C-v", description: "Paste (Ctrl+V)" },
    Action { category: "Clipboard / editing", action: "C-x", description: "Cut selection (Ctrl+X)" },
    Action { category: "Clipboard / editing", action: "C-z", description: "Undo (Ctrl+Z)" },
    Action { category: "Clipboard / editing", action: "C-S-z", description: "Redo (Ctrl+Shift+Z)" },
    Action { category: "Clipboard / editing", action: "C-y", description: "Redo (Ctrl+Y)" },
    Action { category: "Clipboard / editing", action: "C-a", description: "Select all (Ctrl+A)" },
    Action { category: "Clipboard / editing", action: "C-s", description: "Save (Ctrl+S)" },
    Action { category: "Clipboard / editing", action: "C-f", description: "Find (Ctrl+F)" },
    Action { category: "Media", action: "volumeup", description: "Increase speaker volume" },
    Action { category: "Media", action: "volumedown", description: "Decrease speaker volume" },
    Action { category: "Media", action: "mute", description: "Toggle audio mute" },
    Action { category: "Media", action: "playpause", description: "Play or pause media" },
    Action { category: "Media", action: "nextsong", description: "Skip to the next track" },
    Action { category: "Media", action: "previoussong", description: "Skip to the previous track" },
    Action { category: "Media", action: "brightnessup", description: "Increase screen brightness" },
    Action { category: "Media", action: "brightnessdown", description: "Decrease screen brightness" },
    Action { category: "System", action: "sysrq", description: "Print Screen / system request" },
    Action { category: "System", action: "scrolllock", description: "Toggle Scroll Lock" },
    Action { category: "System", action: "pause", description: "Pause / Break" },
    Action { category: "System", action: "esc", description: "Escape" },
    Action { category: "System", action: "compose", description: "Compose key" },
    Action { category: "System", action: "menu", description: "Context menu key" },
    Action { category: "Window / desktop", action: "A-tab", description: "Switch windows (Alt+Tab)" },
    Action { category: "Window / desktop", action: "A-f4", description: "Close window (Alt+F4)" },
    Action { category: "Window / desktop", action: "M-left", description: "Move to the previous desktop" },
    Action { category: "Window / desktop", action: "M-right", description: "Move to the next desktop" },
    Action { category: "Knob-friendly", action: "C-minus", description: "Zoom out (Ctrl+Minus)" },
    Action { category: "Knob-friendly", action: "C-equal", description: "Zoom in (Ctrl+Equals)" },
    Action { category: "Knob-friendly", action: "C-0", description: "Reset zoom to 100% (Ctrl+0)" },
    Action { category: "Knob-friendly", action: "C-tab", description: "Next tab (Ctrl+Tab)" },
    Action { category: "Knob-friendly", action: "C-S-tab", description: "Previous tab (Ctrl+Shift+Tab)" },
    Action { category: "Knob-friendly", action: "C-pageup", description: "Previous tab, second style (Ctrl+PageUp)" },
    Action { category: "Knob-friendly", action: "C-pagedown", description: "Next tab, second style (Ctrl+PageDown)" },
    Action { category: "Function keys", action: "f13", description: "Function key 13" },
    Action { category: "Function keys", action: "f14", description: "Function key 14" },
    Action { category: "Function keys", action: "f15", description: "Function key 15" },
    Action { category: "Function keys", action: "f16", description: "Function key 16" },
    Action { category: "Function keys", action: "f17", description: "Function key 17" },
    Action { category: "Function keys", action: "f18", description: "Function key 18" },
    Action { category: "Function keys", action: "f19", description: "Function key 19" },
    Action { category: "Function keys", action: "f20", description: "Function key 20" },
    Action { category: "Function keys", action: "f21", description: "Function key 21" },
    Action { category: "Function keys", action: "f22", description: "Function key 22" },
    Action { category: "Function keys", action: "f23", description: "Function key 23" },
    Action { category: "Function keys", action: "f24", description: "Function key 24" },
];

/// The description for a keyd action, when the catalog knows it.
pub fn action_description(action: &str) -> Option<&'static str> {
    let wanted = action.to_lowercase();
    CATALOG
        .iter()
        .find(|entry| entry.action.to_lowercase() == wanted)
        .map(|entry| entry.description)
}

/// A preset pack: a named group of bindings merged into `[nav]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pack {
    pub name: &'static str,
    pub description: &'static str,
    pub binds: &'static [(&'static str, &'static str)],
}

pub const PACKS: &[Pack] = &[
    Pack {
        name: "Vim arrows",
        description: "h/j/k/l → arrows; u/o → page up/down",
        binds: &[("h", "left"), ("j", "down"), ("k", "up"), ("l", "right"), ("u", "pageup"), ("o", "pagedown")],
    },
    Pack {
        name: "Media shortcuts",
        description: "; , . for volume/mute; F1–F5 for playback and brightness",
        binds: &[
            ("semicolon", "volumeup"),
            ("comma", "volumedown"),
            ("dot", "mute"),
            ("f1", "previoussong"),
            ("f2", "playpause"),
            ("f3", "nextsong"),
            ("f4", "brightnessdown"),
            ("f5", "brightnessup"),
        ],
    },
    Pack {
        name: "Editing & clipboard",
        description: "Common Ctrl shortcuts plus word deletion",
        binds: &[
            ("b", "C-backspace"),
            ("d", "C-delete"),
            ("c", "C-c"),
            ("v", "C-v"),
            ("x", "C-x"),
            ("z", "C-z"),
            ("y", "C-y"),
            ("a", "C-a"),
            ("s", "C-s"),
            ("f", "C-f"),
        ],
    },
    Pack {
        name: "Window controls",
        description: "tab → Alt+Tab; q → Alt+F4; [ / ] → previous/next desktop",
        binds: &[("tab", "A-tab"), ("q", "A-f4"), ("leftbrace", "M-left"), ("rightbrace", "M-right")],
    },
    Pack {
        name: "Knob: zoom",
        description: "Caps + knob left/right → zoom out/in; press → reset zoom",
        binds: &[("volumedown", "C-minus"), ("volumeup", "C-equal"), ("mute", "C-0")],
    },
    Pack {
        name: "Knob: tabs",
        description: "Caps + knob left/right → previous/next tab (Ctrl+Shift+Tab / Ctrl+Tab)",
        binds: &[("volumedown", "C-S-tab"), ("volumeup", "C-tab")],
    },
    Pack {
        name: "Knob: tabs (page)",
        description: "Caps + knob left/right → previous/next tab (Ctrl+PageUp / Ctrl+PageDown)",
        binds: &[("volumedown", "C-pageup"), ("volumeup", "C-pagedown")],
    },
    Pack {
        name: "Knob: scrub",
        description: "Caps + knob left/right → left/right arrow (seek in players, cursor)",
        binds: &[("volumedown", "left"), ("volumeup", "right")],
    },
    Pack {
        name: "Knob: brightness",
        description: "Caps + knob left/right → screen brightness down/up",
        binds: &[("volumedown", "brightnessdown"), ("volumeup", "brightnessup")],
    },
    Pack {
        name: "Knob: undo/redo",
        description: "Caps + knob left/right → undo/redo (Ctrl+Z / Ctrl+Shift+Z)",
        binds: &[("volumedown", "C-z"), ("volumeup", "C-S-z")],
    },
];

/// The main keyboard, row by row, in keyd key names.
pub const KEYBOARD_ROWS: &[&[&str]] = &[
    &["grave", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "minus", "equal", "backspace"],
    &["tab", "q", "w", "e", "r", "t", "y", "u", "i", "o", "p", "leftbrace", "rightbrace", "backslash"],
    &["capslock", "a", "s", "d", "f", "g", "h", "j", "k", "l", "semicolon", "apostrophe", "enter"],
    &["leftshift", "102nd", "z", "x", "c", "v", "b", "n", "m", "comma", "dot", "slash", "rightshift"],
    &["leftcontrol", "leftmeta", "leftalt", "space", "rightalt", "rightmeta", "menu", "rightcontrol"],
];

pub const ARROW_KEYS: [&str; 4] = ["up", "left", "down", "right"];
pub const KNOB_KEYS: [&str; 3] = ["volumedown", "volumeup", "mute"];

/// F1–F24, as the map's function row shows them.
pub fn function_keys() -> Vec<String> {
    (1..=24).map(|n| format!("f{n}")).collect()
}

/// The cap's printed label. Long keyd names get their symbol, as the TUI shows them.
pub fn key_label(key: &str) -> String {
    let named = match key {
        "esc" => Some("Esc"),
        "grave" => Some("`"),
        "minus" => Some("-"),
        "equal" => Some("="),
        "backspace" => Some("⌫"),
        "up" => Some("↑"),
        "down" => Some("↓"),
        "left" => Some("←"),
        "right" => Some("→"),
        "tab" => Some("Tab"),
        "leftbrace" => Some("["),
        "rightbrace" => Some("]"),
        "backslash" => Some("\\"),
        "capslock" => Some("Caps"),
        "leftshift" | "rightshift" => Some("⇧"),
        "enter" => Some("Enter"),
        "semicolon" => Some(";"),
        "apostrophe" => Some("'"),
        "comma" => Some(","),
        "dot" => Some("."),
        "slash" => Some("/"),
        "102nd" => Some("<>"),
        "leftcontrol" | "rightcontrol" => Some("Ctrl"),
        "leftmeta" | "rightmeta" => Some("❖"),
        "leftalt" => Some("Alt"),
        "rightalt" => Some("AltGr"),
        "space" => Some("Space"),
        "menu" => Some("Menu"),
        "volumedown" => Some("◀"),
        "volumeup" => Some("▶"),
        "mute" => Some("●"),
        _ => None,
    };
    if let Some(label) = named {
        return label.to_string();
    }
    if let Some(number) = key.strip_prefix('f')
        && number.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
    {
        return format!("F{number}");
    }
    if key.chars().count() == 1 {
        return key.to_uppercase();
    }
    key.to_string()
}

/// The cap's width in terminal cells (a plain key is 6), from `app.tcss`.
pub fn keycap_cells(key: &str) -> u32 {
    match key {
        "up" | "down" | "left" | "right" => 6,
        "backslash" => 7,
        "backspace" => 10,
        "tab" | "rightshift" => 8,
        "capslock" => 9,
        "leftshift" | "enter" => 11,
        "102nd" | "leftcontrol" | "rightcontrol" | "leftalt" | "menu" => 6,
        "leftmeta" | "rightmeta" | "rightalt" => 8,
        "space" => 27,
        "volumedown" | "volumeup" | "mute" => 9,
        _ => 6,
    }
}
