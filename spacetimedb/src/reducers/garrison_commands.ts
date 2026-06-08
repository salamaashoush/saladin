import { t, SenderError } from 'spacetimedb/server';
import { BUILDING_DEFS, UNIT_DEFS } from '../../../shared/defs.ts';
import {
  GatherState,
  type BuildingKind as BuildingKindT,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { canGarrison, garrisonFreeSlots } from '../../../shared/garrison.ts';
import { spacetimedb } from '../schema/db.ts';
import { occupantCount, ejectAll } from '../world/garrison.ts';

// Shelter one of the caller's units inside one of the caller's structures. The
// unit leaves the field (movement/combat loops skip it, the client hides it) and,
// if ranged, lends fire to the host. Authorizes both rows, enforces the host's
// garrisonCap and the data-driven canGarrison rule (no cavalry/siege).
export const garrisonUnit = spacetimedb.reducer(
  { unitId: t.u64(), buildingId: t.u64() },
  (ctx, { unitId, buildingId }) => {
    const u = ctx.db.unit.entityId.find(unitId);
    if (!u || !u.owner.equals(ctx.sender))
      throw new SenderError('not your unit');
    if (u.garrisonedIn !== 0n) return; // already sheltered — benign repeat
    const b = ctx.db.building.entityId.find(buildingId);
    if (!b || !b.owner.equals(ctx.sender))
      throw new SenderError('not your building');

    const udef = UNIT_DEFS[u.kind as UnitKindT];
    if (!canGarrison(udef)) throw new SenderError('that unit cannot garrison');
    const bdef = BUILDING_DEFS[b.kind as BuildingKindT];
    if (garrisonFreeSlots(bdef, occupantCount(ctx, buildingId)) <= 0)
      throw new SenderError('garrison is full');

    ctx.db.garrison.insert({
      slotId: 0n,
      building: buildingId,
      unit: unitId,
      owner: ctx.sender,
    });
    // Pull the unit off the field: clear movement + combat intent so the skipped
    // tick loops never act on a stale target. The entity row is left in place
    // (hidden client-side) so the entityId stays stable for ungarrison.
    ctx.db.unit.entityId.update({
      ...u,
      garrisonedIn: buildingId,
      hasTarget: false,
      path: [],
      pathIdx: 0,
      attackTarget: 0n,
      gatherState: GatherState.Idle,
      targetNode: 0n,
    });
  }
);

// Empty a structure: pop every occupant back onto the field at the host edge.
export const ungarrisonBuilding = spacetimedb.reducer(
  { buildingId: t.u64() },
  (ctx, { buildingId }) => {
    const b = ctx.db.building.entityId.find(buildingId);
    if (!b || !b.owner.equals(ctx.sender))
      throw new SenderError('not your building');
    const be = ctx.db.entity.entityId.find(buildingId);
    if (!be) return;
    ejectAll(ctx, b, be);
  }
);
