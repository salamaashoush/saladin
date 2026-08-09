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
   The SEA is a second movement domain with the same shape: `sailable_grid` /
   `water_region_grid` / `main_water_body` / `sea_reachable` mirror the land
   four, each with its OWN thread-local last-seed cell (one shared cell would
   thrash — A* touches the closure ~16x per expansion). `biomes::water_class`
   is the single authority on "is this wet"; never write `!biome_passable`,
   which calls a cliff water. The A* core knows nothing about domains: it takes
   a `passable` closure, and `approach_tile_in` takes the region labelling as a
   parameter so a hull and a walker share one implementation.
8. **The map preset rides in the seed's top 3 bits** (`compose_seed(base,
   preset)`), so every per-seed cache and the wire stay plain u32. Always
   compose before writing `WorldConfig.seed`; `seed_base`/`seed_preset`/
   `seed_bias` decode.

## Commands

```bash
cargo test --workspace                 # 492 tests, all must stay green
cargo run -p saladin-client --bin saladin-client          # single player
cargo run -p saladin-client --bin saladin-client connect <ip>   # dev shortcut (menus cover all MP flows)
cargo run -p saladin-server                                # internet relay (rooms) — VPS docs: crates/server/README.md
cargo run --release -p saladin-protocol --example net_bench -- 2 50000 200
                                       # lockstep benchmark: clients units ticks
cargo run --release -p saladin-protocol --example naval_war -- [diff] [secs] [seeds]
                                       # DOES THE BOT SAIL: two bots on the
                                       # archipelago seeds whose first two starts
                                       # are on DIFFERENT islands (the only
                                       # configuration a land army cannot
                                       # resolve). Reports harbours, hulls, men
                                       # aboard, men put ashore on the far
                                       # island, and whether the match ended.
                                       # NW_BEACH=1 checks EVERY tick that no
                                       # hull stands on land and no land unit
                                       # stands in water; NW_DETAIL=1 prints the
                                       # ladder's stocks and buildings; NW_SEEDS,
                                       # NW_PRESET override the sweep.
cargo run -p saladin-sim --example mapdump -- <base> <preset> [out.ppm]
                                       # worldgen tuning: biome map + dominant-region dump
cargo run --release -p saladin-protocol --example farm_worth
                                       # THE food balance table: food/s a field
                                       # delivers at each crew size, what that
                                       # buys in men mustered, and what the road
                                       # costs a column at every depth
cargo run --release -p saladin-sim --example worldstat -- [seeds] [preset|all] [--per-seed] [--seeds a,b,c]
                                       # biome/climate histogram + height/slope
                                       # quantiles + high-country share: THE
                                       # diversity and shape dial
cargo run --release -p saladin-sim --example massifprobe [start <preset>]
                                       # per-seed counts worldstat rounds away;
                                       # `start` finds seeds with a massif by slot 0
cargo run --release -p saladin-sim --example navalprobe -- [seeds] [--sea|--starts|--reach|--grids|--fish|--nodes]
                                       # THE water audit: is the sea one body,
                                       # where the eight starts sit, what share
                                       # of the map's nodes a start can reach on
                                       # foot vs by sea, what the sea grids cost,
                                       # where fisheries land. `--starts` output
                                       # is diffable: that is how a change to the
                                       # seating rule is PROVED not to move the
                                       # mainland presets.
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
   # farm (FIVE farms finished through the REAL construction path so their
   #   FieldOf crops are sown, camera framed on the plots, each field pinned
   #   EVERY FRAME to one of the five crop stages — stubble/shoots/green/ripe/
   #   lodged — so one shot holds the whole lifecycle; first farm selected.
   #   SALADIN_CROP=<n> pins every field to n instead (two runs at different n
   #   diff to zero over the farm mesh: the crop is NOT on the building),
   #   SALADIN_SURVEY=1 tallies what the food-node variant table draws on every
   #   farm-eligible tile of the map, SALADIN_WORK=1 pins peasants in the
   #   reaping pose) |
   # units (every unit kind + one node of each type + a SHELF fishery and a
   #   DEEP one beside the keep, which are two different meshes — model
   #   verification) |
   # ferry (the naval loop as one still: a barge standing off the beach and
   #   running in with a party aboard AND SELECTED so the card shows its hold,
   #   a landing party on the sand, a skiff under way over the nearest school,
   #   a Harbour on the beach beside the anchorage, camera framed on the
   #   landing. Pair it with SALADIN_YAW=2 — an iso camera hides a berth behind
   #   its own keep — and SALADIN_SHOT_AT=<s> to catch the boat where you want
   #   it) |
   # harbour (the quay conjured + selected beside the keep, like every other
   #   building mode — inland, so it is the MODEL and the panel under test).
   # Overrides: SALADIN_SEED, SALADIN_PRESET, SALADIN_TAB, SALADIN_ZOOM
   # (view_size, min 4 = close-up model inspection), SALADIN_YAW,
   # SALADIN_PERF=1 (starts the F3 frame-time overlay on, so a shot can be
   # read as a benchmark).

