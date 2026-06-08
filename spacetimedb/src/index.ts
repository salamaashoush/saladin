// ─────────────────────────────────────────────────────────────────────────────
// Saladin — authoritative RTS simulation (SpacetimeDB TS module)
//
// Architecture mirrors the canonical SpacetimeDB game (Blackholio): data is split
// across tables by write-frequency + sharing, not one row per game object.
//
//   entity        position (hot, written every move tick) — global entity_id
//   unit          ownership + movement intent + gather state (like Circle)
//   building      static structures (Keep)
//   resource_node trees (like Food) — position lives in `entity`
//   player        identity / session / stockpile
//   config        singleton world settings
//   move_timer    scheduled -> moveUnits  (50ms)  integrate positions
//   ai_timer      scheduled -> unitAi     (200ms) gather state machine
//
// Determinism comes from ctx.random / ctx.timestamp. Reducers are transactional.
// ─────────────────────────────────────────────────────────────────────────────
import { schema, t, table, SenderError } from 'spacetimedb/server';
import { ScheduleAt, Identity } from 'spacetimedb';
import {
  WORLD_SIZE,
  MOVE_TICK_MS,
  AI_TICK_MS,
  COMBAT_TICK_MS,
  AI_BRAIN_TICK_MS,
  MOVE_DT,
  AI_DT,
  COMBAT_DT,
  AI_BRAIN_DT,
  ARRIVE_EPS,
  HARVEST_RANGE,
  DEPOSIT_RANGE,
  HARVEST_TIME,
  TREE_COUNT,
  TREE_WOOD,
  START_PEASANTS,
  START_WOOD,
  PEASANT_COST,
  SPAWN_CLUSTER,
  MAX_PLAYERS,
} from '../../shared/constants.ts';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
  PLAYER_COLORS,
  AI_PROFILES,
  MAX_AI_OPPONENTS,
  aiName,
  enemyFaction,
  spawnCorner,
  allocSlot,
} from '../../shared/defs.ts';
import {
  stepToward,
  nearestIndex,
  applyDamage,
  nearestWithin,
  type Located,
} from '../../shared/sim.ts';
import {
  effectiveDamage,
  combatAction,
  DEFENSIVE_LEASH,
} from '../../shared/combat.ts';
import { sampleTerrain, treeDensity } from '../../shared/terrain.ts';
import {
  isPassable,
  nearestPassableGrid,
  findPathGrid,
  type Passable,
} from '../../shared/pathfinding.ts';
import {
  footprintCenter,
  canPlace,
  findBuildableNear,
  occupancySet,
  type Occupant,
} from '../../shared/buildings.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
  GatherState,
  Faction,
  Stance,
  type UnitKind as UnitKindT,
  type BuildingKind as BuildingKindT,
} from '../../shared/enums.ts';

// ── Tables ───────────────────────────────────────────────────────────────────

const entity = table(
  { name: 'entity', public: true },
  {
    entityId: t.u64().primaryKey().autoInc(),
    x: t.f32(),
    y: t.f32(),
    facing: t.f32(),
  }
);

const PathPoint = t.object('PathPoint', { x: t.f32(), y: t.f32() });

const unit = table(
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
    harvestTimer: t.f32(),
    hp: t.u32(),
    attackTarget: t.u64(),
    attackCooldown: t.f32(),
    stance: t.u8(),
    homeX: t.f32(), // posted position — Defensive units leash to it
    homeY: t.f32(),
    path: t.array(PathPoint),
    pathIdx: t.u32(),
  }
);

const building = table(
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
const shot = table(
  { name: 'shot', public: true, event: true },
  {
    fromX: t.f32(),
    fromY: t.f32(),
    toX: t.f32(),
    toY: t.f32(),
  }
);

const resourceNode = table(
  { name: 'resource_node', public: true },
  {
    entityId: t.u64().primaryKey(),
    resType: t.u8(),
    remaining: t.u32(),
  }
);

const player = table(
  { name: 'player', public: true },
  {
    identity: t.identity().primaryKey(),
    playerId: t.u32().unique().autoInc(),
    name: t.string(),
    faction: t.u8(),
    wood: t.u32(),
    color: t.u8(),
    online: t.bool(),
    keepEntity: t.u64(),
    defeated: t.bool(),
    slot: t.u8(), // stable spawn-corner slot (0..MAX_PLAYERS-1)
  }
);

// Skirmish opponents. One row per AI player; the brain reducer iterates these.
// `host` is the human whose match this bot belongs to — teardown is scoped to it
// so one player's reset never touches another's opponents.
const ai = table(
  { name: 'ai', public: true },
  {
    identity: t.identity().primaryKey(),
    host: t.identity().index('btree'),
    difficulty: t.u8(),
    decisionCd: t.f32(),
    waveTimer: t.f32(),
  }
);

const config = table(
  { name: 'config', public: true },
  {
    id: t.u32().primaryKey(),
    worldSize: t.u32(),
    seed: t.u32(),
    initialized: t.bool(),
    nextBotId: t.u64(), // monotonic source of unique bot identities
  }
);

const moveTimer = table(
  { name: 'move_timer', scheduled: (): any => moveUnits },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

const aiTimer = table(
  { name: 'ai_timer', scheduled: (): any => unitAi },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

const combatTimer = table(
  { name: 'combat_timer', scheduled: (): any => combatTick },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

const aiBrainTimer = table(
  { name: 'ai_brain_timer', scheduled: (): any => aiBrain },
  {
    scheduledId: t.u64().primaryKey().autoInc(),
    scheduledAt: t.scheduleAt(),
  }
);

const spacetimedb = schema({
  entity,
  unit,
  building,
  resourceNode,
  player,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
});
export default spacetimedb;

// ── Helpers ──────────────────────────────────────────────────────────────────

function dist(ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  return Math.sqrt(dx * dx + dy * dy);
}

function clampWorld(v: number): number {
  return Math.max(0, Math.min(WORLD_SIZE, v));
}

// ctx is typed inside reducers; helpers take `any` to avoid threading the schema
// generic everywhere. They only touch typed table rows.
function spawnUnitEntity(
  ctx: any,
  owner: any,
  kind: number,
  x: number,
  y: number
): bigint {
  const def = UNIT_DEFS[kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
  const e = ctx.db.entity.insert({ entityId: 0n, x, y, facing: 0 });
  ctx.db.unit.insert({
    entityId: e.entityId,
    owner,
    kind,
    targetX: x,
    targetY: y,
    hasTarget: false,
    speed: def.speed,
    gatherState: GatherState.Idle,
    targetNode: 0n,
    carrying: 0,
    harvestTimer: 0,
    hp: def.maxHp,
    attackTarget: 0n,
    attackCooldown: 0,
    stance: Stance.Aggressive,
    homeX: x,
    homeY: y,
    path: [],
    pathIdx: 0,
  });
  return e.entityId;
}

function spawnBuilding(
  ctx: any,
  owner: any,
  kind: number,
  x: number,
  y: number
): bigint {
  const def = BUILDING_DEFS[kind as BuildingKindT] ?? BUILDING_DEFS[BuildingKind.Keep];
  const e = ctx.db.entity.insert({ entityId: 0n, x, y, facing: 0 });
  ctx.db.building.insert({
    entityId: e.entityId,
    owner,
    kind,
    hp: def.maxHp,
    cooldown: 0,
    rallyX: x,
    rallyY: y,
  });
  return e.entityId;
}

function spawnTree(ctx: any, x: number, y: number): void {
  const e = ctx.db.entity.insert({ entityId: 0n, x, y, facing: 0 });
  ctx.db.resourceNode.insert({
    entityId: e.entityId,
    resType: ResourceType.Wood,
    remaining: TREE_WOOD,
  });
}

// Rejection-sample `count` trees across the map: dense in forest, sparse in
// grass/steppe, none on sand/desert/water. Shared by init and match reset.
function scatterTrees(ctx: any, seed: number, count: number): void {
  let placed = 0;
  let attempts = 0;
  while (placed < count && attempts < count * 60) {
    attempts++;
    const x = 3 + ctx.random() * (WORLD_SIZE - 6);
    const y = 3 + ctx.random() * (WORLD_SIZE - 6);
    if (ctx.random() < treeDensity(sampleTerrain(seed, x, y).biome)) {
      spawnTree(ctx, x, y);
      placed++;
    }
  }
}

interface NodePos {
  id: bigint;
  x: number;
  y: number;
}

function getSeed(ctx: any): number {
  const cfg = ctx.db.config.id.find(0);
  return cfg ? cfg.seed : 1;
}

function popInfo(ctx: any, owner: any): { pop: number; cap: number } {
  let pop = 0;
  for (const u of [...ctx.db.unit.iter()]) if (u.owner.equals(owner)) pop++;
  let cap = 0;
  for (const b of [...ctx.db.building.iter()])
    if (b.owner.equals(owner))
      cap += (
        BUILDING_DEFS[b.kind as BuildingKindT] ?? BUILDING_DEFS[BuildingKind.Keep]
      ).pop;
  return { pop, cap };
}

function hasBarracks(ctx: any, owner: any): boolean {
  for (const b of [...ctx.db.building.iter()])
    if (b.owner.equals(owner) && b.kind === BuildingKind.Barracks) return true;
  return false;
}

// Resolve every building to its world position for the shared occupancy builder.
function buildingOccupants(ctx: any): Occupant[] {
  const items: Occupant[] = [];
  for (const b of [...ctx.db.building.iter()]) {
    const e = ctx.db.entity.entityId.find(b.entityId);
    if (e) items.push({ kind: b.kind as BuildingKindT, x: e.x, y: e.y });
  }
  return items;
}

// Tiles blocked for PATHING — passable buildings (gatehouse) excluded so units
// walk through them.
function buildOccupancy(ctx: any): Set<number> {
  return occupancySet(buildingOccupants(ctx), false);
}

// Tiles blocked for PLACEMENT — every footprint incl. passable (no stacking).
function allBuildingTiles(ctx: any): Set<number> {
  return occupancySet(buildingOccupants(ctx), true);
}

function passableWith(seed: number, occ: Set<number>): Passable {
  return (px, py) => isPassable(seed, px, py) && !occ.has(py * WORLD_SIZE + px);
}

function movePatch(
  ctx: any,
  ex: number,
  ey: number,
  tx: number,
  ty: number
): any {
  const seed = getSeed(ctx);
  const passable = passableWith(seed, buildOccupancy(ctx));
  const snap = nearestPassableGrid(passable, tx, ty);
  const path = findPathGrid(passable, ex, ey, snap.x, snap.y);
  if (path.length === 0) return { hasTarget: false, path: [], pathIdx: 0 };
  return {
    path,
    pathIdx: 0,
    targetX: path[0].x,
    targetY: path[0].y,
    hasTarget: true,
  };
}

function buildNodes(ctx: any): NodePos[] {
  const nodes: NodePos[] = [];
  for (const n of [...ctx.db.resourceNode.iter()]) {
    const e = ctx.db.entity.entityId.find(n.entityId);
    if (e) nodes.push({ id: n.entityId, x: e.x, y: e.y });
  }
  return nodes;
}

function assignGather(
  ctx: any,
  unitId: bigint,
  px: number,
  py: number,
  nodes: NodePos[]
): void {
  const idx = nearestIndex(px, py, nodes);
  if (idx < 0) return;
  const u = ctx.db.unit.entityId.find(unitId);
  if (!u) return;
  ctx.db.unit.entityId.update({
    ...u,
    gatherState: GatherState.ToResource,
    targetNode: nodes[idx].id,
  });
}

// Found a new base for `owner`: keep at the next free corner, starting peasants
// already gathering. Shared by enterGame (human) and addAi (skirmish bot) so both
// sides start identically. Returns the keep entity id.
function foundPlayer(
  ctx: any,
  owner: any,
  name: string,
  faction: number
): bigint {
  // Stable slot from the set of slots in use — survives leavers, so two players
  // can never share a corner (overlapping keeps). Caller guards MAX_PLAYERS.
  const used = [...ctx.db.player.iter()].map((p: any) => p.slot);
  const slot = Math.max(0, allocSlot(used, MAX_PLAYERS));
  const seed = getSeed(ctx);
  const corner = spawnCorner(slot);
  const base = findBuildableNear(
    seed,
    corner.x,
    corner.y,
    BUILDING_DEFS[BuildingKind.Keep].footprint
  );
  const keepId = spawnBuilding(ctx, owner, BuildingKind.Keep, base.x, base.y);

  ctx.db.player.insert({
    identity: owner,
    playerId: 0,
    name,
    faction,
    wood: START_WOOD,
    color: slot % PLAYER_COLORS.length,
    online: true,
    keepEntity: keepId,
    defeated: false,
    slot,
  });

  const nodes = buildNodes(ctx);
  for (let i = 0; i < START_PEASANTS; i++) {
    const a = (i / START_PEASANTS) * Math.PI * 2;
    const px = clampWorld(base.x + Math.cos(a) * SPAWN_CLUSTER);
    const py = clampWorld(base.y + Math.sin(a) * SPAWN_CLUSTER);
    const id = spawnUnitEntity(ctx, owner, UnitKind.Peasant, px, py);
    assignGather(ctx, id, px, py, nodes);
  }
  return keepId;
}

// Remove everything belonging to `owner`: units, buildings, their entity rows,
// the ai row (if a bot) and the player row. Used to tear down a match cleanly.
function clearOwner(ctx: any, owner: any): void {
  for (const u of [...ctx.db.unit.iter()])
    if (u.owner.equals(owner)) {
      ctx.db.unit.entityId.delete(u.entityId);
      ctx.db.entity.entityId.delete(u.entityId);
    }
  for (const b of [...ctx.db.building.iter()])
    if (b.owner.equals(owner)) {
      ctx.db.building.entityId.delete(b.entityId);
      ctx.db.entity.entityId.delete(b.entityId);
    }
  if (ctx.db.ai.identity.find(owner)) ctx.db.ai.identity.delete(owner);
  if (ctx.db.player.identity.find(owner)) ctx.db.player.identity.delete(owner);
}

// True if a human other than `caller` is in the world (bots have an ai row).
function otherHumansPresent(ctx: any, caller: any): boolean {
  for (const p of [...ctx.db.player.iter()])
    if (!p.identity.equals(caller) && !ctx.db.ai.identity.find(p.identity))
      return true;
  return false;
}

// Tear down the caller's match: the caller plus only the bots they host — never
// another human's opponents. Refresh the forest only when the caller is alone,
// so a reset can't wipe resources out from under another human's match.
function resetMatch(ctx: any, caller: any): void {
  const alone = !otherHumansPresent(ctx, caller);
  clearMatch(ctx, caller);
  if (alone) {
    for (const n of [...ctx.db.resourceNode.iter()]) {
      ctx.db.resourceNode.entityId.delete(n.entityId);
      ctx.db.entity.entityId.delete(n.entityId);
    }
    scatterTrees(ctx, getSeed(ctx), TREE_COUNT);
  }
}

// The caller and the bots they host.
function clearMatch(ctx: any, caller: any): void {
  clearOwner(ctx, caller);
  for (const bot of [...ctx.db.ai.iter()])
    if (bot.host.equals(caller)) clearOwner(ctx, bot.identity);
}

// Spawn one bot for `host`'s match. Identity comes from a persistent monotonic
// counter so it is globally unique and never reused across resets.
function spawnAi(
  ctx: any,
  host: any,
  difficulty: number,
  faction: Faction
): void {
  const diff = AI_PROFILES[difficulty] ? difficulty : 1;
  const cfg = ctx.db.config.id.find(0);
  const n = cfg ? cfg.nextBotId : 1n;
  if (cfg) ctx.db.config.id.update({ ...cfg, nextBotId: n + 1n });
  const botId = new Identity((1n << 255n) | n);
  foundPlayer(ctx, botId, aiName(faction, Number(n)), faction);
  ctx.db.ai.insert({
    identity: botId,
    host,
    difficulty: diff,
    decisionCd: 0,
    waveTimer: AI_PROFILES[diff].firstWaveDelay,
  });
}

// Owner-parameterized command logic. The player-facing reducers authorize via
// ctx.sender then delegate here; the AI brain calls these directly with the bot
// identity. Each returns null on success or an error string. ctx.sender cannot
// be spoofed, so authority lives in the reducers — never here.
function trainFrom(ctx: any, owner: any, b: any, kind: number): string | null {
  const bdef = BUILDING_DEFS[b.kind as BuildingKindT];
  if (!bdef || !bdef.trains.includes(kind))
    return 'this building cannot train that';
  const udef = UNIT_DEFS[kind as UnitKindT];
  if (!udef) return 'unknown unit';
  const p = ctx.db.player.identity.find(owner);
  if (!p) return 'not in game';
  if (p.wood < udef.cost) return 'not enough wood';
  const pop = popInfo(ctx, owner);
  if (pop.pop >= pop.cap) return 'population full — build houses';

  const be = ctx.db.entity.entityId.find(b.entityId);
  const bx = be ? be.x : WORLD_SIZE / 2;
  const by = be ? be.y : WORLD_SIZE / 2;
  ctx.db.player.identity.update({ ...p, wood: p.wood - udef.cost });
  // Snap the jittered spawn onto passable, unoccupied ground so a building hemmed
  // in by water/walls never strands its trained units on an impassable tile.
  const rawX = clampWorld(bx + (ctx.random() - 0.5) * 2);
  const rawY = clampWorld(by + bdef.footprint / 2 + 0.8 + ctx.random());
  const seed = getSeed(ctx);
  const snap = nearestPassableGrid(
    passableWith(seed, buildOccupancy(ctx)),
    rawX,
    rawY
  );
  const id = spawnUnitEntity(ctx, owner, kind, snap.x, snap.y);
  if (Math.hypot(b.rallyX - bx, b.rallyY - by) > 1.2) {
    const u = ctx.db.unit.entityId.find(id);
    if (u)
      ctx.db.unit.entityId.update({
        ...u,
        ...movePatch(ctx, snap.x, snap.y, b.rallyX, b.rallyY),
      });
  }
  return null;
}

function placeFor(
  ctx: any,
  owner: any,
  kind: number,
  x: number,
  y: number
): string | null {
  const def = BUILDING_DEFS[kind as BuildingKindT];
  if (!def || !def.buildable) return 'cannot build that';
  const p = ctx.db.player.identity.find(owner);
  if (!p) return 'not in game';
  if (p.wood < def.cost) return 'not enough wood';

  const seed = getSeed(ctx);
  const occ = allBuildingTiles(ctx);
  const ok = canPlace(
    kind as BuildingKindT,
    x,
    y,
    (tx, ty) => isPassable(seed, tx, ty),
    (tx, ty) => occ.has(ty * WORLD_SIZE + tx)
  );
  if (!ok) return 'blocked or on water';

  const c = footprintCenter(def.footprint, x, y);
  ctx.db.player.identity.update({ ...p, wood: p.wood - def.cost });
  spawnBuilding(ctx, owner, kind, c.x, c.y);
  return null;
}

// Send every idle gatherer owned by `owner` to its nearest resource node.
function assignIdleGatherers(ctx: any, owner: any): void {
  const nodes = buildNodes(ctx);
  for (const u of [...ctx.db.unit.iter()]) {
    if (!u.owner.equals(owner)) continue;
    if (u.gatherState !== GatherState.Idle) continue;
    const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
    if (def.carry <= 0) continue;
    const e = ctx.db.entity.entityId.find(u.entityId);
    if (!e) continue;
    const idx = nearestIndex(e.x, e.y, nodes);
    if (idx < 0) continue;
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.ToResource,
      targetNode: nodes[idx].id,
    });
  }
}

// Spiral out from (nx,ny) for the nearest tile where `kind` fully fits. Returns
// raw placement coords (placeFor recentres) or null if nothing fits nearby.
function aiFindSpot(
  ctx: any,
  kind: number,
  nx: number,
  ny: number
): { x: number; y: number } | null {
  const seed = getSeed(ctx);
  const occ = allBuildingTiles(ctx);
  const fits = (x: number, y: number) =>
    canPlace(
      kind as BuildingKindT,
      x,
      y,
      (tx, ty) => isPassable(seed, tx, ty),
      (tx, ty) => occ.has(ty * WORLD_SIZE + tx)
    );
  if (fits(nx, ny)) return { x: nx, y: ny };
  for (let r = 2; r < 26; r++)
    for (let a = 0; a < 16; a++) {
      const ang = (a / 16) * Math.PI * 2;
      const x = clampWorld(nx + Math.cos(ang) * r);
      const y = clampWorld(ny + Math.sin(ang) * r);
      if (fits(x, y)) return { x, y };
    }
  return null;
}

// Nearest enemy keep to (x,y); falls back to any enemy building. Drives assaults.
function nearestEnemyKeep(
  ctx: any,
  owner: any,
  x: number,
  y: number
): { id: bigint; x: number; y: number } | null {
  let best: { id: bigint; x: number; y: number } | null = null;
  let bestD = Infinity;
  let fallback: { id: bigint; x: number; y: number } | null = null;
  let fbD = Infinity;
  for (const b of [...ctx.db.building.iter()]) {
    if (b.owner.equals(owner)) continue;
    const e = ctx.db.entity.entityId.find(b.entityId);
    if (!e) continue;
    const d = dist(x, y, e.x, e.y);
    if (d < fbD) {
      fbD = d;
      fallback = { id: b.entityId, x: e.x, y: e.y };
    }
    if (b.kind === BuildingKind.Keep && d < bestD) {
      bestD = d;
      best = { id: b.entityId, x: e.x, y: e.y };
    }
  }
  return best ?? fallback;
}

// A keep just fell — its owner is out. Idempotent.
function markDefeated(ctx: any, owner: any): void {
  const p = ctx.db.player.identity.find(owner);
  if (p && !p.defeated) ctx.db.player.identity.update({ ...p, defeated: true });
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

export const init = spacetimedb.init((ctx) => {
  const seed = ctx.random.integerInRange(1, 2_000_000_000);
  ctx.db.config.insert({
    id: 0,
    worldSize: WORLD_SIZE,
    seed,
    initialized: true,
    nextBotId: 1n,
  });

  scatterTrees(ctx, seed, TREE_COUNT);

  ctx.db.moveTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(MOVE_TICK_MS) * 1000n),
  });
  ctx.db.aiTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(AI_TICK_MS) * 1000n),
  });
  ctx.db.combatTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(COMBAT_TICK_MS) * 1000n),
  });
  ctx.db.aiBrainTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(AI_BRAIN_TICK_MS) * 1000n),
  });
});

export const onConnect = spacetimedb.clientConnected((ctx) => {
  const p = ctx.db.player.identity.find(ctx.sender);
  if (p) ctx.db.player.identity.update({ ...p, online: true });
});

export const onDisconnect = spacetimedb.clientDisconnected((ctx) => {
  const p = ctx.db.player.identity.find(ctx.sender);
  if (p) ctx.db.player.identity.update({ ...p, online: false });
});

// ── Command reducers (player intents) ─────────────────────────────────────────

// Join the shared world (multiplayer). Founds a base on first call; reconnects
// just flip online. Faction is the player's chosen side.
export const enterGame = spacetimedb.reducer(
  { name: t.string(), faction: t.u8() },
  (ctx, { name, faction }) => {
    const side =
      faction === Faction.Crusader ? Faction.Crusader : Faction.Ayyubid;
    const existing = ctx.db.player.identity.find(ctx.sender);
    if (existing) {
      ctx.db.player.identity.update({
        ...existing,
        online: true,
        name: name || existing.name,
        faction: side,
      });
      return;
    }
    if ([...ctx.db.player.iter()].length >= MAX_PLAYERS)
      throw new SenderError('the world is full');
    foundPlayer(ctx, ctx.sender, name || 'Amir', side);
  }
);

// Begin a fresh single-player skirmish: wipe the caller + all bots, reset the
// map, then found the player and one bot per requested difficulty.
export const startSkirmish = spacetimedb.reducer(
  { name: t.string(), faction: t.u8(), enemies: t.array(t.u8()) },
  (ctx, { name, faction, enemies }) => {
    resetMatch(ctx, ctx.sender);
    const human =
      faction === Faction.Crusader ? Faction.Crusader : Faction.Ayyubid;
    foundPlayer(ctx, ctx.sender, name || 'Amir', human);
    const foe = enemyFaction(human as 0 | 1);
    const count = Math.min(enemies.length, MAX_AI_OPPONENTS);
    for (let i = 0; i < count; i++) spawnAi(ctx, ctx.sender, enemies[i], foe);
  }
);

