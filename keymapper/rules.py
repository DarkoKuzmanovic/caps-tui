"""Binding rules, presets, keyboard highlights, and editor history."""

from .data import ACTION_DESCRIPTIONS, CATALOG, KEYBOARD_ROWS


_MONITOR_MODIFIER_ORDER = ("C", "M", "A", "S", "G")
_MONITOR_MODIFIER_ALIASES = {
    "control": "C", "ctrl": "C", "leftcontrol": "C", "rightcontrol": "C",
    "leftctrl": "C", "rightctrl": "C",
    "meta": "M", "leftmeta": "M", "rightmeta": "M",
    "alt": "A", "leftalt": "A",
    "shift": "S", "leftshift": "S", "rightshift": "S",
    "altgr": "G", "rightalt": "G",
}


def compose_monitor_chord(held_modifier_key_names, key):
    """Format a monitor chord using keyd's C, M, A, S, G modifier order."""
    held = {
        _MONITOR_MODIFIER_ALIASES[name.casefold()]
        for name in held_modifier_key_names
        if isinstance(name, str) and name.casefold() in _MONITOR_MODIFIER_ALIASES
    }
    modifiers = [modifier for modifier in _MONITOR_MODIFIER_ORDER if modifier in held]
    return "-".join((*modifiers, key)) if modifiers else key


def monitor_chords_equal(left, right):
    """Compare monitor chords without relying on the order of modifier prefixes."""
    def normalized(chord):
        if not isinstance(chord, str):
            return chord
        parts = chord.split("-")
        modifiers = []
        while len(parts) > 1 and parts[0].upper() in _MONITOR_MODIFIER_ORDER:
            modifiers.append(parts.pop(0).upper())
        if not modifiers:
            return chord.casefold()
        ordered = tuple(modifier for modifier in _MONITOR_MODIFIER_ORDER
                        if modifier in modifiers)
        return ordered, parts[-1].casefold()

    return normalized(left) == normalized(right)


def _copy_snapshot(value):
    if isinstance(value, list):
        return [_copy_snapshot(item) for item in value]
    if isinstance(value, tuple):
        return tuple(_copy_snapshot(item) for item in value)
    return value


class History:
    """Bounded undo/redo snapshots for complete editor states."""
    def __init__(self, limit=50):
        self.limit = max(1, limit)
        self.undo_stack = []
        self.redo_stack = []

    def record(self, before, after, description):
        before, after = _copy_snapshot(before), _copy_snapshot(after)
        if before == after:
            return False
        self.undo_stack.append((before, description))
        del self.undo_stack[:-self.limit]
        self.redo_stack.clear()
        return True

    def undo(self, current):
        if not self.undo_stack:
            return None
        snapshot, description = self.undo_stack.pop()
        self.redo_stack.append((_copy_snapshot(current), description))
        del self.redo_stack[:-self.limit]
        return snapshot, description

    def redo(self, current):
        if not self.redo_stack:
            return None
        snapshot, description = self.redo_stack.pop()
        self.undo_stack.append((_copy_snapshot(current), description))
        del self.undo_stack[:-self.limit]
        return snapshot, description


def filter_actions(query, catalog=CATALOG):
    """Return catalog entries matching a case-insensitive name/description/category query."""
    q = query.casefold().strip()
    return tuple(entry for entry in catalog
                 if not q or q in " ".join(entry).casefold())



def filter_binding_indices(binds, query):
    """Return real binding indexes matching key, action, or known action description."""
    q = query.casefold().strip()
    if not q:
        return tuple(range(len(binds)))
    descriptions = {action.casefold(): description for action, description in ACTION_DESCRIPTIONS.items()}
    return tuple(
        index for index, (key, action) in enumerate(binds)
        if q in key.casefold() or q in action.casefold()
        or q in descriptions.get(action.casefold(), "").casefold()
    )


def merge_pack(binds, pack):
    """Merge pack bindings in order; return (merged, added, replaced) without mutating inputs."""
    merged = list(binds)
    added, replaced = [], []
    for key, action in pack:
        index = next((i for i, (bound_key, _) in enumerate(merged) if bound_key == key), None)
        if index is None:
            merged.append((key, action))
            added.append((key, action))
        else:
            old_action = merged[index][1]
            merged[index] = (key, action)
            if old_action != action:
                replaced.append((key, old_action, action))
    return merged, added, replaced


def upsert_binding(binds, key, action, idx=None):
    """Add a binding or replace the selected row, removing duplicate mappings for its new key."""
    if idx is None:
        return [(k, v) for k, v in binds if k != key] + [(key, action)]
    if idx < 0 or idx >= len(binds):
        return list(binds)
    before = sum(1 for i, (k, _) in enumerate(binds[:idx]) if k != key)
    result = [(k, v) for i, (k, v) in enumerate(binds) if i != idx and k != key]
    result.insert(min(before, len(result)), (key, action))
    return result


def keyboard_highlights(binds, selected_key=None):
    """Return map-key styles: selected takes precedence over other bound keys."""
    bound = {key.casefold() for key, _ in binds}
    selected = selected_key.casefold() if selected_key is not None else None
    return {key: ("selected" if key.casefold() == selected else
                  "bound" if key.casefold() in bound else "plain")
            for row in KEYBOARD_ROWS for key in row}


def keyboard_unshown_bindings(binds):
    shown = {key.casefold() for row in KEYBOARD_ROWS for key in row}
    return [key for key, _ in binds if key.casefold() not in shown]


def binding_warnings(binds, tap):
    """Return (key, level, message) warnings for risks in [nav] bindings."""
    warnings = []
    for key, action in binds:
        key_lower, action_lower = key.casefold(), action.casefold()
        if action_lower in {"a-f4", "c-q", "c-w"}:
            warnings.append((key, "warn", f"{action} may close a window or application."))
        if action_lower.startswith("command("):
            warnings.append((key, "warn", "command(...) runs as root; review its contents carefully."))
        if key_lower == action_lower:
            warnings.append((key, "warn", f"No-op: {key} sends itself."))
        if key_lower == "capslock":
            tap_text = f" (tap sends {tap})" if tap else ""
            warnings.append((key, "warn", f"Caps Lock is the layer key{tap_text}; binding it inside [nav] is hard to reach."))
        if action_lower.startswith(("m-", "super-")):
            warnings.append((key, "info", f"{action} may be intercepted by the desktop."))
    return warnings


def warnings_for_key(warnings, key):
    return [(level, message) for warned_key, level, message in warnings
            if warned_key.casefold() == key.casefold()]


def warning_preview_lines(binds, tap):
    warnings = binding_warnings(binds, tap)
    if not warnings:
        return []
    lines = ["Warnings:"]
    lines.extend(f"  {level.upper()} {key}: {message}" for key, level, message in warnings)
    return lines


def pack_summary(pack, added, replaced, tap=None):
    lines = [f"Pack: {pack['name']}"]
    if added:
        lines.append("Added:")
        lines.extend(f"  {key} → {action}" for key, action in added)
        warnings = binding_warnings(added, tap)
        if warnings:
            lines.append("Warnings for added bindings:")
            lines.extend(f"  {level.upper()} {key}: {message}" for key, level, message in warnings)
    if replaced:
        lines.append("Replaced:")
        lines.extend(f"  {key}: {old} → {new}" for key, old, new in replaced)
    if not added and not replaced:
        lines.append("No bindings changed; all pack actions were already present.")
    return lines


def editor_state(binds, tap):
    return tuple(binds), tap


def restore_editor_state(state, binds):
    bindings, tap = state
    binds[:] = bindings
    return tap
