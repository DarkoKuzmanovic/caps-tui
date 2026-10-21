"""Paths rooted beside the keymap.py launcher, including symlink invocations."""

import os

HERE = os.path.dirname(os.path.dirname(os.path.realpath(__file__)))


CONF = os.path.join(HERE, "default.conf")
