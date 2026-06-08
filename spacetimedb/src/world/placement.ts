import { WORLD_SIZE } from '../../../shared/constants.ts';
import { BUILDING_DEFS } from '../../../shared/defs.ts';
import type { BuildingKind as BuildingKindT } from '../../../shared/enums.ts';
import {
  isPassable,
  nearestPassableGrid,
  findPathGrid,
} from '../../../shared/pathfinding.ts';
import {
  footprintCenter,
  canPlace,
  occupancySet,
  type Occupant,
} from '../../../shared/buildings.ts';
import { clampWorld, getSeed, passableWith } from './util.ts';
import { spawnBuilding } from './spawn.ts';

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
export function buildOccupancy(ctx: any): Set<number> {
  return occupancySet(buildingOccupants(ctx), false);
}

// Tiles blocked for PLACEMENT — every footprint incl. passable (no stacking).
export function allBuildingTiles(ctx: any): Set<number> {
  return occupancySet(buildingOccupants(ctx), true);
}

export function movePatch(
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

export function placeFor(
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

// Spiral out from (nx,ny) for the nearest tile where `kind` fully fits. Returns
// raw placement coords (placeFor recentres) or null if nothing fits nearby.
export function aiFindSpot(
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
