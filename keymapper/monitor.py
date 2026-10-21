"""Chord composition and annotations for keyd monitor events."""

from .rules import compose_monitor_chord, monitor_chords_equal
from .system import parse_monitor_event

MONITOR_MODIFIER_KEYS = frozenset({
    "control", "leftcontrol", "rightcontrol", "shift", "leftshift", "rightshift",
    "alt", "leftalt", "meta", "leftmeta", "rightmeta", "altgr", "rightalt",
})


def monitor_annotations(binds, output_key):
    """Describe editor bindings whose configured action matches keyd output."""
    arrows = {"left": "←", "right": "→", "up": "↑", "down": "↓"}
    matches = []
    for key, action in binds:
        if not monitor_chords_equal(action, output_key):
            continue
        arrow = arrows.get(key.casefold()) or arrows.get(action.casefold())
        matches.append(f"{arrow} Caps + {key}" if arrow else f"Caps + {key} → {action}")
    return matches


def record_monitor_event(events, line, binds, held_modifiers, active_events):
    """Compose modifier chords without briefly listing their held modifiers alone."""
    parsed = parse_monitor_event(line)
    if parsed is None:
        return False
    device, key, state = parsed
    fields = line.rstrip("\r\n").split("\t")
    device_identity = (device, fields[1].strip() if len(fields) > 1 else device)
    identity = (device_identity, key.casefold())
    is_modifier = key.casefold() in MONITOR_MODIFIER_KEYS

    if is_modifier:
        held = held_modifiers.setdefault(device_identity, set())
        if state == "down":
            held.add(key.casefold())
            active_events[identity] = {
                "device": device, "key": key, "state": "down",
                "annotation": [], "chord": False, "_pending_modifier": True,
            }
            return False

        held.discard(key.casefold())
        previous = active_events.pop(identity, None)
        if previous is not None and previous.get("_pending_modifier"):
            previous["state"] = "down/up"
            if previous.get("_consumed"):
                return False
            previous.pop("_pending_modifier")
            events.append(previous)
            del events[:-200]
            return True
        display_key, chord = key, False
    else:
        chord_key = compose_monitor_chord(held_modifiers.get(device_identity, ()), key)
        display_key, chord = chord_key, chord_key.casefold() != key.casefold()
        if chord:
            for modifier in held_modifiers.get(device_identity, ()):
                pending = active_events.get((device_identity, modifier))
                if pending is not None and pending.get("_pending_modifier"):
                    pending["_consumed"] = True

    if state == "up":
        previous = active_events.pop(identity, None)
        if previous is not None:
            previous["state"] = "down/up"
            return True
    event = {
        "device": device, "key": display_key, "state": state,
        "annotation": monitor_annotations(binds, display_key), "chord": chord,
    }
    events.append(event)
    del events[:-200]
    if state == "down":
        active_events[identity] = event
    return True
