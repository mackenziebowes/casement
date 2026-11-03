#!/usr/bin/env bash
# A headless Hyprland of its own, so input tests never touch the live desktop.
# LIBSEAT_BACKEND=noop stops it taking the login session (without it Hyprland crashes:
# "Could not take control of session"). Usage: headless.sh [scale]. Prints WAYLAND_DISPLAY and the instance signature.
# It runs under bwrap with /dev/input emptied: without that it read the real
# keyboard (Mackenzie's typing pressed buttons in the test window) 
# Stop it with `hyprctl -i <signature> dispatch exit`.
set -eu
conf=$(mktemp -d)/hyprland.conf
printf 'monitor=HEADLESS-1,1280x720@60,0x0,%s\n' "${1:-1}" > "$conf"
# An explicit socket no one holds: left to itself Hyprland takes the first name, which is the desktop's (wayland-1), and dies on its lock (#2810).
# Hyprland takes a socket only as a pair, --socket NAME with --wayland-fd FD (its socket handover), so we make the listening socket here.
new=wayland-rig-$$
python3 -I - "$XDG_RUNTIME_DIR/$new" "$new" "$conf" "${conf%/*}/log" <<'PY' &
import os, socket, subprocess, sys
path, name, conf, log = sys.argv[1:]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(path)
s.listen(32)
env = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE")}
env.update(LIBSEAT_BACKEND="noop", AQ_BACKENDS="headless")
subprocess.run(["bwrap", "--dev-bind", "/", "/", "--tmpfs", "/dev/input", "Hyprland", "--socket", name, "--wayland-fd", str(s.fileno()), "-c", conf],
               env=env, stdout=open(log, "w"), stderr=subprocess.STDOUT, pass_fds=[s.fileno()], start_new_session=True)
os.unlink(path)
PY
for _ in $(seq 50); do
  sleep 0.2
  [ -S "$XDG_RUNTIME_DIR/$new" ] && break
done
[ -S "$XDG_RUNTIME_DIR/$new" ] || new=
# The caller runs this as eval "$(headless.sh)": a failure must stop the caller, or it goes on against the real desktop.
[ -n "${new:-}" ] || { echo "headless Hyprland did not start; log: ${conf%/*}/log" >&2; echo "echo 'rig: headless Hyprland did not start, stopping' >&2; exit 1"; exit 1; }
sig=$(ls -t "$XDG_RUNTIME_DIR/hypr" | head -1)
# The socket shows up before Hyprland takes commands: wait until hyprctl can talk to it (up to 10 s) before using it.
for _ in $(seq 50); do hyprctl -i "$sig" version >/dev/null 2>&1 && break; sleep 0.2; done
hyprctl -i "$sig" version >/dev/null 2>&1 || { echo "headless Hyprland ($sig) never answered hyprctl; log: ${conf%/*}/log" >&2; echo "echo 'rig: Hyprland never answered, stopping' >&2; exit 1"; exit 1; }
# The rig has no monitor of its own (HDMI-A-1 comes up 0x0), so make a headless one (HEADLESS-1, 1280x720 at scale $1, default 1).
hyprctl -i "$sig" output create headless >/dev/null
hyprctl -i "$sig" keyword monitor "HDMI-A-1,disable" >/dev/null
hyprctl -i "$sig" dismissnotify >/dev/null # its warning banners would sit over screenshots
echo "WAYLAND_DISPLAY=$new"
echo "HYPRLAND_INSTANCE_SIGNATURE=$sig"
