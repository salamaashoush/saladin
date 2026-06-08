import { BUILDING_DEFS, UNIT_DEFS } from '../../../shared/defs.ts';
import {
  type BuildingKind as BuildingKindT,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { nearestPassableGrid } from '../../../shared/pathfinding.ts';
import type { GarrisonOccupant } from '../../../shared/garrison.ts';
import { getSeed, passableWith } from './util.ts';
import { buildOccupancy } from './placement.ts';

// All garrison rows whose host is `buildingId`.
export function occupantsOf(ctx: any, buildingId: bigint): any[] {
  return [...ctx.db.garrison.building.filter(buildingId)];
}

// How many units currently shelter in `buildingId`.
export function occupantCount(ctx: any, buildingId: bigint): number {
  return occupantsOf(ctx, buildingId).length;
}

// Map a host's live occupants to the pure firepower contract (attack + ranged),
// skipping any garrison row whose unit has since vanished.
export function occupantFireProfile(ctx: any, buildingId: bigint): GarrisonOccupant[] {
  const out: GarrisonOccupant[] = [];
  for (const g of occupantsOf(ctx, buildingId)) {
    const u = ctx.db.unit.entityId.find(g.unit);
    if (!u) continue;
    const def = UNIT_DEFS[u.kind as UnitKindT];
    if (!def) continue;
    out.push({ attack: def.attack, ranged: !!def.ranged });
  }
  return out;
}

// Best-case firing reach + cadence the ranged occupants give a host that does not
// shoot on its own (a manned Wall/Gatehouse): the longest occupant range and the
// fastest occupant reload. Returns null when no ranged occupant is present.
export function garrisonRangeRate(
  ctx: any,
  buildingId: bigint
): { range: number; rate: number } | null {
  let range = 0;
  let rate = Infinity;
  for (const g of occupantsOf(ctx, buildingId)) {
    const u = ctx.db.unit.entityId.find(g.unit);
    if (!u) continue;
    const def = UNIT_DEFS[u.kind as UnitKindT];
    if (!def || !def.ranged || def.attack <= 0) continue;
    range = Math.max(range, def.range);
    rate = Math.min(rate, def.attackRate);
  }
  return range > 0 ? { range, rate } : null;
}

// Snap a unit back onto the field at the host's edge: the nearest passable tile
// around the structure so ejected occupants never land on water/inside a wall.
function fieldExit(ctx: any, host: any, hostEntity: any): { x: number; y: number } {
  const seed = getSeed(ctx);
  const def = BUILDING_DEFS[host.kind as BuildingKindT];
  const r = (def?.footprint ?? 1) / 2 + 0.6;
  const passable = passableWith(seed, buildOccupancy(ctx));
  // Probe a ring just outside the footprint; nearestPassableGrid finalizes the
  // landing tile (and handles the rare case the probe point itself is blocked).
  for (let a = 0; a < 12; a++) {
    const ang = (a / 12) * Math.PI * 2;
    const px = hostEntity.x + Math.cos(ang) * r;
    const py = hostEntity.y + Math.sin(ang) * r;
    if (passable(Math.floor(px), Math.floor(py)))
      return nearestPassableGrid(passable, px, py);
  }
  return nearestPassableGrid(passable, hostEntity.x, hostEntity.y);
}

// Return one sheltered unit to the field at `exit`: place its entity there, clear
// its garrison flag, repost home, and delete the garrison slot. The entity row
// was kept while garrisoned (just hidden client-side), so re-seat rather than
// re-insert to preserve the stable entityId every other table references.
export function ejectOne(ctx: any, g: any, host: any, hostEntity: any): void {
  const u = ctx.db.unit.entityId.find(g.unit);
  const e = u ? ctx.db.entity.entityId.find(g.unit) : null;
  if (u && e) {
    const exit = fieldExit(ctx, host, hostEntity);
    ctx.db.entity.entityId.update({ ...e, x: exit.x, y: exit.y });
    ctx.db.unit.entityId.update({
      ...u,
      garrisonedIn: 0n,
      hasTarget: false,
      path: [],
      pathIdx: 0,
      attackTarget: 0n,
      homeX: exit.x,
      homeY: exit.y,
    });
  }
  ctx.db.garrison.slotId.delete(g.slotId);
}

// Empty a host: pop every occupant back to the field. Used by the ungarrison
// reducer and by building death when the host's garrison survives.
export function ejectAll(ctx: any, host: any, hostEntity: any): void {
  for (const g of occupantsOf(ctx, host.entityId)) ejectOne(ctx, g, host, hostEntity);
}

// A host structure is dying. If its def says the garrison survives, eject the
// occupants to the field; otherwise the occupants die with it (their unit +
// entity rows are removed). Either way no garrison row is left orphaned. Call
// BEFORE the building/entity rows are deleted (eject reads the host position).
export function evacuateOnDeath(ctx: any, host: any, hostEntity: any): void {
  const def = BUILDING_DEFS[host.kind as BuildingKindT];
  if (def?.garrisonSurvivesDeath) {
    ejectAll(ctx, host, hostEntity);
    return;
  }
  for (const g of occupantsOf(ctx, host.entityId)) {
    if (ctx.db.unit.entityId.find(g.unit)) ctx.db.unit.entityId.delete(g.unit);
    if (ctx.db.entity.entityId.find(g.unit)) ctx.db.entity.entityId.delete(g.unit);
    ctx.db.garrison.slotId.delete(g.slotId);
  }
}

// Remove one unit completely: its garrison slot (if sheltered), its unit row, and
// its entity row — leaving nothing orphaned. Used when a unit dies OUTSIDE the
// combat loop (e.g. starves to death), so it stops drawing upkeep instead of
// lingering as a zero-hp zombie that deadlocks the food economy.
export function removeUnit(ctx: any, unitId: bigint): void {
  for (const g of [...ctx.db.garrison.iter()])
    if (g.unit === unitId) ctx.db.garrison.slotId.delete(g.slotId);
  if (ctx.db.unit.entityId.find(unitId)) ctx.db.unit.entityId.delete(unitId);
  if (ctx.db.entity.entityId.find(unitId)) ctx.db.entity.entityId.delete(unitId);
}
