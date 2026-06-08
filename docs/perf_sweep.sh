#!/usr/bin/env bash
# Client FPS / draw-call sweep against the saladinci stress world.
#
#   docs/perf_sweep.sh <label> [counts...]
#
# For each unit count: clear stress rows, spawn a fresh army, then immediately
# drive a headless Chromium (ferridriver) to park over the centre and average
# window.__perf inside the brief stable window before the armies grind each other
# down. Writes a TSV summary and one JSON line per count.
#
# DB ISOLATION: only ever touches `saladinci` (never the user's `saladin`).
set -u
ROOT=/home/sashoush/Workspace/saladin
DB=saladinci
SRV=local
URL=${URL:-http://127.0.0.1:5180/}
VIEW=${VIEW:-26}
SETTLE=${SETTLE:-1200}
SAMPLE=${SAMPLE:-1600}
LABEL=${1:-before}
shift || true
COUNTS=("$@")
[ ${#COUNTS[@]} -eq 0 ] && COUNTS=(100 250 500 1000 2000)

SCRIPT="$ROOT/docs/perf_measure.mjs"
SUMMARY="$ROOT/.ferridriver/artifacts/perf-${LABEL}.tsv"
printf 'count\tonScreen\tfps\tframeMs\tdrawCalls\ttriangles\tprograms\n' > "$SUMMARY"

for n in "${COUNTS[@]}"; do
  spacetime call "$DB" -s "$SRV" debug_stress_clear >/dev/null 2>&1
  sleep 1
  spacetime call "$DB" -s "$SRV" debug_stress 1 "$n" >/dev/null 2>&1
  png="$ROOT/.ferridriver/artifacts/${LABEL}-${n}.png"
  raw=$(timeout 70 ferridriver run "$SCRIPT" -- "$URL" "${LABEL}-${n}" "$VIEW" "$SETTLE" "$SAMPLE" "$png" 2>/dev/null)
  row=$(printf '%s' "$raw" | node -e '
    let s=""; process.stdin.on("data",d=>s+=d).on("end",()=>{
      try { const v=JSON.parse(s).value;
        process.stdout.write(["'"$n"'",v.onScreenUnits,v.fps,v.frameMs,v.drawCalls,v.triangles,v.programs].join("\t"));
      } catch(e){ process.stdout.write("'"$n"'\tERR"); }
    });')
  printf '%s\n' "$row" | tee -a "$SUMMARY"
done

spacetime call "$DB" -s "$SRV" debug_stress_clear >/dev/null 2>&1
echo "=== summary ($SUMMARY) ==="
cat "$SUMMARY"