// Leave the current match — tear down the caller and the bots they host, back to
// a clean slate so the client can return to the menu. Other matches untouched.
export const leaveGame = spacetimedb.reducer((ctx) => {
  clearMatch(ctx, ctx.sender);
});

// Add one bot to the caller's match, on the side opposing the caller.
export const addAi = spacetimedb.reducer(
  { difficulty: t.u8() },
  (ctx, { difficulty }) => {
    const p = ctx.db.player.identity.find(ctx.sender);
    if (!p) throw new SenderError('not in game');
    if ([...ctx.db.player.iter()].length >= MAX_PLAYERS)
      throw new SenderError('match is full');
    spawnAi(ctx, ctx.sender, difficulty, enemyFaction(p.faction as 0 | 1));
  }
);

// Benign stale clicks (unit died, target gone, not yours after a desync) return
// silently — they are races, not user errors, and must not flood the error log.
export const moveUnit = spacetimedb.reducer(
  { entityId: t.u64(), x: t.f32(), y: t.f32() },
  (ctx, { entityId, x, y }) => {
    const u = ctx.db.unit.entityId.find(entityId);
    if (!u || !u.owner.equals(ctx.sender)) return;
    const e = ctx.db.entity.entityId.find(entityId);
    if (!e) return;
    const hx = clampWorld(x);
    const hy = clampWorld(y);
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.Idle,
      targetNode: 0n,
      attackTarget: 0n,
      homeX: hx,
      homeY: hy,
      ...movePatch(ctx, e.x, e.y, hx, hy),
    });
  }
);