cargo run -p saladin-protocol --bin saladin-headless -- --port 7777 --seed 4
                                       # THE SIM WITH NO WINDOW on a scripted
                                       # clock: nothing moves until {"step": N}
                                       # grants ticks. --free runs it flat out.
python3 scripts/playtest.py --port 7777 [--record f] [--replay f] [--shot png]
                                       # an agent playing a whole match over
                                       # devctl: base, army, march, assert
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

## devctl: driving the game from outside (protocol/src/devctl/)

`SALADIN_DEVCTL=<port>` opens a line-delimited JSON socket so a script or an
agent plays the game and reads it back. Unset: no listener, no systems, no
cost. Full protocol table in the README; two rules govern every change here.

1. **WRITES GO THROUGH `PlayerCommand`, NEVER THROUGH `World`.** A request
   parses into a command and lands in `Devctl::outbox`, which the host drains
   into its lockstep driver beside the local player's clicks. That is the only
   reason this is multiplayer-safe, and
   `tests/devctl.rs::a_devctl_driven_peer_stays_hash_identical_to_a_plain_one`
   is the proof — two peers, one driven over the socket, hashes compared on
   every one of 1200+ ticks. Compare PER TICK NUMBER: the two drivers run a
   tick apart (each stalls until the other has submitted), so a per-round
   compare never fires and passes vacuously.
   A debug action with no command (spawn ten knights) needs a NEW variant
   APPENDED (bincode encodes the index), `PROTOCOL_VERSION` bumped, and a gate
   in SIM STATE — an env-var gate means the peer without it refuses what the
   devctl peer applied, which is a desync. None exists today.
2. **READS NEVER MUTATE.** `state.rs` goes through `World::try_query`, which
   takes `&World`; the ordinary `world.query()` wants `&mut World` and a read
   that can mutate is a read that can desync. A component nothing has spawned
   reads as an empty list. Rows come out sorted by `GameId` — archetype order
   is not stable enough to diff two captures against, and diffing is the point.

The one thing devctl keeps of its own is a mirror of `CommandFeedback`:
`apply_commands` clears it EVERY TICK, so a host that runs 600 ticks between
polls must call `capture_feedback` after each one or a script only ever sees
the last tick's refusals. Never part of the state hash (the `ShotEvents` rule).

`Fx` crosses the wire as a plain decimal parsed digit by digit.
`Fx::from_num(f64)` would do it in one line and put a float in the protocol
crate. Output is f64 and read by nothing.

`command_to_json`'s match is exhaustive: a new `PlayerCommand` variant breaks
the build there, which is what keeps the parser and `COMMAND_NAMES` honest.

Hosts wire it in three lines — publish `DevctlLink` (submit tick, may_step,
renders), drain `take_outbox` into the driver, `capture_feedback` after each
tick. `step` is HEADLESS ONLY (`may_step`); in a client, time belongs to the
fixed-update clock and in a match to the lockstep group. Screenshots are the
client's half (`client/src/devctl_client.rs`): the reply is completed INSIDE
the capture observer, after the PNG is written, so a script never races a
half-written file. `{"camera": ...}` uses `camera::snap_to_targets` —
`smooth_camera` early-returns once live == target, so assigning both and
shooting leaves the transform where it was.

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
then an unfiltered one that keeps the guarantee absolute). `start_point` snaps
spawns to `start_regions(seed)`: on a mainland preset that is exactly
`[dominant_region]` and nothing moved, but where a preset sets
`MapBias::sea_starts` (archipelago only) it is every island of at least
`START_REGION_MIN` tiles with a shore on the main water body, seated
area-proportionally with a per-island cap. The guarantee is no longer "one
landmass" but "landmasses the sea connects" — which is why every start island
must touch `main_water_body`, and why the keep-site test asserts the start's own
island rather than the dominant one.
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
+ rain, penalized by slope), fishery (IN the water, split inshore/offshore -
lake teems, river runs thin, the deep is richer and further out), vein (mineralized rock only), placer (channel gravel below
ore-bearing highlands - cheap, safe, early, finite), motherlode (remote high
country, gated on ore and broken ground). `NodeSite.slope` is the same field
the camera draws and the pathfinder charges for, so a deposit that reads as
clinging to a scarp is on one. `resprobe` is the dial.

