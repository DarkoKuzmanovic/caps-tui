"""Textual screens, pickers, previews, and interactive keyboard widgets."""

import filecmp
import glob
import os

from rich.text import Text
from textual import events
from textual.app import ComposeResult
from textual.containers import Container, Horizontal, Vertical, VerticalScroll
from textual.screen import ModalScreen, Screen
from textual.widgets import Button, DataTable, Footer, Input, Label, RichLog, Static
from textual.widgets._footer import FooterKey
from textual.message import Message
from ..data import ACTION_DESCRIPTIONS, CATALOG, CUSTOM, KEYBOARD_ROWS, PACKS
from ..macro import MacroStep, build_macro, parse_macro
from ..rules import filter_actions, merge_pack, pack_summary
from ..system import check, keyd_bin
from ..keys import key_name

MACRO_BUILDER = "__macro_builder__"

KEY_LABELS = {
    "esc": "Esc", "grave": "`", "minus": "-", "equal": "=", "backspace": "⌫",
    "up": "↑", "down": "↓", "left": "←", "right": "→",
    "tab": "Tab", "leftbrace": "[", "rightbrace": "]", "backslash": "\\",
    "capslock": "Caps", "leftshift": "⇧", "rightshift": "⇧", "enter": "Enter",
    "semicolon": ";", "apostrophe": "'", "comma": ",", "dot": ".", "slash": "/",
    "102nd": "<>", "leftcontrol": "Ctrl", "rightcontrol": "Ctrl",
    "leftmeta": "❖", "rightmeta": "❖", "leftalt": "Alt", "rightalt": "AltGr",
    "space": "Space", "menu": "Menu", "volumedown": "◀", "volumeup": "▶", "mute": "●",
    **{f"f{number}": f"F{number}" for number in range(1, 25)},
}

KEYCAP_WIDTHS = {
    "up": "unit-arrow", "left": "unit-arrow", "down": "unit-arrow", "right": "unit-arrow",
    "tab": "unit-15", "backslash": "unit-14",
    "capslock": "unit-175", "leftshift": "unit-225", "rightshift": "unit-15",
    "102nd": "unit-125", "enter": "unit-225", "backspace": "unit-2",
    "leftcontrol": "unit-12", "rightcontrol": "unit-12", "leftalt": "unit-12",
    "menu": "unit-12", "leftmeta": "unit-meta", "rightmeta": "unit-meta",
    "rightalt": "unit-altgr", "space": "unit-space",
    "volumedown": "unit-knob", "volumeup": "unit-knob", "mute": "unit-knob",
}

ARROW_KEYS = ("up", "left", "down", "right")
KNOB_KEYS = ("volumedown", "volumeup", "mute")
FUNCTION_KEYS = tuple(f"f{number}" for number in range(1, 25))


def _keycap_label(key: str) -> str:
    return KEY_LABELS.get(key, key.upper() if key.isalpha() else key)


def _short_action(action: str, max_length: int = 4) -> str:
    if action.casefold() in {"home", "end", "pageup", "pagedown"}:
        return {"home": "Home", "end": "End", "pageup": "PgUp", "pagedown": "PgDn"}[action.casefold()]
    return action if len(action) <= max_length else action[:max_length - 1] + "…"



class TextEntryScreen(ModalScreen[str | None]):
    """A single-line, properly editable prompt."""

    def __init__(self, prompt: str, value: str = "", *, placeholder: str = "", preserve: bool = False) -> None:
        super().__init__()
        self.prompt = prompt
        self.initial_value = value
        self.placeholder = placeholder
        self.preserve = preserve

    def get_value(self, value: str) -> str:
        return value if self.preserve else value.strip()

    def compose(self) -> ComposeResult:
        with Container(classes="dialog"):
            yield Label(self.prompt, classes="dialog-title")
            yield Input(value=self.initial_value, placeholder=self.placeholder, id="entry")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Use", variant="primary", id="accept")
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        self.query_one("#entry", Input).focus()
        self.query_one("#entry", Input).select_all()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "entry":
            self.dismiss(self.get_value(event.value))

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "accept":
            self.dismiss(self.get_value(self.query_one("#entry", Input).value))
        elif event.button.id == "cancel":
            self.dismiss(None)

    def action_cancel(self) -> None:
        self.dismiss(None)


class CaptureScreen(ModalScreen[tuple[str | None, str]]):
    """Capture keyd output asynchronously, or accept a typed keyd key name."""

    BINDINGS = [("escape", "cancel", "Cancel"), ("ctrl+g", "cancel", "Cancel"),
                ("ctrl+t", "show_typed", "Type key name")]

    def __init__(self, command, timeout: float = 10) -> None:
        super().__init__()
        self.command = command
        self.timeout = timeout
        self.typed_queue = None
        self.cancel_event = None
        self.finished = False

    def compose(self) -> ComposeResult:
        with Container(classes="dialog capture-dialog"):
            yield Label("Capture binding key", classes="dialog-title")
            yield Static("Press a key alone (without Caps). Ctrl+T types a keyd name; Ctrl+G cancels.",
                         id="capture-help")
            yield Static("Waiting for keyd monitor…", id="capture-state")
            yield Input(placeholder="keyd name, e.g. leftcontrol", id="typed-key", classes="hidden")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        self.typed_queue, self.cancel_event = self.app.start_capture_worker(self)
        if self.command:
            self.run_worker(self._capture, thread=True, exclusive=True, name="key-capture")
        else:
            self.query_one("#capture-state", Static).update(
                "Monitor unavailable. Type a key, or Ctrl+T to enter its keyd name.")

    def _capture(self) -> None:
        from .. import system
        key, message = system.monitor_capture(
            self.command, self.timeout, cancel_event=self.cancel_event, typed_queue=self.typed_queue)
        if message.startswith("keyd monitor unavailable"):
            self.app.call_from_thread(self._use_terminal_fallback, message)
        else:
            self.app.call_from_thread(self._finish, key, message)

    def _use_terminal_fallback(self, message: str) -> None:
        self.command = []
        if self.cancel_event is not None:
            self.cancel_event.set()
        self.query_one("#capture-state", Static).update(
            f"{message} Press a key alone, or Ctrl+T to type a keyd name.")

    def _finish(self, key: str | None, message: str) -> None:
        if self.finished:
            return
        self.finished = True
        self.dismiss((key, message))

    def show_typed(self) -> None:
        field = self.query_one("#typed-key", Input)
        field.remove_class("hidden")
        field.focus()

    def on_key(self, event: events.Key) -> None:
        if event.key in ("ctrl+t", "ctrl+g", "escape", "enter"):
            return
        if self.app.focused is not None and self.app.focused.id == "typed-key":
            return
        if not self.command:
            name = key_name(event.key) or key_name(event.character or "")
            if name:
                self._finish(name, "")

    def on_input_submitted(self, event: Input.Submitted) -> None:
        value = event.value.strip()
        if value:
            self.typed_queue.put(("type", value))
            if not self.command:
                self._finish(value, "")
        else:
            self.typed_queue.put(("cancel", None))
            if not self.command:
                self._finish(None, "Cancelled.")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "cancel":
            self.action_cancel()

    def action_cancel(self) -> None:
        if self.cancel_event is not None:
            self.cancel_event.set()
        if self.typed_queue is not None:
            self.typed_queue.put(("cancel", None))
        if not self.command:
            self._finish(None, "Cancelled.")

    def on_unmount(self) -> None:
        if self.cancel_event is not None:
            self.cancel_event.set()


