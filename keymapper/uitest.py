"""Headless integration flows for the Textual editor (run with ``--uitest``)."""

import asyncio
import os
import sys
import tempfile
from pathlib import Path
from unittest.mock import patch

from textual.widgets import DataTable, Input, RichLog

from .data import PACKS
from .macro import MacroStep
from .rules import merge_pack, pack_summary
from .tui.app import KeymapApp
from .tui.screens import (
    ActionPickerScreen, BackupPickerScreen, CaptureScreen, ConfirmScreen, KeymapFooter,
    KeyboardMapScreen, LiveScreen, MacroBuilderScreen, PackPickerScreen, PreviewScreen,
    TextEntryScreen,
)


async def _wait_for_screen(app, pilot, screen_type, *, steps=40):
    for _ in range(steps):
        await pilot.pause(0.05)
        if isinstance(app.screen, screen_type):
            return app.screen
    raise AssertionError(f"did not reach {screen_type.__name__}; current={type(app.screen).__name__}")


async def _footer_text(app, pilot):
    footer = app.screen.query_one(KeymapFooter)
    text = ""
    for _ in range(20):
        await pilot.pause(0.05)
        text = " ".join(child.render().plain for child in footer.children)
        if "a Add" in text:
            return footer, text
    return footer, text


async def _useful_footer(app, pilot):
    footer, text = await _footer_text(app, pilot)
    assert "a Add" in text and "^p palette" in text, \
        f"core footer bindings missing: {text!r}"
    return footer, text


async def _add_typed(pilot, app, key: str, action: str) -> None:
    await pilot.press("a")
    await _wait_for_screen(app, pilot, CaptureScreen)
    await pilot.press("ctrl+t")
    field = app.screen.query_one("#typed-key", Input)
    field.value = key
    await pilot.press("enter")
    await _wait_for_screen(app, pilot, ActionPickerScreen)
    field = app.screen.query_one("#action-filter", Input)
    field.value = action
    await pilot.press("tab")
    await pilot.press("enter")
    await pilot.pause(0.1)