export const gatherResource = spacetimedb.reducer(
  { entityId: t.u64(), nodeId: t.u64() },
  (ctx, { entityId, nodeId }) => {
    const u = ctx.db.unit.entityId.find(entityId);
    if (!u || !u.owner.equals(ctx.sender)) return;
    const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
    if (def.carry <= 0) return;
    if (!ctx.db.resourceNode.entityId.find(nodeId)) return;
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.ToResource,
      targetNode: nodeId,
      attackTarget: 0n,
      hasTarget: false,
    });
  }
);

export const trainUnit = spacetimedb.reducer(
  { buildingId: t.u64(), kind: t.u8() },
  (ctx, { buildingId, kind }) => {
    const b = ctx.db.building.entityId.find(buildingId);
    if (!b || !b.owner.equals(ctx.sender)) return;
    const err = trainFrom(ctx, ctx.sender, b, kind);
    if (err) throw new SenderError(err);
  }
);

export const setRally = spacetimedb.reducer(
  { entityId: t.u64(), x: t.f32(), y: t.f32() },
  (ctx, { entityId, x, y }) => {
    const b = ctx.db.building.entityId.find(entityId);
    if (!b || !b.owner.equals(ctx.sender)) return;
    ctx.db.building.entityId.update({
      ...b,
      rallyX: clampWorld(x),
      rallyY: clampWorld(y),
    });
  }
);

// Set combat posture for a batch of the caller's units; posts each unit's home
// at its current position so Defensive units leash to where they were set.
export const setStance = spacetimedb.reducer(
  { entityIds: t.array(t.u64()), stance: t.u8() },
  (ctx, { entityIds, stance }) => {
    const s = stance > Stance.HoldGround ? Stance.Aggressive : stance;
    for (const id of entityIds) {
      const u = ctx.db.unit.entityId.find(id);
      if (!u || !u.owner.equals(ctx.sender)) continue;
      const e = ctx.db.entity.entityId.find(id);
      ctx.db.unit.entityId.update({
        ...u,
        stance: s,
        homeX: e ? e.x : u.homeX,
        homeY: e ? e.y : u.homeY,
      });
    }
  }
);

export const attackUnit = spacetimedb.reducer(
  { entityId: t.u64(), targetId: t.u64() },
  (ctx, { entityId, targetId }) => {
    const u = ctx.db.unit.entityId.find(entityId);
    if (!u || !u.owner.equals(ctx.sender)) return;
    const tu = ctx.db.unit.entityId.find(targetId);
    const tb = tu ? null : ctx.db.building.entityId.find(targetId);
    const target = tu ?? tb;
    if (!target || target.owner.equals(ctx.sender)) return;
    ctx.db.unit.entityId.update({
      ...u,
      attackTarget: targetId,
      gatherState: GatherState.Idle,
      targetNode: 0n,
      hasTarget: false,
    });
  }
);

export const placeBuilding = spacetimedb.reducer(
  { kind: t.u8(), x: t.f32(), y: t.f32() },
  (ctx, { kind, x, y }) => {
    const err = placeFor(ctx, ctx.sender, kind, x, y);
    if (err) throw new SenderError(err);
  }
);

const WallTile = t.object('WallTile', { x: t.f32(), y: t.f32() });