`ResourceNode` carries `cap` + `regen`. Wild timber/ore/herds are FINITE on
purpose. A node drawn to zero is DESPAWNED exactly when nothing brings it back
(`regen == 0`, full stop) — a field reaped bare is stubble, not a hole, and
deleting it is what used to turn every worked farm into 50 wood of scenery. The
gate used to ALSO spare anything near a fishing hut, which made every wood and
stone node emptied within six tiles of one a permanent zero-remaining row.

**Farming is a SEASON, not a bucket** (`sim/farming.rs` + `systems/economy.rs`).
A rock's output is a function of how many hands you put on it; a field's is a
function of TIME AND CARE. Two axes:
- SOIL sets how big: `field_cap(soil)` spans `FARM_CAP_MIN..FARM_CAP_MAX`
  (70..190) between `FARM_MIN_FERTILITY` and `FARM_SOIL_RICH`, ROUNDED — the old
  truncated `1 + soil*7` regen landed on 2 or 3 across 84-94% of sowable land,
  so every farm in the world was the same farm. Siting still paints fertility
  onto the terrain (mesh UV.x, `TerrainExtension.overlay`) and now that number
  still matters after the ghost goes.
- LABOUR sets how fast: `field_growth(hands, cap, aura)` = a rain-fed creep
  (`FARM_REGEN_IDLE`) + `cap * work_step(hands, ECONOMY_DT, FARM_TEND_TIME)` +
  the Granary's aura. Hands ride `BUILDER_RATE`, so three hands over three farms
  beat three on one.

The wheel: sown at `cap/FARM_SOW_DIVISOR` -> GROW -> `Crop.ripe` LATCHES at cap
-> REAP -> stubble -> grow again. **A growing crop cannot be cut** (`reapable()`
gates `best_node`, the idle-gatherer balancer and the harvest itself) — which is
what stops draw outrunning growth and inverts the old perversity where a short
haul killed a field fastest. A ripe crop nobody cuts counts `Crop.standing`, and
past `FARM_RIPE_GRACE` (doubled under a hub) LODGES at `lodge_loss(cap)` a tick:
visible, gradual, salvageable, never a delete. Cutting it resets that clock.
Tending needs NO new command: a farmhand IS a committed builder
(`wants_work()` keeps a crew on a standing farm, `PlayerCommand::Repair` puts
more in, `Building.builders` is the crew count the economy reads, and an explicit
Move/Gather/Attack takes a hand back). `reap_orphan_fields` — the building
falling — is now the ONLY way a field dies.

Movement costs are real: `find_path_costed` + `move_cost_at` make marsh drag
and dunes bite. Costs clamp at 1 so the octile heuristic stays admissible.

## The larder: what food is FOR (sim/supply.rs)

**Food buys men and moves them; it does not tax them for existing.** ONE model,
two halves of the same sentence, and nothing else:

1. **A soldier is raised with bread.** Three quarters of every fighting man's
   old timber price is FOOD (`UnitDef.cost`; engines, hulls and peasants have no
   stomach and stayed on timber). This is what the whole farming and fishing
   economy is now paid for, and it is the AoE limiter that actually works: army
   SIZE is bounded by the pop cap and by what a man costs to muster.