async def _run_flows(config_path: Path, etc_path: Path, monitor_path: Path) -> None:
    fake_command = f"{sys.executable} {monitor_path} capture"
    live_command = f"{sys.executable} {monitor_path} live"
    with patch.dict(os.environ, {
        "KEYMAP_ETC": str(etc_path),
        "KEYMAP_MONITOR_CMD": fake_command,
        "KEYMAP_MONITOR_TIMEOUT": "5",
    }):
        app = KeymapApp(conf_path=config_path)
        async with app.run_test(size=(100, 32)) as pilot:
            footer, footer_text = await _useful_footer(app, pilot)
            for token in ("e Edit", "d Delete", "/ Find", "s Save", "u Undo",
                          "? Help", "q Quit"):
                assert token in footer_text, f"core footer binding missing: {token!r}; {footer_text!r}"
            assert "^p palette" in footer_text
            assert "Live" not in footer_text and "Pack" not in footer_text, \
                "narrow footer should reserve space for core keys"
            assert app.live_config_state() == "matches"
            assert "live config: matches" in app.screen.query_one("#tap-status").render().plain
            initial = config_path.read_text()
            await pilot.press("s")
            await pilot.pause(0.2)
            assert config_path.read_text() == initial, "no-diff save must preserve config without sudo"

            await _add_typed(pilot, app, "n", "home")
            assert ("n", "home") in app.binds, "typed key capture plus action picker adds binding"

            await pilot.press("enter")
            await _wait_for_screen(app, pilot, ActionPickerScreen)
            field = app.screen.query_one("#action-filter", Input)
            field.value = "end"
            await pilot.press("enter")
            await pilot.pause(0.1)
            assert ("n", "end") in app.binds, "edit changes selected binding action"

            await pilot.press("d")
            await pilot.pause(0.1)
            assert not any(key == "n" for key, _ in app.binds), "delete removes selected real binding"
            await pilot.press("u")
            await pilot.pause(0.1)
            assert ("n", "end") in app.binds, "undo restores deletion"
            await pilot.press("ctrl+r")
            await pilot.pause(0.1)
            assert not any(key == "n" for key, _ in app.binds), "redo reapplies deletion"
            await pilot.press("u")
            await pilot.pause(0.1)
            assert ("n", "end") in app.binds, "undo can restore it again"

            await pilot.press("/")
            await pilot.press(*list("volumedown"))
            await pilot.pause(0.1)
            assert app.filter_query.casefold() == "volumedown"
            assert app.row_indices and all(app.binds[index][0] == "volumedown"
                                          for index in app.row_indices.values()), \
                "filtered rows must map back to their actual binding indexes"
            await pilot.press("escape")
            await pilot.pause(0.1)
            assert app.filter_query == "", "Escape clears binding search"

            await pilot.press("p")
            pack_screen = await _wait_for_screen(app, pilot, PackPickerScreen)
            for _ in range(4):
                await pilot.press("down")
            await pilot.press("enter")
            preview = await _wait_for_screen(app, pilot, PreviewScreen)
            assert "Knob: zoom" in preview.title_text
            before_pack = list(app.binds)
            await pilot.click("#cancel")
            await pilot.pause(0.1)
            assert app.binds == before_pack, "cancelling pack preview leaves bindings unchanged"

            await pilot.press("p")
            await _wait_for_screen(app, pilot, PackPickerScreen)
            for _ in range(4):
                await pilot.press("down")
            await pilot.press("enter")
            await _wait_for_screen(app, pilot, PreviewScreen)
            await pilot.click("#confirm")
            await pilot.pause(0.1)
            assert ("volumedown", "C-minus") in app.binds, "pack confirmation merges bindings"
            assert ("volumeup", "C-equal") in app.binds

            await pilot.press("t")
            await pilot.press("down")
            await pilot.press("enter")
            await pilot.pause(0.1)
            assert app.tap == "esc", "tap picker changes and validates tap action"

            backup_config = initial.replace("[nav]" + chr(10), "[nav]" + chr(10) + "b = esc" + chr(10), 1)
            (etc_path / "default.conf.bak.2026-10-20-081530").write_text(backup_config)
            await pilot.press("b")
            await _wait_for_screen(app, pilot, BackupPickerScreen)
            await pilot.press("enter")
            backup_preview = await _wait_for_screen(app, pilot, PreviewScreen)
            assert "Backup" in backup_preview.title_text
            await pilot.click("#confirm")
            await pilot.pause(0.1)
            assert ("b", "esc") in app.binds, "backup restore loads [nav] bindings"
            assert app.tap == "esc", "backup restore leaves the current tap action untouched"

            await _add_typed(pilot, app, "m", "home")
            # Replace the catalog action with a newly-built macro through the action picker.
            await pilot.press("e")
            await _wait_for_screen(app, pilot, ActionPickerScreen)
            await pilot.click("#macro")
            builder = await _wait_for_screen(app, pilot, MacroBuilderScreen)
            await pilot.click("#text")
            entry = await _wait_for_screen(app, pilot, TextEntryScreen)
            entry.query_one("#entry", Input).value = "hello"
            await pilot.press("enter")
            await _wait_for_screen(app, pilot, MacroBuilderScreen)
            await pilot.click("#save")
            await pilot.pause(0.2)
            assert any(key == "m" and action == "macro(hello)" for key, action in app.binds), \
                "macro builder builds and saves a validated action"

            with patch.dict(os.environ, {"KEYMAP_MONITOR_CMD": live_command}):
                await pilot.press("l")
                live = await _wait_for_screen(app, pilot, LiveScreen)
                await pilot.pause(0.35)
                assert any(event["key"] == "C-minus" for event in live.events), \
                    "live panel composes leftcontrol+minus into C-minus"
                assert "C-minus · Keyboard  ← Caps + volumedown" in live.rendered_lines, \
                    "chord is one line with its Caps binding annotation"
                assert not any(line.startswith("leftcontrol") for line in live.rendered_lines), \
                    "a modifier consumed by a chord is not shown separately"
                await pilot.press("escape")
                await pilot.pause(0.1)

            await pilot.press("s")
            review = await _wait_for_screen(app, pilot, PreviewScreen)
            assert review.title_text == "Review config diff"
            assert any(line.startswith("+") or line.startswith("-") for line in review.lines), \
                "diff review shows candidate changes"
            assert "".join(review.rendered_lines).strip(), "diff lines render in the review panel"
            await pilot.click("#cancel")
            await pilot.pause(0.1)
            assert app.dirty, "cancelling diff review leaves edits unsaved"

            # Check the command palette exposes the requested named pack command.
            await pilot.press("ctrl+p")
            await pilot.pause(0.2)
            palette = app.screen
            assert "CommandPalette" in type(palette).__name__, "Ctrl+P opens Textual's command palette"
            palette.query_one("Input", Input).focus()
            await pilot.press(*list("Knob: zoom"))
            await pilot.pause(0.5)
            command_list = palette.query_one("CommandList")
            commands = [str(command_list.get_option_at_index(index).prompt)
                        for index in range(command_list.option_count)]
            assert any(command.startswith("Apply pack: Knob: zoom") for command in commands), \
                f"palette offer is missing: {commands!r}"
            await pilot.press("escape")
            await pilot.pause(0.1)
            # The map is interactive: clicking a key cap selects its binding.
            before_map_edit = list(app.binds)
            await pilot.press("m")
            await _wait_for_screen(app, pilot, KeyboardMapScreen)
            await pilot.click("#keycap-p")
            await _wait_for_screen(app, pilot, ActionPickerScreen)
            await pilot.press("escape")
            await pilot.pause(0.1)
            assert app.binds == before_map_edit, "cancelling a map key edit leaves bindings unchanged"


            await pilot.press("q")
            confirm = await _wait_for_screen(app, pilot, ConfirmScreen)
            assert "Unsaved changes" in confirm.message
            await pilot.click("#stay")
            await pilot.pause(0.1)
            assert app.is_running, "stay keeps the editor open"
            await pilot.press("q")
            await _wait_for_screen(app, pilot, ConfirmScreen)
            await pilot.press("y")
            await pilot.pause(0.1)


