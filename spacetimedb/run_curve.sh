#!/usr/bin/env bash
# Drive the as-is scaling curve: for each (matches, unitsPerMatch) level, clear the
# DB, spawn, record the live unit count at window start, measure achieved Hz over
# WINDOW seconds. Combat is slow enough that the count stays near peak across a 10s
# window, so the recorded count is the load under test.
DB=saladinci
SRV=local
WINDOW="${1:-10}"

count_units() {
  spacetime sql "$DB" -s "$SRV" "SELECT COUNT(*) AS n FROM unit" 2>/dev/null \
    | awk 'NR==3 { gsub(/ /,"",$1); print $1 }'
}
read_counts() {
  spacetime sql "$DB" -s "$SRV" "SELECT move_ticks, combat_ticks FROM tick_count" 2>/dev/null \
    | awk -F'|' 'NR==3 { gsub(/ /,"",$1); gsub(/ /,"",$2); print $1, $2 }'
}

measure_level() {
  local matches="$1" upm="$2"
  spacetime call "$DB" -s "$SRV" debug_stress_clear >/dev/null 2>&1
  sleep 1
  spacetime call "$DB" -s "$SRV" debug_stress "$matches" "$upm" >/dev/null 2>&1
  sleep 1
  local n_start; n_start=$(count_units)
  read -r M1 C1 < <(read_counts); local T1; T1=$(date +%s.%N)
  sleep "$WINDOW"
  read -r M2 C2 < <(read_counts); local T2; T2=$(date +%s.%N)
  local n_end; n_end=$(count_units)
  python3 - "$matches" "$upm" "$n_start" "$n_end" "$M1" "$C1" "$M2" "$C2" "$T1" "$T2" <<'PY'
import sys
matches,upm,ns,ne,m1,c1,m2,c2,t1,t2=sys.argv[1:11]
ns,ne,m1,c1,m2,c2=int(ns),int(ne),int(m1),int(c1),int(m2),int(c2)
dt=float(t2)-float(t1)
mhz=(m2-m1)/dt; chz=(c2-c1)/dt
print(f"matches={matches:>2} upm={upm:>4} | units {ns}->{ne} | move {mhz:6.2f} Hz ({100*mhz/20:3.0f}%) | combat {chz:5.2f} Hz ({100*chz/5:3.0f}%)")
PY
}

# Empty baseline first.
measure_level 0 0
for spec in "$@"; do :; done
# Levels passed as remaining args as "matches:upm"; default curve if none.
shift || true
LEVELS=("$@")
if [ ${#LEVELS[@]} -eq 0 ]; then
  LEVELS=("2:50" "2:120" "2:245" "2:495")
fi
for lvl in "${LEVELS[@]}"; do
  m="${lvl%%:*}"; u="${lvl##*:}"
  measure_level "$m" "$u"
done
spacetime call "$DB" -s "$SRV" debug_stress_clear >/dev/null 2>&1
