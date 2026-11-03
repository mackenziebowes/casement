#!/usr/bin/env bash
# Does casement show what the web render shows for the same IntUI tree? Starts the headless rig,
# posts <tree.json> to casement's /intui, reads /tree and runs rig/web-compare.ts against it.
# Usage: intui-check.sh <tree.json>      Needs target/release/casement built and ~/dev/projects/intui.
set -eu
cd "$(dirname "$0")/.."
tree=$(realpath "${1:?usage: intui-check.sh <tree.json>}")
out=$(mktemp -d)
eval "$(bash rig/headless.sh 1 | sed 's/^/export /')"
cleanup() { for p in $(lsof -ti tcp:9341 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; sleep 1; }
trap cleanup EXIT
# The web render in happy-dom has no layout, so it draws charts 560 px wide; a 592 px window (16 px margins) gives ours the same 560.
./target/release/casement --port 9341 --size 592x900 >"$out/casement.log" 2>&1 &
sleep 1.5
curl -sf -X POST localhost:9341/intui --data-binary @"$tree"
sleep 0.7
curl -s localhost:9341/tree -o "$out/tree.json"
curl -s localhost:9341/frame.png -o "$out/frame.png"
echo "files: $out"
bun rig/web-compare.ts "$tree" "$out/tree.json"
