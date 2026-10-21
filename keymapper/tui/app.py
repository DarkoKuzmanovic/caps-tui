"""Textual application for editing the keyd Caps Lock layer."""

import os
import queue
import threading
from pathlib import Path

from textual import events
from textual.app import App, Binding
from textual.widgets import DataTable, Input, Static

from .. import system
from ..config import (
    backup_preview, diff_lines, parse, read_tap, render_editor, tap_label as format_tap_label,
)
from ..data import CUSTOM
from ..rules import (
    History, binding_warnings, editor_state, filter_binding_indices, merge_pack,
    pack_summary, restore_editor_state, upsert_binding, warning_preview_lines,
)
from ..system import (
    backup_entries, check, keyd_etc_dir, monitor_command, read_live_config, run_apply,
)
from ..tui.screens import (
    ActionPickerScreen, BackupPickerScreen, CaptureScreen, ChoiceScreen, ConfirmScreen,
    EditorScreen, HelpScreen, KeyboardMapScreen, LiveScreen, MacroBuilderScreen,
    PackPickerScreen, PreviewScreen, TextEntryScreen, MACRO_BUILDER,
)
from .commands import KeymapCommands


class KeymapApp(App[None]):
    """The stateful editor. Config paths are injectable for isolated UI tests."""

    TITLE = "keyd · Caps Lock layer"
    CSS_PATH = Path(__file__).with_name("app.tcss")
    SCREENS = {"editor": EditorScreen}
    COMMANDS = App.COMMANDS | {KeymapCommands}
    BINDINGS = [
        Binding("a", "add_binding", "Add", show=True),
        Binding("e", "edit_binding", "Edit", show=True),
        Binding("d", "delete_binding", "Delete", show=True),
        Binding("/", "show_filter", "Find", show=True),
        Binding("l", "live_panel", "Live", show=True),
        Binding("p", "choose_pack", "Pack", show=True),
        Binding("b", "restore_backup", "Backup", show=True),
        Binding("t", "choose_tap", "Tap", show=True),
        Binding("m", "keyboard_map", "Map", show=True),
        Binding("s", "save_config", "Save", show=True),
        Binding("u", "undo", "Undo", show=True),
        Binding("ctrl+r", "redo", "Redo", show=True),
        Binding("?", "help", "Help", show=True),
        Binding("q", "quit_editor", "Quit", show=True),
        Binding("escape", "clear_filter", "Clear filter", show=False),
    ]

    def __init__(self, *, conf_path=None) -> None:
        super().__init__()
        self.conf_path = Path(conf_path or system.CONF)
        self.text = self.conf_path.read_text()
        self.binds = parse(self.text)
        self.tap, self.tap_editable, self.tap_message = read_tap(self.text)
        self.original_tap = self.tap
        self.saved = editor_state(self.binds, self.tap)
        self.history = History()
        self.filter_query = ""
        self.selected_index = 0
        self.row_indices: dict[str, int] = {}
        self.theme = "textual-dark"
        self.title = self.TITLE
        self.sub_title = ""

    @property
    def dirty(self) -> bool:
        return editor_state(self.binds, self.tap) != self.saved


    def on_mount(self) -> None:
        self.push_screen("editor")
        if not system.keyd_bin():
            self.notify("keyd not installed: config checks skipped until apply.",
                        severity="warning", timeout=6)
        self.call_after_refresh(self.refresh_editor)

    def filtered_indices(self) -> tuple[int, ...]:
        return filter_binding_indices(self.binds, self.filter_query)

    def warnings_by_key(self) -> dict[str, list[tuple[str, str]]]:
        warnings: dict[str, list[tuple[str, str]]] = {}
        for key, level, message in binding_warnings(self.binds, self.tap):
            warnings.setdefault(key.casefold(), []).append((level, message))
        return warnings

    def tap_label(self) -> str:
        return format_tap_label(self.tap, self.tap_editable, self.tap_message)

    def refresh_editor(self) -> None:
        try:
            screen = self.get_screen("editor")
        except Exception:
            return
        if screen.is_mounted:
            screen.refresh_table()

    def notify_status(self, message: str, *, severity: str | None = None) -> None:
        if not message:
            return
        if severity is None:
            lower = message.casefold()
            severity = ("error" if lower.startswith(("not saved", "keyd rejects", "apply failed", "error:",
                                                        "cannot read backup")) else
                        "warning" if lower.startswith(("warning:", "info:", "cancelled", "apply cancelled",
                                                         "capture timed out", "keyd monitor unavailable",
                                                         "no matching", "no keyd backups")) else
                        "information")
        self.notify(message, severity=severity, timeout=5)

    def record_change(self, before, description: str) -> bool:
        changed = self.history.record(before, editor_state(self.binds, self.tap), description)
        if changed:
            self.refresh_editor()
        return changed

    def current_selection(self) -> tuple[int | None, tuple[str, str] | None]:
        index = self.selected_index
        if 0 <= index < len(self.binds):
            return index, self.binds[index]
        return None, None

    def action_show_filter(self) -> None:
        self.get_screen("editor").action_show_filter()

    def action_clear_filter(self) -> None:
        self.get_screen("editor").action_clear_filter()

    def action_add_binding(self) -> None:
        self.run_worker(self.edit_binding(None), exclusive=True, name="add-binding")

    def action_edit_binding(self) -> None:
        index, _ = self.current_selection()
        if index is not None:
            self.run_worker(self.edit_binding(index), exclusive=True, name="edit-binding")

    async def edit_binding(self, index: int | None, *, key: str | None = None) -> None:
        old = self.binds[index] if index is not None and 0 <= index < len(self.binds) else None
        if key is None:
            if old:
                key = old[0]
            else:
                command, needs_sudo = self._capture_command()
                if needs_sudo and command:
                    with self.suspend():
                        if not system.authenticate_monitor():
                            command = []
                field_timeout = self._capture_timeout()
                key, capture_message = await self.push_screen_wait(CaptureScreen(command, field_timeout))
                if not key:
                    self.notify_status(capture_message or "Cancelled.")
                    return
        current_action = old[1] if old else ""
        if old and old[1].startswith("macro("):
            steps = parse_macro(old[1])
            if steps is not None:
                action = await self.push_screen_wait(MacroBuilderScreen(steps))
            else:
                action = await self.push_screen_wait(ActionPickerScreen(current_action))
        else:
            action = await self.push_screen_wait(ActionPickerScreen(current_action))
        if action is None:
            self.notify_status("Cancelled.")
            return
        if action == MACRO_BUILDER:
            action = await self.push_screen_wait(MacroBuilderScreen())
        elif action == CUSTOM:
            action = await self.push_screen_wait(TextEntryScreen(
                f"Caps + {key} sends · keyd action", current_action,
                placeholder="e.g. home, C-left, macro(hello)"))
        if not action:
            self.notify_status("Cancelled.")
            return
        error = check(f"[ids]\n*\n[nav]\nx = {action}\n")
        if error:
            self.notify_status("keyd rejects that: " + error.splitlines()[-1].strip(), severity="error")
            return
        before = editor_state(self.binds, self.tap)
        self.binds[:] = upsert_binding(self.binds, key.casefold(), action, index)
        self.selected_index = next((i for i, (bound, _) in enumerate(self.binds)
                                    if bound.casefold() == key.casefold()), max(0, len(self.binds) - 1))
        description = "Added" if old is None else "Updated"
        self.record_change(before, f"{description} Caps + {key}")
        self.notify_status(f"{description} Caps + {key} → {action}.")

    def _capture_timeout(self) -> float:
        try:
            return float(os.environ.get("KEYMAP_MONITOR_TIMEOUT", "10"))
        except ValueError:
            return 10

    def _capture_command(self):
        try:
            return monitor_command()
        except ValueError as exc:
            self.notify_status(f"keyd monitor unavailable: {exc}", severity="warning")
            return [], False

    def start_capture_worker(self, screen: CaptureScreen):
        return queue.Queue(), threading.Event()

    def action_delete_binding(self) -> None:
        index, old = self.current_selection()
        if index is None or old is None:
            self.notify_status("No binding selected.", severity="warning")
            return
        before = editor_state(self.binds, self.tap)
        key, action = self.binds.pop(index)
        self.selected_index = min(index, max(0, len(self.binds) - 1))
        self.record_change(before, f"Deleted Caps + {key}")
        self.notify_status(f"Deleted Caps + {key} → {action}.")

    def action_undo(self) -> None:
        result = self.history.undo(editor_state(self.binds, self.tap))
        if result:
            state, description = result
            self.tap = restore_editor_state(state, self.binds)
            self.selected_index = min(self.selected_index, max(0, len(self.binds) - 1))
            self.refresh_editor()
            self.notify_status(f"Undid {description}.")
        else:
            self.notify_status("Nothing to undo.", severity="warning")

    def action_redo(self) -> None:
        result = self.history.redo(editor_state(self.binds, self.tap))
        if result:
            state, description = result
            self.tap = restore_editor_state(state, self.binds)
            self.selected_index = min(self.selected_index, max(0, len(self.binds) - 1))
            self.refresh_editor()
            self.notify_status(f"Redid {description}.")
        else:
            self.notify_status("Nothing to redo.", severity="warning")

    def action_choose_pack(self) -> None:
        self.run_worker(self.choose_pack(), exclusive=True, name="choose-pack")

    async def choose_pack(self) -> None:
        pack = await self.push_screen_wait(PackPickerScreen())
        if pack:
            await self.apply_pack(pack)

    async def apply_pack(self, pack: dict) -> None:
        _, added, replaced = merge_pack(self.binds, pack["binds"])
        confirmed = await self.push_screen_wait(PreviewScreen(
            f"Preview pack: {pack['name']}", pack_summary(pack, added, replaced, self.tap),
            confirm_label="Apply pack", cancel_label="Cancel"))
        if not confirmed:
            self.notify_status("Pack cancelled.", severity="warning")
            return
        before = editor_state(self.binds, self.tap)
        self.binds[:], added, replaced = merge_pack(self.binds, pack["binds"])
        self.record_change(before, f"Applied {pack['name']} pack")
        self.notify_status(f"Applied {pack['name']} pack: {len(added)} added, {len(replaced)} replaced.")

    async def apply_tap_option(self, tap: str) -> None:
        if not self.tap_editable:
            self.notify_status(self.tap_message, severity="warning")
            return
        if tap == CUSTOM:
            tap = await self.push_screen_wait(TextEntryScreen("Custom tap action", self.tap or ""))
        if not tap:
            return
        try:
            candidate = render_editor(self.text, self.binds, tap, self.original_tap)
        except ValueError as exc:
            self.notify_status(str(exc), severity="error")
            return
        error = check(candidate)
        if error:
            self.notify_status("Not saved, keyd says: " + error.splitlines()[-1].strip(), severity="error")
            return
        if tap == self.tap:
            self.notify_status("Tap action unchanged.")
            return
        before = editor_state(self.binds, self.tap)
        self.tap = tap
        self.record_change(before, "changed tap action")
        self.notify_status(f"Tap action set to {self.tap_label()}.")

    def action_restore_backup(self) -> None:
        self.run_worker(self.restore_backup(), exclusive=True, name="restore-backup")

    async def restore_backup(self) -> None:
        entries = backup_entries()
        if not entries:
            self.notify_status(f"No keyd backups found in {keyd_etc_dir()}.", severity="warning")
            return
        entry = await self.push_screen_wait(BackupPickerScreen(entries))
        if not entry:
            self.notify_status("Backup restore cancelled.")
            return
        path, stamp, _ = entry
        try:
            backup_text = Path(path).read_text()
        except OSError as exc:
            self.notify_status(f"Cannot read backup: {exc}", severity="error")
            return
        current = render_editor(self.text, self.binds, self.tap, self.original_tap)
        lines, _ = backup_preview(current, backup_text)
        confirmed = await self.push_screen_wait(PreviewScreen(
            f"Backup {stamp:%Y-%m-%d %H:%M:%S}", lines,
            confirm_label="Load [nav]", cancel_label="Back"))
        if not confirmed:
            self.notify_status("Backup restore cancelled.")
            return
        before = editor_state(self.binds, self.tap)
        self.binds[:] = parse(backup_text)
        self.selected_index = max(0, len(self.binds) - 1)
        self.record_change(before, "restored backup [nav] bindings")
        self.notify_status(f"Loaded backup [nav] bindings ({len(self.binds)} bindings); other sections unchanged.")

    def action_choose_tap(self) -> None:
        self.run_worker(self.choose_tap(), exclusive=True, name="choose-tap")

    async def choose_tap(self) -> None:
        if not self.tap_editable:
            self.notify_status(self.tap_message, severity="warning")
            return
        options = [("capslock", "Caps Lock"), ("esc", "Esc"), ("backspace", "Backspace"),
                   (CUSTOM, "Custom…")]
        selected = self.tap if self.tap in {"capslock", "esc", "backspace"} else CUSTOM
        tap = await self.push_screen_wait(ChoiceScreen("Caps Lock tap action", options, selected))
        if tap is not None:
            await self.apply_tap_option(tap)
    def action_keyboard_map(self) -> None:
        selected = self.binds[self.selected_index][0] if self.binds and self.selected_index < len(self.binds) else None
        self.push_screen(KeyboardMapScreen(selected))

    def map_key_selected(self, key: str) -> None:
        index = next((i for i, (bound, _) in enumerate(self.binds)
                      if bound.casefold() == key.casefold()), None)
        self.selected_index = index if index is not None else self.selected_index
        self.run_worker(self.edit_binding(index, key=key), exclusive=True)

    def live_config_state(self) -> str:
        try:
            editor_text = render_editor(self.text, self.binds, self.tap, self.original_tap)
        except ValueError:
            return "unavailable"
        live_text, fallback = read_live_config(editor_text)
        if fallback:
            return "unreadable"
        return "matches" if live_text == editor_text else "differs"

    def live_config_message(self) -> str:
        return {
            "matches": "Live config matches the editor",
            "differs": "Live config differs from the editor",
            "unreadable": "Live config unreadable",
            "unavailable": "Live config unavailable",
        }[self.live_config_state()]

    def action_live_panel(self) -> None:
        self.run_worker(self.live_panel(), exclusive=True, name="live-panel")

    async def live_panel(self) -> None:
        state_message = self.live_config_message()
        command, needs_sudo = self._capture_command()
        message = ""
        if needs_sudo and command:
            with self.suspend():
                if not system.authenticate_monitor():
                    command = []
                    message = "keyd monitor unavailable: sudo authentication failed."
        if not command:
            message = message or ("keyd monitor unavailable: keyd is not installed."
                                  if not system.keyd_bin() else "keyd monitor unavailable.")
        await self.push_screen_wait(LiveScreen(command, state_message, message))

    def start_live_worker(self, screen: LiveScreen):
        cancel = threading.Event()
        if not screen.command:
            screen.message = screen.message or "keyd monitor unavailable: keyd is not installed."
            return cancel
        from functools import partial
        screen.run_worker(partial(self._stream_monitor, screen, cancel), thread=True,
                          exclusive=True, name="keyd-monitor")
        return cancel

    def _stream_monitor(self, screen: LiveScreen, cancel: threading.Event) -> None:
        try:
            stream = system.MonitorStream(screen.command)
        except (OSError, ValueError) as exc:
            self.call_from_thread(screen.monitor_stopped, 1)
            self.call_from_thread(setattr, screen, "message", f"keyd monitor unavailable: {exc}")
            return
        try:
            while not cancel.is_set():
                lines, code = stream.poll()
                for line in lines:
                    self.call_from_thread(screen.receive_line, line)
                if code is not None:
                    self.call_from_thread(screen.monitor_stopped, code)
                    break
                cancel.wait(0.03)
        finally:
            stream.close()

    def action_save_config(self) -> None:
        self.run_worker(self.save_config(), exclusive=True, name="save-config")

    async def save_config(self) -> None:
        try:
            new = render_editor(self.text, self.binds, self.tap, self.original_tap)
        except ValueError as exc:
            self.notify_status(str(exc), severity="error")
            return
        error = check(new)
        if error:
            self.notify_status("Not saved, keyd says: " + error.splitlines()[-1].strip(), severity="error")
            return
        live_path = os.path.join(keyd_etc_dir(), "default.conf")
        live, fallback = read_live_config(self.text)
        changes = diff_lines(live, new, f"live {live_path}", f"repo {self.conf_path}")
        if not changes:
            self._save_local(new)
            warnings = warning_preview_lines(self.binds, self.tap)
            if warnings:
                severity = "warning" if any(level == "warn" for _, level, _ in binding_warnings(self.binds, self.tap)) else "information"
                self.notify_status("; ".join(warnings[1:]), severity=severity)
            else:
                self.notify_status("No difference from live config; saved locally, skipped sudo." if not fallback
                                   else "No difference from last-saved repo text (live unreadable); saved locally, skipped sudo.")
            return
        warnings = warning_preview_lines(self.binds, self.tap)
        if warnings:
            changes = warnings + [""] + changes
        if fallback:
            notice_index = len(warnings) + 1 if warnings else 0
            changes.insert(notice_index, "Live config unreadable; diff uses last-saved repo text.")
        confirmed = await self.push_screen_wait(PreviewScreen(
            "Review config diff", changes, confirm_label="Apply", cancel_label="Cancel"))
        if not confirmed:
            self.notify_status("Apply cancelled; config remains unsaved.", severity="warning")
            return
        with self.suspend():
            rc = run_apply(new, conf_path=self.conf_path)
            input(f"\n{'Applied.' if rc == 0 else f'Apply failed (exit {rc}); config saved but not applied.'} Enter to return... ")
        self._mark_saved(new)
        self.notify_status("Applied — test it now." if rc == 0 else
                           "Apply failed; see output above (saved locally).",
                           severity="information" if rc == 0 else "error")

    def _save_local(self, new: str) -> None:
        try:
            self.conf_path.write_text(new)
        except OSError as exc:
            self.notify_status(f"Not saved: {exc}", severity="error")
            return
        self._mark_saved(new)

    def _mark_saved(self, new: str) -> None:
        self.text = new
        self.original_tap = self.tap
        self.tap, self.tap_editable, self.tap_message = read_tap(new)
        self.saved = editor_state(self.binds, self.tap)
        self.refresh_editor()

    def action_help(self) -> None:
        self.push_screen(HelpScreen())

    def help_lines(self) -> list[str]:
        return [
            "a — add a binding; press the key alone without Caps, then choose an action",
            "l — stream live keyd monitor output; this tests the active config, never applies edits",
            "The live panel warns when editor changes are unsaved or differ from the live config",
            "Monitor output for grabbed devices is keyd's virtual output after remapping",
            "Esc / Ctrl+G closes the live panel; unrelated terminal key input is ignored",
            "Capture ignores Caps Lock events; Ctrl+T types a name, Ctrl+G cancels; monitor timeout is configurable",
            "e / Enter — edit the selected binding's action; d — delete it",
            "/ — incrementally filter binding key, action, or description (case-insensitive)",
            "Filter Enter keeps the query; Esc clears it; edits, deletes, and undo use real rows",
            "t — choose Caps Lock, Esc, Backspace, or a custom [main] tap action",
            "Foreign [main] capslock settings are shown read-only; tap edits are refused",
            "p — preview a preset pack; warnings are shown for risky new bindings",
            "b — preview a timestamped backup; confirm loads only its [nav] bindings (undoable)",
            "u — undo the last editor change; Ctrl+R — redo (bindings and tap action)",
            "s — validate and preview the live diff; warnings appear before the diff",
            "Identical live config is saved locally without sudo; active keyd uses reload",
            "Inactive/missing keyd falls back to the full install; backups are made only if changed",
            "q — quit; unsaved binding or tap changes require confirmation",
            "l — live keyd output; modifier key events compose into chords per device (C/M/A/S/G order)",
            "m — keyboard map; inline at 80×30+, map screen at any size; click a key to edit/add",
            "Bound keys are highlighted; F-row, knob, and other non-block bindings are listed separately",
            "↑ / k — select previous; ↓ / j — next; Page Up / Page Down — scroll",
            "? — open this help screen; Ctrl+P — command palette",
            "Action picker: type to filter; arrows/PgUp/PgDn/Home/End move; Enter chooses; Esc cancels",
            "Macro builder… is beside Custom…; existing parseable macros open directly in the builder",
            "Builder steps: text, key/combo, or delay (0–1023ms); reorder and insert symbols; Save validates",
            "Macro tokens are whitespace-separated; `space` emits a literal space",
            "Example: macro(hello space world enter 200ms €); `s pace` writes 'space'",
            "Unicode: /usr/share/keyd/keyd.compose via ~/.XCompose or system Compose",
            "Default US display layout is required for keyd Unicode output; apps may need restarting",
            "Custom… accepts any keyd action, including raw macro(...); packs keep unrelated bindings",
        ]

    def action_quit_editor(self) -> None:
        self.run_worker(self.quit_editor(), exclusive=True, name="quit-confirm")

    async def quit_editor(self) -> None:
        if not self.dirty:
            self.exit()
            return
        quit_anyway = await self.push_screen_wait(ConfirmScreen(
            "Unsaved changes. Quit without saving?", confirm_label="Quit anyway"))
        if quit_anyway:
            self.exit()

    def on_key(self, event: events.Key) -> None:
        if event.key == "escape" and self.filter_query:
            self.action_clear_filter()

    async def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.data_table.id == "bindings":
            row_key = str(event.row_key.value)
            if row_key in self.row_indices:
                self.selected_index = self.row_indices[row_key]



# Exposed for the headless runner and screenshot helper.

def main(*, conf_path=None) -> None:
    KeymapApp(conf_path=conf_path).run()
