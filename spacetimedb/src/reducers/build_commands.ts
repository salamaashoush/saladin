import { t, SenderError } from 'spacetimedb/server';
import { WORLD_SIZE } from '../../../shared/constants.ts';
import { BUILDING_DEFS } from '../../../shared/defs.ts';
import {
  BuildingKind,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import { isPassable } from '../../../shared/pathfinding.ts';
import { footprintCenter, canPlace, occupancySet } from '../../../shared/buildings.ts';
import { canAfford, payCost, refundCost } from '../../../shared/economy.ts';
import { spacetimedb } from '../schema/db.ts';
import { WallTile } from '../schema/tables.ts';
import { getSeed } from '../world/util.ts';
import { allBuildingTiles, placeFor } from '../world/placement.ts';
import { spawnBuilding } from '../world/spawn.ts';
import { trainFrom } from '../world/economy.ts';
import { ejectAll } from '../world/garrison.ts';

export const trainUnit = spacetimedb.reducer(
  { buildingId: t.u64(), kind: t.u8() },
  (ctx, { buildingId, kind }) => {
    const b = ctx.db.building.entityId.find(buildingId);
    if (!b || !b.owner.equals(ctx.sender)) return;
    const err = trainFrom(ctx, ctx.sender, b, kind);
    if (err) throw new SenderError(err);
  }
);

export const placeBuilding = spacetimedb.reducer(
  { kind: t.u8(), x: t.f32(), y: t.f32() },
  (ctx, { kind, x, y }) => {
    const err = placeFor(ctx, ctx.sender, kind, x, y);
    if (err) throw new SenderError(err);
  }
);

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
    // Running balances stamped onto the row once at the end — one update for the
    // whole dragged line, not one per tile.
    let bal = { wood: p.wood, stone: p.stone, food: p.food, gold: p.gold };
    let placed = false;
    for (const tile of tiles) {
      if (!canAfford(bal, def.cost)) break;
      const ok = canPlace(
        BuildingKind.Wall,
        tile.x,
        tile.y,
        (tx, ty) => isPassable(seed, tx, ty),
        (tx, ty) => occ.has(ty * WORLD_SIZE + tx)
      );
      if (!ok) continue;
      const c = footprintCenter(def.footprint, tile.x, tile.y);
      spawnBuilding(ctx, ctx.sender, BuildingKind.Wall, c.x, c.y, p.matchId);
      for (const k of occupancySet([{ kind: BuildingKind.Wall, x: tile.x, y: tile.y }], true))
        occ.add(k);
      bal = payCost(bal, def.cost);
      placed = true;
    }
    if (placed) ctx.db.player.identity.update({ ...p, ...bal });
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
      ctx.db.player.identity.update({ ...p, ...refundCost(p, def.cost, 0.5) });
    // Pop any sheltered units back to the field before razing — a demolish is
    // voluntary, so occupants always survive regardless of garrisonSurvivesDeath.
    const be = ctx.db.entity.entityId.find(entityId);
    if (be) ejectAll(ctx, b, be);
    ctx.db.building.entityId.delete(entityId);
    ctx.db.entity.entityId.delete(entityId);
  }
);
