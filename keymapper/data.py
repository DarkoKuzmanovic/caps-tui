"""Static keyd actions, preset packs, and keyboard layout data."""

SECTION = "[nav]"


CUSTOM = "__custom__"


# (category, keyd action, description). Every action is checked against keyd in --selftest.
CATALOG = (
    ("Navigation", "home", "Move to the start of the line"),
    ("Navigation", "end", "Move to the end of the line"),
    ("Navigation", "pageup", "Move up one page"),
    ("Navigation", "pagedown", "Move down one page"),
    ("Navigation", "up", "Move up"),
    ("Navigation", "down", "Move down"),
    ("Navigation", "left", "Move left"),
    ("Navigation", "right", "Move right"),
    ("Word / line editing", "C-left", "Jump one word left (Ctrl+Left)"),
    ("Word / line editing", "C-right", "Jump one word right (Ctrl+Right)"),
    ("Word / line editing", "C-backspace", "Delete the previous word"),
    ("Word / line editing", "C-delete", "Delete the next word"),
    ("Word / line editing", "delete", "Delete the character under the cursor"),
    ("Word / line editing", "backspace", "Delete the character before the cursor"),
    ("Word / line editing", "insert", "Toggle insert/overwrite mode"),
    ("Word / line editing", "S-home", "Select to the start of the line"),
    ("Word / line editing", "S-end", "Select to the end of the line"),
    ("Clipboard / editing", "C-c", "Copy selection (Ctrl+C)"),
    ("Clipboard / editing", "C-v", "Paste (Ctrl+V)"),
    ("Clipboard / editing", "C-x", "Cut selection (Ctrl+X)"),
    ("Clipboard / editing", "C-z", "Undo (Ctrl+Z)"),
    ("Clipboard / editing", "C-S-z", "Redo (Ctrl+Shift+Z)"),
    ("Clipboard / editing", "C-y", "Redo (Ctrl+Y)"),
    ("Clipboard / editing", "C-a", "Select all (Ctrl+A)"),
    ("Clipboard / editing", "C-s", "Save (Ctrl+S)"),
    ("Clipboard / editing", "C-f", "Find (Ctrl+F)"),
    ("Media", "volumeup", "Increase speaker volume"),
    ("Media", "volumedown", "Decrease speaker volume"),
    ("Media", "mute", "Toggle audio mute"),
    ("Media", "playpause", "Play or pause media"),
    ("Media", "nextsong", "Skip to the next track"),
    ("Media", "previoussong", "Skip to the previous track"),
    ("Media", "brightnessup", "Increase screen brightness"),
    ("Media", "brightnessdown", "Decrease screen brightness"),
    ("System", "sysrq", "Print Screen / system request"),
    ("System", "scrolllock", "Toggle Scroll Lock"),
    ("System", "pause", "Pause / Break"),
    ("System", "esc", "Escape"),
    ("System", "compose", "Compose key"),
    ("System", "menu", "Context menu key"),
    ("Window / desktop", "A-tab", "Switch windows (Alt+Tab)"),
    ("Window / desktop", "A-f4", "Close window (Alt+F4)"),
    ("Window / desktop", "M-left", "Move to the previous desktop"),
    ("Window / desktop", "M-right", "Move to the next desktop"),
    ("Knob-friendly", "C-minus", "Zoom out (Ctrl+Minus)"),
    ("Knob-friendly", "C-equal", "Zoom in (Ctrl+Equals)"),
    ("Knob-friendly", "C-0", "Reset zoom to 100% (Ctrl+0)"),
    ("Knob-friendly", "C-tab", "Next tab (Ctrl+Tab)"),
    ("Knob-friendly", "C-S-tab", "Previous tab (Ctrl+Shift+Tab)"),
    ("Knob-friendly", "C-pageup", "Previous tab, second style (Ctrl+PageUp)"),
    ("Knob-friendly", "C-pagedown", "Next tab, second style (Ctrl+PageDown)"),
    ("Function keys", "f13", "Function key 13"),
    ("Function keys", "f14", "Function key 14"),
    ("Function keys", "f15", "Function key 15"),
    ("Function keys", "f16", "Function key 16"),
    ("Function keys", "f17", "Function key 17"),
    ("Function keys", "f18", "Function key 18"),
    ("Function keys", "f19", "Function key 19"),
    ("Function keys", "f20", "Function key 20"),
    ("Function keys", "f21", "Function key 21"),
    ("Function keys", "f22", "Function key 22"),
    ("Function keys", "f23", "Function key 23"),
    ("Function keys", "f24", "Function key 24"),
)


