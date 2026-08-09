#!/usr/bin/env python3
"""An agent playing Saladin through devctl, keyboard untouched.

Founds nothing itself: it attaches to a game that is already up, then builds a
base, raises an army, marches it at the enemy keep and asserts what happened.
Works against either host — `saladin-headless` (stepped, deterministic) or the
windowed client (its own clock, and screenshots).

    cargo run -p saladin-protocol --bin saladin-headless -- --port 7777 --seed 4
    python3 scripts/playtest.py --port 7777 --record /tmp/repro.jsonl

    SALADIN_DEVCTL=7777 cargo run -p saladin-client --bin saladin-client
    python3 scripts/playtest.py --port 7777 --shot /tmp/fight.png

Re-run a recording against a fresh runner on the same seed:

    python3 scripts/playtest.py --port 7777 --replay /tmp/repro.jsonl
"""

from __future__ import annotations

import argparse
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])

from devctl import (  # noqa: E402
    AttackMove,
    AutoGather,
    Build,
    CancelSite,
    Devctl,
    DevctlError,
    Repair,
    Row,
    Train,
    wait_for,
)

ME = 1


def keep_of(g: Devctl, player: int) -> Row:
    for b in g.state(kinds=["buildings"], player=player).buildings:
        if b.kind == "Keep":
            return b
    raise DevctlError(f"player {player} has no keep")


def enemies(g: Devctl) -> list[int]:
    return [p.player_id for p in g.state(kinds=["players"]).players if p.player_id != ME]


def reachable(g: Devctl, site: Row, patience: int = 60) -> bool:
    """Did a crew actually get to the foundation? `walk_to` drops a hand whose
    A* comes back empty, so a site nobody can reach never gains a builder and
    never gains a point of work."""
    for _ in range(4):
        g.advance(patience)
        now = next(
            (b for b in g.state(kinds=["buildings"], player=ME).buildings if b.id == site.id),
            None,
        )
        if now is None:
            return False
        if now.builders > 0 or now.progress > 0:
            return True
    return False


def found(g: Devctl, kind: str, near: Row, builders: list[int], span: int = 14) -> Row | None:
    """Plant `kind` on the first tile near `near` the sim will accept.

    The ghost's rules are the command's rules, so an agent finds a legal site
    exactly as a player does: try one, read the refusal, try the next. Without
    the feedback channel this loop is blind — a bad site and an empty
    stockpile look identical from outside, and only one of them is fixed by
    moving.
    """
    cx, cz = near.xy
    g.feedback()
    seen: set[str] = set()
    for r in range(3, span):
        for dx, dz in ((r, 0), (0, r), (-r, 0), (0, -r), (r, r), (-r, r), (r, -r), (-r, -r)):
            at = (round(cx + dx), round(cz + dz))
            for _ in range(6):
                before = {b.id for b in g.state(kinds=["buildings"], player=ME).buildings}
                g.cmd(Build(player_id=ME, kind=kind, pos=at, builders=builders))
                g.advance(6)
                new = [
                    b
                    for b in g.state(kinds=["buildings"], player=ME).buildings
                    if b.id not in before and b.kind == kind
                ]
                if new:
                    site = new[0]
                    if reachable(g, site):
                        return site
                    # founded on ground the crew cannot walk to: the placement
                    # rules only promise a walkable tile BORDERS the footprint,
                    # not that anyone can get to it once your own buildings ring
                    # it. Take the refund and look elsewhere.
                    print(f"  {kind} at {at}: no route for the crew, cancelling")
                    g.cmd(CancelSite(player_id=ME, building=site.id))
                    g.advance(6)
                    break
                why = [f["error"] for f in g.feedback()]
                if "CannotAfford" in why:
                    # the site is fine, the purse is not: let the hands work
                    g.advance(600)
                    continue
                for w in why:
                    if w not in seen:
                        seen.add(w)
                        print(f"  {kind}: {w}")
                break
    return None


