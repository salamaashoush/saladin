import { Identity } from 'spacetimedb';
import {
  WORLD_SIZE,
  TREE_WOOD,
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
  aiName,
  spawnCorner,
  allocSlot,
} from '../../../shared/defs.ts';
import { sampleTerrain, treeDensity } from '../../../shared/terrain.ts';
import { findBuildableNear } from '../../../shared/buildings.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
  GatherState,
  Faction,
  Stance,
  type UnitKind as UnitKindT,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import { clampWorld, getSeed, buildNodes, assignGather } from './util.ts';

// ctx is typed inside reducers; helpers take `any` to avoid threading the schema
// generic everywhere. They only touch typed table rows.
export function spawnUnitEntity(
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
    carryType: ResourceType.Wood,
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

export function spawnBuilding(
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
export function scatterTrees(ctx: any, seed: number, count: number): void {
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

// Found a new base for `owner`: keep at the next free corner, starting peasants
// already gathering. Shared by enterGame (human) and addAi (skirmish bot) so both
// sides start identically. Returns the keep entity id.
export function foundPlayer(
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
    stone: START_STONE,
    food: START_FOOD,
    gold: START_GOLD,
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

// Spawn one bot for `host`'s match. Identity comes from a persistent monotonic
// counter so it is globally unique and never reused across resets.
export function spawnAi(
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

// A keep just fell — its owner is out. Idempotent.
export function markDefeated(ctx: any, owner: any): void {
  const p = ctx.db.player.identity.find(owner);
  if (p && !p.defeated) ctx.db.player.identity.update({ ...p, defeated: true });
}
