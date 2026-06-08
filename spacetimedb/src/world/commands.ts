import { BuildingKind } from '../../../shared/enums.ts';
import { dist, getSeed } from './util.ts';
import { scatterNodes } from './spawn.ts';

// Remove everything belonging to `owner`: units, buildings, their entity rows,
// the ai row (if a bot) and the player row. Used to tear down a match cleanly.
export function clearOwner(ctx: any, owner: any): void {
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

// The caller and the bots they host.
export function clearMatch(ctx: any, caller: any): void {
  clearOwner(ctx, caller);
  for (const bot of [...ctx.db.ai.iter()])
    if (bot.host.equals(caller)) clearOwner(ctx, bot.identity);
}

// Tear down the caller's match: the caller plus only the bots they host — never
// another human's opponents. Refresh the forest only when the caller is alone,
// so a reset can't wipe resources out from under another human's match.
export function resetMatch(ctx: any, caller: any): void {
  const alone = !otherHumansPresent(ctx, caller);
  clearMatch(ctx, caller);
  if (alone) {
    for (const n of [...ctx.db.resourceNode.iter()]) {
      ctx.db.resourceNode.entityId.delete(n.entityId);
      ctx.db.entity.entityId.delete(n.entityId);
    }
    scatterNodes(ctx, getSeed(ctx));
  }
}

// Nearest enemy keep to (x,y); falls back to any enemy building. Drives assaults.
export function nearestEnemyKeep(
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
