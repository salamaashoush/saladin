import { WORLD_SIZE } from '../../../shared/constants.ts';
import { isPassable, type Passable } from '../../../shared/pathfinding.ts';
import { GatherState } from '../../../shared/enums.ts';
import { nearestIndex } from '../../../shared/sim.ts';

export interface NodePos {
  id: bigint;
  x: number;
  y: number;
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

export function buildNodes(ctx: any): NodePos[] {
  const nodes: NodePos[] = [];
  for (const n of [...ctx.db.resourceNode.iter()]) {
    const e = ctx.db.entity.entityId.find(n.entityId);
    if (e) nodes.push({ id: n.entityId, x: e.x, y: e.y });
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
