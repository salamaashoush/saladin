I have enough verified ground truth. Key confirmations: 21 reducers in monolithic `index.ts` (1418 lines), `SaladinGame.ts` at 1984 lines, refactor #1 scaffolding exists but empty (only `schedule_refs.ts`), `cost: number` (single wood) in defs, `config.worldSize` written but `scatterTrees` uses `ctx.random` (non-reproducible), no `matchId`/`morale`/`carryType` columns yet, `WORLD_SIZE` compile-time const. The roadmap below is grounded in this verified state.

---

# SALADIN — MASTER EXPANDED ROADMAP

Verified baseline (2026-06-08): `spacetimedb/src/index.ts` = 1418 lines, 21 reducers, monolithic. `src/game/SaladinGame.ts` = 1984 lines, monolithic. Refactor-1 scaffolding (`schema/`, `reducers/`, `systems/`, `world/`) exists but EMPTY (only `schema/schedule_refs.ts`). `cost: number` (single wood) in `shared/defs.ts`. `config.worldSize` is written at init but never read; `scatterTrees` uses `ctx.random` (client cannot reproduce). No `matchId`/`morale`/`carryType`/`stone/food/gold` columns. `WORLD_SIZE` is a compile-time const. Enums today: 4 UnitKinds, 6 BuildingKinds, ResourceType already has Wood/Stone/Food/Gold, Faction Ayyubid/Crusader.

The roadmap is 11 dependency-ordered iterations. The two refactors come first because EVERY later iteration adds either a reducer/system (module) or a mesh/UI (client), and both target files are monoliths — splitting them first converts every downstream feature from a serialized merge-conflict into a parallel per-file lane.

---

## ITERATION 0 — Module refactor: split index.ts into schema/world/systems/reducers (THE unblocker)

- Slice: Relocate the 1418-line monolith into the already-scaffolded `schema/tables.ts` (all 12 `table()` defs + `schema()` called ONCE in `schema/db.ts` + `Ctx = ReducerCtx<typeof spacetimedb.schemaType>` in `schema/context.ts`), `world/*` typed helpers (spawn/placement/economy/commands — owner-parameterized, never read `ctx.sender`), `systems/*` (movement/gather_ai/combat/ai_brain scheduled reducers), `reducers/*` (match/unit/build command reducers), `lifecycle.ts`. `index.ts` becomes a THIN BARREL that `export {default} from './schema/db.ts'` then `export *` every system/command file. Zero reducer renames → byte-identical bindings.
- Effort: **L**
- dependsOn: none (scaffolding dirs already exist)
- WHY FIRST: SpacetimeDB discovers reducers ONLY via named exports of the entry module (verified mechanism: `Schema[moduleHooks]` walks `Object.entries(exports)`). A reducer in a new file is invisible unless re-exported from `index.ts`. Establishing the `Ctx` type + single `spacetimedb` singleton + owner-parameterized `world/*` helpers is the stable seam that lets Iterations 2/5/7/8 each drop a new system into its own file. Without it, every later module change collides in one 1400→3000-line file.
- **Most important ferridriver check:** describe-parity gate first (`spacetime describe saladin --json` must list all 21 reducers + 12 tables identically pre/post), then drive client: enter game → train a unit → place a wall → start skirmish vs AI. Behavior byte-identical.

## ITERATION 1 — Client refactor: split SaladinGame.ts into meshes/coordinators/orchestrator (parallel-model unblocker)

