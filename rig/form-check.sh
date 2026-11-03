#!/usr/bin/env bash
# Can a person fill in a Form? Posts rig/trees/more.json, clicks the Title field with a real pointer, types
# "helo", Left, "l", End, "!", Home, "X", Delete with real key events (wtype: expect "Xello!"), then a click at the left edge of the field and "Y": expect "YXello!", clicks the submit button, and reads GET /events. Then submits an
# empty form to see it refused (required field).
set -eu
cd "$(dirname "$0")/.."
eval "$(bash rig/headless.sh 1 | sed 's/^/export /')"
cleanup() { for p in $(lsof -ti tcp:9341 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; sleep 1; }
trap cleanup EXIT
./target/release/casement --port 9341 >/tmp/form-check.log 2>&1 &
sleep 1.5
curl -sf -X POST localhost:9341/intui --data-binary @rig/trees/more.json >/dev/null
sleep 0.5
at() { # centre of node $1 in the window, plus the window's (21,21) origin on the output
  curl -s localhost:9341/tree | python3 -c "
import json,sys
def walk(n):
    yield n
    for c in n.get('children',[]): yield from walk(c)
t=json.load(sys.stdin)
n=next(n for n in walk(t) if n['id']=='$1')
b=n['bounds']; print(21+b['x']+b['w']//2, 21+b['y']+b['h']//2)"
}
read -r tx ty <<<"$(at new-idea.title)"; read -r sx sy <<<"$(at new-idea.submit)"; read -r ox oy <<<"$(at new-idea.source)"
v=./target/release/examples/vptr
echo "submit with the Title empty (expect nothing, and a reason in the log):"
$v 600 600 1280 720 >/dev/null; $v $sx $sy 1280 720 click >/dev/null; curl -s localhost:9341/events; grep "not submitted" /tmp/form-check.log || true
$v 600 600 1280 720 >/dev/null; $v $tx $ty 1280 720 click >/dev/null
wtype -s 300 helo -k Left l -k End "!" -k Home X -k Delete -s 1500 & typing=$!
sleep 1.2; curl -s localhost:9341/frame.png -o /tmp/form-typing.png   # while the keyboard is ours: the caret shows
wait "$typing"
# Click at the far left edge of the Title field: the caret goes to the start; then type "Y".
$v 600 600 1280 720 >/dev/null; $v $((21+20)) $ty 1280 720 click >/dev/null
wtype -s 300 Y -s 300
echo "state after typing: $(curl -s localhost:9341/state)"
$v 600 600 1280 720 >/dev/null; $v $ox $oy 1280 720 click >/dev/null   # the select: None -> self
$v 600 600 1280 720 >/dev/null; $v $sx $sy 1280 720 click >/dev/null
echo "events after filling in and submitting:"; curl -s localhost:9341/events
