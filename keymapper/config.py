"""Pure text parsing, rendering, and comparison for keyd configurations."""

import difflib
import os
import re
from datetime import datetime

from .data import SECTION


def parse(text):
    """Return [(key, action)] from the [nav] section."""
    out, inside = [], False
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("["):
            inside = s == SECTION
        elif inside and "=" in s and not s.startswith("#"):
            k, v = s.split("=", 1)
            out.append((k.strip(), v.strip()))
    return out


def read_tap(text):
    """Return (action, editable, message) for [main] capslock overload(nav, action)."""
    lines = text.splitlines()
    main_start = next((i for i, line in enumerate(lines) if line.strip().casefold() == "[main]"), None)
    if main_start is None:
        return None, True, "Tap action is not configured ([main] section missing)."
    main_end = next((i for i in range(main_start + 1, len(lines))
                     if lines[i].strip().startswith("[")), len(lines))
    settings = [(i, line) for i, line in enumerate(lines[main_start + 1:main_end], main_start + 1)
                if "=" in line and not line.lstrip().startswith("#")
                and line.split("=", 1)[0].strip().casefold() == "capslock"]
    if not settings:
        return None, True, "Tap action is not configured (missing [main] capslock setting)."
    if len(settings) != 1:
        found = "; ".join(line.strip() for _, line in settings)
        return None, False, f"Read-only [main] capslock settings: {found}; refusing to edit."
    _, line = settings[0]
    match = re.match(r"^(\s*capslock\s*=\s*overload\s*\(\s*nav\s*,\s*)(.+?)(\s*\)\s*(?:#.*)?)$",
                     line, re.IGNORECASE)
    if not match or not match.group(2).strip():
        return None, False, (f"Read-only [main] capslock setting: {line.strip()} "
                             "(expected overload(nav, action)); refusing to edit.")
    return match.group(2).strip(), True, ""


def write_tap(text, action):
    """Set the tap action while preserving all unrelated text and line endings."""
    if not isinstance(action, str) or not action.strip() or "\n" in action or "\r" in action:
        raise ValueError("Tap action must be a non-empty single-line keyd action.")
    requested_action = action.strip()
    lines = text.splitlines(keepends=True)
    current_tap, editable, message = read_tap(text)
    if not editable:
        raise ValueError(message)
    if current_tap == requested_action:
        return text
    newline = "\r\n" if "\r\n" in text else "\n"
    main_start = next((i for i, line in enumerate(lines)
                       if line.strip().casefold() == "[main]"), None)
    if main_start is None:
        out = text
        if out and not out.endswith(("\n", "\r")):
            out += newline
        if out and not out.endswith(newline * 2):
            out += newline
        return out + "[main]" + newline + "capslock = overload(nav, " + requested_action + ")" + newline
    main_end = next((i for i in range(main_start + 1, len(lines))
                     if lines[i].strip().startswith("[")), len(lines))
    settings = [(i, line) for i, line in enumerate(lines[main_start + 1:main_end], main_start + 1)
                if "=" in line and not line.lstrip().startswith("#")
                and line.split("=", 1)[0].strip().casefold() == "capslock"]
    if settings:
        index, line = settings[0]
        body = line.rstrip("\r\n")
        ending = line[len(body):]
        match = re.match(r"^(\s*capslock\s*=\s*overload\s*\(\s*nav\s*,\s*)(.+?)(\s*\)\s*(?:#.*)?)$",
                         body, re.IGNORECASE)
        lines[index] = match.group(1) + requested_action + match.group(3) + ending
    else:
        if main_end == len(lines) and lines and not lines[-1].endswith(("\n", "\r")):
            lines[-1] += newline
        lines.insert(main_end, f"capslock = overload(nav, {requested_action}){newline}")
    return "".join(lines)


def render_editor(text, binds, tap, original_tap):
    rendered = render(text, binds)
    return write_tap(rendered, tap) if tap != original_tap else rendered


def tap_label(tap, editable, message):
    if tap is not None:
        names = {"capslock": "Caps Lock", "esc": "Esc", "backspace": "Backspace"}
        return names.get(tap.casefold(), tap)
    return message.removeprefix("Read-only: ") if not editable else "not configured"


def render(text, binds):
    """Replace the body of [nav] (keeping everything else) with binds."""
    lines, out, inside = text.splitlines(), [], False
    for line in lines:
        s = line.strip()
        if s.startswith("["):
            inside = s == SECTION
            out.append(line)
            if inside:
                out += [f"{k} = {v}" for k, v in binds]
            continue
        if not inside:
            out.append(line)
    if SECTION not in (l.strip() for l in lines):
        out += ["", SECTION] + [f"{k} = {v}" for k, v in binds]
    return "\n".join(out) + "\n"


def diff_lines(old_text, new_text, old_name="live default.conf", new_name="new default.conf"):
    """Return a unified diff, with no entries when the texts are identical."""
    diff = difflib.unified_diff(old_text.splitlines(keepends=True), new_text.splitlines(keepends=True),
                                fromfile=old_name, tofile=new_name, lineterm="")
    return [line.rstrip("\r\n") for line in diff]


def parse_backup_timestamp(path):
    """Parse the timestamp in default.conf.bak.YYYY-MM-DD-HHMMSS, else return None."""
    prefix = "default.conf.bak."
    name = os.path.basename(path)
    if not name.startswith(prefix):
        return None
    try:
        return datetime.strptime(name[len(prefix):], "%Y-%m-%d-%H%M%S")
    except ValueError:
        return None


def sorted_backups(paths):
    return sorted((path for path in paths if parse_backup_timestamp(path)),
                  key=parse_backup_timestamp, reverse=True)


def backup_preview(current_text, backup_text):
    lines = diff_lines(current_text, backup_text, "current editor", "backup")
    outside_nav = without_nav(current_text) != without_nav(backup_text)
    current_tap, current_editable, current_message = read_tap(current_text)
    backup_tap, backup_editable, backup_message = read_tap(backup_text)
    current_signature = current_tap if current_editable else current_message
    backup_signature = backup_tap if backup_editable else backup_message
    tap_differs = current_signature != backup_signature
    if not lines:
        return ["No differences; the backup has the same configuration."], outside_nav
    if tap_differs:
        current_name = current_tap if current_editable and current_tap else "not configured/read-only"
        backup_name = backup_tap if backup_editable and backup_tap else "not configured/read-only"
        lines.insert(0, f"Tap action differs: editor={current_name}, backup={backup_name}; restore changes [nav] only.")
    if outside_nav:
        lines.insert(0, "Other sections differ; [nav] only")
    return lines, outside_nav


def without_nav(text):
    """Return config text with [nav] bindings removed, for safe backup comparisons."""
    out, inside = [], False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            inside = stripped == SECTION
            out.append(line)
        elif not inside:
            out.append(line)
    return "\n".join(out) + "\n"
