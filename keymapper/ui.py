"""Lazy Textual editor entry point; importing this module does not import Textual."""


def main(*, conf_path=None):
    """Run the Textual editor, optionally against an injected config file."""
    from .tui.app import KeymapApp

    KeymapApp(conf_path=conf_path).run()
