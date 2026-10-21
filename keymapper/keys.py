"""Translate terminal key events into keyd key names without a UI dependency."""

SPECIAL = {
    "left": "left", "right": "right", "up": "up", "down": "down",
    "pageup": "pageup", "pagedown": "pagedown", "insert": "insert", "delete": "delete",
    "backspace": "backspace", "home": "home", "end": "end", "enter": "enter",
    **{f"f{number}": f"f{number}" for number in range(1, 25)},
}

CHARS = {
    " ": "space", "\t": "tab", "\n": "enter", "\r": "enter", "\x7f": "backspace",
    "\x1b": "esc", "-": "minus", "=": "equal", "[": "leftbrace", "]": "rightbrace",
    ";": "semicolon", "'": "apostrophe", "`": "grave", "\\": "backslash",
    ",": "comma", ".": "dot", "/": "slash",
}


def key_name(key):
    """Translate Textual/Pilot names or a single typed character to a keyd name."""
    if not isinstance(key, str):
        return None
    value = key.casefold()
    if value in SPECIAL:
        return SPECIAL[value]
    if key in CHARS:
        return CHARS[key]
    if len(key) == 1 and key.isascii() and key.isalnum():
        return key.lower()
    return None
