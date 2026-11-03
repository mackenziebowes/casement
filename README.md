# casement

A native Wayland window we paint ourselves, that an agent can drive from outside: it reads the
node tree, presses a button by id, and takes a PNG of the window's own buffer. MIT licensed.

## Build

Casement reuses IntUI's parser and layout, so clone both side by side (`Cargo.toml` finds IntUI at `../intui`), then build:

```
git clone https://github.com/mackenziebowes/intui
git clone https://github.com/mackenziebowes/casement
cd casement
cargo build --release
```

## Run

```
target/release/casement --port 9341            # an xdg window; --layer for an overlay (--keys lets a click give it the keyboard); --log for state changes
curl localhost:9341/frame.png -o frame.png     # the pixels we painted
curl localhost:9341/state                      # pacing, counters, and hover/press/focus
curl localhost:9341/tree                       # the node tree as JSON
curl -X POST localhost:9341/intui --data-binary @rig/trees/mixed.json   # show an IntUI tree
curl -X POST localhost:9341/click/next.keep    # press a button by id
curl localhost:9341/events                     # IntUI press events people caused, one JSON line each
```

## What is in it

1. `src/pace.rs`: when the window paints and when it waits (one state machine, frame callbacks).
2. `src/input.rs`: pointer, keyboard and focus as one state machine; the seat handlers in `main.rs` only translate.
3. `src/paint.rs`, `src/text.rs`: tone to colour, text with cosmic-text, drawn in device pixels at any fractional scale.
4. `src/intui_scene.rs`: an IntUI tree (parsed by `../intui`) laid out as nodes.
5. `src/inspect.rs`: the localhost socket.
6. `bin/casement-show`: `hub map tree --json | casement-show` shows any IntUI tree in a running casement; `--events` prints what people do with it.
7. `bin/casement-follow --every 5 --on-event 'CMD' -- hub map tree --json`: keeps the window in step with any command that prints a tree, and gives each IntUI event to CMD on stdin.

## Checks (all drive the real program from outside)

`rig/headless.sh [scale]` starts a Hyprland of its own with no access to the real keyboard, so tests never touch the desktop.

1. `rig/scale-check.sh 1.25`: a button edge is two colours and the window shows our pixels unresampled.
2. `rig/intui-check.sh rig/trees/mixed.json`: the words and buttons match the web render of the same IntUI tree.
3. `rig/events-check.sh`: a real pointer click on a Choice option comes out as an IntUI press event.
4. `rig/form-check.sh`: a Form can be filled in with real key events and submitted; an empty required field is refused.

Needs `hyprland`, `bwrap`, `grim`, `wtype` and ImageMagick; `intui-check.sh` also needs `bun` and a checkout of IntUI next to this one.
