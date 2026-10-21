"""Non-interactive command line for the keyd Caps Lock editor."""

import argparse
import os
import sys

from .config import diff_lines, parse, read_tap, render, tap_label, write_tap
from .data import ACTION_DESCRIPTIONS, PACKS
from .paths import CONF
from .rules import (
    binding_warnings, merge_pack, pack_summary, upsert_binding, warning_preview_lines, warnings_for_key,
)
from .system import check, keyd_bin, keyd_etc_dir, read_live_config, run_apply


class CliError(Exception):
    """A user-facing failure carrying its exit code."""

    def __init__(self, message, code=1):
        super().__init__(message)
        self.code = code


def resolve_pack(name, packs=PACKS):
    """Resolve a pack by exact or unique case-insensitive prefix; return (pack, candidates)."""
    query = name.strip().casefold()
    exact = [pack for pack in packs if pack["name"].casefold() == query]
    if exact:
        return exact[0], ()
    candidates = tuple(pack for pack in packs if pack["name"].casefold().startswith(query))
    if len(candidates) == 1:
        return candidates[0], ()
    return None, candidates


def load():
    try:
        with open(CONF) as f:
            return f.read()
    except OSError as exc:
        raise CliError(f"cannot read {CONF}: {exc}") from exc


def save(text):
    try:
        with open(CONF, "w") as f:
            f.write(text)
    except OSError as exc:
        raise CliError(f"cannot write {CONF}: {exc}") from exc


def validate(text):
    """Raise CliError when keyd rejects text; warn when keyd is unavailable."""
    if not keyd_bin():
        print("keyd not installed; config not validated.", file=sys.stderr)
        return
    error = check(text)
    if error:
        raise CliError(f"keyd rejects the resulting config:\n{error}")


def binding_line(key, action):
    description = ACTION_DESCRIPTIONS.get(action)
    return f"Caps + {key} → {action}" + (f"  — {description}" if description else "")


def print_warnings(binds, tap):
    for line in warning_preview_lines(binds, tap):
        print(line)


def confirm(prompt):
    try:
        answer = input(prompt)
    except EOFError:
        print()
        return False
    return answer.strip().casefold() in ("y", "yes")


def apply_flow(yes, text=None):
    """Show the live/repo diff, confirm, then install; return an exit code."""
    text = load() if text is None else text
    live_path = os.path.join(keyd_etc_dir(), "default.conf")
    live, fallback = read_live_config(text)
    lines = diff_lines(live, text, f"live {live_path}", f"repo {CONF}")
    if fallback:
        print(f"note: cannot read {live_path}; the installer will create it.", file=sys.stderr)
    elif not lines:
        print("No differences; nothing to apply.")
        return 0
    for line in lines:
        print(line)
    print_warnings(parse(text), read_tap(text)[0])
    if not yes and not confirm("Apply these changes with sudo? [y/N] "):
        print("Cancelled; nothing applied.")
        return 1
    return run_apply(text, announce=False)


def finish(args, text, new_text, message):
    """Validate, save, report an edit, and optionally apply it."""
    validate(new_text)
    changed = new_text != text
    if changed:
        save(new_text)
    print(message)
    if changed:
        print(f"Saved {CONF}.")
    binds = parse(new_text)
    tap = read_tap(new_text)[0]
    if getattr(args, "key", None):
        for level, warning in warnings_for_key(binding_warnings(binds, tap), args.key):
            print(f"{level.upper()} {args.key}: {warning}")
    else:
        print_warnings(binds, tap)
    if getattr(args, "apply", False):
        return apply_flow(getattr(args, "yes", False), new_text)
    return 0


def valid_key(key):
    return bool(key) and not any(ch.isspace() for ch in key) and "=" not in key


def cmd_list(args):
    text = load()
    binds = parse(text)
    if not binds:
        print("No bindings in [nav].")
    for key, action in binds:
        print(binding_line(key, action))
    print(f"Tap: {tap_label(*read_tap(text))}")
    print_warnings(binds, read_tap(text)[0])
    return 0


def cmd_add(args):
    key = args.key.strip().casefold()
    action = args.action.strip()
    if not valid_key(key):
        raise CliError(f"invalid key name: {args.key!r}")
    if not action or "\n" in action or "\r" in action:
        raise CliError("action must be a non-empty single-line keyd action.")
    text = load()
    binds = parse(text)
    new_text = render(text, upsert_binding(binds, key, action))
    existing = next((bound for bound_key, bound in binds if bound_key == key), None)
    if existing is None:
        message = f"Added: {binding_line(key, action)}"
    elif existing == action:
        message = f"Already bound: {binding_line(key, action)}"
    else:
        message = f"Replaced: Caps + {key} → {action} (was {existing})"
    return finish(args, text, new_text, message)


def cmd_rm(args):
    key = args.key.strip().casefold()
    if not valid_key(key):
        raise CliError(f"invalid key name: {args.key!r}")
    text = load()
    binds = parse(text)
    index = next((i for i, (bound_key, _) in enumerate(binds) if bound_key.casefold() == key), None)
    if index is None:
        raise CliError(f"not bound: {args.key}")
    bound_key, action = binds[index]
    new_text = render(text, binds[:index] + binds[index + 1:])
    return finish(args, text, new_text, f"Removed: {binding_line(bound_key, action)}")