def playtest(g: Devctl, shot: str | None) -> int:
    host = g.tick()
    print(f"attached: seed {host['seed']}, tick {host['tick']}, may_step={host['may_step']}")

    # a freshly launched host has queued its Join but not yet run the tick it
    # lands on: there is no keep to build near until the match has started
    for _ in range(40):
        if g.state(kinds=["players"]).players:
            break
        g.advance(10)

    mine = keep_of(g, ME)
    foes = enemies(g)
    if not foes:
        raise DevctlError("no opponent in this match")
    foe = foes[0]
    print(f"my keep {mine.id} at {mine.xy}; opponent is player {foe}")

    # ── a base ───────────────────────────────────────────────────────────────
    hands = [u.id for u in g.state(kinds=["units"], player=ME).units_of(ME, "Peasant")]
    print(f"{len(hands)} peasants to hand")
    g.cmd(AutoGather(player_id=ME))

    raised = {}
    for kind, crew in (("House", hands[:2]), ("Barracks", hands[:3]), ("Farm", hands[:2])):
        site = found(g, kind, mine, crew)
        if site is None:
            raise DevctlError(f"could not site a {kind} anywhere near the keep")
        raised[kind] = site.id
        print(f"founded {kind} {site.id} at {site.xy}")

    g.advance(600)
    # Whatever is still a foundation gets a crew put on it. The hands named at
    # founding are the ones already raising the last site, and AutoGather takes
    # them back the moment they are done — Repair IS the verb that reassigns
    # them, founding and mending being one loop.
    site_ids = {b.id for b in g.state(kinds=["buildings"], player=ME).buildings if not b.complete}
    for bid in site_ids & set(raised.values()):
        for hand in hands[:2]:
            g.cmd(Repair(player_id=ME, unit=hand, building=bid))
        g.advance(600)

    done = {b.id: b for b in g.state(kinds=["buildings"], player=ME).buildings}
    for kind, bid in raised.items():
        b = done.get(bid)
        assert b is not None, f"the {kind} vanished — {g.feedback()}"
        print(f"  {kind}: {b.state} progress {b.progress:.2f} crew {b.builders}")
        assert b.complete, f"the {kind} never topped out: {g.feedback()}"

    # ── an army ──────────────────────────────────────────────────────────────
    for _ in range(6):
        g.cmd(Train(player_id=ME, kind="Spearman"))
    g.advance(1600)
    army = [u.id for u in g.state(kinds=["units"], player=ME).units_of(ME, "Spearman")]
    print(f"army: {len(army)} spearmen")
    assert army, f"nothing was trained: {g.feedback()} / {g.state(kinds=['players']).player(ME).stock}"

    # ── the attack ───────────────────────────────────────────────────────────
    theirs = keep_of(g, foe)
    before = theirs.hp
    g.cmd(AttackMove(player_id=ME, units=army, target=theirs.pos, formation=1))
    print(f"marching on the enemy keep at {theirs.xy} (hp {before})")

    contact = None
    for _ in range(20):
        g.advance(300)
        s = g.state()
        theirs = s.by_id(theirs.id)
        mine_now = s.units_of(ME, "Spearman")
        fighting = [u for u in mine_now if u.attack_target]
        if theirs is None or theirs.hp < before or fighting:
            contact = (theirs, mine_now, fighting)
            break

    if shot:
        aim = contact[1][0].pos if contact and contact[1] else theirs.pos
        print("screenshot:", g.screenshot(shot, camera={"pos": aim, "zoom": 14}))

    assert contact, "the column never reached the enemy in 300 seconds"
    theirs, mine_now, fighting = contact
    print(
        f"CONTACT at tick {g.tick()['tick']}: {len(fighting)} of {len(mine_now)} engaged; "
        f"enemy keep {'destroyed' if theirs is None else f'{theirs.hp}/{before} hp'}"
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, default=7777)
    ap.add_argument("--record", metavar="FILE", help="log the request stream for a repro")
    ap.add_argument("--replay", metavar="FILE", help="re-run a recording and pin the state hash")
    ap.add_argument("--shot", metavar="PNG", help="screenshot the fight (windowed client only)")
    args = ap.parse_args()

    g = wait_for(args.port)
    try:
        if args.replay:
            end = g.replay(args.replay)
            print(f"replayed {args.replay} to tick {end.get('tick')}, hash {end.get('hash')}")
            return 0
        if args.record:
            g.record(args.record)
        code = playtest(g, args.shot)
        if args.record:
            g.stop_recording()
            print(f"recorded to {args.record}")
        return code
    finally:
        g.close()


if __name__ == "__main__":
    sys.exit(main())
