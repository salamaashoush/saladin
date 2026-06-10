# Saladin — Bevy/Rust RTS

Historic RTS (Crusades-era) built on **Bevy 0.19** with **deterministic
lockstep multiplayer**. All game code lives in `saladin-bevy/` (cargo
workspace). The old TypeScript/SpacetimeDB game was deleted; it exists only in
git history and is NOT a reference — design correctness here directly.

## Workspace layout

```
saladin-bevy/crates/
  sim/        pure deterministic game math + data. NO bevy, NO floats.
              fixed-point (Fx = I32F32), stat tables, terrain/worldgen,
              pathfinding (A*), combat/morale/economy formulas, AI planner.
  protocol/   the simulation as Bevy ECS (headless subcrates only:
              bevy_app/ecs/time/platform). Components mirror game rows
              (Unit/Building/Player/...), systems run in SimSchedule,
              PlayerCommand = the lockstep input surface, net (TCP lobby
              relay + transports), save/load (ECS snapshot).
  server/     dedicated relay binary (same relay a hosting client embeds).
  client/     full bevy umbrella: render, camera, input, UI, menus.
```

## The iron rules (lockstep determinism)

1. **Sim state mutates ONLY via `PlayerCommand`s applied in `SimSchedule`.**
   Clients ship commands; every peer re-simulates. Render/UI never write sim
   components.
2. **No floats, no trig, no wall clock, no `rand` in sim/protocol.** All
   gameplay math is `Fx` fixed-point via `saladin_sim`. Randomness =
   `SimRng`/`hash2` (deterministic). f32 is allowed ONLY in the client render
   layer.
3. **`fx!("1.5")`, never `Fx::lit("1.5")` in runtime code.** `Fx::lit` is
   const fn but parses its decimal string per call in runtime position — it
   once ate 66% of total CPU. The `fx!` macro forces inline-const evaluation.
4. **Cross-entity references use `GameId` (deterministic u64), never Bevy
   `Entity`** (ids differ across clients). `GameIndex` maps back.
5. **Deterministic iteration**: sort snapshots by `GameId` before order-
   dependent mutation; `bevy_platform` HashMap where iteration order leaks
   into state; systems fully `.chain()`ed in SimSchedule.
6. **`StateHash`** (commutative per-row digest) is the desync detector —
   every netcode/feature test should assert hash equality across worlds.
7. Expensive pure terrain queries are cached per seed and leaked:
   `passable_grid` / `region_grid` / `elevation_at` (thread-local last-seed
   memo). Use them — never resample fbm in a hot loop.
   `node_reachable(seed, from, to)` answers "can a walker ever get there".

## Commands

```bash
cd saladin-bevy
cargo test --workspace                 # 100 tests, all must stay green
cargo run -p saladin-client --bin saladin-client          # single player
cargo run -p saladin-client --bin saladin-client connect <ip>   # join LAN game
cargo run -p saladin-server                                # dedicated relay
cargo run --release -p saladin-protocol --example net_bench -- 2 50000 200
                                       # lockstep benchmark: clients units ticks
SALADIN_AUTO=1 cargo run -p saladin-client --bin saladin-client
   # skip menu + screenshot to /tmp/saladin_shot.png at ~6s (headless verify:
   # then `magick /tmp/saladin_shot.png -crop ...` and view the crop)
```

Multiplayer: menu "Host Game (LAN)" embeds the relay (port 5000) and
self-connects; joiners use `connect <ip>`; lobby shows the roster; host
clicks Start. Lockstep = inputs only on the wire; client count barely affects
cost. TCP is intentional (lockstep needs reliable+ordered; UDP buys nothing
at 20 Hz). `net_ws.rs` (ewebsock) exists for a future browser build but has
a known client-side stall — unused.

## Sim cadences

Base tick 50 ms (20 Hz). Movement+separation every tick/2; gather+combat
every 4 (200 ms); brain+research every 20 (1 s); economy every 40 (2 s).
Run-conditions via `every(n)`; `MatchStatuses` gates paused matches.

## Perf doctrine

Worst-case all-out melee on one box: ~920 t/s @20k units, ~220 @50k (2
clients re-simulating). Hot-path rules: no per-tick allocation (scratch
resources with retained buffers — see `CombatScratch`), flat cell grid
(`CELL_SIZE` 4, `cell_of`), ring-ordered nearest scans with early exit,
squared-distance compares (`dist2` vs r²; `fx_sqrt` only when unavoidable),
pursuit A* capped (`PURSUIT_EXPANSIONS`) + per-tick budget
(`PURSUIT_BUDGET`). Profile before optimizing: `perf record` on net_bench;
the last three bottlenecks were string parsing, fbm resampling, and hashmap
churn — never the network.

## Testing pattern

Integration tests build a headless `App` with `SimPlugin`, spawn rows
directly or push `PlayerCommand`s into `CommandQueue`, `step(world)` N times,
then assert on components and `StateHash`. Determinism tests run TWO worlds
and compare hashes every tick. Net tests use the real relay on localhost.
Every gameplay fix ships with a test (`crates/protocol/tests/`).

## Client notes

- Bevy 0.19: `TextFont { font: handle.into(), font_size: FontSize::Px(13.0) }`;
  do NOT downgrade to 0.18 (its text renderer shreds glyphs on this machine).
- Ortho iso camera: keep `near` at 0 — negative near pulls behind-camera
  geometry (the ocean disc) over the map.
- UI = `ui/` module: theme tokens, widget builders, `UiAction` central
  dispatch, digest-based rebuild (rebuild section only when its state key
  changes). Render = shared mesh+material handles per kind×team so Bevy
  auto-instances; sim→render reconciliation in `render/sync.rs`.
- The embedded `ui.ttf` (DejaVu) has no emoji glyphs — text labels only.
