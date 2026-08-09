#!/usr/bin/env python3
"""Run bot matches across seeds and presets and report what breaks.

Launches `saladin-headless` itself, steps a match in chunks, and after every
chunk asks the game what must never be true (`{"query": "invariants"}`) plus a
handful of things only a WATCHER can see — a site with a crew that banks no
work, a town whose stockpile never moves, a match that never resolves. A
violation is printed with the seed and tick that produced it, which is a repro.

    python3 scripts/soak.py --seeds 8 --minutes 10
    python3 scripts/soak.py --seeds 1,7,31 --preset 3 --minutes 20 --keep-going
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time

sys.path.insert(0, __file__.rsplit("/", 1)[0])

from devctl import Devctl, DevctlError, draw, wait_for  # noqa: E402

BIN = "target/release/saladin-headless"
CHUNK = 400  # ticks between checks: 20 s of game time


class Finding:
    def __init__(self, seed: int, preset: int, tick: int, rule: str, detail: str) -> None:
        self.seed, self.preset, self.tick, self.rule, self.detail = seed, preset, tick, rule, detail

    def __str__(self) -> str:
        return f"seed {self.seed} preset {self.preset} tick {self.tick}: {self.rule} — {self.detail}"


def stalled_sites(g: Devctl, memory: dict) -> list[tuple[str, str]]:
    """A foundation with a crew on it must bank work. One that does not is the
    class of bug that hides best: the cost is paid, the crew is committed, and
    nothing anywhere says the building will never rise."""
    out = []
    for b in g.state(kinds=["buildings"]).buildings:
        if b.complete:
            memory.pop(b.id, None)
            continue
        was = memory.get(b.id)
        if was is not None and b.builders > 0 and b.progress <= was[0]:
            stuck = was[1] + 1
            memory[b.id] = (b.progress, stuck)
            if stuck == 3:  # three chunks = a minute of game time
                out.append((
                    "site banks no work",
                    f"{b.kind} {b.id} at {b.xy}, crew {b.builders}, progress {b.progress:.3f}",
                ))
        else:
            memory[b.id] = (b.progress, 0)
    return out


def confirmed(g: Devctl, settle: int = 20) -> list[dict]:
    """Violations that survive a second look.

    The sim is a state machine mid-stride: a hand walking to a node emptied
    this instant holds a dead id until the next gather tick, four ticks later,
    and a sampler that lands in that window reports it forever. Everything the
    soak prints has to be worth chasing, so a finding must still be there after
    the world has moved on.
    """
    first = {(v["rule"], v["id"]): v for v in g.invariants()["violations"]}
    if not first:
        return []
    g.step(settle)
    again = {(v["rule"], v["id"]) for v in g.invariants()["violations"]}
    return [v for k, v in first.items() if k in again]


def idle_hands(g: Devctl, players: list[int]) -> list[tuple[str, str]]:
    """Half a town standing still with nothing to do is not a strategy — for a
    BOT. The scripted seat has no brain and nobody driving it, so its peasants
    are idle by definition and flagging them is noise."""
    out = []
    for p in players:
        peasants = g.state(kinds=["units"], player=p).units_of(p, "Peasant")
        if len(peasants) < 4:
            continue
        idle = [u for u in peasants if u.gather_state == "Idle" and not u.job_site]
        if len(idle) * 2 > len(peasants):
            out.append((
                "town standing idle",
                f"player {p}: {len(idle)} of {len(peasants)} peasants idle, ids {[u.id for u in idle][:6]}",
            ))
    return out


def soak(seed: int, preset: int, ticks: int, port: int, verbose: bool) -> list[Finding]:
    proc = subprocess.Popen(
        [BIN, "--port", str(port), "--seed", str(seed), "--preset", str(preset),
         "--ai", "2", "--difficulty", "Hard"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.STDOUT,
    )
    found: list[Finding] = []
    try:
        g = wait_for(port, seconds=60)
        sites: dict = {}
        seen_rules: set[str] = set()
        while True:
            info = g.step(CHUNK)
            tick = info["tick"]
            for v in confirmed(g):
                key = v["rule"]
                if key in seen_rules:
                    continue
                seen_rules.add(key)
                found.append(Finding(seed, preset, tick, v["rule"], f"row {v['id']}: {v['detail']}"))
            players = [p.player_id for p in g.state(kinds=["players"]).players if p.bot]
            for rule, detail in stalled_sites(g, sites) + idle_hands(g, players):
                if rule in seen_rules:
                    continue
                seen_rules.add(rule)
                found.append(Finding(seed, preset, tick, rule, detail))
            if verbose and tick % (CHUNK * 10) == 0:
                s = g.state(kinds=["players", "buildings", "units"])
                print(f"    tick {tick}: {len(s.units)} units, {len(s.buildings)} buildings, "
                      f"hash {s.hash}")
            if tick >= ticks:
                return found
    except DevctlError as e:
        found.append(Finding(seed, preset, -1, "the channel died", str(e)))
        return found
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--seeds", default="6", help="a count, or a comma list of seeds")
    ap.add_argument("--preset", type=int, default=0)
    ap.add_argument("--minutes", type=float, default=10.0, help="game time per seed")
    ap.add_argument("--port", type=int, default=7900)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    seeds = (
        [int(s) for s in args.seeds.split(",")]
        if "," in args.seeds or not args.seeds.isdigit() or len(args.seeds) > 3
        else list(range(1, int(args.seeds) + 1))
    )
    ticks = int(args.minutes * 60 * 20)
    print(f"soak: {len(seeds)} seeds x {args.minutes} min of game time, preset {args.preset}")

    all_found: list[Finding] = []
    for i, seed in enumerate(seeds):
        t0 = time.time()
        found = soak(seed, args.preset, ticks, args.port + i, args.verbose)
        mark = "CLEAN" if not found else f"{len(found)} finding(s)"
        print(f"  seed {seed}: {mark} ({time.time() - t0:.1f}s)")
        for f in found:
            print(f"    {f}")
        all_found += found

    print()
    if not all_found:
        print(f"clean over {len(seeds)} seeds")
        return 0
    print(f"{len(all_found)} finding(s) over {len(seeds)} seeds:")
    for f in all_found:
        print(f"  {f}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
