# Saladin

A historic real-time strategy game (Crusades era) written in Rust on
[Bevy](https://bevy.org), with deterministic-lockstep multiplayer over TCP.

The repo root is the cargo workspace:

- `crates/sim` — pure deterministic game core (fixed-point math, worldgen,
  pathfinding, combat/economy/AI formulas)
- `crates/protocol` — the simulation as Bevy ECS + lockstep netcode + save/load
- `crates/server` — dedicated relay binary (optional; clients can host)
- `crates/client` — the game: rendering, camera, input, UI

```bash
cargo test --workspace
cargo run -p saladin-client --bin saladin-client            # play vs AI
cargo run -p saladin-client --bin saladin-client connect <ip>  # join a LAN game
```

To host a multiplayer game, click **Host Game (LAN)** in the menu; friends
join with `connect <your-ip>`. See `CLAUDE.md` for architecture notes.

## The world

Every match is generated from one seed, in three layers. Drifting tectonic
plates decide where land is and where mountains chain along their seams. A
climate model — latitude, an elevation lapse rate, and a moisture parcel that
recharges over water and rains out on uplift — decides how warm and how wet
each tile is, with real rain shadows behind the ranges. Biomes then fall out of
a Whittaker lookup on temperature against precipitation, refined for the arid
cases: sand seas need flat ground and a dry fetch, stone pavement takes the
slopes, closed basins hold lakes where it rains and evaporate into salt pans
where it does not.

Each seed also draws one of eight climate archetypes, modelled on real theatres
of the Crusades — Levantine Coast, Fertile Crescent, Arabian Frontier,
Anatolian Upland, River of Egypt, Maghreb Shore, Aegean Reach, Northern
Marches. The map preset picks the geography; the seed picks the weather, and
the weather is most of why one map is a cedar upland and the next a sand sea.
The menu names it before you commit.

That world is not scenery. Gold sits in mineralized rock and in the channel
gravel below it; herds follow the grazing; a shore's fishery depends on whether
it faces open sea, a lake or a river. Farms only take on soil the drainage
actually enriched, so floodplains and oases are worth holding — and marsh drags
at an army that tries to cross it.

## devctl — driving the game from outside

`SALADIN_DEVCTL=<port>` opens a line-delimited JSON socket on
`127.0.0.1:<port>`, so a script or an agent can play the game, read the whole
match back, and assert on the result. Unset, nothing is added: no listener, no
systems, no cost.

```bash
cargo run -p saladin-protocol --bin saladin-headless -- --port 7777 --seed 4
python3 scripts/playtest.py --port 7777 --record /tmp/repro.jsonl

SALADIN_DEVCTL=7777 cargo run -p saladin-client --bin saladin-client   # windowed
```

One request per line, one reply per line — `nc` works. An optional `"id"` is
echoed back so a client can multiplex.

| request | answer |
| --- | --- |
| `{"cmd": {"Train": {"player_id": 1, "kind": "Spearman"}}}` | `{"ok": true, "applied_tick": 4822}` |
| `{"query": "tick"}` | tick, hash, seed, paused, and what this host allows |
| `{"query": "state", "kinds": ["units"], "player": 1, "near": {"pos": [120, 80], "radius": 30}}` | the match as JSON |
| `{"step": 60}` | replies once the 60 ticks have run (headless only) |
| `{"feedback": true}` | every refusal since the last call, with its `PlaceError` |
| `{"screenshot": "/tmp/x.png", "camera": {"pos": [120, 80], "zoom": 6, "yaw": 2}}` | replies once the PNG is on disk |

An error is a value, never a panic: a malformed request answers
`{"ok": false, "error": "..."}` and the game runs on.

**Writes go through `PlayerCommand`.** A request never touches `World`; it
parses into a command and is handed to the lockstep driver beside the local
player's clicks, so an injected order is replicated, ordered and re-simulated
on every peer exactly like a click.
`devctl::a_devctl_driven_peer_stays_hash_identical_to_a_plain_one` runs two
peers in one match, one of them driven over the socket, and compares the state
hash on every one of 1200+ ticks. That test is the whole design.

If a debug action has no `PlayerCommand` — "spawn ten knights" — the way to add
one is a NEW variant **appended** to the enum (bincode encodes the variant
index, so inserting renumbers every later variant), `PROTOCOL_VERSION` bumped,
and the cheat gated on a flag that lives in sim state, not in an env var: a
peer without the flag would refuse what the devctl peer applied, and that is a
desync. No such variant exists today.

**Reads never mutate.** Every query is a projection taken through
`World::try_query`, which takes `&World` — a read that can mutate is a read
that can desync a peer. The one thing devctl keeps of its own is a mirror of
`CommandFeedback`, because `apply_commands` clears it every tick and a polling
script would otherwise never learn why its order was refused.

**Time.** `step` is headless-only. In a running client time belongs to the
fixed-update clock, and in a match to the lockstep group; `{"query": "tick"}`
reports `may_step` so a script can tell. There is no `wait` verb — poll with
`step` plus a query, which is what `Devctl.advance()` in the driver does.

`scripts/devctl.py` is the driver: typed command builders, attribute access
over the capture, reconnect on a dropped socket, and `record()`/`replay()` —
a recording re-runs against a fresh match on the same seed and compares the
hash at every step, so a bug repro is also a regression test.

```python
from devctl import Devctl, Build, Train

with Devctl(port=7777) as g:
    g.cmd(Build(player_id=1, kind="Farm", pos=(12, 30), builders=[peasant]))
    g.step(600)
    farm = g.state(kinds=["buildings"]).buildings_of(1, "Farm")[0]
    assert farm.complete, g.feedback()
    g.screenshot("/tmp/farm.png", camera={"pos": farm.pos, "zoom": 6})
```
