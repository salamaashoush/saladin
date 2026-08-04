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
