"""Discoverable command-palette actions for the keymap editor."""

from textual.command import DiscoveryHit, Hit, Hits, Provider

from ..data import PACKS


class KeymapCommands(Provider):
    """Expose common editor actions, named packs, and tap choices to Ctrl+P."""

    def _commands(self):
        app = self.app
        commands = [
            ("Add binding", "Capture a key and choose its action", app.action_add_binding),
            ("Edit selected binding", "Change the selected binding action", app.action_edit_binding),
            ("Delete selected binding", "Remove the selected binding", app.action_delete_binding),
            ("Undo", "Undo the latest editor change", app.action_undo),
            ("Redo", "Redo the latest undone change", app.action_redo),
            ("Review diff and save", "Preview changes and optionally apply them", app.action_save_config),
            ("Open live monitor", "Stream keyd output and composed chords", app.action_live_panel),
            ("Keyboard map", "Open the clickable keyboard map", app.action_keyboard_map),
            ("Change tap action", "Choose what a short Caps press sends", app.action_choose_tap),
            ("Help", "Show shortcuts and editor behavior", app.action_help),
        ]
        for pack in PACKS:
            commands.append((f"Apply pack: {pack['name']}", pack["description"],
                             lambda pack=pack: app.run_worker(app.apply_pack(pack), exclusive=True)))
        for value, label in (("capslock", "Caps Lock"), ("esc", "Esc"),
                             ("backspace", "Backspace"), ("__custom__", "Custom…")):
            commands.append((f"Tap option: {label}", f"Set the Caps tap action to {label}",
                             lambda value=value: app.run_worker(app.apply_tap_option(value), exclusive=True)))
        return commands

    async def search(self, query: str) -> Hits:
        matcher = self.matcher(query)
        for title, help_text, callback in self._commands():
            score = matcher.match(title)
            if score > 0:
                yield Hit(score, matcher.highlight(title), callback, help=help_text)

    async def discover(self) -> Hits:
        for title, help_text, callback in self._commands():
            yield DiscoveryHit(title, callback, help=help_text)