class ActionPickerScreen(ModalScreen[str | None]):
    """Filtered catalog with custom action and macro-builder choices."""

    BINDINGS = [
        ("escape", "cancel", "Cancel"), ("enter", "choose", "Choose"),
        ("ctrl+u", "clear_filter", "Clear filter"),
    ]

    def __init__(self, current: str = "", *, include_macro: bool = True) -> None:
        super().__init__()
        self.current = current
        self.include_macro = include_macro
        self.row_actions: dict[str, str] = {}
        self.preferred = current if current in ACTION_DESCRIPTIONS else (CUSTOM if current else "home")

    def compose(self) -> ComposeResult:
        with Container(classes="dialog picker-dialog"):
            yield Label("Choose an action", classes="dialog-title")
            yield Input(placeholder="Filter by action, description, or category…", id="action-filter")
            yield DataTable(id="action-table", cursor_type="row", zebra_stripes=True)
            with Horizontal(classes="dialog-buttons"):
                yield Button("Custom…", id="custom")
                if self.include_macro:
                    yield Button("Macro builder…", id="macro")
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        table = self.query_one("#action-table", DataTable)
        table.add_columns("Category", "Action", "Description")
        self.query_one("#action-filter", Input).focus()
        self.refresh_actions()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "action-filter":
            table = self.query_one("#action-table", DataTable)
            if table.row_count and self.row_actions:
                keys = list(self.row_actions)
                selected = max(0, min(table.cursor_row, len(keys) - 1))
                self.preferred = self.row_actions[keys[selected]]
            self.refresh_actions()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "action-filter":
            self.action_choose()

    def refresh_actions(self) -> None:
        table = self.query_one("#action-table", DataTable)
        table.clear(columns=False)
        self.row_actions.clear()
        entries = list(filter_actions(self.query_one("#action-filter", Input).value))
        for i, (category, action, description) in enumerate(entries):
            key = f"catalog-{i}"
            table.add_row(category, action, description, key=key)
            self.row_actions[key] = action
        table.add_row("Custom", "Custom…", "Enter a raw keyd action", key="custom")
        self.row_actions["custom"] = CUSTOM
        if self.include_macro:
            table.add_row("Custom", "Macro builder…", "Compose text, keys, and delays", key="macro")
            self.row_actions["macro"] = MACRO_BUILDER
        target = next((key for key, action in self.row_actions.items() if action == self.preferred), None)
        if target is not None:
            table.move_cursor(row=list(self.row_actions).index(target), column=1, animate=False)

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.data_table.id == "action-table":
            self.dismiss(self.row_actions.get(str(event.row_key.value)))

    def action_choose(self) -> None:
        table = self.query_one("#action-table", DataTable)
        if table.row_count:
            keys = list(self.row_actions)
            index = max(0, min(table.cursor_row, len(keys) - 1))
            self.dismiss(self.row_actions[keys[index]])

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "custom":
            self.dismiss(CUSTOM)
        elif event.button.id == "macro":
            self.dismiss(MACRO_BUILDER)
        elif event.button.id == "cancel":
            self.dismiss(None)

    def action_cancel(self) -> None:
        self.dismiss(None)
    def action_clear_filter(self) -> None:
        field = self.query_one("#action-filter", Input)
        field.value = ""

    def on_key(self, event: events.Key) -> None:
        if self.app.focused is None or self.app.focused.id != "action-table":
            return
        table = self.query_one("#action-table", DataTable)
        if event.key in ("j", "k") and table.row_count:
            delta = 1 if event.key == "j" else -1
            table.move_cursor(row=max(0, min(table.cursor_row + delta, table.row_count - 1)),
                              column=1, animate=False)


