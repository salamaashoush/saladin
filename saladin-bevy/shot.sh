#!/bin/bash
# shot.sh <outfile> [env assignments...]  — serialized screenshot run with
# stale-file protection. Usage: ./shot.sh /tmp/x.png SALADIN_ZOOM=6 SALADIN_PRESET=3
set -u
out="$1"; shift
cd "$(dirname "$0")"
rm -f /tmp/saladin_shot.png
env SALADIN_AUTO=1 "$@" timeout 30 cargo run -p saladin-client --bin saladin-client >/tmp/shot_run.log 2>&1
if [ ! -f /tmp/saladin_shot.png ]; then
  echo "FAILED — no screenshot produced; log tail:"
  tail -5 /tmp/shot_run.log
  exit 1
fi
cp /tmp/saladin_shot.png "$out"
echo "saved $out"
