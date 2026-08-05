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
cargo test --workspace                 # 166 tests, all must stay green
cargo run -p saladin-client --bin saladin-client          # single player
cargo run -p saladin-client --bin saladin-client connect <ip>   # dev shortcut (menus cover all MP flows)
cargo run -p saladin-server                                # internet relay (rooms) — VPS docs: crates/server/README.md
cargo run --release -p saladin-protocol --example net_bench -- 2 50000 200
                                       # lockstep benchmark: clients units ticks
cargo run -p saladin-sim --example mapdump -- <base> <preset> [out.ppm]
                                       # worldgen tuning: biome map + dominant-region dump
cargo run --release -p saladin-sim --example worldstat -- [seeds] [preset|all] [--per-seed] [--seeds a,b,c]
                                       # biome/climate histogram + height/slope
                                       # quantiles + high-country share: THE
                                       # diversity and shape dial
cargo run --release -p saladin-sim --example massifprobe [start <preset>]
                                       # per-seed counts worldstat rounds away;
                                       # `start` finds seeds with a massif by slot 0
cargo run --release -p saladin-sim --example resprobe -- [seeds] [preset|all] [--per-seed] [--seeds a,b,c] [--fair]
                                       # where the resource scatter lands vs the
                                       # terrain it reads (slope/ore/fertility per
                                       # node kind); `--fair` audits fair-start
                                       # headroom over all 100 test worlds
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
   # layout (computed-rect dump) | soil (farm siting + fertility overlay) |
   # units (every unit kind + one node of each type + a water fish node beside
   # the keep — model verification).
   # Overrides: SALADIN_SEED, SALADIN_PRESET, SALADIN_TAB, SALADIN_ZOOM
   # (view_size, min 4 = close-up model inspection), SALADIN_YAW,
   # SALADIN_PERF=1 (starts the F3 frame-time overlay on, so a shot can be
   # read as a benchmark).
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

## Worldgen (sim/plates.rs -> climate.rs -> worldgrid.rs)

WORLD_SIZE 384. Three layers, all fixed-point and cache-compatible:

1. **Tectonics** (`plates.rs`): a domain-WARPED Worley lattice of drifting
   plates (warp first, or the cell walls print straight coastlines). Relative
   motion at each seam picks the landform - convergence raises orogenic belts /
   volcanic arcs / island arcs, divergence opens rifts with uplifted shoulders,
   shear offsets ridges. Ranges chain along their suture; ORE follows the seams
   that mineralize.
2. **Climate** (`climate.rs`): temperature = latitude + elevation lapse rate;
   precipitation = an advected moisture parcel (recharges over water, rains out
   on uplift, mixes cross-wind so shadows do not stripe). Each seed draws one of
   8 CLIMATE ARCHETYPES (Levantine Coast / Fertile Crescent / Arabian Frontier /
   Anatolian Upland / River of Egypt / Maghreb Shore / Aegean Reach / Northern
   Marches); `target_precip` pins the map's mean, the sweep supplies its shape.
   The archetype is shown in the skirmish menu - preset picks GEOGRAPHY, seed
   picks WEATHER.
3. **Pipeline** (`worldgrid.rs`): thermal erosion -> 2 stream-power incision
   passes -> Barnes priority-flood -> basins (lakes where it rains, salt pans
   where it does not) -> D8 flow + accumulation -> rivers/fords/deltas/marsh ->
   climate -> soil fertility + ore -> classify.

Classification is Whittaker(temp x precip) for lowlands, climate-aware
highlands, then arid refinement (dunes need flat ground and a dry fetch,
hammada takes the slopes, oasis where fresh water is in reach). 25 biomes: the
original 15 plus Lake, Marsh, Wadi, SaltFlat, Hammada, Savanna, Scrub, Pine
(cedar), OliveGrove, Alpine. `WorldGrid` also carries `temp`, `fertility`,
`ore`, `belt` and `slope` - the resource system and farms read them.

**Shape is one system.** `terrain::surface_height(h, elev_gain)` is THE
vertical scale of the world, a single monotone function of the height field
(the old per-biome `height_emphasis` invented geometry at render time and
turned every label flip into a wall). Ranges are BROAD: a low-frequency massif
envelope along the orogenic seam supplies the mass, the ridged fold is
multiplied by it so crests are texture ON a mountain, and the pass noise carves
real notches before the last erosion sweeps. `WorldGrid.slope` - the
world-space drop per tile of exactly the surface the client meshes - then
decides the Cliff/Mountain label, and through the label decides passability,
move cost (`1 + slope * CLIMB_COST`), buildability and keep siting. So what you
SEE is where you can walk: nothing above `max_walkable_slope(seed)` is ever
passable, cliffs are edges (never area fills), gentle summit plateaus and
saddles stay walkable, and `PlaceError::TooSteep` refuses a foundation on a
hillside for the command, the AI and the ghost alike.