ACTION_DESCRIPTIONS = {action: description for _, action, description in CATALOG}


PACKS = (
    {
        "name": "Vim arrows",
        "description": "h/j/k/l → arrows; u/o → page up/down",
        "binds": (("h", "left"), ("j", "down"), ("k", "up"), ("l", "right"),
                  ("u", "pageup"), ("o", "pagedown")),
    },
    {
        "name": "Media shortcuts",
        "description": "; , . for volume/mute; F1–F5 for playback and brightness",
        "binds": (("semicolon", "volumeup"), ("comma", "volumedown"), ("dot", "mute"),
                  ("f1", "previoussong"), ("f2", "playpause"), ("f3", "nextsong"),
                  ("f4", "brightnessdown"), ("f5", "brightnessup")),
    },
    {
        "name": "Editing & clipboard",
        "description": "Common Ctrl shortcuts plus word deletion",
        "binds": (("b", "C-backspace"), ("d", "C-delete"), ("c", "C-c"), ("v", "C-v"),
                  ("x", "C-x"), ("z", "C-z"), ("y", "C-y"), ("a", "C-a"),
                  ("s", "C-s"), ("f", "C-f")),
    },
    {
        "name": "Window controls",
        "description": "tab → Alt+Tab; q → Alt+F4; [ / ] → previous/next desktop",
        "binds": (("tab", "A-tab"), ("q", "A-f4"), ("leftbrace", "M-left"), ("rightbrace", "M-right")),
    },
    {
        "name": "Knob: zoom",
        "description": "Caps + knob left/right → zoom out/in; press → reset zoom",
        "binds": (("volumedown", "C-minus"), ("volumeup", "C-equal"), ("mute", "C-0")),
    },
    {
        "name": "Knob: tabs",
        "description": "Caps + knob left/right → previous/next tab (Ctrl+Shift+Tab / Ctrl+Tab)",
        "binds": (("volumedown", "C-S-tab"), ("volumeup", "C-tab")),
    },
    {
        "name": "Knob: tabs (page)",
        "description": "Caps + knob left/right → previous/next tab (Ctrl+PageUp / Ctrl+PageDown)",
        "binds": (("volumedown", "C-pageup"), ("volumeup", "C-pagedown")),
    },
    {
        "name": "Knob: scrub",
        "description": "Caps + knob left/right → left/right arrow (seek in players, cursor)",
        "binds": (("volumedown", "left"), ("volumeup", "right")),
    },
    {
        "name": "Knob: brightness",
        "description": "Caps + knob left/right → screen brightness down/up",
        "binds": (("volumedown", "brightnessdown"), ("volumeup", "brightnessup")),
    },
    {
        "name": "Knob: undo/redo",
        "description": "Caps + knob left/right → undo/redo (Ctrl+Z / Ctrl+Shift+Z)",
        "binds": (("volumedown", "C-z"), ("volumeup", "C-S-z")),
    },
)


KEYBOARD_ROWS = (
    ("grave", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "minus", "equal", "backspace"),
    ("tab", "q", "w", "e", "r", "t", "y", "u", "i", "o", "p", "leftbrace", "rightbrace", "backslash"),
    ("capslock", "a", "s", "d", "f", "g", "h", "j", "k", "l", "semicolon", "apostrophe", "enter"),
    ("leftshift", "102nd", "z", "x", "c", "v", "b", "n", "m", "comma", "dot", "slash", "rightshift"),
    ("leftcontrol", "leftmeta", "leftalt", "space", "rightalt", "rightmeta", "menu", "rightcontrol"),
)
