import { WORLD_SIZE } from '../../../shared/constants.ts';
import { BUILDING_DEFS } from '../../../shared/defs.ts';
import type { BuildingKind as BuildingKindT } from '../../../shared/enums.ts';
import {
  isPassable,
  nearestPassableGrid,
  nearestReachablePassableGrid,
  findPathGrid,
} from '../../../shared/pathfinding.ts';
import {
  footprintCenter,
  canPlace,
  isWaterAdjacent,
  occupancySet,
  type Occupant,
} from '../../../shared/buildings.ts';
import { hasPrereq } from '../../../shared/tech.ts';
import { canAfford, payCost } from '../../../shared/economy.ts';
import { clampWorld, getSeed, passableWith } from './util.ts';
import { spawnBuilding } from './spawn.ts';

// Set of building kinds the owner already has — feeds the shared tech gate.
export function ownedBuildingKinds(ctx: any, owner: any): Set<BuildingKindT> {
  const s = new Set<BuildingKindT>();
  for (const b of [...ctx.db.building.iter()])
    if (b.owner.equals(owner)) s.add(b.kind as BuildingKindT);
  return s;
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
export function buildOccupancy(ctx: any): Set<number> {
  return occupancySet(buildingOccupants(ctx), false);
}

// Tiles blocked for PLACEMENT — every footprint incl. passable (no stacking).
export function allBuildingTiles(ctx: any): Set<number> {
  return occupancySet(buildingOccupants(ctx), true);
}

// `occ` is the pathing-blocked tile set. Pass a per-tick prebuilt one (Rank 3a,
// docs/STDB_PERF.md §3) so a hot system doesn't rebuild occupancy — a full
// building.iter() + per-building .find() — on EVERY path request. When omitted it
// rebuilds (kept for one-off callers like reducers outside the tick loop).
export function movePatch(
  ctx: any,
  ex: number,
  ey: number,
  tx: number,
  ty: number,
  occ?: Set<number>
): any {
  const seed = getSeed(ctx);
  const passable = passableWith(seed, occ ?? buildOccupancy(ctx));
  // Snap the destination to the passable tile nearest the target that is in the
  // mover's OWN connected region. The plain nearest-passable tile can be a pocket
  // cut off by water/walls (common on coastal keeps); routing there returns no
  // path and freezes the unit. Reachable-snap guarantees a walkable approach so a
  // carrier always reaches its dropoff and the economy never stalls.
  const snap =
    nearestReachablePassableGrid(passable, ex, ey, tx, ty) ??
    nearestPassableGrid(passable, tx, ty);
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
  if (!hasPrereq(ownedBuildingKinds(ctx, owner), def))
    return `requires ${BUILDING_DEFS[def.requires as BuildingKindT].label}`;
  if (!canAfford(p, def.cost)) return 'not enough resources';

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
  if (
    def.requiresWater &&
    !isWaterAdjacent(def.footprint, x, y, (tx, ty) => isPassable(seed, tx, ty))
  )
    return 'must be built on the shore';

  const c = footprintCenter(def.footprint, x, y);
  ctx.db.player.identity.update({ ...p, ...payCost(p, def.cost) });
  spawnBuilding(ctx, owner, kind, c.x, c.y, p.matchId);
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