class ChoiceScreen(ModalScreen[str | None]):
    """Small list picker used for tap actions, symbols, and macro step kinds."""

    BINDINGS = [("escape", "cancel", "Cancel"), ("enter", "choose", "Choose")]

    def __init__(self, title: str, choices: list[tuple[str, str]], current: str | None = None) -> None:
        super().__init__()
        self.title_text = title
        self.choices = choices
        self.current = current

    def compose(self) -> ComposeResult:
        with Container(classes="dialog compact-dialog"):
            yield Label(self.title_text, classes="dialog-title")
            yield DataTable(id="choice-table", cursor_type="row")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Choose", variant="primary", id="accept")
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        table = self.query_one("#choice-table", DataTable)
        table.add_columns("Option", "Description")
        for i, (value, label) in enumerate(self.choices):
            table.add_row(value, label, key=str(i))
        current_row = next((i for i, (value, _) in enumerate(self.choices)
                            if value.casefold() == (self.current or "").casefold()), 0)
        table.move_cursor(row=current_row, column=0, animate=False)
        table.focus()

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.data_table.id == "choice-table":
            self.dismiss(self.choices[event.cursor_row][0])

    def action_choose(self) -> None:
        table = self.query_one("#choice-table", DataTable)
        if self.choices:
            index = max(0, min(table.cursor_row, len(self.choices) - 1))
            self.dismiss(self.choices[index][0])

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "accept":
            self.action_choose()
        elif event.button.id == "cancel":
            self.dismiss(None)

    def action_cancel(self) -> None:
        self.dismiss(None)
    def on_key(self, event: events.Key) -> None:
        if self.title_text == "Caps Lock tap action" and event.key in ("1", "2", "3", "4"):
            index = int(event.key) - 1
            if index < len(self.choices):
                self.dismiss(self.choices[index][0])
                return
        if event.key not in ("j", "k") or not self.choices:
            return
        table = self.query_one("#choice-table", DataTable)
        delta = 1 if event.key == "j" else -1
        current = max(0, table.cursor_row)
        row = (current + delta) % len(self.choices) if self.title_text == "Caps Lock tap action" else (
            max(0, min(current + delta, len(self.choices) - 1)))
        table.move_cursor(row=row, column=0, animate=False)


class PreviewScreen(ModalScreen[bool]):
    """Scrollable, syntax-colored diff/list preview with explicit confirmation."""

    BINDINGS = [("escape", "cancel", "Back"), ("enter", "confirm", "Confirm")]

    def __init__(self, title: str, lines, *, confirm_label: str = "Confirm", cancel_label: str = "Back") -> None:
        super().__init__()
        self.title_text = title
        self.lines = list(lines)
        self.confirm_label = confirm_label
        self.cancel_label = cancel_label
        self.rendered_lines: list[str] = []

    def compose(self) -> ComposeResult:
        with Container(classes="dialog preview-dialog"):
            yield Label(self.title_text, classes="dialog-title")
            yield RichLog(id="preview-log", wrap=False, highlight=False, markup=False)
            with Horizontal(classes="dialog-buttons"):
                yield Button(self.confirm_label, variant="primary", id="confirm")
                yield Button(self.cancel_label, id="cancel")

    def on_mount(self) -> None:
        log = self.query_one("#preview-log", RichLog)
        self.rendered_lines.clear()
        for item in self.lines:
            if isinstance(item, tuple):
                line, role = item
                text = Text(str(line), style={"warn": "bold yellow", "error": "bold red"}.get(role, ""))
            else:
                line = str(item)
                text = Text(line)
                if line.startswith("+"):
                    text.stylize("green")
                elif line.startswith("-"):
                    text.stylize("red")
                elif line.startswith("@@"):
                    text.stylize("cyan")
                elif line.startswith(("Warning:", "Warnings:", "WARN ", "INFO ", "Other sections differ")):
                    text.stylize("yellow")
            self.rendered_lines.append(str(line))
            log.write(text)
        log.focus()

    def action_confirm(self) -> None:
        self.dismiss(True)

    def action_cancel(self) -> None:
        self.dismiss(False)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss(event.button.id == "confirm")


class ConfirmScreen(ModalScreen[bool]):
    """Explicit stay/continue confirmation, including unsaved quit."""

    BINDINGS = [
        ("escape", "stay", "Stay"), ("enter", "confirm", "Confirm"),
        ("y", "confirm", "Quit anyway"), ("n", "stay", "Stay"),
    ]

    def __init__(self, message: str, confirm_label: str = "Continue") -> None:
        super().__init__()
        self.message = message
        self.confirm_label = confirm_label

    def compose(self) -> ComposeResult:
        with Container(classes="dialog compact-dialog"):
            yield Label(self.message, classes="dialog-title")
            with Horizontal(classes="dialog-buttons"):
                yield Button(self.confirm_label, variant="error", id="confirm")
                yield Button("Stay", id="stay")

    def on_mount(self) -> None:
        self.query_one("#stay", Button).focus()

    def action_confirm(self) -> None:
        self.dismiss(True)

    def action_stay(self) -> None:
        self.dismiss(False)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss(event.button.id == "confirm")


class HelpScreen(ModalScreen[None]):
    BINDINGS = [("escape", "close", "Close"), ("enter", "close", "Close")]

    def compose(self) -> ComposeResult:
        with Container(classes="dialog help-dialog"):
            yield Label("Keymap editor · help", classes="dialog-title")
            with VerticalScroll(id="help-content"):
                yield Static("", id="help-lines")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Close", variant="primary", id="close")

    def on_mount(self) -> None:
        self.query_one("#help-lines", Static).update("\n".join(self.app.help_lines()))

    def action_close(self) -> None:
        self.dismiss(None)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.action_close()


class KeyCap(Static, can_focus=True):
    """Clickable key cap carrying its keyd name, action, and readable legend."""

    class Pressed(Message):
        def __init__(self, key_cap: "KeyCap") -> None:
            super().__init__()
            self.key_cap = key_cap

    def __init__(self, keyd_key: str) -> None:
        self.keyd_key = keyd_key
        classes = ["keycap"]
        width = KEYCAP_WIDTHS.get(keyd_key)
        if width:
            classes.append(f"keycap-{width}")
        super().__init__(
            _keycap_label(keyd_key), id=f"keycap-{keyd_key}",
            classes=" ".join(classes), markup=False)
        self.tooltip = keyd_key

    def on_click(self, event: events.Click) -> None:
        self.post_message(self.Pressed(self))

    def on_key(self, event: events.Key) -> None:
        if event.key in {"enter", "space"}:
            event.stop()
            self.post_message(self.Pressed(self))