2. **THE BAGGAGE TRAIN.** A man within `SUPPLY_RADIUS` (34) of one of his own
   drop-offs draws NOTHING — exactly zero, not "a little". Past it,
   `strain(dist)` ramps one full ration per `SUPPLY_SPAN` (34) tiles, capped at
   `MAX_STRAIN` (3), and `FIELD_RATION` (0.1 food per man per economy tick per
   unit of strain) is THE ONE RATE. Sim, AI, HUD and tooling all price the road
   through it. Supply bounds DEPTH AND DURATION, never size.

Everything else in the file is consequence, and there is no hp term anywhere in
it: a shortfall rations PROPORTIONALLY over the field force, caps morale
(`morale_ceiling`), slows the arm (`fatigue_ticks`), and after
`STARVE_GRACE_TICKS` the men with the least heart WALK AWAY (`deserts`). A
column standing on a wild herd forages, thinly, and strips it.

**The counterplay is a forward store** (`tests/starvation.rs::
a_forward_store_ends_the_famine`). Strain is measured to the NEAREST own
drop-off, so a Storehouse planted at a siege camp zeroes the bill — which is
what a besieging camp IS, and a building the defender can sortie against.

Why the flat per-head tax is gone, in one line: a flow subtracted from a STOCK
has no band. `bill = men * FOOD_PER_UNIT` at the rate that bit was a death
spiral; at a quarter of it, measured, ten soldiers drew 1.25 food/s against an
1868 stockpile and the mechanic was decoration. `FOOD_PER_UNIT`, `STARVE_DPS`,
`apply_upkeep` and the two-band near/far bill are all DELETED.

Measured (`examples/farm_worth`): a 3-hand field delivers 2.53 food/s and buys a
Spearman every 11.8 s. Twenty men at full strain cost 3.0 food/s — 1.18 fields'
entire output — so 500 food funds a deep siege for 167 s. At home the same
twenty men cost nothing, forever.

The bot prices all of it off `campaign_reserve(soldiers)` = one campaign at full
depth. `food_cushion` (that plus `food_floor * 4`) is ONE high-water mark with
three users — `next_trade` sells past it, `field_labour` stops staffing the
wheat past it, and the gatherer steer moves hands off food past it. Measured on
three seeds, that took the bot from 1868/928/1784 food with 10/13/6 soldiers to
327/635/867 with 11/12/12, and its wood from 10/8/6 to 200/76/44.

## The naval roster (sim/units.rs + buildings_defs.rs)

`UnitRole::Boat` is the one role that moves in `Domain::Sea`. `UnitDef.domain`
and `UnitDef.cargo_cap` are STATIC (nothing serialized, nothing hashed), so a
hull costs zero bytes on the wire and zero in a save. Two hulls, both
FACTION_BOTH — crossing the sea is not an asymmetry; WHAT you land is:
- **Fishing Skiff** (13) — trained at the Fishing Hut, `carry` 20, the only hand
  that can work a fishery now that fisheries are in the water.
- **Barge** (14) — trained at the Harbour, `cargo_cap` 6. Its `radius` is
  EXACTLY the Ram's, the widest body already in the roster, so `MAX_SEP` and
  the separation cell scan do not widen for every unit on the map.

A hull never fights (`attack == 0`), so it never enters the duel matrix, never
draws rations (`draws_rations` reads ROLE), never builds (`UnitDef::builds()` is
`role == Worker`, NOT `carry > 0` — a skiff carries more than a peasant and
cannot reach a site), and combat keeps exactly ONE passability grid.

**A hull launches at a BERTH or not at all.** `buildings::berth_of(seed,
footprint, pos)` is the water tile orthogonally bordering a footprint — ocean
first, then lowest `tile_key`, so a hut wedged between a puddle and the sea
berths its skiff on the sea and every peer picks the same tile forever. No
berth = the training order is REFUSED and refunded (`spawn_trained`), because a
hull snapped onto land would be beached forever: `movement` walks whatever path
it is handed and never tests terrain.

**Harbour** (BuildingKind 16, Economy tab) carries `needs_sea_berth`: shoreline
is not scarce (93-99.7% of coastal land passes `requires_water`), so the
constraint that makes siting a naval base a decision is a berth on
`main_water_body` PLUS the Fishing Hut prerequisite PLUS 40 stone. `requires_water`
alone would float a harbour on a one-tile puddle.

## The fishing loop (systems/gather.rs + economy.rs)

