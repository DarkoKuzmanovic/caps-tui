"""Assertions for keymapper text, rules, and keyd integration."""

import subprocess

from .keys import key_name

from .data import CATALOG, KEYBOARD_ROWS, PACKS
from .cli import resolve_pack
from .config import (
    backup_preview, diff_lines, parse, parse_backup_timestamp, read_tap, render, sorted_backups, write_tap,
)
from .macro import MacroStep, build_macro, parse_macro
from .rules import (
    History, binding_warnings, compose_monitor_chord, editor_state, filter_actions,
    filter_binding_indices, keyboard_highlights, keyboard_unshown_bindings,
    merge_pack, monitor_chords_equal, upsert_binding, warning_preview_lines,
)
from .system import check, keyd_bin, parse_monitor_event, parse_monitor_line
from .paths import CONF


def selftest():
    src = "# hdr\n[ids]\n0e6a:02c0\n\n[main]\ncapslock = overload(nav, capslock)\n\n[nav]\nleft = home\n# c\nright = end\n"
    b = parse(src)
    macro_steps = (
        MacroStep("text", "hello world"),
        MacroStep("key", "enter"),
        MacroStep("delay", 200),
        MacroStep("text", "€"),
    )
    representative_macros = [build_macro(macro_steps)]
    assert representative_macros[0] == "macro(hello space world enter 200ms €)"
    parsed_macro = parse_macro(representative_macros[0])
    assert build_macro(parsed_macro) == representative_macros[0]
    assert MacroStep("key", "enter") in parsed_macro
    assert MacroStep("delay", 200) in parsed_macro
    escaped_macro = build_macro((MacroStep("text", "close ) and \\"),))
    assert "\\)" in escaped_macro and "\\\\" in escaped_macro
    assert build_macro(parse_macro(escaped_macro)) == escaped_macro
    literal_space_word = build_macro((MacroStep("text", "space"),))
    assert literal_space_word == "macro(s pace)"
    assert build_macro(parse_macro(literal_space_word)) == literal_space_word
    combo_macro = build_macro((
        MacroStep("key", "C-S-tab"), MacroStep("key", "3+5"),
        MacroStep("delay", 0), MacroStep("delay", 1023),
    ))
    assert parse_macro(combo_macro) == (
        MacroStep("key", "C-S-tab"), MacroStep("key", "3+5"),
        MacroStep("delay", 0), MacroStep("delay", 1023),
    )
    assert parse_macro("macro(1024ms)") is None
    for malformed in (MacroStep("delay", 1024), MacroStep("text", ""), MacroStep("key", "C-")):
        try:
            build_macro((malformed,))
        except ValueError:
            pass
        else:
            raise AssertionError(f"invalid macro step was accepted: {malformed!r}")
    assert compose_monitor_chord(("leftcontrol", "leftshift"), "tab") == "C-S-tab"
    assert compose_monitor_chord(("rightalt", "leftmeta"), "left") == "M-G-left"
    assert compose_monitor_chord(("altgr", "rightshift"), "tab") == "S-G-tab"
    assert compose_monitor_chord(
        ("rightcontrol", "rightmeta", "leftalt", "rightshift"), "tab"
    ) == "C-M-A-S-tab"
    assert compose_monitor_chord(
        ("leftcontrol", "rightmeta", "rightalt", "leftshift"), "tab"
    ) == "C-M-S-G-tab"
    assert monitor_chords_equal("C-S-tab", "S-C-tab")
    assert monitor_chords_equal("M-G-left", "G-M-left")
    assert not monitor_chords_equal("C-S-tab", "C-tab")
    if keyd_bin():
        for action in representative_macros + [escaped_macro, literal_space_word, combo_macro]:
            assert check(render("[ids]\n*\n", [("x", action)])) is None, action
    assert b == [("left", "home"), ("right", "end")], b
    out = render(src, b + [("p", "sysrq")])
    assert parse(out) == b + [("p", "sysrq")] and out.startswith("# hdr\n[ids]\n0e6a:02c0") and "capslock = overload" in out, out
    assert parse(render("[ids]\n*\n", [("x", "y")])) == [("x", "y")]
    tap_src = "# keep\n[ids]\n*\n\n[main]\n# preserve this\ncapslock = overload(nav, capslock) # inline tap note\n\n[nav]\nx = left\n"
    assert read_tap(tap_src) == ("capslock", True, "")
    tap_changed = write_tap(tap_src, "esc")
    assert read_tap(tap_changed) == ("esc", True, "")
    assert "# keep\n" in tap_changed and "# preserve this\n" in tap_changed
    assert "# inline tap note" in tap_changed and "[nav]\nx = left\n" in tap_changed
    no_main_tap = write_tap("[ids]\n*\n", "backspace")
    assert read_tap(no_main_tap) == ("backspace", True, "")
    missing_line = write_tap("[main]\n# retained\n\n[nav]\nx = left\n", "esc")
    assert read_tap(missing_line) == ("esc", True, "") and "# retained" in missing_line
    assert missing_line.index("capslock = overload") < missing_line.index("[nav]")
    foreign_tap = "[main]\ncapslock = esc\n[nav]\nx = left\n"
    foreign_info = read_tap(foreign_tap)
    assert foreign_info[0] is None and not foreign_info[1] and "read-only" in foreign_info[2].casefold()
    try:
        write_tap(foreign_tap, "capslock")
    except ValueError as exc:
        assert "refusing to edit" in str(exc)
    else:
        raise AssertionError("foreign [main] capslock setting was editable")
    assert read_tap("[main]\n# no setting\n[nav]\n") == (
        None, True, "Tap action is not configured (missing [main] capslock setting).")
    highlights = keyboard_highlights([("q", "left"), ("volumedown", "esc")], "q")
    assert highlights["q"] == "selected" and highlights["w"] == "plain"
    assert keyboard_unshown_bindings([("q", "left"), ("volumedown", "esc")]) == ["volumedown"]
    risk_binds = [("q", "A-f4"), ("w", "C-q"), ("e", "C-w"), ("r", "command(test)"),
                  ("a", "a"), ("capslock", "esc"), ("b", "M-left")]
    risks = binding_warnings(risk_binds, "esc")
    assert any(key == "q" and level == "warn" and "close" in text for key, level, text in risks)
    assert any(key == "w" and "C-q" in text for key, _, text in risks)
    assert any(key == "e" and "C-w" in text for key, _, text in risks)
    assert any(key == "r" and "runs as root" in text for key, _, text in risks)
    assert any(key == "a" and "No-op" in text for key, _, text in risks)
    assert any(key == "capslock" and "tap sends esc" in text for key, _, text in risks)
    assert any(key == "b" and level == "info" for key, level, _ in risks)
    assert binding_warnings([("M-a", "left")], "capslock") == []
    assert binding_warnings([("left", "right"), ("right", "left")], "capslock") == []
    assert any(line.startswith("Warnings:") for line in warning_preview_lines(risk_binds, "esc"))
    backup_lines, _ = backup_preview(tap_src, tap_changed)
    assert any(line.startswith("Tap action differs:") for line in backup_lines)
    assert key_name("left") == "left" and key_name("P") == "p" and key_name(".") == "dot" and key_name("ш") is None
    assert "C-left" in [action for _, action, _ in filter_actions("ctrl+left")]
    assert {"C-left", "C-right"} <= {action for _, action, _ in filter_actions("j")}
    assert {action for _, action, _ in filter_actions("speaker volume")} == {"volumeup", "volumedown"}
    filtered_binds = [("volumeup", "C-equal"), ("x", "volumeup"), ("needle", "home")]
    assert filter_binding_indices(filtered_binds, "VoLuMe") == (0, 1)
    assert filter_binding_indices(filtered_binds, "zoom in") == (0,)
    assert filter_binding_indices(filtered_binds, "NEEDLE") == (2,)
    assert filter_binding_indices(filtered_binds, "missing") == ()
    assert filter_binding_indices(filtered_binds, "") == (0, 1, 2)
    assert filter_actions("not-a-catalog-entry") == ()
    with open(CONF) as f:
        shipped_keys = {key for key, _ in parse(f.read())}
    protected = {"left", "right", "p"}
    assert protected <= shipped_keys, f"expected shipped default bindings: {protected}"
    pack_keys = [key for pack in PACKS for key, _ in pack["binds"]]
    assert not protected.intersection(pack_keys), f"preset touches a protected default key: {protected.intersection(pack_keys)}"
    knob_packs = [pack for pack in PACKS if pack["name"].startswith("Knob: ")]
    plain_packs = [pack for pack in PACKS if not pack["name"].startswith("Knob: ")]
    plain_keys = [key for pack in plain_packs for key, _ in pack["binds"]]
    assert len(plain_keys) == len(set(plain_keys)), "preset packs collide on binding keys"
    knob_keys = {"volumedown", "volumeup", "mute"}
    for pack in knob_packs:
        keys = [key for key, _ in pack["binds"]]
        assert len(keys) == len(set(keys)), f"knob pack binds a key twice: {pack['name']}"
        assert set(keys) <= knob_keys, f"knob pack binds a non-knob key: {pack['name']}"
    assert not set(plain_keys).intersection(knob_keys), "knob packs collide with non-knob packs"
    exact_pack, exact_candidates = resolve_pack("Knob: tabs")
    assert exact_pack["name"] == "Knob: tabs" and exact_candidates == ()
    unique_pack, unique_candidates = resolve_pack("VIM")
    assert unique_pack["name"] == "Vim arrows" and unique_candidates == ()
    ambiguous_pack, ambiguous_candidates = resolve_pack("Knob")
    assert ambiguous_pack is None and {pack["name"] for pack in ambiguous_candidates} == {
        pack["name"] for pack in PACKS if pack["name"].startswith("Knob")}
    missing_pack, missing_candidates = resolve_pack("no-such-pack")
    assert missing_pack is None and missing_candidates == ()
    merged, added, replaced = merge_pack([("left", "home"), ("p", "sysrq")],
                                         [("left", "left"), ("h", "left")])
    assert merged == [("left", "left"), ("p", "sysrq"), ("h", "left")]
    assert added == [("h", "left")] and replaced == [("left", "home", "left")]
    assert upsert_binding([("x", "home"), ("y", "esc")], "x", "left") == [("y", "esc"), ("x", "left")]
    assert upsert_binding([("x", "home"), ("y", "esc"), ("z", "left")], "y", "up", 1) == [
        ("x", "home"), ("y", "up"), ("z", "left")]
    assert parse_monitor_line("DaKai Keyboard\t0e6a:02c0:1\tvolumedown down\n") == "volumedown"
    assert parse_monitor_line("DaKai Keyboard\t0e6a:02c0:1\tvolumedown up\n") is None
    assert parse_monitor_line("DaKai Keyboard\t0e6a:02c0:1\tcapslock down\n") is None
    assert parse_monitor_line("malformed event") is None
    assert parse_monitor_line("\t\tvolumedown down\n") is None
    assert parse_monitor_event("DaKai Keyboard\t0e6a:02c0:1\thome down\n") == (
        "DaKai Keyboard", "home", "down")
    assert parse_monitor_event("DaKai Keyboard\t0e6a:02c0:1\thome up\n") == (
        "DaKai Keyboard", "home", "up")
    assert parse_monitor_event("malformed event") is None
    old_backup = "default.conf.bak.2026-10-19-120000"
    new_backup = "default.conf.bak.2026-10-20-081530"
    assert parse_backup_timestamp(old_backup).day == 19
    assert sorted_backups([old_backup, "default.conf.bak.bad", new_backup]) == [new_backup, old_backup]
    assert diff_lines("same\n", "same\n") == []
    changed_diff = diff_lines("before\n", "after\n")
    assert any(line.startswith("-before") for line in changed_diff)
    assert any(line.startswith("+after") for line in changed_diff)

    history = History(limit=2)
    state0 = [("a", "home")]
    state1 = state0 + [("b", "left")]
    state2 = state1 + [("c", "up")]
    state3 = state2 + [("d", "down")]
    history.record(state0, state1, "add")
    history.record(state1, state2, "pack")
    assert history.undo(state2)[0] == state1
    assert history.redo(state1)[0] == state2
    history.undo(state2)
    history.record(state1, state3, "restore")
    assert history.redo(state1) is None
    bounded = History(limit=2)
    bounded.record([], state1, "one")
    bounded.record(state1, state2, "two")
    bounded.record(state2, state3, "three")
    assert bounded.undo(state3)[0] == state2
    assert bounded.undo(state2)[0] == state1
    assert bounded.undo(state1) is None
    before_editor = editor_state([("q", "left")], "capslock")
    after_editor = editor_state([("q", "A-f4")], "esc")
    editor_history = History()
    editor_history.record(before_editor, after_editor, "tap and binding")
    assert editor_history.undo(after_editor)[0] == before_editor
    assert editor_history.redo(before_editor)[0] == after_editor
    if keyd_bin():
        listed = subprocess.run([keyd_bin(), "list-keys"], capture_output=True, text=True)
        assert listed.returncode == 0, f"keyd list-keys failed: {listed.stderr.strip()}"
        keyd_names = {line.strip() for line in listed.stdout.splitlines() if line.strip()}
        map_names = {key for row in KEYBOARD_ROWS for key in row}
        assert map_names <= keyd_names, f"unknown keyboard map keyd names: {sorted(map_names - keyd_names)}"
        for pack in PACKS:
            assert check(render("[ids]\n*\n", pack["binds"])) is None, f"keyd rejects preset {pack['name']}"
        assert check(out) is None and check(render(src, [("left", "hoem")]))
        failures = [(action, check(render("[ids]\n*\n", [("x", action)])))
                    for _, action, _ in CATALOG]
        failures = [(action, error) for action, error in failures if error]
        assert not failures, f"keyd rejects catalog actions: {failures}"
    print("selftest ok" + ("" if keyd_bin() else " (keyd check skipped)"))