class KeyboardMap(Static):
    """Keyboard diagram with responsive key caps and secondary binding clusters."""

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self.compact = True
        self.inline = False
        self.show_actions = False

    def compose(self) -> ComposeResult:
        with Horizontal(id="map-content", classes="map-content"):
            with Vertical(id="main-keyboard", classes="keyboard-rows"):
                for row_index, keys in enumerate(KEYBOARD_ROWS):
                    with Horizontal(classes=f"keyboard-row row-{row_index}"):
                        for key in keys:
                            yield KeyCap(key)
            with Vertical(id="map-side", classes="map-side"):
                with Vertical(classes="map-cluster arrows-cluster"):
                    yield Label("Arrows", classes="cluster-title")
                    with Horizontal(classes="arrow-row"):
                        yield Static("", classes="keycap-spacer")
                        yield KeyCap("up")
                        yield Static("", classes="keycap-spacer")
                    with Horizontal(classes="arrow-row"):
                        yield KeyCap("left")
                        yield KeyCap("down")
                        yield KeyCap("right")
                with Vertical(classes="knob-row"):
                    for key in KNOB_KEYS:
                        yield KeyCap(key)
                with Vertical(id="map-function-row", classes="map-cluster f-row"):
                    yield Label("F-row", classes="cluster-title")
                    for first in (1, 7, 13, 19):
                        with Horizontal(id=f"function-row-{first}", classes="function-row"):
                            for number in range(first, first + 6):
                                yield KeyCap(f"f{number}")
        yield Static("", id="map-extra", classes="map-extra")
        yield Static("Filled = bound · outlined = selected · click a key to edit/add",
                     id="map-legend", classes="map-legend")

    def set_layout(
        self, *, compact: bool, side_by_side: bool, inline: bool, show_actions: bool,
    ) -> None:
        self.compact = compact
        self.inline = inline
        self.show_actions = show_actions
        for class_name in (
            "map-compact", "map-detailed", "map-side-by-side",
            "map-stacked", "map-inline", "map-actions",
        ):
            self.remove_class(class_name)
        self.add_class("map-compact" if compact else "map-detailed")
        self.add_class("map-side-by-side" if side_by_side else "map-stacked")
        if inline:
            self.add_class("map-inline")
        if show_actions:
            self.add_class("map-actions")
        selection = "accent = selected" if compact else "outlined = selected"
        self.query_one("#map-legend", Static).update(
            f"Filled = bound · {selection} · click a key to edit/add")

    def update_bindings(self, binds, selected_key: str | None) -> None:
        actions = {key.casefold(): action for key, action in binds}
        selected = selected_key.casefold() if selected_key else None
        for cap in self.query(KeyCap):
            key = cap.keyd_key.casefold()
            action = actions.get(key)
            label = _keycap_label(cap.keyd_key)
            cap.remove_class("bound", "selected", "layer-key", "has-action")
            if action is not None:
                cap.add_class("bound")
                if self.show_actions:
                    max_length = 7 if key in KNOB_KEYS else 4
                    cap.update(f"{label}\n{_short_action(action, max_length)}")
                    cap.add_class("has-action")
                else:
                    cap.update(label)
            else:
                cap.update(label)
            if key == selected:
                cap.add_class("selected")
            if key == "capslock":
                cap.add_class("layer-key")
            cap.tooltip = f"{cap.keyd_key} → {action}" if action is not None else cap.keyd_key

        function_bindings = {key.casefold() for key, _ in binds if key.casefold() in FUNCTION_KEYS}
        for cap in self.query(KeyCap):
            if cap.keyd_key in FUNCTION_KEYS:
                cap.styles.display = "block" if cap.keyd_key in function_bindings else "none"
        for first in (1, 7, 13, 19):
            row_keys = {f"f{number}" for number in range(first, first + 6)}
            self.query_one(f"#function-row-{first}", Horizontal).styles.display = (
                "block" if row_keys & function_bindings else "none")
        self.query_one("#map-function-row", Vertical).styles.display = (
            "block" if function_bindings else "none")

        drawn = {key.casefold() for row in KEYBOARD_ROWS for key in row}
        drawn.update(key.casefold() for key in ARROW_KEYS + KNOB_KEYS)
        drawn.update(function_bindings)
        extra = [key for key, _ in binds if key.casefold() not in drawn]
        extra_widget = self.query_one("#map-extra", Static)
        extra_widget.update("Also bound: " + ", ".join(extra) if extra else "")
        extra_widget.styles.display = "block" if extra else "none"

class KeyboardMapScreen(ModalScreen[None]):
    BINDINGS = [("escape", "close", "Close"), ("enter", "close", "Close")]

    def __init__(self, selected_key: str | None = None) -> None:
        super().__init__()
        self.selected_key = selected_key

    def compose(self) -> ComposeResult:
        with Container(classes="dialog map-dialog"):
            yield Label("Keyboard map · keyd names", classes="dialog-title")
            yield KeyboardMap(id="full-keyboard-map")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Close", id="close")

    def on_mount(self) -> None:
        self.configure_map()
        self.call_after_refresh(self.fit_dialog)


    def on_resize(self, event: events.Resize) -> None:
        self.configure_map()
        self.call_after_refresh(self.fit_dialog)

    def configure_map(self) -> None:
        width, height = self.size.width, self.size.height
        compact = width < 100 or height < 25
        map_widget = self.query_one("#full-keyboard-map", KeyboardMap)
        map_widget.set_layout(
            compact=compact, side_by_side=not compact, inline=False, show_actions=True)
        map_widget.update_bindings(self.app.binds, self.selected_key)

    def fit_dialog(self) -> None:
        dialog = self.query_one(".map-dialog", Container)
        content = self.query_one("#map-content", Horizontal)
        desired = content.size.width + 4
        dialog.styles.width = min(desired, max(1, self.size.width - 2))
    def on_key_cap_pressed(self, event: KeyCap.Pressed) -> None:
        self.dismiss(None)
        self.app.map_key_selected(event.key_cap.keyd_key)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "close":
            self.dismiss(None)

    def action_close(self) -> None:
        self.dismiss(None)


