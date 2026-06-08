import { BuildingKind, UnitKind } from '../../../shared/enums.ts';
import { mapPresetById } from '../../../shared/presets.ts';
import type { AssaultIntel, TacticalTarget } from '../../../shared/ai.ts';
import { dist } from './util.ts';

// Regenerate the SHARED map for a fresh match: pick a seed (0 = a random one) and a
// preset, store both on config (the client reads them back to render the terrain).
// Resource nodes are per-match now and scattered by the caller (startSkirmish) into
// the new match's id, so this only rolls the global seed/preset. Returns the chosen
// seed + preset id so the caller can snapshot them onto the match row.
export function regenerateWorld(
  ctx: any,
  seed: number,
  preset: string
): { seed: number; preset: string } {
  const cfg = ctx.db.config.id.find(0);
  const finalSeed =
    seed > 0 ? seed >>> 0 : ctx.random.integerInRange(1, 2_000_000_000);
  const presetId = mapPresetById(preset).id;
  if (cfg) ctx.db.config.id.update({ ...cfg, seed: finalSeed, preset: presetId });
  return { seed: finalSeed, preset: presetId };
}

// Nearest enemy keep to (x,y); falls back to any enemy building. Drives assaults.
export function nearestEnemyKeep(
  ctx: any,
  owner: any,
  x: number,
  y: number,
  matchId: bigint
): { id: bigint; x: number; y: number } | null {
  let best: { id: bigint; x: number; y: number } | null = null;
  let bestD = Infinity;
  let fallback: { id: bigint; x: number; y: number } | null = null;
  let fbD = Infinity;
  for (const b of [...ctx.db.building.iter()]) {
    if (b.owner.equals(owner) || b.matchId !== matchId) continue;
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
