#!/usr/bin/env bash
# Own picture of a scene: starts the headless rig, runs casement at <WxH>, posts <scene.json> to /tree and saves
# casement's own /frame.png to <out.png>. Several scenes: shot.sh <WxH> <out-dir> <scene.json>... names each <scene>.png.
# Usage: shot.sh <WxH> <out-dir> <scene.json>...     Needs target/release/casement built.
set -eu
cd "$(dirname "$0")/.."
size=${1:?usage: shot.sh <WxH> <out-dir> <scene.json>...}; dir=$2; shift 2
mkdir -p "$dir"
eval "$(bash rig/headless.sh 1 | sed 's/^/export /')"
cleanup() { for p in $(lsof -ti tcp:9342 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; for _ in $(seq 30); do [ -e "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY.lock" ] && [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] || break; sleep 0.2; done; }
trap cleanup EXIT
./target/release/casement --size "$size" --port 9342 >"$dir/casement.log" 2>&1 &
sleep 1.5
for scene in "$@"; do
  code=$(curl -s -o "$dir/reply.txt" -w "%{http_code}" -X POST localhost:9342/tree --data-binary @"$scene"); [ "$code" = 200 ] || { echo "casement refused $scene ($code): $(cat "$dir/reply.txt")" >&2; exit 1; }
  sleep 0.5
  curl -s localhost:9342/frame.png -o "$dir/$(basename "$scene" .json).png"
done
echo "$dir"
