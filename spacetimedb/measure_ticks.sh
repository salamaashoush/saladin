#!/usr/bin/env bash
# DEV harness: poll tick_count twice over WINDOW seconds and print achieved Hz.
# Usage: measure_ticks.sh <window_seconds> <label>
WINDOW="${1:-10}"
LABEL="${2:-}"
DB=saladinci
SRV=local

read_counts() {
  spacetime sql "$DB" -s "$SRV" "SELECT move_ticks, combat_ticks FROM tick_count" 2>/dev/null \
    | awk -F'|' 'NR==3 { gsub(/ /,"",$1); gsub(/ /,"",$2); print $1, $2 }'
}

read -r M1 C1 < <(read_counts)
T1=$(date +%s.%N)
sleep "$WINDOW"
read -r M2 C2 < <(read_counts)
T2=$(date +%s.%N)

python3 - "$M1" "$C1" "$M2" "$C2" "$T1" "$T2" "$LABEL" <<'PY'
import sys
m1,c1,m2,c2,t1,t2,label = sys.argv[1:8]
m1,c1,m2,c2=int(m1),int(c1),int(m2),int(c2)
dt=float(t2)-float(t1)
mhz=(m2-m1)/dt
chz=(c2-c1)/dt
print(f"[{label}] window={dt:.2f}s  move {m1}->{m2} = {mhz:.2f} Hz (target 20, {100*mhz/20:.0f}%)  combat {c1}->{c2} = {chz:.2f} Hz (target 5, {100*chz/5:.0f}%)")
PY