// Batched wall placement for a dragged line: ONE reducer call instead of one per
// tile. Places every affordable, valid Wall tile and skips the rest silently —
// occupancy is computed once and stamped incrementally, so it is O(line), not
// O(line × buildings), and never floods the error log.
export const placeWall = spacetimedb.reducer(
  { tiles: t.array(WallTile) },
  (ctx, { tiles }) => {
    const p = ctx.db.player.identity.find(ctx.sender);
    if (!p) return;
    const def = BUILDING_DEFS[BuildingKind.Wall];
    const seed = getSeed(ctx);
    const occ = allBuildingTiles(ctx);
    let wood = p.wood;
    let placed = false;
    for (const tile of tiles) {
      if (wood < def.cost) break;
      const ok = canPlace(
        BuildingKind.Wall,
        tile.x,
        tile.y,
        (tx, ty) => isPassable(seed, tx, ty),
        (tx, ty) => occ.has(ty * WORLD_SIZE + tx)
      );
      if (!ok) continue;
      const c = footprintCenter(def.footprint, tile.x, tile.y);
      spawnBuilding(ctx, ctx.sender, BuildingKind.Wall, c.x, c.y);
      for (const k of occupancySet([{ kind: BuildingKind.Wall, x: tile.x, y: tile.y }], true))
        occ.add(k);
      wood -= def.cost;
      placed = true;
    }
    if (placed) ctx.db.player.identity.update({ ...p, wood });
  }
);

export const demolishBuilding = spacetimedb.reducer(
  { entityId: t.u64() },
  (ctx, { entityId }) => {
    const b = ctx.db.building.entityId.find(entityId);
    if (!b || !b.owner.equals(ctx.sender)) return;
    if (b.kind === BuildingKind.Keep)
      throw new SenderError('cannot demolish your keep');
    const def = BUILDING_DEFS[b.kind as BuildingKindT];
    const p = ctx.db.player.identity.find(ctx.sender);
    if (p && def)
      ctx.db.player.identity.update({
        ...p,
        wood: p.wood + Math.floor(def.cost / 2),
      });
    ctx.db.building.entityId.delete(entityId);
    ctx.db.entity.entityId.delete(entityId);
  }
);

// Send every idle gatherer owned by the caller to the nearest resource node.
export const autoGather = spacetimedb.reducer((ctx) => {
  assignIdleGatherers(ctx, ctx.sender);
});

// ── Scheduled systems ─────────────────────────────────────────────────────────

// Movement integration — runs every MOVE_TICK_MS. Only touches movers.
export const moveUnits = spacetimedb.reducer(
  { timer: moveTimer.rowType },
  (ctx) => {
    for (const u of [...ctx.db.unit.iter()]) {
      if (!u.hasTarget) continue;
      const e = ctx.db.entity.entityId.find(u.entityId);
      if (!e) continue;

      const r = stepToward(
        e.x,
        e.y,
        u.targetX,
        u.targetY,
        u.speed * MOVE_DT,
        ARRIVE_EPS
      );
      ctx.db.entity.entityId.update({ ...e, x: r.x, y: r.y, facing: r.facing });
      if (!r.arrived) continue;
      const next = u.pathIdx + 1;
      if (next < u.path.length) {
        const wp = u.path[next];
        ctx.db.unit.entityId.update({
          ...u,
          pathIdx: next,
          targetX: wp.x,
          targetY: wp.y,
        });
      } else {
        ctx.db.unit.entityId.update({ ...u, hasTarget: false });
      }
    }
  }
);

// Gather AI state machine — runs every AI_TICK_MS, sets movement targets.
export const unitAi = spacetimedb.reducer({ timer: aiTimer.rowType }, (ctx) => {
  const nodes = buildNodes(ctx);

  // A gatherer whose node is gone heads to the nearest remaining node, and
  // only idles when the whole map is exhausted. Without this, peasants freeze
  // forever the moment their tree is chopped out.
  const retarget = (u: any, e: any) => {
    const idx = nearestIndex(e.x, e.y, nodes);
    if (idx < 0) {
      ctx.db.unit.entityId.update({
        ...u,
        gatherState: GatherState.Idle,
        hasTarget: false,
        targetNode: 0n,
      });
      return;
    }
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.ToResource,
      targetNode: nodes[idx].id,
      hasTarget: false,
    });
  };

  for (const u of [...ctx.db.unit.iter()]) {
    if (u.gatherState === GatherState.Idle) continue;
    const e = ctx.db.entity.entityId.find(u.entityId);
    if (!e) continue;

    if (u.gatherState === GatherState.ToResource) {
      const node = ctx.db.resourceNode.entityId.find(u.targetNode);
      const ne = node ? ctx.db.entity.entityId.find(node.entityId) : null;
      if (!node || !ne) {
        retarget(u, e);
        continue;
      }
      if (dist(e.x, e.y, ne.x, ne.y) <= HARVEST_RANGE) {
        ctx.db.unit.entityId.update({
          ...u,
          gatherState: GatherState.Harvesting,
          harvestTimer: 0,
          hasTarget: false,
        });
      } else if (!u.hasTarget) {
        ctx.db.unit.entityId.update({
          ...u,
          ...movePatch(ctx, e.x, e.y, ne.x, ne.y),
        });
      }
    } else if (u.gatherState === GatherState.Harvesting) {
      const node = ctx.db.resourceNode.entityId.find(u.targetNode);
      if (!node) {
        retarget(u, e);
        continue;
      }
      const timer = u.harvestTimer + AI_DT;
      if (timer < HARVEST_TIME) {
        ctx.db.unit.entityId.update({ ...u, harvestTimer: timer });
        continue;
      }
      const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
      const take = Math.min(def.carry, node.remaining);
      const rem = node.remaining - take;
      if (rem <= 0) {
        ctx.db.resourceNode.entityId.delete(node.entityId);
        ctx.db.entity.entityId.delete(node.entityId);
      } else {
        ctx.db.resourceNode.entityId.update({ ...node, remaining: rem });
      }
      ctx.db.unit.entityId.update({
        ...u,
        carrying: take,
        harvestTimer: 0,
        gatherState: GatherState.ToStockpile,
      });
    } else if (u.gatherState === GatherState.ToStockpile) {
      const p = ctx.db.player.identity.find(u.owner);
      const keep = p ? ctx.db.entity.entityId.find(p.keepEntity) : null;
      if (!p || !keep) {
        ctx.db.unit.entityId.update({
          ...u,
          gatherState: GatherState.Idle,
          hasTarget: false,
        });
        continue;
      }
      // Keep blocks its own footprint, so the peasant stops at the wall edge;
      // accept deposits within the keep's radius too.
      const depositRange =
        DEPOSIT_RANGE + BUILDING_DEFS[BuildingKind.Keep].footprint / 2;
      if (dist(e.x, e.y, keep.x, keep.y) <= depositRange) {
        ctx.db.player.identity.update({ ...p, wood: p.wood + u.carrying });
        const node = ctx.db.resourceNode.entityId.find(u.targetNode);
        if (node) {
          ctx.db.unit.entityId.update({
            ...u,
            carrying: 0,
            hasTarget: false,
            gatherState: GatherState.ToResource,
          });
        } else {
          // Assigned tree is gone — pick the next nearest instead of idling.
          retarget({ ...u, carrying: 0 }, e);
        }
      } else if (!u.hasTarget) {
        ctx.db.unit.entityId.update({
          ...u,
          ...movePatch(ctx, e.x, e.y, keep.x, keep.y),
        });
      }
    }
  }
});

