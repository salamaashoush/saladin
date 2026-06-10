# Saladin — Bevy/Rust RTS

Historic RTS (Crusades-era) built on **Bevy 0.19** with **deterministic
lockstep multiplayer**. The repo root is the cargo workspace. The old TypeScript/SpacetimeDB game was deleted; it exists only in
git history and is NOT a reference — design correctness here directly.

## Workspace layout

```
crates/
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
8. **The map preset rides in the seed's top 3 bits** (`compose_seed(base,
   preset)`), so every per-seed cache and the wire stay plain u32. Always
   compose before writing `WorldConfig.seed`; `seed_base`/`seed_preset`/
   `seed_bias` decode.

## Commands

```bash
cargo test --workspace                 # 116 tests, all must stay green
cargo run -p saladin-client --bin saladin-client          # single player
cargo run -p saladin-client --bin saladin-client connect <ip>   # dev shortcut (menus cover all MP flows)
cargo run -p saladin-server                                # internet relay (rooms) — VPS docs: crates/server/README.md
cargo run --release -p saladin-protocol --example net_bench -- 2 50000 200
                                       # lockstep benchmark: clients units ticks
cargo run -p saladin-sim --example mapdump -- <base> <preset> [out.ppm]
                                       # worldgen tuning: biome map + dominant-region dump
uv run scripts/bake_voices.py          # Chatterbox TTS bark bake -> assets/voices/
   # (gitignored; engine falls back to procedural formant voices per missing
   # file — see client/src/audio/voice.rs. TTS_DEVICE=cpu if CUDA acts up.)
./shot.sh /tmp/out.png [SALADIN_*=...]   # screenshot harness: builds nothing,
   # rm's the stale shot, runs SALADIN_AUTO=1 (override with SALADIN_AUTO=x in
   # args), FAILS LOUDLY if no screenshot was written. ALWAYS use this over a
   # raw `SALADIN_AUTO=1 cargo run` — a crashed run otherwise leaves the
   # previous /tmp/saladin_shot.png in place and you stare at a STALE shot
   # (burned an hour TWICE now). `cargo build` FIRST so the 30s timeout
   # doesn't eat the build. Inspect via `magick out.png -crop ... && view`.
   # Modes: menu | sp | mp | settings | lobby | pause | research | market |
   # layout (computed-rect dump) | units (conjures every unit kind + one node
   # of each type + a water fish node beside the keep — model verification).
   # Overrides: SALADIN_SEED, SALADIN_PRESET, SALADIN_TAB, SALADIN_ZOOM
   # (view_size, min 4 = close-up model inspection), SALADIN_YAW.
```

Multiplayer (all menu-driven; protocol v2 handshake rejects mismatched builds):
- Host LAN: embeds the relay (port 5000), self-connects, shows LAN IPs.
- Join by IP: text input (LAN/port-forwarded hosts).
- Host Internet / Join Room: both sides connect OUTBOUND to a public relay
  (`saladin-server` on any VPS) — room-keyed (`relay_core::Rooms`), 6-char
  codes, zero NAT config. Relay address in `~/.config/saladin/config.toml`.
- Lobby: names (persisted in config), per-player faction, ready flags, host
  adds AI seats + picks map (seed+preset ship in `Welcome`; only the host
  originates `AddAi` commands — still lockstep-deterministic).
- Mid-match drops broadcast `PeerLeft`: survivors get a banner, ticks
  complete without the leaver. `TcpTransport` shuts the socket down on Drop
  (the reader thread's fd clone otherwise keeps dead clients seated).
Lockstep = inputs only on the wire; client count barely affects cost. TCP is
intentional (lockstep needs reliable+ordered; UDP buys nothing at 20 Hz).
`net_ws.rs` (ewebsock) shares the same wire protocol for a future browser
build but has a known client-side stall — unused.

## Worldgen (sim/terrain.rs)

WORLD_SIZE 288. Seeded continental-plate noise blended with a weak center
dome (no two seeds share geography; ocean ring guaranteed). Features, all
fixed-point + cache-compatible: rivers with FORD crossings (River/Ford
biomes; fords walkable, never buildable), gradient CLIFFS with ramp openings,
mountain passes (ranges never bisect the mainland), clustered forest groves,
desert oasis pockets. 4 presets: Continental / River Valley / Highlands /
Archipelago (`MAP_PRESETS`, bias fields incl. river/cliff/island gain).
Fair starts: `fair_start_nodes` tops every spawn slot up to wood/stone/food
minima within `FAIR_RADIUS`; `start_point` snaps spawns to the dominant
region (`dominant_region`). Invariants tested across 100 worlds in
`sim/tests/worldgen.rs` — keep them green when touching terrain.

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
- UI = `ui/` module: ALL art is baked procedurally at startup in
  `ui/assets.rs` (parchment 9-slice panels, flat bronze buttons, 31 pixel-art
  icons as string-art tables, ring/flag textures) — no binary assets. Widget
  builders in `ui/widgets.rs` (`tool_button` icon cards, `screen_button`/
  `wide_button`, `panel_bg`); button states are ImageNode tints via
  `button_feedback`. `UiAction` central dispatch, digest-based rebuild
  (rebuild section only when its state key changes). Text inputs:
  `ui/text_input.rs` (values live in `MpForm` so rebuilds never eat typed
  text — always compare-before-write to avoid rebuild loops).
- HUD UX rules: market trading lives on the selected Market only; Orders
  (Gather/Demolish mode) only when nothing is selected; build tabs sit ABOVE
  the card grid. Absolute bottom-anchored panels need explicit min_height.
- Render = shared mesh+material handles per kind×team so Bevy
  auto-instances; sim→render reconciliation in `render/sync.rs`.
- Units are RIGS, not single meshes: `unit_rig(kind)` returns parts tagged
  `RigGroup` (Body/legs/arms/wheel slots) with joint pivots; mesh verts are
  pivot-relative, one child entity per part (still instanced per
  kind×team×group). `animate_units` drives walk/chop/aim/wheel-spin/gallop
  procedurally from `AnimState` (mirrored sim flags: has_target,
  attack_target, Harvesting) + wall time. Team color BAKES into white verts
  (`bake_team`) — unit material stays white; a colored material would tint
  wood/steel/skin green-plastic again. Units face +Z when moving — author
  models forward = +Z (the ram is yawed in `unit_rig` because it was built
  +X). Wheel axles along X. Mounted: wheel-group slots are the four horse
  legs (per-leg hip pivots — a shared pivot sweeps sawhorse arcs); rider is
  authored foot-size then shrunk `RIDER_SCALE` about the saddle.
- Animal food nodes wander render-only (`AnimalNode` + `animate_animals`):
  graze/stand mesh swap at waypoints around the SIM anchor (gatherers walk
  to the anchor), and on the first harvest tick (remaining < first-seen)
  they swap to a carcass mesh and stop forever — AoE-style. Never write
  sim state from any of this.
- The embedded `ui.ttf` (DejaVu) has no emoji glyphs AND the atlas pre-warm
  is ASCII-only — never put non-ASCII in UI strings (em dashes included).
- Config (`~/.config/saladin/config.toml`): player name, relay address,
  edge-scroll, UI scale, volume placeholder — `client/src/config.rs`.
- Game states: Menu → (Lobby) → Loading (one rendered frame before the heavy
  world build) → Playing (Esc = pause overlay; SP also ships Pause command) →
  GameOver (MatchStats: trained/lost/gathered tallied in sim at train/death/
  deposit sites).
