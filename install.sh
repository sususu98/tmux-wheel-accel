#!/usr/bin/env bash
set -euo pipefail

echo "==> Building tmux-wheel-accel (Rust)..."
cargo build --release

INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "${INSTALL_DIR}"

echo "==> Installing binary to ${INSTALL_DIR}/tmux-wheel-accel..."
cp target/release/tmux-wheel-accel "${INSTALL_DIR}/tmux-wheel-accel"
chmod +x "${INSTALL_DIR}/tmux-wheel-accel"

echo "==> Installation complete!"
echo ""
echo "Please ensure the following lines are in your ~/.tmux.conf:"
echo ""
echo 'bind-key -T copy-mode WheelUpPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" up"'
echo 'bind-key -T copy-mode WheelDownPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" down"'
echo 'bind-key -T copy-mode-vi WheelUpPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" up"'
echo 'bind-key -T copy-mode-vi WheelDownPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" down"'
echo ""
echo "Then reload tmux config via: tmux source-file ~/.tmux.conf"
