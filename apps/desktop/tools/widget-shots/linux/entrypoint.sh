#!/bin/sh
# Runs inside the container: starts a headless sway with Waybar, feeds the
# example module the fixture and captures the bar with grim.
#
# Input: /fixture/widget.json. Output: /out/linux-waybar.png.
# SHOT_DEBUG=1 traces the script and prints the sway and Waybar logs.
set -eu
if [ -n "${SHOT_DEBUG:-}" ]; then set -x; fi

export XDG_RUNTIME_DIR=/tmp/xdg
export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER=pixman
mkdir -p -m 0700 "$XDG_RUNTIME_DIR"
mkdir -p /tmp/cuotai
# The bar text carries no ages, so the fixture is used as it is.
cp /fixture/widget.json /tmp/cuotai/widget.json

dbus-run-session -- sway --config /etc/widget-shots/sway.conf >/tmp/sway.log 2>&1 &
sway_pid=$!

# Wait for the compositor socket, then for Waybar to draw its first frame.
tries=0
until ls "$XDG_RUNTIME_DIR"/sway-ipc.* >/dev/null 2>&1; do
    tries=$((tries + 1))
    if [ "$tries" -gt 50 ]; then cat /tmp/sway.log >&2; exit 1; fi
    sleep 0.2
done
export WAYLAND_DISPLAY=wayland-1
sleep 4

# The bar: 30 logical pixels high across the 1100-point-wide output (scale 2).
grim -g "0,0 1100x30" /out/linux-waybar.png

if [ -n "${SHOT_DEBUG:-}" ]; then cat /tmp/sway.log /tmp/waybar.log >&2 || true; fi
kill "$sway_pid" 2>/dev/null || true
echo "wrote /out/linux-waybar.png"
