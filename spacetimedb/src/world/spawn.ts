import { Identity } from 'spacetimedb';
import {
  START_PEASANTS,
  START_WOOD,
  START_STONE,
  START_FOOD,
  START_GOLD,
  SPAWN_CLUSTER,
  MAX_PLAYERS,
} from '../../../shared/constants.ts';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
  PLAYER_COLORS,
  AI_PROFILES,
  NODE_KINDS,
  aiName,
  spawnCorner,
  allocSlot,
} from '../../../shared/defs.ts';
import { AiPhase } from '../../../shared/ai.ts';
import {
  effectiveUnitDef,
  effectiveBuildingDef,
} from '../../../shared/research.ts';
import { MORALE_MAX } from '../../../shared/morale.ts';
import { scatterNodes as scatterNodesPure } from '../../../shared/terrain.ts';
import { findBuildableNear } from '../../../shared/buildings.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
  GatherState,
  Faction,
  Stance,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { clampWorld, getSeed, buildNodes, assignGatherBalanced } from './util.ts';

// ctx is typed inside reducers; helpers take `any` to avoid threading the schema
// generic everywhere. They only touch typed table rows.

// An owner's completed-tech bitmask, defaulting to 0n before the player row exists
// (the keep is spawned inside foundPlayer, just before player.insert). Newly
// trained units/structures fold researched bonuses (hp/armor) at spawn.
function ownerTechMask(ctx: any, owner: any): bigint {
  return ctx.db.player.identity.find(owner)?.techMask ?? 0n;
}
// Every spawned row is stamped with the match it belongs to (matchId), on BOTH the
// shared entity row and its table row, so per-match teardown/save can sweep by id.
export function spawnUnitEntity(
  ctx: any,
  owner: any,
  kind: number,
  x: number,
  y: number,
  matchId: bigint
): bigint {
  const base = UNIT_DEFS[kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
  // Fold the owner's researched techs so a new unit starts at its EFFECTIVE hp
  // (e.g. Conscription/Plate). speed is unchanged by tech — read from base.
  const def = effectiveUnitDef(kind, ownerTechMask(ctx, owner)) ?? base;
  const e = ctx.db.entity.insert({ entityId: 0n, x, y, facing: 0, matchId });
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
    carryType: ResourceType.Wood,
    harvestTimer: 0,
    hp: def.maxHp,
    attackTarget: 0n,
    attackCooldown: 0,
    stance: Stance.Aggressive,
    morale: MORALE_MAX,
    routing: false,
    homeX: x,
    homeY: y,
    garrisonedIn: 0n,
    path: [],
    pathIdx: 0,
    matchId,
  });
  return e.entityId;
}

export function spawnBuilding(
  ctx: any,
  owner: any,
  kind: number,
  x: number,
  y: number,
  matchId: bigint
): bigint {
  const def =
    effectiveBuildingDef(kind, ownerTechMask(ctx, owner)) ??
    BUILDING_DEFS[BuildingKind.Keep];
  const e = ctx.db.entity.insert({ entityId: 0n, x, y, facing: 0, matchId });
  ctx.db.building.insert({
    entityId: e.entityId,
    owner,
    kind,
    hp: def.maxHp,
    cooldown: 0,
    rallyX: x,
    rallyY: y,
    matchId,
  });
  return e.entityId;
}

function spawnNode(
  ctx: any,
  x: number,
  y: number,
  resType: number,
  remaining: number,
  matchId: bigint
): void {
  const e = ctx.db.entity.insert({ entityId: 0n, x, y, facing: 0, matchId });
  ctx.db.resourceNode.insert({ entityId: e.entityId, resType, remaining, matchId });
}

