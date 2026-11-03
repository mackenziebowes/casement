#!/usr/bin/env bash
# 24-hour soak (hub #2544). Two casement processes, a colour change every second:
#   shown  - a layer overlay on a headless output (frame callbacks flow, nobody is disturbed)
#   hidden - an xdg window on workspace 7 (no callbacks); once an hour it is moved to the
#            headless output and back, and the time to leave Stalled is logged.
# A sample a minute: cpu ticks, RSS, and the process's own counters from GET /state.
# usage: soak.sh [hours]   (default 24). Output: soak/soak-<start>.csv and soak/soak-<start>.summary
set -u
HOURS=${1:-24}
BIN=${CASEMENT_BIN:-$HOME/.cargo/bin/casement}
DIR=$(cd "$(dirname "$0")" && pwd)
STAMP=$(date +%Y%m%d-%H%M)
CSV=$DIR/soak-$STAMP.csv
SUM=$DIR/soak-$STAMP.summary
SHOWN_PORT=48140
HIDDEN_PORT=48141

hyprctl output create headless >/dev/null
sleep 0.5
HEAD=$(hyprctl monitors -j | python3 -I -c "
import json,sys
m=[m for m in json.load(sys.stdin) if m['name'].startswith('HEADLESS')][-1]
print(m['name'], m['activeWorkspace']['id'])")
HNAME=${HEAD% *}; HWS=${HEAD#* }

$BIN --layer --size 200x100 --output "$HNAME" --port $SHOWN_PORT --tick 1000 --exit-after $((HOURS*3600+120)) >/dev/null 2>&1 &
SHOWN_PID=$!
hyprctl dispatch "hl.dsp.exec_cmd(\"[workspace 7 silent] $BIN --size 200x100 --port $HIDDEN_PORT --tick 1000 --exit-after $((HOURS*3600+120))\")" >/dev/null
sleep 3
HIDDEN_PID=$(lsof -ti tcp:$HIDDEN_PORT -sTCP:LISTEN | head -1)
echo "started: headless $HNAME ws $HWS, shown pid $SHOWN_PID, hidden pid $HIDDEN_PID" | tee "$SUM"
echo "ts,label,cpu_ticks,rss_kb,state,paints,callbacks,allocs,resume_ms" > "$CSV"

addr_of() { hyprctl clients -j | python3 -I -c "
import json,sys
print([c['address'] for c in json.load(sys.stdin) if c['pid']==$1][0])"; }
sample() { # label pid port resume_ms
  local st; st=$(curl -s --max-time 2 http://127.0.0.1:$3/state | tr -d '\n')
  local ticks rss
  ticks=$(awk '{print $14+$15}' /proc/$2/stat 2>/dev/null || echo -1)
  rss=$(awk '/VmRSS/{print $2}' /proc/$2/status 2>/dev/null || echo -1)
  echo "$(date +%s),$1,$ticks,$rss,${st%% *},$(echo "$st" | sed -E 's/.*paints=([0-9]+).*/\1/'),$(echo "$st" | sed -E 's/.*callbacks=([0-9]+).*/\1/'),$(echo "$st" | sed -E 's/.*allocs=([0-9]+).*/\1/'),$4" >> "$CSV"
}

for minute in $(seq 1 $((HOURS*60))); do
  sleep 60
  resume=""
  if [ $((minute % 60)) -eq 30 ]; then   # hourly: show the hidden window on the headless output
    A=$(addr_of "$HIDDEN_PID")
    t0=$(date +%s%N)
    hyprctl dispatch "hl.dsp.window.move({ workspace = $HWS, window = \"address:$A\", follow = false })" >/dev/null
    for i in $(seq 1 300); do case "$(curl -s --max-time 1 http://127.0.0.1:$HIDDEN_PORT/state)" in Stalled*) ;; *) break;; esac; done
    resume=$(( ($(date +%s%N)-t0)/1000000 ))
    sleep 20
    hyprctl dispatch "hl.dsp.window.move({ workspace = 7, window = \"address:$A\", follow = false })" >/dev/null
  fi
  sample shown "$SHOWN_PID" $SHOWN_PORT ""
  sample hidden "$HIDDEN_PID" $HIDDEN_PORT "$resume"
  kill -0 "$SHOWN_PID" 2>/dev/null || { echo "shown process died at minute $minute" >> "$SUM"; }
  kill -0 "$HIDDEN_PID" 2>/dev/null || { echo "hidden process died at minute $minute" >> "$SUM"; }
done

kill "$SHOWN_PID" 2>/dev/null
kill "$HIDDEN_PID" 2>/dev/null
hyprctl output remove "$HNAME" >/dev/null
echo "finished $(date)" >> "$SUM"