class PackPickerScreen(ModalScreen[dict | None]):
    BINDINGS = [
        ("escape", "cancel", "Cancel"), ("enter", "choose", "Preview"),
        ("j", "next", "Next"), ("k", "previous", "Previous"),
        ("home", "first", "First"), ("end", "last", "Last"),
    ]

    def compose(self) -> ComposeResult:
        with Container(classes="dialog picker-dialog"):
            yield Label("Preset packs", classes="dialog-title")
            yield DataTable(id="pack-table", cursor_type="row", zebra_stripes=True)
            with Horizontal(classes="dialog-buttons"):
                yield Button("Preview", variant="primary", id="preview")
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        table = self.query_one("#pack-table", DataTable)
        table.add_columns("Pack", "Description")
        for i, pack in enumerate(PACKS):
            table.add_row(pack["name"], pack["description"], key=str(i))
        table.focus()

    def selected_pack(self):
        table = self.query_one("#pack-table", DataTable)
        return PACKS[max(0, min(table.cursor_row, len(PACKS) - 1))]

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.data_table.id == "pack-table":
            self.dismiss(PACKS[event.cursor_row])

    def action_choose(self) -> None:
        self.dismiss(self.selected_pack())

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "preview":
            self.action_choose()
        elif event.button.id == "cancel":
            self.dismiss(None)

    def action_cancel(self) -> None:
        self.dismiss(None)

    def action_next(self) -> None:
        table = self.query_one("#pack-table", DataTable)
        table.move_cursor(row=min(table.cursor_row + 1, len(PACKS) - 1), column=0, animate=False)

    def action_previous(self) -> None:
        table = self.query_one("#pack-table", DataTable)
        table.move_cursor(row=max(table.cursor_row - 1, 0), column=0, animate=False)

    def action_first(self) -> None:
        self.query_one("#pack-table", DataTable).move_cursor(row=0, column=0, animate=False)

    def action_last(self) -> None:
        self.query_one("#pack-table", DataTable).move_cursor(row=len(PACKS) - 1, column=0, animate=False)


class BackupPickerScreen(ModalScreen[tuple | None]):
    BINDINGS = [
        ("escape", "cancel", "Cancel"), ("enter", "choose", "Preview"),
        ("j", "next", "Next"), ("k", "previous", "Previous"),
        ("home", "first", "First"), ("end", "last", "Last"),
    ]

    def __init__(self, entries: list[tuple]) -> None:
        super().__init__()
        self.entries = entries

    def compose(self) -> ComposeResult:
        with Container(classes="dialog picker-dialog"):
            yield Label("Backups · newest first", classes="dialog-title")
            yield DataTable(id="backup-table", cursor_type="row", zebra_stripes=True)
            with Horizontal(classes="dialog-buttons"):
                yield Button("Preview", variant="primary", id="preview")
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        table = self.query_one("#backup-table", DataTable)
        table.add_columns("Timestamp", "Bindings")
        for i, (_, stamp, count) in enumerate(self.entries):
            table.add_row(stamp.strftime("%Y-%m-%d %H:%M:%S"),
                          "unreadable" if count is None else f"{count} [nav]", key=str(i))
        table.focus()

    def selected_entry(self) -> tuple:
        table = self.query_one("#backup-table", DataTable)
        return self.entries[max(0, min(table.cursor_row, len(self.entries) - 1))]

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.data_table.id == "backup-table":
            self.dismiss(self.entries[event.cursor_row])

    def action_choose(self) -> None:
        self.dismiss(self.selected_entry())

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "preview":
            self.action_choose()
        elif event.button.id == "cancel":
            self.dismiss(None)

    def action_cancel(self) -> None:
        self.dismiss(None)

    def action_next(self) -> None:
        table = self.query_one("#backup-table", DataTable)
        table.move_cursor(row=min(table.cursor_row + 1, len(self.entries) - 1), column=0, animate=False)

    def action_previous(self) -> None:
        table = self.query_one("#backup-table", DataTable)
        table.move_cursor(row=max(table.cursor_row - 1, 0), column=0, animate=False)

    def action_first(self) -> None:
        self.query_one("#backup-table", DataTable).move_cursor(row=0, column=0, animate=False)

    def action_last(self) -> None:
        self.query_one("#backup-table", DataTable).move_cursor(
            row=len(self.entries) - 1, column=0, animate=False)


def keyd_compose_configured() -> bool:
    compose = os.path.realpath("/usr/share/keyd/keyd.compose")
    user_compose = os.path.expanduser("~/.XCompose")
    if os.path.realpath(user_compose) == compose:
        return True
    try:
        with open(user_compose, encoding="utf-8", errors="ignore") as source:
            if any(line.lstrip().startswith("include") and "keyd.compose" in line for line in source):
                return True
    except OSError:
        pass
    if not os.path.isfile(compose):
        return False
    for system_compose in glob.glob("/usr/share/X11/locale/*/Compose"):
        if os.path.realpath(system_compose) == compose:
            return True
        try:
            if filecmp.cmp(compose, system_compose, shallow=False):
                return True
            with open(system_compose, encoding="utf-8", errors="ignore") as source:
                if any("keyd.compose" in line for line in source):
                    return True
        except OSError:
            continue
    return False


