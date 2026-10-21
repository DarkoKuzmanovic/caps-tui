"""Pure model and parser for keyd macro actions."""

from dataclasses import dataclass
import re


@dataclass(frozen=True)
class MacroStep:
    """One text, key/chord, or delay operation in a keyd macro."""

    kind: str
    value: str | int


_DELAY = re.compile(r"([0-9]+)ms\Z")
_MODIFIERS = {"C", "M", "A", "S", "G"}
_KEY_NAMES = frozenset({
    "backspace", "capslock", "delete", "down", "end", "enter", "esc",
    "escape", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9",
    "f10", "f11", "f12", "f13", "f14", "f15", "f16", "f17", "f18",
    "f19", "f20", "f21", "f22", "f23", "f24", "home", "insert",
    "left", "menu", "minus", "mute", "numlock", "pagedown", "pageup",
    "pause", "print", "right", "scrolllock", "space", "sysrq", "tab",
    "up", "volumeup", "volumedown", "equal", "dot", "comma", "slash",
    "semicolon", "apostrophe", "leftbrace", "rightbrace", "backslash",
    "grave", "102nd", "leftcontrol", "rightcontrol", "leftshift",
    "rightshift", "leftalt", "rightalt", "leftmeta", "rightmeta",
    "kp0", "kp1", "kp2", "kp3", "kp4", "kp5", "kp6", "kp7", "kp8",
    "kp9", "kpminus", "kpplus", "kpasterisk", "kpslash", "kpequal",
    "kpenter", "kpdelete", "kpdecimal", "kpcomma",
})

_KEY_ATOM = re.compile(r"(?:[A-Za-z0-9_]+|[^\w\s+()\\])\Z")


def _valid_key_atom(value):
    return bool(_KEY_ATOM.fullmatch(value)) and all(char.isprintable() for char in value)


def _looks_like_key_part(value, allow_char=False):
    parts = value.split("-")
    modifiers = []
    while len(parts) > 1 and parts[0] in _MODIFIERS:
        modifiers.append(parts.pop(0))
    if modifiers:
        return len(parts) == 1 and _valid_key_atom(parts[0])
    return len(parts) == 1 and (
        parts[0].casefold() in _KEY_NAMES or
        (allow_char and len(parts[0]) == 1 and _valid_key_atom(parts[0]))
    )


def _looks_like_key(value):
    """Recognize common key names and syntactically clear combos, not prose."""
    if "+" in value:
        parts = value.split("+")
        return len(parts) > 1 and all(_looks_like_key_part(part, allow_char=True)
                                      for part in parts)
    return _looks_like_key_part(value)


def _escape_token(value):
    return value.replace("\\", "\\\\").replace(")", "\\)")


def _text_tokens(value):
    """Represent spaces and reserved words without changing their text meaning."""
    tokens = []
    pieces = value.split(" ")
    for index, part in enumerate(pieces):
        if part:
            if _DELAY.fullmatch(part):
                tokens.extend(part)
            elif _looks_like_key(part) and len(part) > 1:
                tokens.extend(part if "+" in part or "-" in part else (part[0], part[1:]))
            else:
                tokens.append(part)
        if index + 1 < len(pieces):
            tokens.append("space")
    return tokens


def _valid_key(value):
    if not value or any(
            char.isspace() or char in "()\\" or not char.isprintable()
            for char in value):
        return False
    parts = value.split("+")
    if any(not part for part in parts):
        return False
    for part in parts:
        components = part.split("-")
        while len(components) > 1 and components[0] in _MODIFIERS:
            components.pop(0)
        if len(components) != 1 or not _valid_key_atom(components[0]):
            return False
    return True


def build_macro(steps) -> str:
    """Build a keyd ``macro(...)`` action from validated steps."""
    try:
        steps = tuple(steps)
    except TypeError as exc:
        raise ValueError("steps must be an iterable of MacroStep values") from exc
    if not steps:
        raise ValueError("a macro must contain at least one step")

    tokens = []
    for step in steps:
        if not isinstance(step, MacroStep):
            raise ValueError("each macro step must be a MacroStep")
        if step.kind == "text":
            if not isinstance(step.value, str) or not step.value:
                raise ValueError("text steps require a non-empty string")
            if any(char.isspace() and char != " " or not char.isprintable()
                   for char in step.value):
                raise ValueError("text steps may contain only printable characters and spaces")
            tokens.extend(_escape_token(token) for token in _text_tokens(step.value))
        elif step.kind == "key":
            if not isinstance(step.value, str) or not _valid_key(step.value):
                raise ValueError("key steps require a single key or key combo")
            tokens.append(_escape_token(step.value))
        elif step.kind == "delay":
            if type(step.value) is not int or not 0 <= step.value <= 1023:
                raise ValueError("delay steps require an integer from 0 to 1023 ms")
            tokens.append(f"{step.value}ms")
        else:
            raise ValueError(f"unknown macro step kind: {step.kind!r}")
    return "macro(" + " ".join(tokens) + ")"


def _decode_tokens(body):
    tokens = []
    current = []
    index = 0
    while index < len(body):
        char = body[index]
        if char == "\\":
            index += 1
            if index == len(body) or body[index] not in "\\)":
                return None
            current.append(body[index])
        elif char == ")":
            return None
        elif char.isspace():
            if current:
                tokens.append("".join(current))
                current.clear()
        else:
            current.append(char)
        index += 1
    if current:
        tokens.append("".join(current))
    return tokens


def parse_macro(action) -> tuple[MacroStep, ...] | None:
    """Best-effort parse of a keyd macro action; unsupported syntax returns None."""
    if not isinstance(action, str) or not action.startswith("macro(") or not action.endswith(")"):
        return None
    tokens = _decode_tokens(action[6:-1])
    if not tokens or any(not all(char.isprintable() for char in token) for token in tokens):
        return None

    steps = []
    for token in tokens:
        delay = _DELAY.fullmatch(token)
        if delay:
            value = int(delay.group(1))
            if value > 1023:
                return None
            steps.append(MacroStep("delay", value))
        elif token.casefold() == "space":
            steps.append(MacroStep("text", " "))
        elif _looks_like_key(token):
            steps.append(MacroStep("key", token))
        else:
            steps.append(MacroStep("text", token))
    return tuple(steps)