async def _screenshots(
    config_path: Path, etc_path: Path, monitor_path: Path,
) -> dict[str, str]:
    paths = {}
    with patch.dict(os.environ, {"KEYMAP_ETC": str(etc_path)}):
        app = KeymapApp(conf_path=config_path)
        async with app.run_test(size=(100, 32)) as pilot:
            await pilot.pause(0.2)
            paths["main-100x32"] = app.save_screenshot(
                filename="keymap-main-100x32.svg", path="/tmp")
            app.push_screen(KeyboardMapScreen("left"))
            await pilot.pause(0.2)
            dialog = app.screen.query_one(".map-dialog")
            assert dialog.region.x > 0 and dialog.region.width < app.size.width, \
                f"keyboard map modal must fit within the screen: {dialog.region}"
            rows = app.screen.query(".keyboard-row")
            assert len(rows) == 5 and {row.region.width for row in rows} == {75}, \
                "ISO keyboard rows share the 15-unit panel width"
            assert "C-minus" in app.screen.query_one("#keycap-volumedown").render().plain
            assert not app.screen.query(".cluster-legend"), "knob actions render only inside keycaps"
            paths["map-100x32"] = app.save_screenshot(
                filename="keymap-map-100x32.svg", path="/tmp")
            await pilot.press("escape")
            await pilot.pause(0.1)

            app.push_screen(ActionPickerScreen(current="home"))
            await pilot.pause(0.2)
            paths["action-picker-100x32"] = app.save_screenshot(
                filename="keymap-action-picker-100x32.svg", path="/tmp")
            await pilot.press("escape")
            await pilot.pause(0.1)

            pack = next(item for item in PACKS if item["name"] == "Knob: tabs")
            _, added, replaced = merge_pack(app.binds, pack["binds"])
            app.push_screen(PreviewScreen(
                f"Preview pack: {pack['name']}",
                pack_summary(pack, added, replaced, app.tap),
                confirm_label="Apply pack", cancel_label="Cancel"))
            await pilot.pause(0.2)
            paths["pack-preview-100x32"] = app.save_screenshot(
                filename="keymap-pack-preview-100x32.svg", path="/tmp")
            await pilot.press("escape")
            await pilot.pause(0.1)

            app.push_screen(MacroBuilderScreen([
                MacroStep("text", "hello"), MacroStep("key", "enter"), MacroStep("delay", 200),
            ]))
            await pilot.pause(0.2)
            paths["macro-builder-100x32"] = app.save_screenshot(
                filename="keymap-macro-builder-100x32.svg", path="/tmp")
            await pilot.press("escape")
            await pilot.pause(0.1)

            live = LiveScreen(
                [sys.executable, str(monitor_path), "live"],
                app.live_config_message(), "")
            app.push_screen(live)
            for _ in range(30):
                await pilot.pause(0.05)
                if any("C-minus" in line for line in live.rendered_lines):
                    break
            assert "C-minus · Keyboard  ← Caps + volumedown" in live.rendered_lines
            assert not any(line.startswith("leftcontrol") for line in live.rendered_lines)
            assert live.state_message == "Live config matches the editor"
            paths["live-100x32"] = app.save_screenshot(
                filename="keymap-live-100x32.svg", path="/tmp")
            await pilot.press("escape")
            await pilot.pause(0.1)

        for width, height in ((80, 24), (140, 40)):
            app = KeymapApp(conf_path=config_path)
            async with app.run_test(size=(width, height)) as pilot:
                footer, footer_text = await _footer_text(app, pilot)
                if width < 135:
                    assert "a Add" in footer_text and "^p palette" in footer_text, \
                        f"{width} {footer.size} {footer_text!r}"
                    assert "Live" not in footer_text and "Pack" not in footer_text
                else:
                    assert "l Live" in footer_text and "p Pack" in footer_text, \
                        f"{width} {footer.size} {footer_text!r}"
                name = f"main-{width}x{height}"
                paths[name] = app.save_screenshot(
                    filename=f"keymap-{name}.svg", path="/tmp")
    return paths


def run_uitests() -> int:
    """Run headless flows and save the requested SVG review screenshots."""
    source = Path(__file__).resolve().parent.parent / "default.conf"
    with tempfile.TemporaryDirectory(prefix="keymap-uitest-") as temporary:
        root = Path(temporary)
        config_path = root / "default.conf"
        config_path.write_text(source.read_text())
        etc_path = root / "keyd"
        etc_path.mkdir()
        (etc_path / "default.conf").write_text(source.read_text())
        monitor_path = root / "fake-monitor.py"
        monitor_path.write_text(
            "import sys, time\n"
            "mode = sys.argv[1]\n"
            "if mode == 'capture':\n"
            "    time.sleep(1.2)\n"
            "    print('Keyboard\\t0e6a:02c0:1\\tq down', flush=True)\n"
            "else:\n"
            "    rows = [\n"
            "        'Keyboard\\t0e6a:02c0:1\\tleftcontrol down',\n"
            "        'Keyboard\\t0e6a:02c0:1\\tminus down',\n"
            "        'Keyboard\\t0e6a:02c0:1\\tminus up',\n"
            "        'Keyboard\\t0e6a:02c0:1\\tleftcontrol up',\n"
            "        'Keyboard\\t0e6a:02c0:1\\tvolumedown down',\n"
            "        'Keyboard\\t0e6a:02c0:1\\tvolumedown up',\n"
            "    ]\n"
            "    for row in rows:\n"
            "        print(row, flush=True)\n"
            "        time.sleep(0.04)\n"
            "    time.sleep(0.3)\n")
        # Render review images with the app's dark color theme even when the
        # caller runs tests under a non-color environment (e.g. NO_COLOR=1).
        no_color = os.environ.pop("NO_COLOR", None)
        try:
            with patch.dict(os.environ, {
                "TERM": "xterm-256color", "COLORTERM": "truecolor",
            }):
                screenshots = asyncio.run(_screenshots(config_path, etc_path, monitor_path))
        finally:
            if no_color is not None:
                os.environ["NO_COLOR"] = no_color
        asyncio.run(_run_flows(config_path, etc_path, monitor_path))
    print("uitest ok")
    for name in ("main-100x32", "map-100x32", "main-80x24", "main-140x40",
                 "action-picker-100x32", "live-100x32", "pack-preview-100x32",
                 "macro-builder-100x32"):
        print(f"{name} screenshot: {screenshots[name]}")
    return 0