class MacroBuilderScreen(ModalScreen[str | None]):
    """Build keyd macro steps using editable prompts and explicit validation."""

    BINDINGS = [
        ("escape", "cancel", "Cancel"), ("enter", "save", "Save"),
        ("a", "add_step", "Add step"), ("e", "edit_step", "Edit step"),
        ("d", "delete_step", "Delete step"), ("[", "move_up", "Move up"),
        ("]", "move_down", "Move down"), ("y", "symbols", "Symbols"),
        ("j", "next", "Next"), ("k", "previous", "Previous"),
        ("home", "first", "First"), ("end", "last", "Last"),
    ]
    SYMBOLS = (
        ("→", "right arrow"), ("←", "left arrow"), ("↑", "up arrow"), ("↓", "down arrow"),
        ("€", "euro"), ("£", "pound"), ("°", "degree"), ("±", "plus/minus"),
        ("×", "multiply"), ("÷", "divide"), ("—", "em dash"), ("…", "ellipsis"),
        ("“", "left double quote"), ("”", "right double quote"), ("‘", "left single quote"),
        ("’", "right single quote"), ("✓", "check"), ("•", "bullet"),
    )

    def __init__(self, initial=()) -> None:
        super().__init__()
        self.steps = list(initial)
        self.validation_error: str | None = None
        self.selected = 0

    def compose(self) -> ComposeResult:
        with Container(classes="dialog macro-dialog"):
            yield Label("Macro builder", classes="dialog-title")
            yield Static("", id="macro-validation")
            yield DataTable(id="macro-steps", cursor_type="row", zebra_stripes=True)
            yield Static("", id="macro-preview", classes="macro-preview")
            yield Static("", id="macro-caveat", classes="macro-caveat")
            with Horizontal(classes="macro-actions"):
                yield Button("+ Text", id="text")
                yield Button("+ Key", id="key")
                yield Button("+ Delay", id="delay")
                yield Button("Edit", id="edit")
                yield Button("Delete", id="delete")
                yield Button("↑", id="up")
                yield Button("↓", id="down")
                yield Button("Symbols", id="symbols")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Save macro", variant="primary", id="save")
                yield Button("Cancel", id="cancel")

    def on_mount(self) -> None:
        self.query_one("#macro-steps", DataTable).add_columns("#", "Step", "Value")
        self.refresh_steps()

    def refresh_steps(self) -> None:
        table = self.query_one("#macro-steps", DataTable)
        table.clear(columns=False)
        for index, step in enumerate(self.steps):
            label = {"text": "Text", "key": "Key/combo", "delay": "Delay"}[step.kind]
            value = f"{step.value}ms" if step.kind == "delay" else repr(step.value) if step.kind == "text" else step.value
            table.add_row(str(index + 1), label, str(value), key=str(index))
        if self.steps:
            self.selected = max(0, min(self.selected, len(self.steps) - 1))
            table.move_cursor(row=self.selected, column=0, animate=False)
        self.update_preview()

    def update_preview(self) -> None:
        try:
            action = build_macro(self.steps)
            error = check(chr(10).join(("[ids]", "*", "[nav]", f"x = {action}", "")))
        except ValueError as exc:
            action, error = "macro(<invalid step>)", str(exc)
        self.validation_error = error
        if error:
            last = error.splitlines()[-1].strip()
            status = f"Invalid macro: {last}" if action.endswith("<invalid step>)") else f"Keyd rejects: {last}"
        else:
            status = "Keyd check: valid" if keyd_bin() else "keyd unavailable — validation skipped"
        self.query_one("#macro-validation", Static).update(status)
        self.query_one("#macro-preview", Static).update(f"Preview: {action}")
        unicode_text = any(step.kind == "text" and any(ord(char) > 127 for char in step.value)
                           for step in self.steps)
        self.query_one("#macro-caveat", Static).update(
            "Unicode needs keyd.compose via ~/.XCompose or system Compose; default US layout."
            if unicode_text and not keyd_compose_configured() else "")
    async def add_step(self, kind: str, current: MacroStep | None = None) -> None:
        defaults = {"text": "", "key": "enter", "delay": "100"}
        initial = str(current.value) if current else defaults[kind]
        if kind == "key":
            value = await self.app.push_screen_wait(ActionPickerScreen(initial, include_macro=False))
            if value == CUSTOM:
                value = await self.app.push_screen_wait(TextEntryScreen("Key/combo", initial))
        else:
            prompt = {"text": "Text step", "delay": "Delay milliseconds (0–1023)"}[kind]
            value = await self.app.push_screen_wait(
                TextEntryScreen(prompt, initial, preserve=kind == "text"))
        if value is None or value == "":
            return
        if kind == "delay":
            try:
                value = int(value)
            except ValueError:
                self.app.notify("Delay must be an integer from 0 to 1023.", severity="error")
                return
            if not 0 <= value <= 1023:
                self.app.notify("Delay must be an integer from 0 to 1023.", severity="error")
                return
        step = MacroStep(kind, value)
        try:
            build_macro([*self.steps, step])
        except ValueError as exc:
            self.app.notify(str(exc), severity="error")
            return
        if current is not None:
            self.steps[self.selected] = step
        else:
            self.selected = min(self.selected + 1, len(self.steps))
            self.steps.insert(self.selected, step)
        self.refresh_steps()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        button = event.button.id
        if button in ("text", "key", "delay"):
            self.run_worker(self.add_step(button), exclusive=True)
        elif button == "edit" and self.steps:
            step = self.steps[self.selected]
            self.run_worker(self.add_step(step.kind, step), exclusive=True)
        elif button == "delete" and self.steps:
            self.steps.pop(self.selected)
            self.selected = min(self.selected, max(0, len(self.steps) - 1))
            self.refresh_steps()
        elif button == "up" and self.selected > 0:
            self.steps[self.selected - 1], self.steps[self.selected] = self.steps[self.selected], self.steps[self.selected - 1]
            self.selected -= 1
            self.refresh_steps()
        elif button == "down" and self.selected + 1 < len(self.steps):
            self.steps[self.selected + 1], self.steps[self.selected] = self.steps[self.selected], self.steps[self.selected + 1]
            self.selected += 1
            self.refresh_steps()
        elif button == "symbols":
            self.run_worker(self.insert_symbol(), exclusive=True)
        elif button == "save":
            self.action_save()
        elif button == "cancel":
            self.dismiss(None)

    async def insert_symbol(self) -> None:
        value = await self.app.push_screen_wait(ChoiceScreen("Insert symbol as text", list(self.SYMBOLS)))
        if value is None:
            return
        if self.steps and self.steps[self.selected].kind == "text":
            old = self.steps[self.selected]
            self.steps[self.selected] = MacroStep("text", str(old.value) + value)
        else:
            self.selected = min(self.selected + 1, len(self.steps))
            self.steps.insert(self.selected, MacroStep("text", value))
        self.refresh_steps()

    def action_save(self) -> None:
        try:
            action = build_macro(self.steps)
        except ValueError as exc:
            self.app.notify(str(exc), severity="error")
            return
        error = check(f"[ids]\n*\n[nav]\nx = {action}\n")
        if error:
            self.app.notify("keyd rejects macro: " + error.splitlines()[-1].strip(), severity="error")
            return
        self.dismiss(action)

    def action_cancel(self) -> None:
        self.dismiss(None)

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.data_table.id == "macro-steps" and self.steps:
            self.selected = event.cursor_row
    def action_add_step(self) -> None:
        self.run_worker(self.choose_step_type(), exclusive=True)

    async def choose_step_type(self) -> None:
        kind = await self.app.push_screen_wait(ChoiceScreen(
            "Add macro step", [("text", "Text"), ("key", "Key or combo"), ("delay", "Delay")]))
        if kind:
            await self.add_step(kind)

    def action_edit_step(self) -> None:
        self.run_worker(self.edit_selected_step(), exclusive=True)

    async def edit_selected_step(self) -> None:
        if self.steps:
            step = self.steps[self.selected]
            await self.add_step(step.kind, step)
    def action_delete_step(self) -> None:
        if self.steps:
            self.steps.pop(self.selected)
            self.selected = min(self.selected, max(0, len(self.steps) - 1))
            self.refresh_steps()

    def action_move_up(self) -> None:
        if self.selected > 0:
            self.steps[self.selected - 1], self.steps[self.selected] = self.steps[self.selected], self.steps[self.selected - 1]
            self.selected -= 1
            self.refresh_steps()

    def action_move_down(self) -> None:
        if self.selected + 1 < len(self.steps):
            self.steps[self.selected + 1], self.steps[self.selected] = self.steps[self.selected], self.steps[self.selected + 1]
            self.selected += 1
            self.refresh_steps()

    def action_symbols(self) -> None:
        self.run_worker(self.insert_symbol(), exclusive=True)

    def action_next(self) -> None:
        self.selected = min(self.selected + 1, max(0, len(self.steps) - 1))
        if self.steps:
            self.query_one("#macro-steps", DataTable).move_cursor(row=self.selected, column=0, animate=False)

    def action_previous(self) -> None:
        self.selected = max(0, self.selected - 1)
        if self.steps:
            self.query_one("#macro-steps", DataTable).move_cursor(row=self.selected, column=0, animate=False)

    def action_first(self) -> None:
        self.selected = 0
        if self.steps:
            self.query_one("#macro-steps", DataTable).move_cursor(row=0, column=0, animate=False)

    def action_last(self) -> None:
        self.selected = max(0, len(self.steps) - 1)
        if self.steps:
            self.query_one("#macro-steps", DataTable).move_cursor(row=self.selected, column=0, animate=False)