**A node on water is workable ONLY by a `Domain::Sea` unit, and a node on land
ONLY by a walker.** `node_domain(seed, pos)` answers it as a pure O(1) read of
the cached sailable grid — no field on the row, no word in the StateHash, no
save migration. The gate sits in FOUR places and needs all four: `best_node`'s
candidate loop (before the `NODE_TRIES` budget, or a shore start burns every
retry on schools no walker can work), the idle-gatherer balancer, the `Gather`
command, and the `ToResource`/`Harvesting` arms themselves — because
`harvest_reach` on a water node is 1.7 tiles, which is exactly enough for a
peasant on the sand to net the first tile of sea.

`move_patch` and `best_node` build their closures from `domain_passable` and
pick their region grid to match (`region_grid` / `water_region_grid`,
`node_reachable` / `sea_reachable`). Sea pathing uses a FLAT step cost: there is
no naval move-cost grid, and the flat cost is what keeps `clear_straight_line`'s
fast path, which is the common open-water hop.

**A hull banks at a BERTH.** `Dropoff` carries `berth: Option<V2>`, computed only
for `requires_water` structures, and a sea hauler steers for it while the
banking test still measures the BUILDING — a waterside store always has a
sailable tile abutting it. A hull's drop-offs are filtered to berths its own
water body reaches, so a skiff never picks the keep two tiles inland.

**A boat HOLDS STATION over a school it has emptied**, in `Harvesting` and on
the return leg both. This needs no state: `gather` never looks at an idle hand
with no job site, and a hull has none, so standing it down at the last fish
loses it for the match. `reapable` takes the node out of the candidate list and
puts it back on its own, and a boat on station has always just banked.

**A hut MULTIPLIES its fishery's regrowth; it does not supply it.** The flat
top-up this replaces was measurably NEGATIVE — the same aura doubles the draw,
so a tended school emptied 20% faster than an untended one — and it filled to
`FOOD_YIELD` instead of the node's own cap. `WorkAura.regen` therefore means
growth-per-tick for `Field` and a MULTIPLIER for `WaterFood`.

Measured (`examples/fish_econ`, 600 s runs, against a farm's 1.36 food/s per
hand forever): tended inshore 1.19 food/s per skiff, tended offshore at a hut
1.48, harbour-tended offshore 3.49. **The per-node flow is the cap** — a skiff
drains at ~4.7 food/s, so a second boat on one school halves each one's rate
with no magic number anywhere. An average start has 2.5-3.6 fisheries inside
`TOWN_RADIUS`: the sea is a supplement that never out-scales the plough.

## The second movement domain, and the ferry

**A domain is chosen where a closure is BUILT, and nowhere else.** `movement`
walks whatever path it is handed with no terrain test at all, so a wrong-domain
closure does not fail a pathfind — it drives a boat inland, silently, and no
existing test notices. Every site that builds one reads the mover's
`unit_def(kind).domain`: `path_to`, `move_unit`, `lay_march`,
`assign_idle_gatherers`, `best_node`, `move_patch`, `spawn_trained` and
`separation`. `crates/protocol/tests/naval.rs::no_boat_ever_stands_on_land` is
the net.

`separation` filters PAIRS by domain as well as landings. Without the landing
filter a hull on open water has all three candidate tiles refused and stacks
into one sprite; without the pair filter a column on the beach shoves the barge
it is unloading from. Measured cost of the pair filter at 20k units: nil
(3.577 ms vs 3.598 ms sim avg, inside noise).

