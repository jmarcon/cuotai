#!/bin/sh
# Captures Waybar running Cuotai's example module, in a container (PRD 0014).
#
# Builds the image from this directory and runs a headless sway session in
# it; the PNGs land in the output directory. Needs Docker (or a compatible
# engine); works on Linux and macOS hosts, amd64 or arm64.
#
# Usage: apps/desktop/tools/widget-shots/linux/capture.sh [widget.json] [out-dir]
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../../../.." && pwd)
fixture=${1:-"$here/../fixtures/widget.json"}
out=${2:-"$root/docs/assets/screenshots/widgets"}
image=cuotai-widget-shots-linux

mkdir -p "$out"
docker build -q -t "$image" "$here" >/dev/null

# Copy in and out instead of bind-mounting, so the script also works when
# the repository is outside the engine's shared folders (Docker Desktop).
container=$(docker create -e SHOT_DEBUG="${SHOT_DEBUG:-}" "$image")
trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT
docker cp "$fixture" "$container:/fixture/widget.json"
docker start -a "$container"
docker cp "$container:/out/linux-waybar.png" "$out/linux-waybar.png"
