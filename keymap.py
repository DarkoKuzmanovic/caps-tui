#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["textual>=8.2,<9"]
# ///
"""Launch the keyd Caps Lock editor or its non-interactive commands.

Usage:
  ./keymap.py                    interactive Textual editor for the [nav] layer
  uv run keymap.py                same, with Textual supplied by uv
  python3 keymap.py --selftest    validate the code and keyd actions
  python3 keymap.py COMMAND ...  non-interactive commands (see python3 keymap.py -h)
"""

import os
import sys

HERE = os.path.dirname(os.path.realpath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from keymapper.cli import main as cli_main
from keymapper.selftest import selftest


def run_tui():
    try:
        from keymapper.ui import main
        main()
    except ModuleNotFoundError as exc:
        if exc.name != "textual":
            raise
        print("Textual is missing; run: uv run keymap.py", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif sys.argv[1:] == ["--uitest"]:
        from keymapper.uitest import run_uitests
        sys.exit(run_uitests())
    elif sys.argv[1:]:
        sys.exit(cli_main(sys.argv[1:]))
    else:
        sys.exit(run_tui())