class LiveScreen(ModalScreen[None]):
    """Asynchronous keyd monitor output with chord composition and annotations."""

    BINDINGS = [("escape", "close", "Close"), ("ctrl+g", "close", "Close")]

    def __init__(self, command, state_message: str, message: str) -> None:
        super().__init__()
        self.command = command
        self.state_message = state_message
        self.message = message
        self.events: list[dict] = []
        self.held_modifiers = {}
        self.active_events = {}
        self.cancel_event = None
        self.rendered_lines: list[str] = []

    def compose(self) -> ComposeResult:
        with Container(classes="dialog live-dialog"):
            yield Label("LIVE KEYD OUTPUT", classes="dialog-title")
            yield Static(self.state_message, id="live-state")
            yield Static("key · state · device (chords include binding annotations)", id="live-hint")
            yield RichLog(id="live-log", wrap=False, highlight=False, markup=False)
            yield Static(self.message, id="live-status")
            with Horizontal(classes="dialog-buttons"):
                yield Button("Close", variant="primary", id="close")

    def on_mount(self) -> None:
        self.cancel_event = self.app.start_live_worker(self)
        self.write_status()

    def receive_line(self, line: str) -> None:
        from ..monitor import record_monitor_event
        if not record_monitor_event(self.events, line, self.app.binds,
                                    self.held_modifiers, self.active_events):
            return
        log = self.query_one("#live-log", RichLog)
        log.clear()
        self.rendered_lines.clear()
        for event in self.events:
            if event["chord"]:
                line = f"{event['key']} · {event['device']}"
                notes = []
                for annotation in event["annotation"]:
                    note = annotation.split(" → ", 1)[0]
                    if not note.startswith(("←", "→", "↑", "↓")):
                        note = "← " + note
                    notes.append(note)
                if notes:
                    line += "  " + ", ".join(notes)
                self.rendered_lines.append(line)
                key_text = Text(f"{event['key']} · {event['device']}", style="bold cyan")
                if notes:
                    key_text.append("  " + ", ".join(notes), style="green")
                log.write(key_text)
                continue

            state = "d/u" if event["state"] == "down/up" else event["state"]
            line = f"{event['key']} {state} · {event['device']}"
            self.rendered_lines.append(line)
            log.write(Text(line))
            for annotation in event["annotation"]:
                self.rendered_lines.append(f"  {annotation}")
                log.write(Text(f"  {annotation}", style="green"))
        self.write_status()

    def monitor_stopped(self, code: int | None) -> None:
        if code is not None:
            self.message = (f"keyd monitor exited (status {code}); Esc/Ctrl+G closes."
                            if code else "keyd monitor stopped; Esc/Ctrl+G closes.")
            self.write_status()

    def write_status(self) -> None:
        if self.is_mounted:
            self.query_one("#live-status", Static).update(self.message or "Waiting for keyd monitor events…")

    def action_close(self) -> None:
        if self.cancel_event is not None:
            self.cancel_event.set()
        self.dismiss(None)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "close":
            self.action_close()

    def on_unmount(self) -> None:
        if self.cancel_event is not None:
            self.cancel_event.set()



