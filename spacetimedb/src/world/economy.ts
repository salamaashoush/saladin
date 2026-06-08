import { WORLD_SIZE } from '../../../shared/constants.ts';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
} from '../../../shared/defs.ts';
import {
  UnitKind,
  BuildingKind,
  GatherState,
  type UnitKind as UnitKindT,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import { nearestIndex } from '../../../shared/sim.ts';
import { nearestPassableGrid } from '../../../shared/pathfinding.ts';
import { clampWorld, getSeed, passableWith, buildNodes } from './util.ts';
import { buildOccupancy, movePatch } from './placement.ts';
import { spawnUnitEntity } from './spawn.ts';

export function popInfo(ctx: any, owner: any): { pop: number; cap: number } {
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

export function hasBarracks(ctx: any, owner: any): boolean {
  for (const b of [...ctx.db.building.iter()])
    if (b.owner.equals(owner) && b.kind === BuildingKind.Barracks) return true;
  return false;
}

// Send every idle gatherer owned by `owner` to its nearest resource node.
export function assignIdleGatherers(ctx: any, owner: any): void {
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

// Owner-parameterized command logic. The player-facing reducers authorize via
// ctx.sender then delegate here; the AI brain calls these directly with the bot
// identity. Each returns null on success or an error string. ctx.sender cannot
// be spoofed, so authority lives in the reducers — never here.
export function trainFrom(
  ctx: any,
  owner: any,
  b: any,
  kind: number
): string | null {
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
