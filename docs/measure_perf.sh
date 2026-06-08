#!/usr/bin/env bash
# Measure achieved move/combat tick Hz on saladinci for a sweep of unit counts.
# Polls the tick_count singleton twice over a wall-clock window and divides the
# delta by elapsed seconds: target 20Hz move / 5Hz combat. Drift = achieved<target.
set -u
DB=saladinci
SRV=local
WINDOW=${WINDOW:-10}   # seconds per measurement window
WARMUP=${WARMUP:-4}    # seconds to let the scheduler settle after spawn

sql() { spacetime sql "$DB" -s "$SRV" "$1" 2>/dev/null | tail -1 | tr -d ' '; }
call() { spacetime call "$DB" -s "$SRV" "$@" 2>/dev/null; }

measure_one() {
  local matches=$1 perMatch=$2
  call debug_stress_clear
  sleep 1
  call debug_stress "$matches" "$perMatch"
  local units; units=$(sql "SELECT COUNT(*) AS n FROM unit")
  sleep "$WARMUP"
  local m0 c0 m1 c1
  m0=$(sql "SELECT move_ticks AS n FROM tick_count WHERE id=0")
  c0=$(sql "SELECT combat_ticks AS n FROM tick_count WHERE id=0")
  local t0; t0=$(date +%s.%N)
  sleep "$WINDOW"
  m1=$(sql "SELECT move_ticks AS n FROM tick_count WHERE id=0")
  c1=$(sql "SELECT combat_ticks AS n FROM tick_count WHERE id=0")
  local t1; t1=$(date +%s.%N)
  awk -v m0="$m0" -v m1="$m1" -v c0="$c0" -v c1="$c1" -v t0="$t0" -v t1="$t1" \
      -v units="$units" -v mm="$matches" -v pm="$perMatch" 'BEGIN{
    el=t1-t0; mh=(m1-m0)/el; ch=(c1-c0)/el;
    printf "matches=%-4s perMatch=%-5s units=%-6s | moveHz=%6.2f/20  combatHz=%6.2f/5  (window=%.1fs)\n", mm, pm, units, mh, ch, el;
  }'
}

echo "=== Saladin perf sweep (WINDOW=${WINDOW}s WARMUP=${WARMUP}s) ==="
# args: pairs of "matches perMatch"; default sweep one match at increasing sizes
if [ "$#" -gt 0 ]; then
  while [ "$#" -gt 0 ]; do measure_one "$1" "$2"; shift 2; done
else
  for pm in 100 250 500 1000 2000 4000; do measure_one 1 "$pm"; done
fi
call debug_stress_clear >/dev/null
echo "=== done ==="
