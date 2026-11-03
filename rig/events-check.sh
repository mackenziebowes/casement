#!/usr/bin/env bash
# Does a real pointer click on a Choice option come out as an IntUI press event? Starts the rig,
# posts rig/trees/mixed.json, clicks the "Keep going" button with vptr, reads GET /events.
set -eu
cd "$(dirname "$0")/.."
eval "$(bash rig/headless.sh 1 | sed 's/^/export /')"
cleanup() { for p in $(lsof -ti tcp:9341 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; sleep 1; }
trap cleanup EXIT
./target/release/casement --port 9341 >/dev/null 2>&1 &
sleep 1.5
curl -sf -X POST localhost:9341/intui --data-binary @rig/trees/mixed.json >/dev/null
sleep 0.5
# The window sits at (21,21) on the 1280x720 output; the button is at (16..,252..292) in the window.
./target/release/examples/vptr 400 400 1280 720 >/dev/null
./target/release/examples/vptr $((21+100)) $((21+270)) 1280 720 click >/dev/null
echo "events after a pointer click:"; curl -s localhost:9341/events
echo "events after POST /click/next.stop (the Choice is used up: expect nothing):"; curl -s -X POST localhost:9341/click/next.stop >/dev/null; curl -s localhost:9341/events
echo "events read again (drained, expect nothing):"; curl -s localhost:9341/events