**`line_of_sight` is a SAMPLED test and `clear_straight_line` is an exact one.**
A leg that clips a tile corner between two samples reads as clear — invisible
when a man shaves a wall, a boat on a hillside when a hull shaves a headland.
`AStar::find_path_costed_in` therefore takes a `Smoothing`, and `Domain::
smoothing()` is the single place that says a hull is `Exact`: it measures every
string-pulled leg with the DDA **and the tail onto the caller's raw target**,
which `find_path_costed` otherwise hands back unchecked ("pass a passable target
for a clean finish" is a footgun, not a contract). Land stays `Sampled` and every
land path in the game is bit-identical — `net_bench 2 3000 400` returns the same
lockstep hash, which is the proof.

**A CORRECT PATH IS NOT ENOUGH: `movement` refuses to beach a hull.** `step_toward`
is fixed point and `separation` nudges, so a leg that runs along a tile boundary
crosses it on a hair of drift with a perfectly legal path still in hand —
MEASURED, a Hard bot's fishing fleet beached itself inside fifteen minutes on
four of eight archipelago seeds. `movement` slides a `Domain::Sea` step along
whichever axis still floats, the same three-way landing `separation` uses, and a
slid hull is NOT counted as arrived. Land pays one enum compare.
`tests/naval.rs::a_hull_handed_a_leg_across_land_still_never_stands_on_it` hands
a hull a leg straight through a headland, which is the worst any producer could
do; the order-driven test above it cannot catch this class because every order
lays a leg that WAS clear when it was laid.

**The mirror image: a land unit must not be ordered onto water.** `move_unit`
snaps its target into the mover's own domain in BOTH directions now (`lay_march`
always did) — a column mustered onto a harbour berth marched into the sea and
stood there for the rest of the match. And a pursuit whose quarry stands where
the hunter cannot go — a hull at sea, which is new — asks for `Exact` so the
chase ends at the water's edge instead of walking three crossbowmen into open
water. One bitset read per pursuit; a pursuit over ground the hunter can stand on
is unchanged.

**Cargo is `Unit::garrisoned_in` pointed at a HULL.** Already serialized,
already hashed, already skipped by movement and combat — zero new state. It has
its own verb (`Embark`/`Disembark`) because `garrison()` demands a `Building`
row and a `garrisonable` kind. Three things a moving host broke, all fixed in
place: `movement` now copies a hull's `Pos` onto its cargo every move tick (so
supply, foraging, desertion and the state hash all read a TRUTHFUL position
instead of the beach it boarded from — hosts are resolved through `GameIndex`,
with a scan only when the four-tick index has not caught up); `combat` walks a
UNIT-host death branch (`CombatScratch.cargo`) so a sunk barge's party DROWNS
rather than orphaning units that point at a dead GameId; and `Disembark` snaps
to the legal LAND tile nearest the click within `LANDING_REACH`, so a landing
needs no harbour at the far end.

## The bot at sea (sim/ai.rs + systems/ai_brain.rs)