// Live enemy units (as Located) for target acquisition. `units` is the tick
// snapshot; each is re-fetched so a unit already killed this tick is excluded.
// Shared by the soldier and tower loops so acquisition logic lives in one place.
function enemyUnitsAround(
  ctx: any,
  units: any[],
  owner: any,
  selfId: bigint
): Located[] {
  const out: Located[] = [];
  for (const o of units) {
    if (o.entityId === selfId || o.owner.equals(owner)) continue;
    if (!ctx.db.unit.entityId.find(o.entityId)) continue; // dead this tick
    const oe = ctx.db.entity.entityId.find(o.entityId);
    if (oe) out.push({ id: o.entityId, x: oe.x, y: oe.y });
  }
  return out;
}

// Combat — runs every COMBAT_TICK_MS. Soldiers auto-acquire nearby enemies,
// close to range, and strike on cooldown. Dead units are removed.
export const combatTick = spacetimedb.reducer(
  { timer: combatTimer.rowType },
  (ctx) => {
    const units = [...ctx.db.unit.iter()];
    for (const snap of units) {
      // Re-fetch every iteration: a unit (or its target) killed earlier this
      // same tick must never act, nor be acted on, via a stale snapshot row.
      const u = ctx.db.unit.entityId.find(snap.entityId);
      if (!u) continue;
      const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
      if (def.attack <= 0) continue; // non-combatants never fight

      const e = ctx.db.entity.entityId.find(u.entityId);
      if (!e) continue;

      let targetId = u.attackTarget;
      // No current target: auto-acquire the nearest LIVE enemy unit in range.
      if (targetId === 0n && def.aggroRange > 0) {
        const enemies = enemyUnitsAround(ctx, units, u.owner, u.entityId);
        const near = nearestWithin(e.x, e.y, enemies, def.aggroRange);
        if (near) targetId = near.id;
      }

      // Resolve the target fresh — stale hp can't double-hit or "revive".
      const tu = targetId !== 0n ? ctx.db.unit.entityId.find(targetId) : null;
      const tb =
        !tu && targetId !== 0n ? ctx.db.building.entityId.find(targetId) : null;
      const te = tu || tb ? ctx.db.entity.entityId.find(targetId) : null;

      const cd = Math.max(0, u.attackCooldown - COMBAT_DT);
      if (!te || (!tu && !tb)) {
        if (u.attackTarget !== 0n || u.attackCooldown !== cd)
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: cd,
          });
        continue;
      }

      // Big buildings can be hit from anywhere on their footprint edge.
      const targetR = tb
        ? (BUILDING_DEFS[tb.kind as BuildingKindT]?.footprint ?? 1) / 2
        : 0;
      const d = dist(e.x, e.y, te.x, te.y);
      if (d <= def.range + targetR) {
        if (cd > 0) {
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: targetId,
            attackCooldown: cd,
            hasTarget: false,
          });
          continue;
        }
        const targetArmor = tu
          ? UNIT_DEFS[tu.kind as UnitKindT].armorClass
          : BUILDING_DEFS[tb!.kind as BuildingKindT].armorClass;
        const newHp = applyDamage(
          tu ? tu.hp : tb!.hp,
          effectiveDamage(def, targetArmor)
        );
        if (newHp <= 0) {
          if (tu) ctx.db.unit.entityId.delete(targetId);
          else {
            if (tb!.kind === BuildingKind.Keep) markDefeated(ctx, tb!.owner);
            ctx.db.building.entityId.delete(targetId);
          }
          ctx.db.entity.entityId.delete(targetId);
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: def.attackRate,
            hasTarget: false,
          });
        } else {
          if (tu) ctx.db.unit.entityId.update({ ...tu, hp: newHp });
          else ctx.db.building.entityId.update({ ...tb!, hp: newHp });
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: targetId,
            attackCooldown: def.attackRate,
            hasTarget: false,
          });
        }
      } else {
        // Out of range — posture decides whether to chase, fall back, or hold.
        const act = combatAction(
          u.stance as Stance,
          false,
          dist(e.x, e.y, u.homeX, u.homeY),
          DEFENSIVE_LEASH
        );
        if (act === 'approach') {
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: targetId,
            attackCooldown: cd,
            ...(u.hasTarget ? {} : movePatch(ctx, e.x, e.y, te.x, te.y)),
          });
        } else if (act === 'return') {
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: cd,
            ...(u.hasTarget ? {} : movePatch(ctx, e.x, e.y, u.homeX, u.homeY)),
          });
        } else {
          // hold: stand fast, do not chase.
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: cd,
          });
        }
      }
    }

    // Towers auto-fire at the nearest enemy unit within range.
    for (const b of [...ctx.db.building.iter()]) {
      const bdef =
        BUILDING_DEFS[b.kind as BuildingKindT] ?? BUILDING_DEFS[BuildingKind.Keep];
      if (bdef.attack <= 0) continue;
      const be = ctx.db.entity.entityId.find(b.entityId);
      if (!be) continue;
      const cd = Math.max(0, b.cooldown - COMBAT_DT);
      const enemies = enemyUnitsAround(ctx, units, b.owner, 0n);
      const near = nearestWithin(be.x, be.y, enemies, bdef.range);
      const fresh = near ? ctx.db.unit.entityId.find(near.id) : null;
      if (near && fresh && cd <= 0) {
        ctx.db.shot.insert({ fromX: be.x, fromY: be.y, toX: near.x, toY: near.y });
        const newHp = applyDamage(
          fresh.hp,
          effectiveDamage(
            { attack: bdef.attack, damageType: bdef.damageType },
            UNIT_DEFS[fresh.kind as UnitKindT].armorClass
          )
        );
        if (newHp <= 0) {
          ctx.db.unit.entityId.delete(fresh.entityId);
          ctx.db.entity.entityId.delete(fresh.entityId);
        } else {
          ctx.db.unit.entityId.update({ ...fresh, hp: newHp });
        }
        ctx.db.building.entityId.update({ ...b, cooldown: bdef.attackRate });
      } else if (b.cooldown !== cd) {
        ctx.db.building.entityId.update({ ...b, cooldown: cd });
      }
    }
  }
);

