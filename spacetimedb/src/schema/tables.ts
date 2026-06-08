// Tables are organized by write-frequency + sharing, not one row per game object:
//
//   entity        position (hot, written every move tick) — global entity_id
//   unit          ownership + movement intent + gather state (like Circle)
//   building      static structures (Keep)
//   resource_node trees (like Food) — position lives in `entity`
//   player        identity / session / stockpile
//   config        singleton world settings
//   move_timer    scheduled -> moveUnits  (50ms)  integrate positions
//   ai_timer      scheduled -> unitAi     (200ms) gather state machine
import { t, table } from 'spacetimedb/server';
import { scheduleRefs } from './schedule_refs.ts';

export const entity = table(
  { name: 'entity', public: true },
  {
    entityId: t.u64().primaryKey().autoInc(),
    x: t.f32(),
    y: t.f32(),
    facing: t.f32(),
  }
);

export const PathPoint = t.object('PathPoint', { x: t.f32(), y: t.f32() });

export const unit = table(
  { name: 'unit', public: true },
  {
    entityId: t.u64().primaryKey(),
    owner: t.identity().index('btree'),
    kind: t.u8(),
    targetX: t.f32(),
    targetY: t.f32(),
    hasTarget: t.bool(),
    speed: t.f32(),
    gatherState: t.u8(),
    targetNode: t.u64(),
    carrying: t.u32(),
    carryType: t.u8(),
    harvestTimer: t.f32(),
    hp: t.u32(),
    attackTarget: t.u64(),
    attackCooldown: t.f32(),
    stance: t.u8(),
    morale: t.f32(), // 0..1 fighting spirit; below ROUT the unit flees home
    routing: t.bool(), // latched rout state — hysteresis: set <ROUT, cleared >RALLY
    homeX: t.f32(), // posted position — Defensive units leash to it
    homeY: t.f32(),
    garrisonedIn: t.u64(), // host building entityId while sheltered (0 = in the field)
    path: t.array(PathPoint),
    pathIdx: t.u32(),
  }
);

// One row per sheltered unit. A garrisoned unit leaves the field loops (movement,
// combat, target acquisition) and, if ranged, lends fire to its host structure.
export const garrison = table(
  { name: 'garrison', public: true },
  {
    slotId: t.u64().primaryKey().autoInc(),
    building: t.u64().index('btree'), // host structure entityId
    unit: t.u64().unique(), // sheltered unit entityId (one slot per unit)
    owner: t.identity(),
  }
);

export const building = table(
  { name: 'building', public: true },
  {
    entityId: t.u64().primaryKey(),
    owner: t.identity().index('btree'),
    kind: t.u8(),
    hp: t.u32(),
    cooldown: t.f32(),
    rallyX: t.f32(),
    rallyY: t.f32(),
  }
);

// Broadcast-only: a tower firing. Clients animate an arrow; not stored.
export const shot = table(
  { name: 'shot', public: true, event: true },
  {
    fromX: t.f32(),
    fromY: t.f32(),
    toX: t.f32(),
    toY: t.f32(),
  }
);

export const resourceNode = table(
  { name: 'resource_node', public: true },
  {
    entityId: t.u64().primaryKey(),
    resType: t.u8(),
    remaining: t.u32(),
  }
);

export const player = table(
  { name: 'player', public: true },
  {
    identity: t.identity().primaryKey(),
    playerId: t.u32().unique().autoInc(),
    name: t.string(),
    faction: t.u8(),
    wood: t.u32(),
    stone: t.u32(),
    food: t.u32(),
    gold: t.u32(),
    color: t.u8(),
    online: t.bool(),
    keepEntity: t.u64(),
    defeated: t.bool(),
    slot: t.u8(), // stable spawn-corner slot (0..MAX_PLAYERS-1)
    techMask: t.u64(), // completed Blacksmith techs as a bitset — combat reads one number
  }
);

// One row per (owner, tech) being researched or completed. The Blacksmith starts
// it; researchSystem advances `progress` each tick and, on completion, flips the
// owner's player.techMask bit and sets `done`. `progress` is the fraction 0..1 so
// the HUD can draw a bar without knowing the per-tech time.
export const research = table(
  { name: 'research', public: true },
  {
    researchId: t.u64().primaryKey().autoInc(),
    owner: t.identity().index('btree'),
    tech: t.u8(),
    progress: t.f32(),
    done: t.bool(),
  }
);

// Skirmish opponents. One row per AI player; the brain reducer iterates these.
// `host` is the human whose match this bot belongs to — teardown is scoped to it
// so one player's reset never touches another's opponents.
export const ai = table(
  { name: 'ai', public: true },
  {
    identity: t.identity().primaryKey(),
    host: t.identity().index('btree'),
    difficulty: t.u8(),
    decisionCd: t.f32(),
    waveTimer: t.f32(),
    phase: t.u8(), // current AiPhase (shared/ai.ts) — drives cadence + telemetry
    scoutId: t.u64(), // entityId of the unit sent to scout (0 = none out)
    threatTimer: t.f32(), // seconds the bot has been under threat near home
  }
);

export const config = table(
  { name: 'config', public: true },
  {
    id: t.u32().primaryKey(),
    worldSize: t.u32(),
    seed: t.u32(),
    preset: t.string(), // map preset id — render flavor; client reads it back
    initialized: t.bool(),
    nextBotId: t.u64(), // monotonic source of unique bot identities
  }
);

export const moveTimer = table(
  { name: 'move_timer', scheduled: (): any => scheduleRefs.moveUnits },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

export const aiTimer = table(
  { name: 'ai_timer', scheduled: (): any => scheduleRefs.unitAi },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

export const combatTimer = table(
  { name: 'combat_timer', scheduled: (): any => scheduleRefs.combatTick },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

export const aiBrainTimer = table(
  { name: 'ai_brain_timer', scheduled: (): any => scheduleRefs.aiBrain },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

export const economyTimer = table(
  { name: 'economy_timer', scheduled: (): any => scheduleRefs.economySystem },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

export const researchTimer = table(
  { name: 'research_timer', scheduled: (): any => scheduleRefs.researchSystem },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

export const WallTile = t.object('WallTile', { x: t.f32(), y: t.f32() });
