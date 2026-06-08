# Saladin Performance Guide — SpacetimeDB TS Module at Scale

Grounded in primary sources only. Every claim is tagged **[VERIFIED]** (read in source — installed
SDK `spacetimedb@2.4.1` under `node_modules/`, or cited from the research briefs which read
`clockworklabs/SpacetimeDB` master + official docs) or **[UNVERIFIED]** (inference, or a number that
is configurable / version-dependent / not published). SDK line numbers below were re-confirmed
against the installed `node_modules/spacetimedb/dist/server/index.mjs` (v2.4.1) for this guide.

Saladin's current shape (verified against `spacetimedb/src` on 2026-06-08):

- Six scheduled systems: `moveUnits` 50ms (`systems/movement.ts`), `unitAi` 200ms
  (`systems/gather_ai.ts`), `combatTick` 200ms (`systems/combat.ts`), `aiBrain` 1s
  (`systems/ai_brain.ts`), `economySystem` 2s (`systems/economy.ts`), `researchSystem` 1s
  (`systems/research.ts`). Each is a `scheduled` table → reducer (`schema/tables.ts:381-427`).
- Every hot system opens with `[...ctx.db.unit.iter()]` — a **full table scan** of all units across
  **all matches**, then filters by `matchId` in JS (`movement.ts:16`, `gather_ai.ts:59`,
  `combat.ts:168`, `economy.ts:25`).
- Per-unit `ctx.db.entity.entityId.find(u.entityId)` to get position (`movement.ts:20`,
  `combat.ts:217`, `gather_ai.ts:63`).
- `combat.ts` target acquisition is genuine **O(N²)**: for each unit it calls `enemyUnitsAround`
  / `nearbyAllies` / `nearSupport`, each of which loops over the whole `units` snapshot again and
  does 1–2 `.find()` host calls per inner element (`combat.ts:54-72, 92-134, 247`).
- Per-unit **A\*** runs inside `movePatch` (`world/placement.ts:51-77`), called from `gather_ai`,
  `combat`, and `ai_brain`. Each call rebuilds occupancy (`buildOccupancy` → full `building.iter()`
  + per-building `.find()`, `placement.ts:31-44`) **and** allocates A\* working arrays of
  `WORLD_SIZE² = 144² = 20736` cells (`shared/pathfinding.ts:29 MAX_EXPANSIONS = W*W`, `:227-231`
  two `Float64Array(20736)` + `Int32Array` + `Uint8Array`). `WORLD_SIZE=144` (`shared/constants.ts:4`).

---

## 1. The real execution / limit model — what actually freezes the game

### Reducers are serialized; a slow tick blocks the entire DB.

- **[VERIFIED]** Reducers run inside one Serializable transaction; on success they commit, on any
  throw/panic the whole call rolls back (brief 1: docs/functions/reducers, transactions-atomicity;
  `scheduler.rs` `begin_mut_tx(IsolationLevel::Serializable)`).
- **[VERIFIED]** Execution is serialized through one synchronous "main lane" — reducers run one at a
  time under a global write lock; while a reducer runs, no other reducer writes and no read
  completes (brief 1: `wasmtime/mod.rs` main-lane comment; brief 3: docs/databases/transactions-
  atomicity "transactions executed one after the other", key-architecture global RWMutex). The docs
  **reserve the right** to run reducers concurrently under MVCC later, but that is not current
  behavior (**[VERIFIED]** as a reserved right, not a guarantee).
- **Consequence for Saladin:** a `combatTick` that runs long does not just lag combat — it stalls
  `moveUnits`, client reads, and every reducer for its whole duration. The visible "freeze/glitch at
  scale" is **head-of-line blocking on the single execution lane**, not a crash.

### What actually causes the symptoms, ranked by which is most likely first:

1. **Tick overrun → scheduler drift + stutter (MOST LIKELY for Saladin).**
   - **[VERIFIED]** Ticks never overlap or queue up. A scheduled row is in the queue exactly once;
     it is re-enqueued only *after* the current run's transaction commits (brief 1: `scheduler.rs`
     `handle_queued`). So a slow tick cannot reenter — it just **delays the next one**.
   - **[VERIFIED]** Interval reschedule in current master is **completion-relative**: next run =
     (completion time) + interval, so a tick whose execution time is non-negligible **drifts later
     every cycle** (brief 1: `scheduler.rs:593,619,626`; GitHub issue #2882; PR #3657 fix is
     Closed/linked but the master path still uses post-completion `now`). **[UNVERIFIED]** whether
     the drift fix shipped in your deployed `spacetime` binary — confirm against your installed
     server version.
   - **Net:** when `combatTick` (200ms budget) or `moveUnits` (50ms budget) starts taking longer
     than its interval, you do not get a hard error — you get the achieved tick rate collapsing and
     units visibly stuttering / moving in slow motion. Saladin already has the instrument to measure
     this: `tick_count` is bumped once per tick (`schema/tables.ts:162`, `world/tick_count.ts`),
     designed to be polled twice over a wall-clock window to read the achieved rate.