- Slice: Split the 1984-line monolith into stateless pure factories `game/meshes/{units,buildings,props,indicators,materials}.ts` (`build*(args)=>THREE.Group`, the pattern `Terrain.ts`/`Environment.ts` already prove), stateful coordinators `EntityPool.ts` (objs Map + interpolation + the `pendingBuildings` cross-table-arrival deferral — keep co-located), `WallGrid.ts` (occupancy + wall orientation), `Picking.ts`, `Selection.ts`, `Commands.ts`, `BuildController.ts`, `DemolishController.ts`, `InputController.ts`, `CameraRig.ts`, `Minimap.ts`, `SceneRig.ts`, and a ~250-line `SaladinGame.ts` orchestrator preserving the exact public API (`setIdentity/attach/detach/setMinimapCanvas/focusWorld/setSelectedStance/dispose/debugInfo`). `useGameSession.ts` and `App.tsx` change ZERO.
- Effort: **L**
- dependsOn: none (independent of Iter 0; can run concurrently)
- WHY SECOND/PARALLEL: The whole point is to make `meshes/units.ts` and `meshes/buildings.ts` separate stateless files so model-polish (Iter 9) and roster meshes (Iter 5) can be sculpted by independent agents with zero merge conflict. Reducer call sites consolidate to exactly three modules, making "what the client can request" auditable (server-authority enforcement).
- **Most important ferridriver check:** full input regression in one session — box-select drag, right-click move, right-click attack enemy, drag-place a wall, place a barracks, demolish-paint, minimap click-to-focus, train-from-building. All byte-identical (no logic changed).

## ITERATION 2 — Multi-resource economy (stone/food/gold) — the ResourceCost contract

- Slice: `shared/economy.ts` (`ResourceCost {wood?,stone?,food?,gold?}`, `canAfford/pay/refund/resourceForNode`); change `defs.ts` `cost: number` → `cost: ResourceCost` (ADDITIVE: keep wood costs valid). ADD `player.stone/food/gold:u32`, `unit.carryType:u8`. Generalize `unitAi` gather/deposit to route by `carryType` (must store carryType BEFORE node deletion). `scatterTrees`→`scatterNodes` (4 kinds; fish on coastal land). New `economy_timer`→`economySystem` reducer for food upkeep + starvation (write HP only on change). `marketTrade` reducer (gold). Client: regenerate bindings, `ResourceBar` 4 stats, `BuildBar` cost badges + `canAfford` dimming, minimap node colors.
- Effort: **L**
- dependsOn: Iter 0 (cost/gather logic lands in `world/economy.ts` + `systems/`), Iter 1 (ResourceBar/BuildBar/mesh per resType)
- WHY HERE: `ResourceCost` is the prerequisite contract for Stable/Blacksmith/Castle/Market costs in Iter 5 and for the AI's multi-resource targeting in Iter 7. Schema change (new columns appended at END) → non-breaking auto-migration, publish WITHOUT `-c`.
- **Most important ferridriver check:** gather stone with a peasant → stone count rises → place a stone-cost wall → food upkeep tick drains food → starvation HP drain when food hits 0 → AI also gathers multiple resources.

## ITERATION 3 — World/Terrain v2: variable size, pure shared worldgen, elevation gameplay, instanced render

- Slice: `shared/biomes.ts` (data catalog: color/passable/buildable/moveCostMul/densities/palette), `shared/terraingen.ts` (domain-warped fbm + river/Nile carve + oasis stamps; `terrain.ts` re-exports), `shared/worldgen.ts` (PURE `generateWorld(seed,world)` via `mulberry32`/`hash2` ONLY — no `ctx.random` — so client reproduces tree/node/decoration positions byte-identically), `shared/elevation.ts` (high-ground vision/range/damage), `shared/maps.ts` (MapPreset list). Thread runtime `world` param through `pathfinding.ts`/`buildings.ts`/`defs.ts` (keep `WORLD_SIZE` as default only); WIRE UP `config.worldSize` read-back on client (~12 sites). `init`/`startSkirmish` use `generateWorld`; `combatTick` reads elevation. Client: `game/Vegetation.ts` (per-chunk InstancedMesh decorations, zero rows), `game/chunking.ts`, SkirmishSetup map picker. **Scope: ship WOOD nodes + cosmetic decor + elevation + variable size; stone/food/gold node placement uses Iter 2's economy.**
- Effort: **XL**
- dependsOn: Iter 0, Iter 1, Iter 2 (so placed stone/food/gold nodes are harvestable, not dead props)
- WHY HERE: `WORLD_SIZE` is compile-time and load-bearing in tile-key math (`ty*W+tx`) on BOTH sides — the variable-size refactor must land before fog-of-war (Iter 6) splits subscriptions and before the AI (Iter 7) reasons about expansion. The pure-shared worldgen is the "data-driven, both sides recompute" win that makes decorations free.
- **Most important ferridriver check:** start a skirmish on a large preset → screenshot shows rivers/oasis/instanced decorations and the minimap matches → move a unit onto a hill → confirm its extended fire range fires first (shot event ordering / enemy HP drops first).