// Scatter every resource node kind across the map for `matchId`. Shared by init
// and match reset. Positions come from the seeded pure worldgen (shared/terrain.ts)
// — NOT ctx.random — so they are reproducible across restarts and the client could
// recompute them. Data-driven from NODE_KINDS: new node kinds need no code here.
export function scatterNodes(ctx: any, seed: number, matchId: bigint): void {
  for (const n of scatterNodesPure(seed, NODE_KINDS)) {
    spawnNode(ctx, n.x, n.y, n.resType, n.yield, matchId);
  }
}

// Found a new base for `owner` in `matchId`: keep at the next free corner, starting
// peasants already gathering. Shared by enterGame (human) and addAi (skirmish bot)
// so both sides start identically. Slot allocation and resource targeting are scoped
// to the match, so two concurrent matches never share corners or nodes. Returns the
// keep entity id.
export function foundPlayer(
  ctx: any,
  owner: any,
  name: string,
  faction: number,
  matchId: bigint
): bigint {
  // Stable slot from the set of slots in use IN THIS MATCH — survives leavers, so two
  // players in the same match can never share a corner (overlapping keeps), while a
  // separate match reuses the same corners on its own copy of the map.
  const used = [...ctx.db.player.iter()]
    .filter((p: any) => p.matchId === matchId)
    .map((p: any) => p.slot);
  const slot = Math.max(0, allocSlot(used, MAX_PLAYERS));
  const seed = getSeed(ctx);
  const corner = spawnCorner(slot);
  const base = findBuildableNear(
    seed,
    corner.x,
    corner.y,
    BUILDING_DEFS[BuildingKind.Keep].footprint
  );
  const keepId = spawnBuilding(ctx, owner, BuildingKind.Keep, base.x, base.y, matchId);

  ctx.db.player.insert({
    identity: owner,
    playerId: 0,
    name,
    faction,
    wood: START_WOOD,
    stone: START_STONE,
    food: START_FOOD,
    gold: START_GOLD,
    color: slot % PLAYER_COLORS.length,
    online: true,
    keepEntity: keepId,
    defeated: false,
    slot,
    techMask: 0n,
    matchId,
  });

  const nodes = buildNodes(ctx, matchId);
  const fresh: any[] = [];
  for (let i = 0; i < START_PEASANTS; i++) {
    const a = (i / START_PEASANTS) * Math.PI * 2;
    const px = clampWorld(base.x + Math.cos(a) * SPAWN_CLUSTER);
    const py = clampWorld(base.y + Math.sin(a) * SPAWN_CLUSTER);
    const id = spawnUnitEntity(ctx, owner, UnitKind.Peasant, px, py, matchId);
    const u = ctx.db.unit.entityId.find(id);
    if (u) fresh.push(u);
  }
  // Balanced food-first assignment so the opening economy never starves.
  assignGatherBalanced(ctx, fresh, nodes);
  return keepId;
}

// Spawn one bot for `host`'s match (`matchId`). Identity comes from a persistent
// monotonic counter so it is globally unique and never reused across resets.
export function spawnAi(
  ctx: any,
  host: any,
  difficulty: number,
  faction: Faction,
  matchId: bigint
): void {
  const diff = AI_PROFILES[difficulty] ? difficulty : 1;
  const cfg = ctx.db.config.id.find(0);
  const n = cfg ? cfg.nextBotId : 1n;
  if (cfg) ctx.db.config.id.update({ ...cfg, nextBotId: n + 1n });
  const botId = new Identity((1n << 255n) | n);
  foundPlayer(ctx, botId, aiName(faction, Number(n)), faction, matchId);
  ctx.db.ai.insert({
    identity: botId,
    host,
    difficulty: diff,
    decisionCd: 0,
    waveTimer: AI_PROFILES[diff].firstWaveDelay,
    phase: AiPhase.Boot,
    scoutId: 0n,
    threatTimer: 0,
    matchId,
  });
}

// A keep just fell — its owner is out. Idempotent.
export function markDefeated(ctx: any, owner: any): void {
  const p = ctx.db.player.identity.find(owner);
  if (p && !p.defeated) ctx.db.player.identity.update({ ...p, defeated: true });
}
