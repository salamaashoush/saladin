import { WORLD_SIZE } from '../../../shared/constants.ts';
import { BUILDING_DEFS } from '../../../shared/defs.ts';
import {
  isPassable,
  nearestReachablePassableGrid,
  type Passable,
  type PathPoint,
} from '../../../shared/pathfinding.ts';
import {
  GatherState,
  ResourceType,
  BuildingKind,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import { nearestIndex } from '../../../shared/sim.ts';
import { balancedGatherTypes } from '../../../shared/economy.ts';

export interface NodePos {
  id: bigint;
  x: number;
  y: number;
  resType: number;
  matchId: bigint;
}

export function dist(ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  return Math.sqrt(dx * dx + dy * dy);
}

export function clampWorld(v: number): number {
  return Math.max(0, Math.min(WORLD_SIZE, v));
}

export function getSeed(ctx: any): number {
  const cfg = ctx.db.config.id.find(0);
  return cfg ? cfg.seed : 1;
}

export function passableWith(seed: number, occ: Set<number>): Passable {
  return (px, py) => isPassable(seed, px, py) && !occ.has(py * WORLD_SIZE + px);
}

export interface Dropoff {
  x: number;
  y: number;
  footprint: number;
}

// Nearest place `owner` can deposit a carry of `carryType` from (x,y): the keep
// always accepts every resource; food-dropoff buildings (FishingHut/Granary)
// accept food only, letting shoreline fishers bank without trekking to the keep.
// Returns null only if the owner has no keep (defeated).
export function nearestDropoff(
  ctx: any,
  owner: any,
  carryType: number,
  x: number,
  y: number
): Dropoff | null {
  let best: Dropoff | null = null;
  let bestD = Infinity;
  for (const b of [...ctx.db.building.iter()]) {
    if (!b.owner.equals(owner)) continue;
    const def = BUILDING_DEFS[b.kind as BuildingKindT];
    if (!def) continue;
    const accepts =
      b.kind === BuildingKind.Keep ||
      (def.foodDropoff && carryType === ResourceType.Food);
    if (!accepts) continue;
    const e = ctx.db.entity.entityId.find(b.entityId);
    if (!e) continue;
    const d = dist(x, y, e.x, e.y);
    if (d < bestD) {
      bestD = d;
      best = { x: e.x, y: e.y, footprint: def.footprint };
    }
  }
  return best;
}

// The tile a gatherer at (x,y) will actually stand on to deposit at `drop`: the
// passable tile nearest the dropoff centre that is reachable from the gatherer's
// own region. Falls back to the dropoff centre if the field is fully blocked
// (degenerate). Used so the deposit-range check measures the REACHABLE approach,
// not the centre a coastal keep can hide behind its own footprint/water.
export function dropoffApproach(
  ctx: any,
  passable: Passable,
  x: number,
  y: number,
  drop: Dropoff
): PathPoint {
  return (
    nearestReachablePassableGrid(passable, x, y, drop.x, drop.y) ?? {
      x: drop.x,
      y: drop.y,
    }
  );
}

// Resource nodes as positions. With `matchId` given, only that match's nodes are
// returned — a gatherer must never walk to another match's forest. Omit it (or pass
// undefined) only when scanning globally is intended.
export function buildNodes(ctx: any, matchId?: bigint): NodePos[] {
  const nodes: NodePos[] = [];
  for (const n of [...ctx.db.resourceNode.iter()]) {
    if (matchId !== undefined && n.matchId !== matchId) continue;
    const e = ctx.db.entity.entityId.find(n.entityId);
    if (e)
      nodes.push({
        id: n.entityId,
        x: e.x,
        y: e.y,
        resType: n.resType,
        matchId: n.matchId,
      });
  }
  return nodes;
}

export function assignGather(
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

// Send a batch of gatherer rows to nodes. With `preferType` every gatherer heads
// for that resource (fallback any); otherwise types are round-robined food-first
// across what's available so the economy never neglects food and starves. Each
// gatherer then takes the nearest node of its assigned type.
export function assignGatherBalanced(
  ctx: any,
  units: any[],
  nodes: NodePos[],
  preferType?: number
): void {
  if (nodes.length === 0 || units.length === 0) return;
  const available = [...new Set(nodes.map((n) => n.resType))];
  const types =
    preferType === undefined
      ? balancedGatherTypes(available, units.length)
      : units.map(() => preferType);
  units.forEach((u, i) => {
    const e = ctx.db.entity.entityId.find(u.entityId);
    if (!e) return;
    const want = types[i];
    const pool = nodes.filter((n) => n.resType === want);
    const chosen = pool.length > 0 ? pool : nodes;
    const idx = nearestIndex(e.x, e.y, chosen);
    if (idx < 0) return;
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.ToResource,
      targetNode: chosen[idx].id,
    });
  });
}