4 presets (`MAP_PRESETS`) bias geography only: sea level, river/cliff/island
gain and `relief_gain` (how much vertical range the land spans - this is what
makes Highlands actually mountainous).

Fair starts: `fair_start_nodes` tops every spawn slot up to wood/stone/food
minima within `FAIR_RADIUS` (guaranteed stone takes a rocky-ground pass first,
then an unfiltered one that keeps the guarantee absolute); `start_point` snaps
spawns to `dominant_region`.
Invariants tested in `sim/tests/worldgen.rs` - fair starts over 100 worlds, no
map is a single biome, archetypes change the world, highlands reach the high
country, soil is richest where the water runs, ore follows the belts. Keep them
green when touching terrain.

## Resources (sim/content.rs scatter rules + protocol farms)

Placement reads geology, climate and RELIEF, never just the biome label:
timber (stand by biome, thickness by rainfall, tapered to nothing between
`TIMBER_SLOPE_T` and `TIMBER_SLOPE_MAX` so no stand grows on ground the client
renders as rock face — the taper tracks `client terrain::ROCK_LO/HI`), quarry
(exposed rock: scarp slope + ore + height, and above `SCREE_T` the soil has
slid off so bedrock quarries whatever the label says — this is what puts
outcrops on foothill flanks instead of the plain), herds (grazing = fertility
+ rain, penalized by slope), fishery (the water the shore FACES - lake teems,
river runs thin), vein (mineralized rock only), placer (channel gravel below
ore-bearing highlands - cheap, safe, early, finite), motherlode (remote high
country, gated on ore and broken ground). `NodeSite.slope` is the same field
the camera draws and the pathfinder charges for, so a deposit that reads as
clinging to a scarp is on one. `resprobe` is the dial.

`ResourceNode` carries `cap` + `regen`. Wild timber/ore/herds are FINITE on
purpose; the renewables are farms and hut-tended fisheries. A **Farm** may only
be sown where `soil_quality >= FARM_MIN_FERTILITY` (`PlaceError::PoorSoil`, one
rule behind the command, the AI and the ghost); it spawns a `FieldOf` food node
that regrows at a rate the soil sets, and `reap_orphan_fields` drops the crop
whatever killed the building. Siting a farm paints fertility straight onto the
terrain (mesh UV.x carries it, `TerrainExtension.overlay` fades it in).

Movement costs are real: `find_path_costed` + `move_cost_at` make marsh drag
and dunes bite. Costs clamp at 1 so the octile heuristic stays admissible.

## Sim cadences

Base tick 50 ms (20 Hz). Movement+separation every tick/2; gather+combat
every 4 (200 ms); brain+research every 20 (1 s); economy every 40 (2 s).
Run-conditions via `every(n)`; `MatchStatuses` gates paused matches.

## Perf doctrine

Worst-case all-out melee on one box: ~150 t/s @20k units, ~30 @50k (2 clients
re-simulating, `net_bench 2 <units> 200`). Costed pathfinding is what those
numbers bought: at 72edec4, before `move_cost_at` existed, the same runs were
~200 and ~47 t/s. A non-uniform cost field breaks A*'s tie-plateaus, so it
expands more nodes per query — budget for it before adding another costed
consumer. Hot-path rules: no per-tick allocation (scratch
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
- **Terrain ships CONTINUOUS FIELDS, never a biome label.** `build_fields`
  bakes N x N arrays — palette (biome colour modulated by the worldgrid's own
  moisture + temperature, with a large-scale relief AO baked in), land
  coverage, rock exposure, aridity and rock hue — blurs palette/rock over
  ~2.5 tiles with WATER AT WEIGHT ZERO, and every vertex reads them through
  one smoothstepped bilinear `tap` (plain bilinear is C0 and its kinks print a
  one-tile lattice down any steep face). Vertex COLOR.rgb = palette, COLOR.a =
  rock exposure, UV_0 = (fertility, land), UV_1 = (aridity, rock hue).
  `render/terrain.wgsl` then does what vertex data structurally cannot: a
  3-octave value-noise BUMP NORMAL (each octave rotated so the lattice never
  streaks, triplanar on steep ground, faded at its own Nyquist limit from
  `fwidth`), triplanar rock with hue-shifted strata banded on world Y, a scree
  apron at the foot of faces, and every mask re-sharpened through a
  noise-warped `smoothstep` at PIXEL resolution. Quad diagonals alternate by
  parity and interior detail vertices jitter +-0.15 tile, so a silhouette is
  an irregular edge instead of a run of identical teeth. `HeightField` samples
  the same half-tile lattice the mesh is built on. Camera: `Msaa::Sample4` +
  TonyMcMapface with a `ColorGrading` contrast bump (AgX was measured and is
  FLATTER here, not sharper).
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
