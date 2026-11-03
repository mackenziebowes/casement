#!/usr/bin/env bash
# How sharp is a button's edge in a screenshot? Finds the box of the face colour in <png> (a grim
# of the rig, so it is what the compositor shows), then counts the colours in that box grown by 3
# pixels. A sharp edge has two (face and what it sits on); every blended pixel adds one.
# Usage: sharp.sh <png> [face colour, default #2E9E5B]
set -eu
png=$1; face=${2:-#2E9E5B}
box=$(magick "$png" -fill black +opaque "$face" -fill white -opaque "$face" -trim -format '%wx%h%X%Y' info:)
echo "face box $box"
[[ $box =~ ^([0-9]+)x([0-9]+)\+([0-9]+)\+([0-9]+)$ ]] || { echo "no face-coloured pixels in $png" >&2; exit 1; }
w=${BASH_REMATCH[1]} h=${BASH_REMATCH[2]} x=${BASH_REMATCH[3]} y=${BASH_REMATCH[4]}
magick "$png" -crop "$((w+6))x$((h+6))+$((x-3))+$((y-3))" +repage -format '%k colours in the box grown by 3 px\n' info:
