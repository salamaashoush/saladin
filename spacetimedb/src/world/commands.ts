import { BuildingKind, UnitKind } from '../../../shared/enums.ts';
import { mapPresetById } from '../../../shared/presets.ts';
import type { AssaultIntel, TacticalTarget } from '../../../shared/ai.ts';
import { dist, getSeed } from './util.ts';
import { scatterNodes } from './spawn.ts';
import { clearGarrisonsOf } from './garrison.ts';

// Remove everything belonging to `owner`: units, buildings, their entity rows,
// the ai row (if a bot) and the player row. Used to tear down a match cleanly.
export function clearOwner(ctx: any, owner: any): void {
  clearGarrisonsOf(ctx, owner); // drop garrison slots so none outlive their units
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
export function otherHumansPresent(ctx: any, caller: any): boolean {
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

// Regenerate the world: pick a seed (0 = a fresh random one) and a preset, store
// both on config, wipe every resource node, and re-scatter from the new seed.
// Only safe when the caller is alone — guarded by resetMatch. The preset is a
// render flavor the client reads back; node placement stays seed-driven.
export function regenerateWorld(
  ctx: any,
  seed: number,
  preset: string
): void {
  const cfg = ctx.db.config.id.find(0);
  const finalSeed =
    seed > 0 ? seed >>> 0 : ctx.random.integerInRange(1, 2_000_000_000);
  const presetId = mapPresetById(preset).id;
  if (cfg) ctx.db.config.id.update({ ...cfg, seed: finalSeed, preset: presetId });
  for (const n of [...ctx.db.resourceNode.iter()]) {
    ctx.db.resourceNode.entityId.delete(n.entityId);
    ctx.db.entity.entityId.delete(n.entityId);
  }
  scatterNodes(ctx, finalSeed);
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

// Defensive structures a siege train should crack first — anything that fights
// back or blocks the way to the keep.
function isDefenseWork(kind: number): boolean {
  return (
    kind === BuildingKind.Wall ||
    kind === BuildingKind.Gatehouse ||
    kind === BuildingKind.Tower ||
    kind === BuildingKind.Watchtower ||
    kind === BuildingKind.Keep
  );
}

// Build the per-bot assault picture from the tick snapshot: the nearest enemy
// keep (main objective), every enemy structure (split out: defenses for the siege
// train), and enemy gatherers (the soft economy raiders hunt). Hostile = a player
// on the opposing faction. Reads positions from the hoisted `pos` map so it never
// re-scans the entity table per row. Pure data shaping — feeds the pure
// targetForRole planner in shared/ai.ts.
export function assaultIntel(
  allUnits: ReadonlyArray<any>,
  allBuildings: ReadonlyArray<any>,
  pos: ReadonlyMap<bigint, { x: number; y: number }>,
  isEnemy: (ownerId: any) => boolean,
  fromX: number,
  fromY: number
): AssaultIntel {
  const defenses: TacticalTarget[] = [];
  const buildings: TacticalTarget[] = [];
  const gatherers: TacticalTarget[] = [];
  let keep: TacticalTarget | null = null;
  let keepD = Infinity;

  for (const b of allBuildings) {
    if (!isEnemy(b.owner)) continue;
    const e = pos.get(b.entityId);
    if (!e) continue;
    const t: TacticalTarget = { id: b.entityId, x: e.x, y: e.y };
    buildings.push(t);
    if (isDefenseWork(b.kind)) defenses.push(t);
    if (b.kind === BuildingKind.Keep) {
      const d = dist(fromX, fromY, e.x, e.y);
      if (d < keepD) {
        keepD = d;
        keep = t;
      }
    }
  }
  for (const u of allUnits) {
    if (u.kind !== UnitKind.Peasant || !isEnemy(u.owner)) continue;
    const e = pos.get(u.entityId);
    if (e) gatherers.push({ id: u.entityId, x: e.x, y: e.y });
  }
  return { keep, defenses, buildings, gatherers };
}
