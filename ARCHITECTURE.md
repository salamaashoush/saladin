# Saladin — Architecture

A real-time strategy game (Stronghold-Crusader-like) from the perspective of the
Muslim world and Salah ad-Din's campaigns. Built on **SpacetimeDB** (authoritative
TS simulation) + **Three.js** (render/input). Data-driven, scalable, multiplayer
from day one.

## Why SpacetimeDB

SpacetimeDB is a relational database that *is* the game server: simulation logic
runs inside the database as WebAssembly modules, clients subscribe to tables and
call reducers. This is purpose-built for real-time multiplayer — it removes the
separate game-server tier and gives us server authority, deterministic ticks, and
state replication for free.

We model the game the way the canonical SpacetimeDB game (Blackholio) does:
**split data across tables by write-frequency and sharing, not one row per object.**

## Layers

```
shared/                  Pure TS. Single source of truth for both sides.
  constants.ts           world size, tick rates, costs, ranges
  enums.ts               UnitKind, BuildingKind, ResourceType, GatherState, Faction
  defs.ts                data-driven unit/building/resource stats + colors + spawns

spacetimedb/src/index.ts AUTHORITY. Tables + reducers + scheduled systems.
                         Runs as a WASM module inside SpacetimeDB. Deterministic.

src/                     Three.js CLIENT. Render + input only. No game rules here.
  game/SaladinGame.ts    iso ortho scene, mesh pool, interpolation, raycast input
  App.tsx                connection wiring, subscriptions, HUD
  module_bindings/       AUTO-GENERATED from the module (do not hand-edit)
```

## Data model (tables = ECS component stores)

Split by access pattern, exactly like Blackholio's `Entity` / `Circle` / `Player`:

| Table           | Role                                   | Write rate        |
| --------------- | -------------------------------------- | ----------------- |
| `entity`        | position (`x,y,facing`), global id     | hot — every move tick |
| `unit`          | owner + kind + movement intent + gather state | on command / arrival |
| `building`      | static structures (Keep)               | rare              |
| `resource_node` | trees (`remaining`); position in `entity` | on harvest        |
| `player`        | identity, stockpile, faction, color    | on deposit/join   |
| `config`        | singleton world settings               | once              |
| `move_timer`    | scheduled → `moveUnits` (50ms)         | —                 |
| `ai_timer`      | scheduled → `unitAi` (200ms)           | —                 |

`entity.entityId` is a single global autoInc id; `unit` / `building` /
`resource_node` are 1:1 extensions keyed by it. This keeps the per-tick hot path
(positions) in one small table that clients subscribe to for smooth interpolation.

## Game loop (scheduled reducers)

SpacetimeDB schedules reducers by inserting a row into a `scheduled` table. We run
two systems at different rates — smooth movement, cheaper AI:

- **`moveUnits` @ 50ms** — integrate position toward `target`, clamp to bounds,
  set `facing`, clear `hasTarget` on arrival.
- **`unitAi` @ 200ms** — gather state machine: `ToResource → Harvesting →
  ToStockpile → (repeat)`. Sets movement targets, depletes nodes, deposits wood.

Both are deterministic: time via `ctx.timestamp`, randomness via `ctx.random`.

## Command reducers (player intents)

`enterGame(name)` · `moveUnit(entityId,x,y)` · `gatherResource(entityId,nodeId)` ·
`trainPeasant()`. Every reducer authorizes via `ctx.sender` — clients cannot move
units they don't own. This is server authority: the client only *requests*.

## Client data flow

```
input (click) ──► conn.reducers.moveUnit(...)         (request)
                         │
                  module tick mutates `entity`         (authority)
                         │
       table subscription onUpdate ──► SaladinGame      (replication)
                         │
            interpolate over 50ms ──► mesh position     (presentation)
```

The client holds NO authoritative state. It mirrors tables into a mesh pool keyed
by `entityId` and lerps between position snapshots. Reconns/late-joiners get full
state from the subscription's initial sync.

## Running it

```bash
# 1. local SpacetimeDB server (or use maincloud — see .env.local)
spacetime start                       # in one terminal

# 2. publish the authoritative module
spacetime publish saladin --server local --module-path spacetimedb -y

# 3. (re)generate client bindings whenever the module schema changes
bun run spacetime:generate

# 4. run the client
bun run dev
```

Open two browser tabs → two players, two keeps, shared trees, live sync.

Switch `.env.local` (`VITE_SPACETIMEDB_HOST`) between `ws://localhost:3000` and
`wss://maincloud.spacetimedb.com` to target local vs cloud.

## Roadmap (layers on top of this slice)

- **Combat**: `health` component, `attack` reducer, soldier units, target acquisition.
- **More economy**: stone/food/gold nodes, granary/stockpile buildings, population cap.
- **Buildings & production**: barracks (train soldiers), build placement reducer, construction progress.
- **Pathfinding**: grid A* in the module (currently straight-line).
- **Campaign**: scripted scenarios — Hattin, Jerusalem, Acre — characters as named heroes.
- **Factions**: Ayyubid vs Crusader asymmetry driven by `defs.ts`.
- **Fog of war**: per-player subscription filters (SpacetimeDB row-level visibility).
```
