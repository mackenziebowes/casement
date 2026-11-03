#!/usr/bin/env bash
# Copy and paste in a text field with real key events. Types "abc" in the Title field and presses
# Ctrl+C (wl-paste, a separate client, must read "abc"); then another program copies "xyz" and
# Ctrl+V pastes it ("abcxyz" goes out in the submit event); Ctrl+X cuts the field empty.
set -eu
cd "$(dirname "$0")/.."
eval "$(bash rig/headless.sh 1 | sed 's/^/export /')"
cleanup() { for p in $(lsof -ti tcp:9341 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; sleep 1; }
trap cleanup EXIT
./target/release/casement --port 9341 --log >/tmp/clip.log 2>&1 &
sleep 1.5
curl -sf -X POST localhost:9341/intui --data-binary @rig/trees/more.json >/dev/null
sleep 0.5
at() { curl -s localhost:9341/tree | python3 -c "
import json,sys
def walk(n):
    yield n
    for c in n.get('children',[]): yield from walk(c)
n=next(n for n in walk(json.load(sys.stdin)) if n['id']=='$1'); b=n['bounds']; print(21+b['x']+b['w']//2, 21+b['y']+b['h']//2)"; }
read -r tx ty <<<"$(at new-idea.title)"; read -r sx sy <<<"$(at new-idea.submit)"
v=./target/release/examples/vptr
$v 600 600 1280 720 >/dev/null; $v $tx $ty 1280 720 click >/dev/null
wtype -s 300 abc -M ctrl -k c -m ctrl -s 300
echo "after Ctrl+C, another client pastes: '$(wl-paste -n 2>&1)'"
printf xyz | wl-copy
wtype -s 300 -M ctrl -k v -m ctrl -s 300
$v 600 600 1280 720 >/dev/null; $v $sx $sy 1280 720 click >/dev/null
echo "submit after Ctrl+V of 'xyz': $(curl -s localhost:9341/events)"