## ITERATION 4 — first-class matchId scoping

- Slice: APPEND `matchId:u64` (default `0n`) to entity/unit/building/resource_node/player/ai (end-of-table → non-breaking); ADD `config.nextMatchId:u64`. New `match` table (matchId PK, name, host, status, seed, turn). `shared/match.ts` (MatchStatus enum, which tables carry matchId). `scope.ts` helpers (`unitsOf/buildingsOf/clearMatchRows`) replacing owner-only `clearOwner/clearMatch`. `createMatch/pauseMatch/resumeMatch/endMatch`; `startSkirmish/enterGame/leaveGame` become matchId-aware. Scheduled systems iterate per-ACTIVE-match only (skip `status != active` — fixes today's cross-human reset hazard + enables clean pause).
- Effort: **L**
- dependsOn: Iter 0 (scope helpers in `world/`, systems gated in `systems/`)
- WHY HERE: A match must be a real entity (not the implicit host-scoped fiction) before saves (Iter 10) can mean "all rows where matchId=X" and before pause/resume works. Lands as a clean schema-only-additive step so the bigger save vertical isn't blocked on it.
- **Most important ferridriver check:** two players (human + AI) in separate matches → `spacetime sql 'SELECT matchId,count FROM unit'` shows isolation → `clearMatchRows(A)` leaves match B untouched → pausing match A freezes its units while B keeps simulating.

## ITERATION 5 — Expanded roster + buildings (PURE DATA) + tech-tree gating

- Slice: New enum members (HorseArcher/Mamluk/Crossbowman/Ram/Mangonel/Imam; Stable/Blacksmith/Market/Granary/FishingHut/Watchtower/CastleII/III). Split `defs.ts`→`shared/units.ts`+`shared/buildings_defs.ts` (cost→ResourceCost from Iter 2, `populationCost`, `requires`, `auraRadius`, `garrisonSlots`, `upgradesTo`). `shared/research.ts` skeleton + `shared/garrison.ts` + `shared/tech_tree.ts` (`hasPrereq`). DAMAGE_MATRIX UNCHANGED (Siege×Stone already modeled + unit-tested — only the unit rows were missing). `trainFrom`/`placeFor` gate via `hasPrereq` + `ResourceCost`. Client: new mesh cases in `meshes/units.ts`/`meshes/buildings.ts` (Iter 1 gave the seam; fallback mesh already draws unmapped kinds), BuildBar categories (Economy/Cavalry/Tech/Siege), data-driven per-kind CommandCard tally.
- Effort: **L** (bulk is data + meshes; tech-gate is small reducer wiring)
- dependsOn: Iter 1 (mesh seam), Iter 2 (ResourceCost), Iter 0 (trainFrom/placeFor in `world/commands.ts`)
- WHY HERE: This is the codebase's headline payoff — "new content = data." Every generic mechanic (combat, A*, footprint, tower-fire, pop cap, training gates) already dispatches on def lookups, so the roster ships with ZERO new tables/reducers beyond the `hasPrereq` wiring. Siege-vs-stone needs NO matrix change.
- **Most important ferridriver check:** build a Stable → train a HorseArcher (gated unit appears) → build a Ram → attack an enemy wall, confirm the Ram damages stone while a Spearman barely scratches it (matrix payoff visible).

## ITERATION 6 — Combat mechanics: morale/rout + garrison + siege target priority

- Slice: ADD `unit.morale` + `unit.garrisonedIn:u64` (hot columns); NEW `garrison` table. `shared/morale.ts` (pure thresholds/recovery; Imam aura) + `shared/garrison.ts` (capacity/firepower). `garrisonUnit`/`ungarrison` reducers. `combatTick` gets a morale pass (rout = cheap straight retreat vector, NOT full A*) + garrison-firepower augment + siege `prefersBuildings`. Centralize an `isInField(u)` guard so every loop (move/gather/combat/popInfo/aiBrain) consistently skips garrisoned units. Client: garrison/ungarrison buttons + morale indicator in CommandCard.
- Effort: **L**
- dependsOn: Iter 0 (combat in `systems/combat.ts`), Iter 1 (CommandCard), Iter 5 (Imam/Watchtower/Castle rows; garrison-capable structures)
- WHY HERE: These fold into the existing `combatTick` and only make sense once the roster (Imam aura, towers with garrison slots, siege units) exists. Splitting `index.ts` first (Iter 0) means morale and garrison each land in `systems/combat.ts` rather than re-bloating a monolith.
- **Most important ferridriver check:** garrison 3 archers into a tower → enemy approaches → tower fires extra shots (firepower sums) → rout a losing enemy squad by killing half (morale collapses, they flee toward home and stop attacking).

## ITERATION 7 — Smarter strategic AI: deterministic phase-machine planner

- Slice: EXTEND `ai` table (phase:u8, phaseTimer, scoutEntity, lastThreatSeen, expandX/Y, rallyX/Y, intelCd). `shared/ai/{types,phases,composition,threat}.ts` — PURE planner: WorldView (perception snapshot) → `nextPhase` state machine (Opening/Expand/BuildUp/Push/Defend/Regroup) → `desiredComposition` (counter-pick via `effectiveDamage` over scouted enemy armor mix) → `AiAction[]`. Module-side `ai/perception.ts` (build a SHARED census once per tick, slice per-bot — not per-bot rescan) + `ai/execute.ts` (map AiAction onto existing owner-parameterized helpers — NO cheats) + `ai/brain.ts`. `AiProfile` gains reactionTicks/scoutEnabled/apmBudget/counterPickWeight/expandThreshold/retreatHpFraction/targetPriority. Easy = today's behavior (counterPick 0, no scout, apm 1).
- Effort: **XL**
- dependsOn: Iter 0 (brain delegates from `systems/ai_brain.ts`), Iter 2 (multi-resource targeting), Iter 3 (Expand to second tree stand), Iter 5 (counter-comp needs the roster), ideally Iter 6 (garrison defense)
- WHY HERE: A counter-picking, expanding, scouting, retreating AI is only meaningful once there's a roster to counter-pick (Iter 5), resources to manage (Iter 2), and map to expand into (Iter 3). Separating PERCEPTION (reads) / DECISION (pure) / EXECUTION (writes via existing helpers) keeps authority intact and the decision core unit-testable.
- **Most important ferridriver check:** start a Duel vs Hard bot, raid its base with knights → `spacetime sql 'SELECT phase FROM ai'` flips Opening→BuildUp→Defend and spearmen appear in response → screenshot the bot expanding to a second tree stand after chopping its corner; same seed produces identical phase trace twice (determinism).

## ITERATION 8 — Blacksmith research / upgrades

- Slice: NEW `research` table (owner, tech, progress, done). Flesh out `shared/research.ts` (Tech enum, TECH_DEFS with typed additive deltas, `applyTechs(def, owned)` → effective def). `researchTech` reducer + `researchTick` system. CRITICAL determinism rule: bonuses are DERIVED in combat math from owned-tech rows via `applyTechs` — NEVER written onto unit rows (a recomputing replica would diverge). Both combat math (module) and tooltips (client) call `applyTechs` so numbers never differ. AI (Iter 7) gets a `researchPriority` and calls `researchTech` like a human.
- Effort: **M**
- dependsOn: Iter 0, Iter 5 (Blacksmith building + the def fields techs modify), Iter 2 (research costs)
- WHY HERE: Self-contained vertical that deepens combat without touching subscriptions or terrain. Fits after the roster exists and before fog (the highest-risk, last mechanic).
- **Most important ferridriver check:** build a Blacksmith → research ArmorMail → confirm both already-trained AND newly-trained units take less damage (derivation auto-applies to existing units) → tooltip shows the upgraded stat matching combat outcome.

## ITERATION 9 — Model/Art polish: faction silhouettes + transform animation + GLTF fallback pipeline

- Slice: `shared/art.ts` (DATA: team-color slots, faction variant params, rig keyframe params, optional modelUrl manifest). `game/meshes/types.ts` + `registry.ts` (MeshFactory seam) + `teamColor.ts` (multi-slot retint). Faction plumbing: build `playerFaction` Map in `onPlayer`, pass faction into builders (faction is ALREADY replicated — no module change). Richer faction-distinct procedural meshes per unit/building. `game/anim/Animator.ts` + `animState.ts` (bone-less transform anim driven by already-replicated unit state: gatherState/hasTarget/attackTarget/attackCooldown/carrying → Idle/Walk/Attack/Gather/Carry). `game/models/ModelRegistry.ts` (GLTFLoader+Draco; prefers loaded `.glb`, falls back to procedural so game never blocks on assets). Tree InstancedMesh (keep individually pickable via instanceId→entityId map).
- Effort: **XL**
- dependsOn: Iter 1 (mesh seam is mandatory), Iter 5 (new-kind meshes should land in the same module structure). NO module/binding change — all anim-driving fields + faction already in schema.
- WHY HERE: Pure presentation; lands late because it's additive and depends on the full roster's meshes existing. Faction becomes visible in 3D for the first time (today builders get only a color).
- **Most important ferridriver check:** start a skirmish vs an AI of the OPPOSITE faction → screenshot confirms (a) own vs enemy distinct faction silhouettes, (b) different team colors, (c) two frames showing a moving unit walking and a fighting unit attacking.

## ITERATION 10 — Saveable & resumable matches (in-DB snapshots + reconnect)

- Slice: NEW `saveSlot` table ((owner,name) unique) + typed `save*` mirror tables (private, NOT public). `shared/save.ts` (SAVE_SCHEMA_VERSION, SAVE_TABLE_DESCRIPTORS, ENTITY_REF_COLUMNS, pure serialize/rehydrate + entityId-remap + default-backfill). `saveMatch(name)`/`loadMatch(saveId)`/`deleteSave` reducers (deterministic table read→write; load clears caller's match, re-inserts, remaps autoInc entityIds, rewrites cross-refs targetNode/attackTarget/keepEntity, re-arms timers). Client: `useSaves.ts` (TanStack `useTable` over saveSlot, cache is source of truth), `SaveLoadScreen.tsx` menu phase, HUD "Save & Quit", token persistence in connection builder + manual reconnect, mesh-pool reset on matchId change (loaded rows get fresh entityIds → full scene reset).
- Effort: **XL**
- dependsOn: Iter 4 (matchId scoping is the prerequisite — a save = all rows where matchId=X), Iter 1 (SaveLoadScreen menu + mesh-pool reset), and every schema-bearing iteration before it (save mirrors must cover all live columns — assert live-vs-mirror parity).
- WHY LAST: Saves must mirror the FINAL table shapes (matchId, morale, garrison, carryType, multi-resource). Doing it last means one schema-parity pass instead of re-extending mirrors after every iteration. STDB gives free durability + republish-survival but NO save/load primitive — it's modeled as data.
- **Most important ferridriver check:** start skirmish → build + train → "Save & Quit" → reload page (same token reconnect) OR fresh menu → "Resume saved battle" → screenshot shows saved units/buildings at saved positions with saved resources; `spacetime sql` confirms referential integrity (no dangling old entityIds).

---

# PARALLELIZATION MAP

**Iter 0 and Iter 1 run in parallel from day one** (different repos/dirs: module vs client). Each is internally parallelizable after its own Phase-0 foundation lands:
- *Iter 0 fan-out:* one agent lands `schema/{tables,db,context}.ts` + `world/*` helpers + tsconfig widen (the singleton + Ctx seam), THEN parallel: `reducers/match.ts`, `reducers/unit_commands.ts`, `reducers/build_commands.ts`, `systems/movement.ts`, `systems/gather_ai.ts`, `systems/combat.ts`, `systems/ai_brain.ts` (each only imports `schema/*`+`world/*`, never each other). One agent assembles `index.ts` barrel LAST (sole shared mutable file).
- *Iter 1 fan-out:* one agent lands `game/types.ts`+`meshes/{materials,indicators}.ts`, THEN four independent coordinator lanes (EntityPool+WallGrid / Selection+Commands+Picking / Build+Demolish+Input / Camera+Minimap+SceneRig) + two model lanes (units / buildings). One agent stitches the orchestrator LAST.

**SEQUENTIAL / SERIALIZATION POINTS (must NOT be parallelized):**
- Any **schema column add + `spacetime generate`** is a serializing barrier — it changes generated bindings every client file imports. Iter 2 (player+unit cols), 3 (config), 4 (matchId on 6 tables), 5 (def shape via ResourceCost), 6 (unit cols + garrison table), 7 (ai cols), 8 (research table), 10 (save tables) each have a one-shot "land schema → regen bindings" gate that one agent owns before client work fans out.
- The **`cost:number → ResourceCost` refactor (Iter 2)** is a serializing contract touching trainFrom/placeFor/placeWall/demolish/aiBrain + every cost tooltip — land it (additively) in one pass.
- **`combatTick` (Iter 6)** is edited by BOTH morale and garrison — do morale then garrison, not parallel.
- **`trainFrom`/`placeFor` (Iter 5/8)** is touched by tech-gate AND research — land economy+tech-gate before research.
- **Fog-of-war (folded into Iter 6's mechanics or deferred)** is the single highest-risk subscription rework — keep it serialized/last within its iteration; prototype STDB 2.4 RLS row-filter expressiveness BEFORE committing the client subscription split (fallback: per-player visibility table).
- **`onConfig` (Iter 3)** and **`combatTick` (Iter 3 elevation + Iter 6 mechanics)** are shared client/module hotspots — serialize edits to them.

**MASSIVELY PARALLEL once Iters 0+1+5 land** (one agent per file, zero shared mutable state):
- **One agent per unit mesh** (peasant/spearman/archer/horse-archer/mamluk/crossbowman/knight/ram/mangonel/imam) in `meshes/units.ts` family + its ART_DEFS row + its test row (Iter 9).
- **One agent per building mesh** (keep/barracks/tower/wall/gatehouse/house/stable/blacksmith/market/granary/fishinghut/watchtower/castle) in `meshes/buildings.ts` family (Iter 9).
- **One agent per resource type** node placement + minimap color + gather routing (Iter 2/3).
- **One agent per pure shared module + its vitest** — `economy.ts`, `morale.ts`, `garrison.ts`, `research.ts`, `ai/composition.ts`, `ai/phases.ts`, `ai/threat.ts`, `worldgen.ts`, `elevation.ts`, `terraingen.ts`, `save.ts` are all pure TS with no STDB/Three deps — fully independent test-first lanes.

**Cross-iteration parallel tracks after the matchId gate (Iter 4):** the Iter 10 save vertical splits into SHARED (save.ts+match.ts+tests, pure), MODULE (save/match reducers, scope helpers), CLIENT (useSaves/SaveLoadScreen/reconnect, gated only on regenerated bindings) — three lanes joining at integration. The **reconnect-resume token fix is fully independent** and can land anytime as a quick win.

---

**Critical-path summary:** Iter 0+1 (parallel) → Iter 2 → Iter 3 → Iter 4 → Iter 5 → {6, 7, 8 mostly parallel after 5} → Iter 9 (parallel with 6-8, needs 1+5) → Iter 10 (last, needs 4 + final schemas). Every iteration is a full vertical (shared data + tests → module + regenerated bindings → client → ferridriver verify) and honors the hard rules: all game rules in the deterministic module, client render+input only, all content data-driven in `shared/`, no piling into monoliths.