2. **Per-call fuel budget exhaustion → that tick aborts, DB keeps running.**
   - **[VERIFIED]** Each reducer call gets a fuel budget (`FunctionBudget`, 1:1 with wasmtime fuel /
     ~CPU instructions). Default `DEFAULT_BUDGET = 2e9 fuel/sec × 60 = 120_000_000_000` fuel,
     "roughly 1 minute of runtime" at an assumed 2 GHz abstract machine (brief 1:
     `client-api-messages/src/energy.rs:137-142`).
   - **[VERIFIED]** On exhaustion the WASM instance **traps**; `remaining == 0` → `OutOfEnergy` →
     the transaction is **rolled back** (that tick's writes are discarded), but the database keeps
     serving (brief 1: `module_host_actor.rs:1116`). The installed SDK carries the matching error
     surface (`errors.ts`: `NoSpace:9`, `ScheduleAtDelayTooLong:13`, `WouldBlockTransaction:17` —
     confirmed in `node_modules/spacetimedb/src/server/errors.ts:63,78,95`).
   - For Saladin's TS module this budget is **[UNVERIFIED numerically]** in wall-clock terms — but a
     full O(N²) acquisition pass plus N A\* searches over thousands of units is exactly the kind of
     work that approaches it. Practically you will hit **drift (symptom 1)** long before the 1-minute
     fuel cap; the budget is the hard ceiling, drift is the soft one you feel first.
   - **[VERIFIED]** There is *also* an epoch wall-clock interrupt every 10ms, but it only **logs**
     long-running functions and immediately resumes — it is **not** a kill switch (brief 1:
     `wasmtime_module.rs:354-362`). A reducer over ~16.67ms (one 60fps frame) is logged as
     "Long running reducer" — a debug log, not a limit (brief 1: `module_host_actor.rs:1708`).

3. **Replication cost is a separate axis and grows with row churn, not a freeze.**
   - **[VERIFIED]** Clients receive only rows matching their subscription, and the server cost of a
     subscription is index-driven: a query filtered on a PK/unique-indexed column is a single
     indexed lookup; a non-indexed/overlapping query makes the server process & serialize each
     matching row multiple times (brief 3: docs/subscriptions). This shows up as **bandwidth and
     serialization overhead**, not the reducer-lane freeze — but the hot `entity` table re-serializes
     every changed row every 50ms, so keep it narrow (it already is: x/y/facing/matchId,
     `schema/tables.ts:14-23`).

**Bottom line:** the freeze at scale is **head-of-line blocking on the serial reducer lane**,
surfacing as **interval drift / stutter** when a tick's wall-clock cost exceeds its interval, with
**fuel-exhaustion rollback** as the hard ceiling behind it. It is a *cost-per-tick* problem, and the
cost is dominated by host-ABI call count + A\* allocations, addressed in §2–3.

---

## 2. Which per-tick operations are genuinely expensive (and why, per how STDB actually works)

### `iter()` IS an unconditional full table scan. **[VERIFIED]**

`ctx.db.unit.iter()` compiles to exactly one `datastore_table_scan_bsatn(table_id)` host call with
no host-side filtering (installed `dist/server/index.mjs:7195`). The `matchId`/`active`/`hasTarget`
filters in `movement.ts:16-19`, `gather_ai.ts:59-62`, etc. run **in JS after every row is already
deserialized**. So every hot tick pays to decode *all* units in *all* matches even if only a handful
are eligible. The read itself is batched on the wire (rows pulled in 64 KiB chunks —
`DEFAULT_BUFFER_CAPACITY = 32*1024*2`, installed `index.mjs:7559`), so the scan is not N host calls;
but it **is** O(N) BSATN deserialization of the whole table, every tick, ×6 systems.

### `count()` is the one O(1) freebie. **[VERIFIED]**

`datastore_table_row_count` "reads datastore metadata, runs in constant time" (brief 2;
installed `index.mjs:7205`). Safe to call per tick if you ever need a population number.

### Per-unit `.find()` is one host call + one BSATN deserialize each. **[VERIFIED]**

`ctx.db.entity.entityId.find(id)` is a single `datastore_index_scan_point_bsatn` (PK point scan,
installed `index.mjs:7281,7388`). It is *correct and indexed* — not a scan — but it is **one host-ABI
crossing per call**. The cost model is **(host-call count) + (BSATN bytes moved)** (brief 5). In
`movement.ts` that is 1 find/unit (fine). In `combat.ts` it explodes: `enemyUnitsAround` does
`unit.find` + `entity.find` for **every other unit, for every acting unit** → O(N²) host calls
(`combat.ts:65-69` inside the `for (const o of units)` loop, itself inside the per-unit `for (const
snap of units)` at `:208`). At N=500 that is ~250k×2 host crossings per `combatTick`, every 200ms.

### Insert / update / delete: one host call + one serialize each; NO batch write API. **[VERIFIED]**

`insert` → `datastore_insert_bsatn`, PK `update` → `datastore_update_bsatn`, table `delete(row)` →
`datastore_delete_all_by_eq_bsatn` (installed `index.mjs:7212,7304,7222`; brief 5). There is no
multi-row insert/update. Reads amortize across a scan; **writes do not** — minimize the count of
individual writes per tick. Saladin already does this well: `movement.ts` only updates rows that
moved, `economy.ts:34,41` writes hp/food only when changed.

### A btree `.filter()` DOES support range queries, and it is index-backed (no full scan). **[VERIFIED]**

This is the key enabler. A btree index's `.filter()`:
- with a plain value → `datastore_index_scan_point_bsatn` (point scan),
- with a `Range` object → `datastore_index_scan_range_bsatn` (range scan),
both index-backed (installed `index.mjs:7376-7393`). `Range`/`Bound` are exported from
`spacetimedb/server` (installed `dist/server/index.d.ts:15`) and Saladin does **not** currently
import them (grep confirmed zero uses). A multi-column btree supports prefix-exact + a range on the
**last** scanned column only — `prefix_elems = range.length - 1` (installed `index.mjs:7445`); you
**cannot** range two columns in one scan (brief 2). So:

- **A 1-D range query is cheap and supported today.** E.g. a btree on `(matchId)` lets you
  `ctx.db.unit.matchId.filter(matchId)` to get only one match's units — turning every hot loop's
  full-table scan into a per-match index scan. (`matchId` is *already* btree-indexed on `unit`,
  `entity`, `building`, `resourceNode`, `player`, `ai` — `schema/tables.ts:21,53,79,101,121,154` —
  but the systems never use it; they `iter()` + JS-filter instead.)
- **A 2-D box query (units in an x/y rectangle) is NOT one index op.** A `(matchId,x)` btree can
  range `x` for one match, but you'd still scan a full vertical strip and filter `y` in JS. **There
  is no engine-provided spatial index** (brief 2, brief 4: confirmed absent in both Blackholio and
  the engine). For true neighborhood queries you must build your own grid/cell bucketing (see §3).

### `movePatch` is the single most expensive call in the codebase. **[VERIFIED in Saladin source]**

Each `movePatch` (`placement.ts:51-77`):
1. `buildOccupancy(ctx)` → full `building.iter()` + a `.find()` per building (`placement.ts:31-44`),
2. allocates 4 typed arrays of 20736 cells (`pathfinding.ts:227-231`) — ~330 KB of `Float64Array`
   alone, **zeroed/filled every call**,
3. runs A\* up to 20736 expansions (`pathfinding.ts:248`).

It is called per-unit from `gather_ai` (every retarget/move), `combat` (every approach/return), and
`ai_brain` (army moves). At scale this is the dominant cost — both the GC pressure of the throwaway
arrays and the redundant occupancy rebuild.

---

## 3. Optimization plan, ranked by expected impact, with the specific STDB mechanism each uses

> Each item names the exact mechanism and cites the evidence it relies on. Where the mechanism is a
> manual structure (no engine support exists), that is stated explicitly with the reason.

### Rank 1 — Stop full-scanning hot tables; drive every system off the existing `matchId` btree index. **[mechanism: VERIFIED]**

**What:** replace `[...ctx.db.unit.iter()].filter(u => active.has(u.matchId))` with a per-active-match
index scan: `for (const mid of active) for (const u of ctx.db.unit.matchId.filter(mid)) {...}`.
**Mechanism:** btree `.filter(value)` → `datastore_index_scan_point_bsatn` (installed
`index.mjs:7388`); `unit.matchId` is already `index('btree')` (`schema/tables.ts:53`). Same for
`building.matchId`, `resourceNode.matchId`, `player.matchId`, `ai.matchId`.
**Impact:** the scan now decodes only rows in active matches instead of every row in every match
(including paused/ended/saved matches that currently still get fully deserialized 6× per tick-group).
For a single live match it's a wash on row count but removes cross-match bleed; for a server hosting
many matches it's the difference between O(all rows) and O(this match's rows) per tick.
**Also:** `economy.ts:25` does `[...ctx.db.unit.iter()].filter(u => u.owner.equals(p.identity))`
**inside a per-player loop** — O(players × all units). `unit.owner` is btree-indexed
(`schema/tables.ts:33`); replace with `ctx.db.unit.owner.filter(p.identity)` →
`datastore_index_scan_point_bsatn`, making it O(players × that player's units).

### Rank 2 — Kill the O(N²) target acquisition with a spatial grid (manual — no engine spatial index exists). **[mechanism: VERIFIED absent; structure is manual]**

**Why not a range query:** a btree only ranges one column, so it cannot answer "enemies within
radius R of (x,y)" in one op (brief 2; installed `index.mjs:7445`). **There is no quadtree / spatial
index in SpacetimeDB** — confirmed absent in the engine and in the reference game Blackholio, which
accepts O(N²) only because it hard-caps entities (brief 4: Blackholio `lib.rs:426-455`, ~600
entities, "hundreds of players"). BitCraft, the production game that scales, solves this with a
**manual `chunk_index` column** (brief 4).

**What to do (BitCraft pattern, brief 4 — VERIFIED in BitCraftPublic source):**
- Add an integer `cell` column to `entity` (or `unit`): `cell = floor(y/CELL)*CELLS_PER_ROW +
  floor(x/CELL)`, btree-indexed. Update it in `moveUnits` only when a unit crosses a cell boundary
  (cheap — most ticks it's unchanged, so no extra write).
- Acquisition then scans only the 3×3 block of cells around the acting unit via
  `ctx.db.entity.cell.filter(cellId)` (point scan per cell, ×9), exactly as BitCraft's
  `chunk_index().filter()` over `surrounding_and_including()` (brief 4: `location.rs:66,214`,
  `components.rs:226-262`, `chunk_coordinates.rs:13-45`, 3×3 block).
- **Mechanism:** btree point scan on the cell column (installed `index.mjs:7388`). Turns O(N²) into
  O(N × units-in-9-cells).
**Impact:** highest-leverage change for combat. This is the difference between "hundreds of units"
and "thousands". With `WORLD_SIZE=144` and a CELL of e.g. 8, that's an 18×18 = 324-cell grid; a
3×3 query touches ≤9 point scans regardless of total unit count.

### Rank 3 — Make `movePatch` / A\* cheap: cache occupancy, reuse A\* buffers, throttle/avoid pathing. **[mechanism: VERIFIED in Saladin source + brief 5]**

Three sub-fixes, all addressing the dominant per-call cost (§2):

3a. **Cache occupancy once per tick, not once per `movePatch`.** `buildOccupancy` does a full
   `building.iter()` + per-building `.find()` (`placement.ts:31-44`) on *every* path request. Build
   it once at the top of each system (like `gather_ai.ts:33` already does with `buildOccupancy(ctx)`
   — pass that `occ` down instead of rebuilding inside `movePatch`). `combat.ts` and `ai_brain.ts`
   do **not** pre-build it, so they pay the full building scan per `movePatch` call.
   **Mechanism:** removes redundant `datastore_table_scan_bsatn` + N point scans per path request.

3b. **Reuse the A\* working arrays.** `findPathGrid` allocates 4 typed arrays of 20736 cells every
   call (`pathfinding.ts:227-231`). Hoist them to module-level scratch buffers and `.fill()`/reset
   per call (a reducer instance is single-threaded so a shared scratch is safe; the SDK itself
   reuses `LEAF_BUF`/`BINARY_WRITER` exactly this way — brief 5, installed `index.mjs:7570`). This
   removes ~330 KB+ of per-call allocation and the GC churn that follows. **[UNVERIFIED]** the exact
   GC cost in V8, but the allocation count is verified and reuse is the documented SDK pattern.

3c. **Don't run A\* every tick per unit.** Path is already stored on the unit row (`unit.path`,
   `pathIdx` — `schema/tables.ts:51-52`) and `movement.ts` follows it without re-pathing. The cost
   is in `gather_ai`/`combat`/`ai_brain` calling `movePatch` to *recompute* paths. Gate re-pathing:
   only call `movePatch` when the target actually changed (combat already guards with
   `if (u.hasTarget) {}` skips at `combat.ts:231,340,347` — extend that discipline; never re-path a
   unit that already has a valid path to the same target).

### Rank 4 — Split tick work by frequency and defer writes out of hot loops. **[mechanism: VERIFIED, reference pattern]**

- **[VERIFIED]** The reference pattern is many small scheduled loops at different rates, each gated
  and timestamp-filtered, **not** one fast global tick (brief 4: BitCraft ~18 agents 1s–300s, each
  `should_run`-gated and acting only on rows whose timestamp elapsed; Blackholio 50/500/5000ms).
  Saladin already does this (50/200/200/1000/2000/1000) — keep it; if combat gets expensive,
  consider dropping `combatTick` to a lower rate before optimizing further, since 200ms combat
  resolution is imperceptible to players.
- **[VERIFIED]** Defer mutation out of the hot read loop: Blackholio's movement tick only *reads* and
  *schedules*; the actual eat/delete happens in a separate `consume_entity` reducer scheduled at
  `ScheduleAt::Time(now)` (brief 4: `lib.rs:442-496`), keeping the hot transaction small. Saladin's
  combat mutates inline; if a tick gets heavy, splitting "decide" (read-only acquisition) from
  "apply" (writes) lets each transaction stay short and reduces lane-hold time.

### Rank 5 — Keep the hot `entity` table narrow and index only what you query. **[mechanism: VERIFIED]**

- **[VERIFIED]** Replication re-serializes every changed row of a subscribed table; narrower rows =
  fewer bytes per 50ms update (brief 3: docs/tables/performance). `entity` is already minimal
  (x/y/facing/matchId) — keep position split from intent/HP (it already is).
- **[VERIFIED]** Each extra index adds O(log n) write upkeep per mutation (brief 2). Saladin's
  indexes look justified (matchId, owner used for scoping; PK on entityId). Don't add a `(matchId,x)`
  index speculatively — only add it if you actually adopt 1-D range scans for a specific query.

---

## 4. Realistic entity-count ceiling per database, TS module, 50ms tick

**There is no published max-entity number for any SpacetimeDB module.** **[VERIFIED absence]** The
official benchmarks (279k–304k TPS, p50 ~7ms) are bank-transfer *transactions*, not moving-entity
ticks, and the TS/V8 module actually *beat* the Rust/WASM module on that workload (303,920 vs
265,541 tx/s) — so "TS is too slow" is **not** supported by evidence; the bottleneck is host-call
count + serialization volume per tick, not interpreted TS (brief 3, brief 5:
spacetimedb.com/blog/benchmarking, May 2026). **[UNVERIFIED]** any per-entity ceiling.

**Reasoning to a defensible estimate for *Saladin specifically*:**

- The hard ceiling per tick is the fuel budget ≈ 1 minute of execution = `120e9` fuel
  (**[VERIFIED]**, brief 1). The soft ceiling you hit first is **interval drift**: a 50ms `moveUnits`
  must finish in <50ms wall-clock or it drifts (**[VERIFIED]**, §1). So the real question is "how
  many units can each system process within its interval."
- `moveUnits` is the cheapest hot system: ~2 host calls/unit (1 find + ≤1 update) and no A\*. This
  scales to **thousands** of units within 50ms — it is not the limiter.
- **The limiter is `combatTick` as written: O(N²) host calls.** At N units it does ~N² × 2 `.find()`
  host crossings for acquisition alone (`combat.ts:65-69`). That is the term that explodes. With the
  current code, expect drift to begin in the **low hundreds of combat units** — consistent with
  Blackholio's accepted ceiling of "hundreds" at O(N²) with ~600 entities (brief 4).
- **With Rank 1 + Rank 2 applied** (per-match index scans + 3×3-cell acquisition), combat becomes
  O(N × small constant) and the ceiling rises to the **low-to-mid thousands** of units per match,
  bounded then by A\* cost (Rank 3) and serialization, not by the quadratic blowup.

**Estimate (explicitly inference, [UNVERIFIED] — measure, don't trust):**
- **As-is:** comfortable to ~**a few hundred** simultaneously-fighting units per database before the
  50ms/200ms ticks drift. Movement-only (gathering, no combat) scales higher.
- **After Rank 1–3:** plausibly **1,000–3,000** units per database at the same tick rates.
- **Measure it, don't assume it:** Saladin already has `tick_count` (`schema/tables.ts:162`,
  `world/tick_count.ts`) purpose-built for this — poll `moveTicks`/`combatTicks` twice over a 10s
  window via `spacetime sql` and compute achieved Hz vs target (20Hz move / 5Hz combat). The brief's
  reference games used the built-in `LogStopwatch`/`Span` timers the same way (brief 4). This is the
  only trustworthy ceiling.

---

## 5. Going bigger — architectural options

### 5a. One database per match (do this first — it's nearly free for an RTS). **[VERIFIED pattern]**

- **[VERIFIED]** A single SpacetimeDB database must fit in RAM on **one machine**; there is **no
  built-in horizontal scaling/sharding of one database** (brief 3: docs/intro/faq "all data in
  memory… limited by host RAM"; BitCraft writeup).
- **[VERIFIED]** The official answer to "too big for one DB" is **many databases + an external
  orchestrator**, one DB per room/match/region (brief 3: docs/intro/faq "create and destroy
  SpacetimeDB databases for each room or match"; brief 4: BitCraft).
- **Saladin fit:** Saladin already models a match as first-class (`match` table, `matchId` on every
  row — `schema/tables.ts:193`, `world/scope.ts`). Today multiple matches share **one** database and
  one shared reducer lane, so match B's combat tick stalls match A. Promoting each match to its **own
  database** (orchestrated externally) gives each match its own serial lane → matches no longer
  contend, and per-DB entity count is naturally capped by the match size. This is the highest-impact
  scaling move and maps cleanly onto the existing matchId scoping. **[UNVERIFIED]** the orchestration
  glue (Saladin currently has none — `index.ts` publishes a single module).

### 5b. Region sharding within a huge match (only if one match outgrows one machine). **[VERIFIED pattern, heavy]**

- **[VERIFIED]** BitCraft partitions a single world across many region modules (`region_count` ≤255,
  `region_count_sqrt²` grid), with entities carrying a `chunk_index`, and migrates entities across
  modules by inserting rows into a shared `inter_module_message_v2` table (not direct calls), plus a
  scheduled `transfer_player` handoff (brief 4: `generic.rs:5-16`, `inter_module/mod.rs:156-208`,
  `transfer_player.rs:66`).
- **Saladin fit:** an RTS skirmish almost certainly fits one machine, so 5b is likely overkill. The
  brief's own guidance: do this only if a single match outgrows RAM; retrofitting cross-module
  handoff is the hard part, so adopt the `chunk_index` column early (it doubles as the Rank-2 spatial
  grid) even if you never split modules.

### 5c. Slower tick / client interpolation. **[VERIFIED levers]**

- **[VERIFIED]** Movement can be **client-sent + server-validated** rather than server-simulated per
  tick — BitCraft never integrates positions in a tick; clients send moves, `validate_move`
  bounds-checks them with a cheating strike counter (brief 4: `move_validation_helpers.rs`). This
  removes per-unit position integration from the server budget entirely. **[UNVERIFIED fit]** for an
  RTS where the server is authoritative over many AI/owned units — Saladin's units are server-driven
  (gather/combat AI), so full client-authority doesn't apply, but **client-side interpolation of the
  50ms position stream** (rendering between server ticks) lets you *lower* the server move rate
  (e.g. 50ms→100ms) without visible stutter, halving the hottest tick's cost. Confirm Saladin's
  client already interpolates before lowering the rate.
- **[VERIFIED]** Lower tick rates are the simplest lever: combat at 200ms is already coarse; dropping
  to 250–300ms, or gating expensive systems behind "only process rows due this tick" (BitCraft's
  timestamp filter, brief 4: `npc_ai_agent.rs:72-77`), trades imperceptible latency for headroom.

---

## Quick reference — the 5 facts that matter most

1. **One serial reducer lane; a slow tick freezes the whole DB.** The symptom at scale is **interval
   drift / stutter** (next run = completion + interval, **[VERIFIED]** brief 1), with **fuel-budget
   rollback** (~1 min, **[VERIFIED]**) as the hard ceiling behind it — not a crash.
2. **`iter()` is a full table scan that decodes every row before your JS filter runs**
   (**[VERIFIED]** installed `index.mjs:7195`). All 6 hot systems do this across all matches. Drive
   them off the **already-present `matchId`/`owner` btree indexes** via `.filter(value)`
   (`index.mjs:7388`) instead. Rank 1.
3. **Combat is O(N²) host calls** (`combat.ts:65-69`); a btree **cannot** do a radius query (only
   1-D ranges, `index.mjs:7445`). Fix with a **manual `chunk_index` grid + 3×3 cell point scans**,
   the BitCraft pattern (**[VERIFIED]** brief 4). Rank 2 — biggest scaling unlock.
4. **`movePatch`/A\* is the costliest call** — rebuilds occupancy and allocates 4×20736-cell arrays
   every call (`placement.ts:31`, `pathfinding.ts:227`). Cache occupancy per tick, reuse A\* buffers
   (SDK does exactly this — `index.mjs:7570`), and don't re-path units that already have a path.
   Rank 3.
5. **Scale out by running one database per match** (**[VERIFIED]** official pattern, brief 3/4) — it
   fits Saladin's existing matchId model and gives each match its own reducer lane. No published
   per-DB entity ceiling exists (**[VERIFIED absence]**) — **measure with the existing `tick_count`
   table**; estimate a few hundred fighting units as-is, ~1–3k after Rank 1–3 (**[UNVERIFIED]**).

---

### Sources

- Installed SDK read for this guide: `node_modules/spacetimedb@2.4.1` —
  `dist/server/index.mjs` (lines 7195, 7205, 7212, 7222, 7281, 7304, 7376-7393, 7388, 7445, 7559,
  7570), `dist/server/index.d.ts:15` (`Range`/`Bound` export), `src/server/errors.ts:63,78,95`.
- Saladin source read for this guide: `spacetimedb/src/schema/tables.ts`, `systems/{movement,
  gather_ai,combat,economy,research,ai_brain}.ts`, `world/{placement,util,scope}.ts`,
  `shared/{constants,pathfinding}.ts`.
- Research briefs (each itself citing `clockworklabs/SpacetimeDB` master + docs.spacetimedb.com +
  Blackholio/BitCraft source): execution/limits (brief 1), tables/indexes/query cost (brief 2),
  scaling many entities (brief 3), reference games Blackholio + BitCraft (brief 4), TS module runtime
  internals (brief 5). Key external URLs surfaced by the briefs:
  spacetimedb.com/docs/functions/reducers, /docs/databases/transactions-atomicity,
  /docs/tables/{indexes,performance}, /docs/functions/views, /docs/subscriptions, /docs/intro/faq,
  /blog/benchmarking; github.com/clockworklabs/SpacetimeDB issue #2882;
  clockwork-labs.medium.com/spacetimedb-and-bitcraft.
