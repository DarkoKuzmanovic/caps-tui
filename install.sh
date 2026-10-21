#!/usr/bin/env bash
# Full install: bash install.sh. Quick update: sudo bash install.sh --reload.
# Launch the editor with ./keymap.py or `uv run keymap.py`; CLI commands also work with python3.
# The installer itself uses bash/keyd only and does not require uv.
set -euo pipefail
here=$(dirname "$(readlink -f "$0")")
keyd_dir=${KEYMAP_ETC:-/etc/keyd}

if [[ ${1-} == --reload ]]; then
  if [[ $# -ne 1 ]]; then
    echo "Usage: $0 [--reload]" >&2
    exit 2
  fi
  if [[ $EUID -ne 0 ]]; then
    echo "--reload must run as root (use sudo)." >&2
    exit 1
  fi
  install -d "$keyd_dir"
  if [[ -e "$keyd_dir/default.conf" ]] && ! cmp -s "$here/default.conf" "$keyd_dir/default.conf"; then
    cp -a "$keyd_dir/default.conf" "$keyd_dir/default.conf.bak.$(date +%F-%H%M%S)"
  fi
  install -m 644 "$here/default.conf" "$keyd_dir/default.conf"
  keyd check "$keyd_dir/default.conf"
  keyd reload
  echo "OK. Reloaded keyd configuration."
  exit 0
fi

if [[ $# -ne 0 ]]; then
  echo "Usage: $0 [--reload]" >&2
  exit 2
fi

sudo pacman -S --needed --noconfirm keyd
sudo install -d "$keyd_dir"
if [[ -e "$keyd_dir/default.conf" ]] && ! cmp -s "$here/default.conf" "$keyd_dir/default.conf"; then
  sudo cp -a "$keyd_dir/default.conf" "$keyd_dir/default.conf.bak.$(date +%F-%H%M%S)"
fi
sudo install -m 644 "$here/default.conf" "$keyd_dir/default.conf"
sudo keyd check "$keyd_dir/default.conf"
sudo systemctl enable --now keyd
sudo systemctl restart keyd   # picks up config if keyd was already running
systemctl is-active keyd
echo "OK. Revert: sudo systemctl disable --now keyd  (optionally: sudo pacman -Rns keyd; sudo rm -r /etc/keyd)"