// Skirmish AI — runs every AI_BRAIN_TICK_MS. Each bot keeps peasants gathering,
// follows a build-order priority (economy → military → defense), and launches
// attack waves. It only calls the same owner-parameterized command logic a human
// goes through; no special powers.
export const aiBrain = spacetimedb.reducer(
  { timer: aiBrainTimer.rowType },
  (ctx) => {
    for (const bot of [...ctx.db.ai.iter()]) {
      const p = ctx.db.player.identity.find(bot.identity);
      if (!p || p.defeated) continue;
      const owner = bot.identity;
      const prof = AI_PROFILES[bot.difficulty] ?? AI_PROFILES[1];

      const keep = ctx.db.building.entityId.find(p.keepEntity);
      const ke = keep ? ctx.db.entity.entityId.find(keep.entityId) : null;
      if (!keep || !ke) continue; // keep gone — combatTick marks it defeated

      // Census of this bot's holdings.
      const myUnits = [...ctx.db.unit.iter()].filter((u) =>
        u.owner.equals(owner)
      );
      const peasants = myUnits.filter((u) => u.kind === UnitKind.Peasant).length;
      const soldiers = myUnits.filter(
        (u) => (UNIT_DEFS[u.kind as UnitKindT]?.attack ?? 0) > 0
      );
      const myBuildings = [...ctx.db.building.iter()].filter((b) =>
        b.owner.equals(owner)
      );
      const barracks = myBuildings.find((b) => b.kind === BuildingKind.Barracks);
      const towers = myBuildings.filter(
        (b) => b.kind === BuildingKind.Tower
      ).length;
      const pop = popInfo(ctx, owner);

      // Keep the economy busy every tick.
      assignIdleGatherers(ctx, owner);

      // One macro action per decision window.
      let decisionCd = bot.decisionCd - AI_BRAIN_DT;
      if (decisionCd <= 0) {
        decisionCd = 1.0;
        if (peasants < prof.peasantTarget && pop.pop < pop.cap) {
          trainFrom(ctx, owner, keep, UnitKind.Peasant);
        } else if (
          pop.cap - pop.pop <= 1 &&
          p.wood >= BUILDING_DEFS[BuildingKind.House].cost
        ) {
          const s = aiFindSpot(ctx, BuildingKind.House, ke.x, ke.y);
          if (s) placeFor(ctx, owner, BuildingKind.House, s.x, s.y);
        } else if (
          !barracks &&
          p.wood >= BUILDING_DEFS[BuildingKind.Barracks].cost
        ) {
          const s = aiFindSpot(ctx, BuildingKind.Barracks, ke.x, ke.y);
          if (s) placeFor(ctx, owner, BuildingKind.Barracks, s.x, s.y);
        } else if (
          barracks &&
          soldiers.length < prof.armyTarget &&
          pop.pop < pop.cap
        ) {
          const roll = ctx.random();
          const kind =
            roll < prof.knightRatio
              ? UnitKind.Knight
              : roll < prof.knightRatio + prof.archerRatio
                ? UnitKind.Archer
                : UnitKind.Spearman;
          trainFrom(ctx, owner, barracks, kind);
        } else if (
          towers < prof.maxTowers &&
          p.wood >= BUILDING_DEFS[BuildingKind.Tower].cost + prof.woodBuffer
        ) {
          const s = aiFindSpot(ctx, BuildingKind.Tower, ke.x, ke.y);
          if (s) placeFor(ctx, owner, BuildingKind.Tower, s.x, s.y);
        }
      }

      // Assault: once an army is mustered and the timer is up, throw everyone at
      // the nearest enemy keep. movePatch routes them; combat auto-aggro fights.
      let waveTimer = bot.waveTimer - AI_BRAIN_DT;
      if (soldiers.length >= prof.waveSize && waveTimer <= 0) {
        const target = nearestEnemyKeep(ctx, owner, ke.x, ke.y);
        if (target) {
          for (const s of soldiers) {
            const su = ctx.db.unit.entityId.find(s.entityId);
            const se = ctx.db.entity.entityId.find(s.entityId);
            if (!su || !se) continue;
            ctx.db.unit.entityId.update({
              ...su,
              attackTarget: target.id,
              gatherState: GatherState.Idle,
              targetNode: 0n,
              ...movePatch(ctx, se.x, se.y, target.x, target.y),
            });
          }
          waveTimer = prof.waveInterval;
        }
      }

      ctx.db.ai.identity.update({ ...bot, decisionCd, waveTimer });
    }
  }
);
