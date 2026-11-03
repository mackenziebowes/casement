#!/usr/bin/env bash
# Is casement sharp at scale $1? Starts the headless rig at that scale, runs casement on it, posts a
# tree, takes a grim of what the compositor shows, and reports (1) the colours at a button edge
# (rig/sharp.sh: 2 is sharp) and (2) how many pixels of the window differ between our own
# /frame.png and the compositor's picture (0 means it shows our pixels unresampled, text included).
# With a second argument, an IntUI tree file: it is posted to /intui instead (the edge test is then
# skipped, and the face box used to line the two pictures up is the first positive-tone button).
# Usage: scale-check.sh <scale> [intui-tree.json]     Needs target/release/casement built.
set -eu
cd "$(dirname "$0")/.."
scale=${1:?usage: scale-check.sh <scale>}
out=$(mktemp -d)
eval "$(bash rig/headless.sh "$scale" | sed 's/^/export /')"
keep=${KEEP:-}
cleanup() { for p in $(lsof -ti tcp:9341 -sTCP:LISTEN); do kill "$p"; done; hyprctl -i "$HYPRLAND_INSTANCE_SIGNATURE" dispatch exit >/dev/null; sleep 1; }
trap cleanup EXIT
./target/release/casement --port 9341 >"$out/casement.log" 2>&1 &
sleep 1.5
tree='{"id":"root","kind":"panel","tone":"neutral","bounds":{"x":0,"y":0,"w":2000,"h":2000},"children":[
 {"id":"b","kind":"button","tone":"positive","bounds":{"x":17,"y":13,"w":151,"h":43},"text":""},
 {"id":"t","kind":"button","tone":"critical","bounds":{"x":200,"y":13,"w":151,"h":43},"text":"Flip colour"},
 {"id":"l","kind":"label","tone":"neutral","bounds":{"x":17,"y":70,"w":300,"h":24},"text":"Sharp text: Hamburgefonstiv 0123"}]}'
if [ -n "${2:-}" ]; then curl -sf -X POST localhost:9341/intui --data-binary @"$2" >/dev/null; else curl -s -X POST localhost:9341/tree -d "$tree" >/dev/null; fi
sleep 0.7
grim "$out/grim.png"
curl -s localhost:9341/frame.png -o "$out/own.png"
echo "scale $scale: $(identify -format '%wx%h' "$out/own.png") device pixels"
[ -n "${2:-}" ] || bash rig/sharp.sh "$out/grim.png"
# Where is the window's top-left in the grim? The face box in the grim minus the same in our own frame.
read -r gx gy <<<"$(magick "$out/grim.png" -fill black +opaque '#2E9E5B' -fill white -opaque '#2E9E5B' -trim -format '%X %Y' info: | tr -d '+')"
read -r ox oy <<<"$(magick "$out/own.png" -fill black +opaque '#2E9E5B' -fill white -opaque '#2E9E5B' -trim -format '%X %Y' info: | tr -d '+')"
w=$(identify -format %w "$out/own.png"); h=$(identify -format %h "$out/own.png")
magick "$out/grim.png" -crop "${w}x${h}+$((gx-ox))+$((gy-oy))" +repage "$out/shown.png"
# Leave out 4 pixels all round: Hyprland draws its own border and rounded corners there.
inner="$((w-8))x$((h-8))+4+4"
diff=$(magick compare -metric AE "$out/shown.png[$inner]" "$out/own.png[$inner]" null: 2>&1 || true)
echo "${diff%% *} of $(((w-8)*(h-8))) pixels differ between our frame and the compositor's picture"
echo "files: $out"