def cmd_tap(args):
    text = load()
    try:
        new_text = write_tap(text, args.action)
    except ValueError as exc:
        raise CliError(str(exc)) from exc
    tap, editable, message = read_tap(new_text)
    if new_text == text:
        message = f"Tap action already {tap}."
    else:
        message = f"Tap action: {tap_label(tap, editable, message)}"
    return finish(args, text, new_text, message)


def cmd_packs(args):
    for pack in PACKS:
        print(f"{pack['name']}  — {pack['description']}")
    return 0


def cmd_pack(args):
    pack, candidates = resolve_pack(args.name)
    if pack is None:
        if candidates:
            listing = "\n".join(f"  {candidate['name']}" for candidate in candidates)
            raise CliError(f"pack {args.name!r} is ambiguous; candidates:\n{listing}", 2)
        raise CliError(f"no pack matches {args.name!r}; see 'keymap.py packs'.", 2)
    text = load()
    binds = parse(text)
    merged, added, replaced = merge_pack(binds, pack["binds"])
    new_text = render(text, merged)
    message = "\n".join(pack_summary(pack, added, replaced, read_tap(text)[0]))
    return finish(args, text, new_text, message)


def cmd_diff(args):
    text = load()
    live_path = os.path.join(keyd_etc_dir(), "default.conf")
    live, fallback = read_live_config(text)
    if fallback:
        print(f"note: cannot read {live_path}; nothing to compare.", file=sys.stderr)
        return 2
    lines = diff_lines(live, text, f"live {live_path}", f"repo {CONF}")
    if not lines:
        print("No differences")
        return 0
    for line in lines:
        print(line)
    return 1


def cmd_check(args):
    text = load()
    if not keyd_bin():
        raise CliError("keyd not installed; cannot validate.")
    error = check(text)
    if error:
        print(error, file=sys.stderr)
        raise CliError(f"{CONF} is invalid.")
    print(f"{CONF}: OK")
    return 0


def cmd_apply(args):
    return apply_flow(args.yes)


def build_parser():
    parser = argparse.ArgumentParser(
        prog="keymap.py",
        description="Edit the keyd Caps Lock [nav] layer without the interactive TUI.",
        epilog="With no arguments keymap.py opens the Textual editor. Commands only change the repo's "
               "default.conf; add --apply to install afterward.",
    )
    commands = parser.add_subparsers(dest="command", metavar="COMMAND")

    add = commands.add_parser("list", help="show bindings, tap action, and warnings")
    add.set_defaults(handler=cmd_list)

    add = commands.add_parser("add", help="add or replace Caps + KEY → ACTION")
    add.add_argument("key", help="keyd key name (for example h, volumedown)")
    add.add_argument("action", help="keyd action (for example left, C-minus)")
    add.add_argument("--apply", action="store_true", help="install after saving")
    add.add_argument("-y", "--yes", action="store_true", help="skip the apply confirmation")
    add.set_defaults(handler=cmd_add)

    rm = commands.add_parser("rm", help="remove a Caps + KEY binding")
    rm.add_argument("key", help="keyd key name")
    rm.add_argument("--apply", action="store_true", help="install after saving")
    rm.add_argument("-y", "--yes", action="store_true", help="skip the apply confirmation")
    rm.set_defaults(handler=cmd_rm)

    tap = commands.add_parser("tap", help="set the [main] capslock tap action")
    tap.add_argument("action", help="keyd action sent on a short Caps tap")
    tap.add_argument("--apply", action="store_true", help="install after saving")
    tap.add_argument("-y", "--yes", action="store_true", help="skip the apply confirmation")
    tap.set_defaults(handler=cmd_tap)

    add = commands.add_parser("packs", help="list the preset packs")
    add.set_defaults(handler=cmd_packs)

    add = commands.add_parser("pack", help="merge a preset pack, by exact name or unique prefix")
    add.add_argument("name", help="pack name or unique case-insensitive prefix")
    add.add_argument("--apply", action="store_true", help="install after saving")
    add.add_argument("-y", "--yes", action="store_true", help="skip the apply confirmation")
    add.set_defaults(handler=cmd_pack)

    add = commands.add_parser(
        "diff", help="diff the live config against default.conf",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description="Compare the live config with this repo's default.conf.\n"
                    "Exit codes:\n"
                    "  0  identical\n"
                    "  1  different\n"
                    "  2  live config unreadable.")
    add.set_defaults(handler=cmd_diff)

    add = commands.add_parser("check", help="run keyd check on default.conf")
    add.set_defaults(handler=cmd_check)

    add = commands.add_parser("apply", help="show the diff, confirm, and install with sudo")
    add.add_argument("-y", "--yes", action="store_true", help="skip the confirmation")
    add.set_defaults(handler=cmd_apply)
    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    if not getattr(args, "handler", None):
        parser.print_help()
        return 2
    try:
        return args.handler(args)
    except CliError as exc:
        print(f"keymap.py: {exc}", file=sys.stderr)
        return exc.code
    except OSError as exc:
        print(f"keymap.py: {exc}", file=sys.stderr)
        return 1