class KeymapFooter(Footer):
    """Keep only core editor controls in the Footer when horizontal space is tight."""

    CORE_ACTIONS = frozenset({
        "add_binding", "edit_binding", "delete_binding", "show_filter",
        "save_config", "undo", "help", "quit_editor",
    })

    def compose(self):
        active = self.screen.active_bindings
        narrow = (self.size.width or self.app.size.width) < 135
        bindings = [
            item for item in active.values()
            if item[1].show and (not narrow or item[1].action in self.CORE_ACTIONS)
        ]
        palette = None
        if self.show_command_palette and self.app.ENABLE_COMMAND_PALETTE:
            palette = active.get(self.app.COMMAND_PALETTE_BINDING)
        self.styles.grid_size_columns = len(bindings) + (palette is not None)
        for _node, binding, enabled, tooltip in bindings:
            yield FooterKey(
                binding.key, self.app.get_key_display(binding), binding.description,
                binding.action, disabled=not enabled, tooltip=tooltip,
            )
        if palette is not None:
            _node, binding, enabled, tooltip = palette
            yield FooterKey(
                binding.key, self.app.get_key_display(binding), binding.description,
                binding.action, disabled=not enabled, tooltip=tooltip,
                classes="-command-palette",
            )

    def on_mount(self) -> None:
        # The parent screen focuses its table after child mount. Recompose
        # after that lifecycle turn so the initial frame has active bindings.
        self.set_timer(0.05, self._refresh_footer)

    def _refresh_footer(self) -> None:
        self.run_worker(self.recompose(), group="footer", exclusive=True)

    def on_screen_resume(self) -> None:
        self.call_after_refresh(self._refresh_footer)

    def on_resize(self, event: events.Resize) -> None:
        self.call_after_refresh(self.recompose)


class EditorScreen(Screen):
    """Main editor layout, with responsive table and optional inline map."""

    def compose(self) -> ComposeResult:
        from textual.widgets import Header
        yield Header(show_clock=False)
        yield Static("", id="tap-status")
        yield Input(placeholder="Filter bindings by key, action, or description…", id="binding-filter", classes="hidden")
        yield DataTable(id="bindings", cursor_type="row", zebra_stripes=True)
        yield KeyboardMap(id="keyboard-map")
        yield KeymapFooter(compact=True)

    def on_mount(self) -> None:
        table = self.query_one("#bindings", DataTable)
        table.add_columns("Key", "Action", "Description")
        table.focus()
        self.refresh_table()

    def refresh_table(self) -> None:
        app = self.app
        table = self.query_one("#bindings", DataTable)
        table.clear(columns=False)
        app.row_indices.clear()
        visible = app.filtered_indices()
        warnings = app.warnings_by_key()
        for index in visible:
            key, action = app.binds[index]
            marks = warnings.get(key.casefold(), [])
            marker = "⚠ " if any(level == "warn" for level, _ in marks) else "ⓘ " if marks else ""
            row_key = str(index)
            table.add_row(marker + key, action, ACTION_DESCRIPTIONS.get(action, ""), key=row_key)
            app.row_indices[row_key] = index
        if visible:
            if app.selected_index not in visible:
                app.selected_index = visible[0]
            table.move_cursor(row=visible.index(app.selected_index), column=0, animate=False)
        self.update_header()
        self.update_map()

    def update_header(self) -> None:
        app = self.app
        tap = app.tap_label()
        dirty = "  [unsaved]" if app.dirty else ""
        self.app.title = f"keyd · Caps Lock layer · Tap: {tap}{dirty}"
        live_state = app.live_config_state()
        saved_state = "unsaved ●" if app.dirty else "saved ✓"
        status = f"{len(app.binds)} bindings · {saved_state} · live config: {live_state}"
        if not app.tap_editable:
            status += " · tap read-only"
        visible = app.filtered_indices()
        if app.filter_query:
            status += f" · Filter {len(visible)}/{len(app.binds)}"
            if not visible:
                status += " · No matching bindings"
        if app.binds and app.selected_index < len(app.binds):
            key = app.binds[app.selected_index][0]
            warnings = app.warnings_by_key().get(key.casefold(), [])
            if warnings:
                level, message = warnings[0]
                status += f" · {'Warning' if level == 'warn' else 'Info'}: {message}"
        self.query_one("#tap-status", Static).update(status)

    def update_map(self) -> None:
        app = self.app
        map_widget = self.query_one("#keyboard-map", KeyboardMap)
        width, height = self.size.width, self.size.height
        visible = width >= 80 and height >= 30
        map_widget.styles.display = "block" if visible else "none"
        if not visible:
            return
        selected = app.binds[app.selected_index][0] if app.binds and app.selected_index < len(app.binds) else None
        compact = width < 130 or height < 38
        map_widget.set_layout(
            compact=compact, side_by_side=width >= 96, inline=True,
            show_actions=not compact)
        map_widget.styles.height = "auto"
        map_widget.update_bindings(app.binds, selected)

    def on_resize(self, event: events.Resize) -> None:
        self.update_map()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "binding-filter":
            self.app.filter_query = event.value
            self.refresh_table()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "binding-filter":
            event.input.add_class("hidden")
            self.query_one("#bindings", DataTable).focus()
            self.refresh_table()

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.data_table.id == "bindings":
            index = self.app.row_indices.get(str(event.row_key.value))
            if index is not None:
                self.app.run_worker(
                    self.app.edit_binding(index), exclusive=True, name="edit-binding")

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.data_table.id == "bindings":
            row_key = str(event.row_key.value)
            if row_key in self.app.row_indices:
                self.app.selected_index = self.app.row_indices[row_key]
                self.update_map()
    def on_key(self, event: events.Key) -> None:
        if self.app.focused is None or self.app.focused.id != "bindings":
            return
        visible = self.app.filtered_indices()
        if not visible:
            return
        try:
            position = visible.index(self.app.selected_index)
        except ValueError:
            position = 0
        if event.key == "k":
            self.query_one("#bindings", DataTable).move_cursor(
                row=max(0, position - 1), column=0, animate=False)
        elif event.key == "j":
            self.query_one("#bindings", DataTable).move_cursor(
                row=min(len(visible) - 1, position + 1), column=0, animate=False)

    def action_show_filter(self) -> None:
        field = self.query_one("#binding-filter", Input)
        field.value = self.app.filter_query
        field.remove_class("hidden")
        field.focus()

    def action_clear_filter(self) -> None:
        if self.app.filter_query:
            self.app.filter_query = ""
            field = self.query_one("#binding-filter", Input)
            field.value = ""
            field.add_class("hidden")
            self.query_one("#bindings", DataTable).focus()
            self.refresh_table()

    def on_key_cap_pressed(self, event: KeyCap.Pressed) -> None:
        self.app.map_key_selected(event.key_cap.keyd_key)
