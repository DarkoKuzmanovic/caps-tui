#!/usr/bin/env bash
# Full install: bash install.sh. Quick update: sudo bash install.sh --reload.
# Launch the editor with ./keymap.py or `uv run keymap.py`; CLI commands also work with python3.
# The installer itself uses bash/keyd only and does not require uv.
set -euo pipefail
here=$(dirname "$(readlink -f "$0")")
keyd_dir=${KEYMAP_ETC:-/etc/keyd}

if [[ ${1-} == --install-policy ]]; then
  if [[ $# -ne 1 ]]; then
    echo "Usage: $0 [--reload|--install-policy]" >&2
    exit 2
  fi
  if [[ $EUID -ne 0 ]]; then
    echo "--install-policy must run as root (use sudo)." >&2
    exit 1
  fi
  install -d /usr/libexec /usr/share/polkit-1/actions
  for helper in caps-gui-monitor caps-gui-apply caps-gui-askpass; do
    install -m 755 "$here/caps-gui/helpers/$helper" "/usr/libexec/$helper"
  done
  install -m 644 "$here/caps-gui/helpers/ma.quz.caps-gui.policy" /usr/share/polkit-1/actions/
  echo "OK. Installed the caps-gui polkit helpers."
  exit 0
fi

if [[ ${1-} == --reload ]]; then
  if [[ $# -ne 1 ]]; then
    echo "Usage: $0 [--reload|--install-policy]" >&2
    exit 2
  fi
  if [[ $EUID -ne 0 ]]; then
    echo "--reload must run as root (use sudo)." >&2
    exit 1
  fi
  install -d "$keyd_dir"
  # Validate a copy first: a bad config must never reach /etc/keyd, even for a moment.
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT
  cp "$here/default.conf" "$tmp"
  keyd check "$tmp"
  if [[ -e "$keyd_dir/default.conf" ]] && ! cmp -s "$tmp" "$keyd_dir/default.conf"; then
    cp -a "$keyd_dir/default.conf" "$keyd_dir/default.conf.bak.$(date +%F-%H%M%S)"
  fi
  install -m 644 "$tmp" "$keyd_dir/default.conf"
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
# Validate a copy first: a bad config must never reach /etc/keyd, even for a moment.
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cp "$here/default.conf" "$tmp"
sudo keyd check "$tmp"
if [[ -e "$keyd_dir/default.conf" ]] && ! sudo cmp -s "$tmp" "$keyd_dir/default.conf"; then
  sudo cp -a "$keyd_dir/default.conf" "$keyd_dir/default.conf.bak.$(date +%F-%H%M%S)"
fi
sudo install -m 644 "$tmp" "$keyd_dir/default.conf"
sudo systemctl enable --now keyd
sudo systemctl restart keyd   # picks up config if keyd was already running
systemctl is-active keyd
echo "OK. Revert: sudo systemctl disable --now keyd  (optionally: sudo pacman -Rns keyd; sudo rm -r /etc/keyd)"
