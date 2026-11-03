#!/usr/bin/env bash
# Does a long tree scroll? Posts the hub overview into a 592x700 window, presses Page Down twice, then
# End, then Home (real key events), and reads the scroll offset from /state each time.
set -eu
cd "$(dirname "$0")/.."
eval "$(bash rig/headless.sh 1 | sed 's/^/export /')"
cleanup() { for p in $(lsof -ti tcp:9341 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; sleep 1; }
trap cleanup EXIT
./target/release/casement --port 9341 --size 592x700 >/dev/null 2>&1 &
sleep 1.5
curl -sf -X POST localhost:9341/intui --data-binary @rig/trees/hub-overview.json >/dev/null
sleep 0.5
s() { curl -s localhost:9341/state | grep -o 'scroll=[0-9]*'; }
echo "at start: $(s)"
wtype -s 300 -k Next -s 300; echo "after Page Down: $(s)"
wtype -s 300 -k Next -s 300; echo "after a second Page Down: $(s)"
wtype -s 300 -k End -s 300; echo "after End: $(s)"
curl -s localhost:9341/frame.png -o /tmp/scroll-end.png
wtype -s 300 -k Home -s 300; echo "after Home: $(s)"
# The wheel: the pointer must be over the window first.
./target/release/examples/vptr 400 300 1280 720 >/dev/null
./target/release/examples/vptr 400 300 1280 720 scroll 30 >/dev/null; echo "after a wheel notch of 30 px down: $(s)"
./target/release/examples/vptr 400 300 1280 720 scroll -10 >/dev/null; echo "after 10 px back up: $(s)"