A naval system the bot cannot use means the Archipelago preset stays unplayable
against bots, which is most of how this game is played. Measured over 8 seeds
whose first two starts are on DIFFERENT islands (46 of 59 archipelago bases are,
0 of 59 on every mainland preset), 30-minute Hard-vs-Hard: **0 of 8 matches
resolved before this landed, 6 of 8 after — 8 of 8 raise a harbour, launch a
barge and put a party on the far island.** `examples/naval_war` is the
instrument (`NW_BEACH=1` checks the hull and land-unit ground invariants EVERY
tick, `NW_DETAIL=1` prints the ladder's stocks and buildings).

**A quay is bought with STONE, and an island runs out of it.** `next_trade` sold
gluts into gold and never bought back, so a bot with 912 wood, a standing Market,
107 gold and zero stone sat for fifteen minutes unable to afford the one building
that could end the match. It now buys the Harbour's shortfall — gated on
`naval_wanted && !enemy_by_land && !owns(Harbour)`, so no mainland preset can
reach the rung at all. That single clause is 5 of 8 resolved -> 6 of 8.

**A LADEN hull outlives its quay.** The crossing used to need a standing Harbour
for its berth, so razing the quay left a barge floating with six men aboard, off
the muster roll and out of the match forever. The water body now falls back to
the hull's own (lowest id, so every peer agrees) and a hold with men in it lands
them. The muster, though, is DRY GROUND or it is nothing — `quay_spot` returning
`None` used to fall back to the berth itself, which is a water tile.

`PlannerState` reads the sea through four fields and nothing else:
`fisheries` + `fishery_centroid` (schools a hut on THIS coast could put a boat
on — reachable by `sea_reachable` from the nearest water, inside
`FISHERY_RANGE`), `offshore_cluster` (a cluster on the bot's water but off its
land, which `remote_cluster` structurally cannot see because everything past the
beach fails `node_reachable`), `boats`/`ferries` (hulls afloat AND queued — a
skiff takes 10 s and a Hard window is 0.6, so without the queue count a
three-boat target buys six), and `enemy_by_land`. `naval_wanted` is
`barge_target > 0 && (offshore_cluster.is_some() || !enemy_by_land)`: the second
clause is what FORCES a harbour on an island map.

**WHERE a building goes is half of what it does.** The anchor match in
`ai_brain` ended `_ => keep_pos`, so the Fishing Hut — whose only working
function is being a drop-off NEAR THE FISH — was planted beside the Keep, which
already accepts food. `shore_anchor` now ranks buildable waterside ground by
what it can REACH (fisheries inside `FISHING_HUT_RANGE` for the hut, distance to
the offshore cluster or the enemy for the harbour), not by distance from the
keep. `SHORE_SCAN` is 20, not 14: a Highlands keep sits up to ~18 tiles from
water, and 20 is still inside `TOWN_RADIUS` on the diagonal.

**A refused shoreline is a COOLDOWN (`Bot.waterside_cd`), never a latch.** The
old `fishing_blocked` bool disabled fishing for a whole match on one blocked
probe — including one blocked by a peasant standing on the only legal tile. It
suppresses the rung through the same `sites_in_flight` mechanism every other
blocked rung uses, and it comes back.

**The crossing is crude ON PURPOSE**: muster on the quay, fill the hold, sail,
put the party on the nearest legal enemy shore, hand straight off to the LAND
assault that already exists. No escort, no beach selection, no withdrawal by
sea. Three things it must keep doing:
- Men and hull are sent to the SAME point (`quay_spot`). A hull at its berth and
  a column at the quay beside it end up outside a gangplank across a corner.
- The muster is ONE ORDER PER MAN, not a group march. `lay_march` routes from
  the group CENTROID; a knot whose centroid falls on a building footprint gets
  an empty route and NOBODY moves — measured, six men re-issued the same doomed
  order every second for the rest of the match while a seventh walked in alone.
- A LADEN hull finishes its crossing whatever the muster gate says now. The gate
  falls the moment the wave takes casualties, and a laden barge left floating is
  a landing party deleted from the match.

`FieldUnit.region` is why none of this costs a pathfind: an order across water is
an A* that spends its whole expansion budget every brain tick and hands back
nothing, so who can reach what is answered with an O(1) read off `region_grid` —
in the assault, the recommit, the retreat and the scout alike. Measured on 6 Hard
bots x 8000 ticks: **+0.2%** on a mainland map (5.135 -> 5.146 ms/tick) and
**-31%** on an archipelago (0.477 -> 0.327), because the doomed A* is what the
region read replaces.

**`SHORE_SCAN` and `SOIL_SCAN` are separate numbers.** `place_near` ring-probes
the full perimeter for anything with a soil or a water requirement, and a bot at
its soil limit re-probes for a farm it cannot site EVERY decision window forever.
Widening the one radius to reach a Highlands shoreline widened the farm probe
too and cost 60% of the whole AI budget on a mainland map — measured, 5.6 ->
9.1 ms/tick. A shore probe is once behind a cooldown; a soil probe is forever.

Hulls are scored OUT of `counter_score`/`counter_dps`: `Census` is indexed by
kind, so a barge lands in one for free, and a bot facing a wall of Sergeants must
never answer it with a ferry.

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
- ANYTHING AFLOAT USES `float_y`, NEVER `height_at`: hulls and fish schools are
  clamped to `WATERLINE_Y` (-0.015, the highest value `surface_height` returns
  for a wet tile). Measured, the drawn water is not one plane — deep water is
  exactly -0.215 on every preset, shallows ramp up to -0.015, rivers floor
  -0.063..-0.192, lakes -0.063..-0.215 — so a fixed sea plane buries a lake
  skiff; and raw `height_at` bilinear-blends the beach in and walks a boat up
  the sand. Hulls are ONE `Body` rig part pivoted at the origin (that origin is
  the waterline) so `animate_units` can heave/roll the whole boat; they get no
  walk `hop`. The wake is one shared quad + one shared material per hull, shown
  only while moving AND floating.
- WHICH FOOD NODE IS A FISHERY IS THE SIM'S ANSWER (`is_sailable`), never a
  render height. The two disagree — 221/6793/8919/0 tiles per preset over base
  11 — and every disagreement is dry ground drawn low, which is how a HERD came
  to wear ripple rings. Deep water draws the bigger `fish_shoal` mesh.
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
