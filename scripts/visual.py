#!/usr/bin/env python3
"""Visual regression for Saladin: shoot named scenes, diff them against
baselines, and report what moved.

Each scene is one `SALADIN_AUTO` mode plus a camera. The client is launched,
driven over devctl, and the clock is FROZEN before the shutter — every pose in
the renderer is a function of wall time, so without that two shots of the same
world are never the same image and a pixel diff means nothing.

    python3 scripts/visual.py --bless            # record baselines
    python3 scripts/visual.py                    # compare against them
    python3 scripts/visual.py --scenes farm,units --out /tmp/vis

Differences land in the output directory as `<scene>.png`, `<scene>.base.png`
and `<scene>.diff.png`, so a change you meant to make is one look away from
being blessed.

Needs ImageMagick (`magick`) for the comparison, which the repo already uses
for cropping screenshots.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, __file__.rsplit("/", 1)[0])

from devctl import Devctl, DevctlError, wait_for  # noqa: E402

CLIENT = "target/debug/saladin-client"
BASELINE = "tests/visual"
#: Fraction of pixels allowed to differ before a scene is called changed. Not
#: zero: the driver is a real GPU and the last bit of a blend is not a promise.
TOLERANCE = 0.002

#: One frame of game time per rendered frame. At 0.05 the fixed-update sim runs
#: exactly one 20 Hz tick per frame, so a scene is pinned by TICK COUNT and the
#: animation phase comes out the same every run.
FRAME_DT = 0.05

#: A scene is a SALADIN_AUTO mode, the tick to freeze at, and the camera to look
#: with. `subject` picks what the camera centres on out of the state capture, so
#: a scene follows its subject across worldgen changes rather than staring at a
#: fixed coordinate that a new map moves out from under it.
SCENES: dict[str, dict] = {
    "menu": {"auto": "menu", "tick": 0, "wait": 4.0, "camera": None},
    "town": {"auto": "1", "tick": 120, "subject": ("buildings", "Keep"), "zoom": 22},
    "keep": {"auto": "1", "tick": 120, "subject": ("buildings", "Keep"), "zoom": 7},
    "units": {"auto": "units", "tick": 140, "subject": ("buildings", "Keep"), "zoom": 12},
    "farm": {"auto": "farm", "tick": 200, "subject": ("buildings", "Farm"), "zoom": 9},
    "harbour": {"auto": "harbour", "tick": 140, "subject": ("buildings", "Harbour"), "zoom": 9},
    "ferry": {"auto": "ferry", "tick": 200, "subject": ("units", "Barge"), "zoom": 10, "yaw": 2},
    "battle": {"auto": "battle", "tick": 220, "subject": ("units", "Spearman"), "zoom": 12},
}


def shoot(scene: str, spec: dict, out: str, port: int, keep_log: str) -> tuple[str, dict]:
    """Run the client once, freeze it, and take one shot. Returns the render
    inventory alongside the image, because half of what a bad frame tells you
    is a count, not a pixel."""
    png = os.path.join(out, f"{scene}.png")
    if os.path.exists(png):
        os.remove(png)
    env = dict(
        os.environ,
        SALADIN_DEVCTL=str(port),
        SALADIN_AUTO=spec["auto"],
        SALADIN_SHOT_AT="999",  # the harness owns the shutter, not the timer
        SALADIN_FIXED_DT=str(FRAME_DT),
    )
    if "seed" in spec:
        env["SALADIN_SEED"] = str(spec["seed"])
    with open(keep_log, "w") as log:
        proc = subprocess.Popen([CLIENT], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        g = wait_for(port, seconds=120)
        # a scene is pinned by TICK, not by wall clock: with FRAME_DT set the
        # whole app is a function of frame count, and waiting a number of
        # seconds instead would put a different world in front of the camera
        # every run
        want = spec.get("tick", 0)
        if want:
            # the CLIENT stops itself at the tick. Polling and then asking it to
            # pause cannot be exact — frames keep rendering between the poll and
            # the request, and one frame of slip is a different pose
            g.request({"query": "clock", "pause_at": want})
            deadline = time.time() + 120
            while not g.request({"query": "clock"}).get("frozen"):
                if time.time() > deadline:
                    raise DevctlError(f"{scene} never reached tick {want}")
                time.sleep(0.05)
        else:
            if spec.get("wait"):
                time.sleep(spec["wait"])
            g.request({"query": "clock", "pause": True})
        camera = None
        if spec.get("subject"):
            kind_group, kind = spec["subject"]
            s = g.state(kinds=[kind_group])
            rows = [r for r in getattr(s, kind_group) if r.get("kind") == kind]
            if rows:
                camera = {"pos": rows[0]["pos"], "zoom": spec.get("zoom", 12)}
                if "yaw" in spec:
                    camera["yaw"] = spec["yaw"]
        elif spec.get("camera"):
            camera = spec["camera"]
        g.screenshot(png, camera=camera)
        render = g.request({"query": "render"})
        g.request({"query": "clock", "pause": False})
        return png, render
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=15)
        except subprocess.TimeoutExpired:
            proc.kill()


def compare(shot: str, base: str, diff: str) -> float | None:
    """Fraction of pixels that differ, or None when the sizes disagree."""
    r = subprocess.run(
        ["magick", "compare", "-metric", "AE", "-fuzz", "1%", shot, base, diff],
        capture_output=True,
        text=True,
    )
    out = (r.stderr or "").strip().split()
    if not out:
        return None
    try:
        differing = float(out[0])
    except ValueError:
        return None
    size = subprocess.run(
        ["magick", "identify", "-format", "%w %h", shot], capture_output=True, text=True
    ).stdout.split()
    if len(size) != 2:
        return None
    return differing / (int(size[0]) * int(size[1]))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--scenes", default="", help="comma list; default is all of them")
    ap.add_argument("--out", default="/tmp/saladin-visual")
    ap.add_argument("--baseline", default=BASELINE)
    ap.add_argument("--bless", action="store_true", help="record what is shot as the new baseline")
    ap.add_argument("--port", type=int, default=7700)
    ap.add_argument("--tolerance", type=float, default=TOLERANCE)
    args = ap.parse_args()

    if not os.path.exists(CLIENT):
        print(f"{CLIENT} is not built: cargo build -p saladin-client --bin saladin-client")
        return 2
    names = [s for s in args.scenes.split(",") if s] or list(SCENES)
    unknown = [n for n in names if n not in SCENES]
    if unknown:
        print(f"unknown scene(s): {', '.join(unknown)}; known: {', '.join(SCENES)}")
        return 2

    os.makedirs(args.out, exist_ok=True)
    os.makedirs(args.baseline, exist_ok=True)
    changed, missing, broken = [], [], []
    for i, name in enumerate(names):
        log = os.path.join(args.out, f"{name}.log")
        try:
            png, render = shoot(name, SCENES[name], args.out, args.port + i, log)
        except (DevctlError, AssertionError) as e:
            print(f"  {name}: FAILED to shoot — {e} (log: {log})")
            broken.append(name)
            continue
        if not os.path.exists(png):
            print(f"  {name}: no image was written (log: {log})")
            broken.append(name)
            continue

        tally = render.get("tally", {})
        note = f"roots {tally.get('roots')}"
        # a defect the pixels may not show: a leak, a mesh-less row, a model
        # drawn away from the row it is drawing, a hull off the waterline
        problems = render.get("problems", [])
        if problems:
            by_rule: dict[str, list] = {}
            for p in problems:
                by_rule.setdefault(p["rule"], []).append(p)
            print(f"  {name}: RENDER PROBLEMS ({note})")
            for rule, rows in sorted(by_rule.items()):
                print(f"     {rule}: {len(rows)} — e.g. {rows[0]['id']}: {rows[0]['detail']}")
            broken.append(name)

        base = os.path.join(args.baseline, f"{name}.png")
        if args.bless:
            shutil.copyfile(png, base)
            print(f"  {name}: blessed ({note})")
            continue
        if not os.path.exists(base):
            print(f"  {name}: no baseline yet — run with --bless ({note})")
            missing.append(name)
            continue
        frac = compare(png, base, os.path.join(args.out, f"{name}.diff.png"))
        if frac is None:
            print(f"  {name}: SIZE CHANGED or compare failed ({note})")
            changed.append(name)
            continue
        if frac > args.tolerance:
            shutil.copyfile(base, os.path.join(args.out, f"{name}.base.png"))
            print(f"  {name}: CHANGED {frac * 100:.2f}% of pixels ({note})")
            changed.append(name)
        else:
            print(f"  {name}: same ({frac * 100:.3f}%, {note})")

    print()
    if args.bless:
        print(f"blessed {len(names) - len(broken)} scene(s) into {args.baseline}")
    if changed:
        print(f"{len(changed)} scene(s) changed: {', '.join(changed)}")
        print(f"look in {args.out} at <scene>.png / .base.png / .diff.png")
    if missing:
        print(f"{len(missing)} scene(s) have no baseline: {', '.join(missing)}")
    if broken:
        print(f"{len(broken)} scene(s) are BROKEN: {', '.join(broken)}")
    return 1 if (changed or broken) else 0


if __name__ == "__main__":
    sys.exit(main())
