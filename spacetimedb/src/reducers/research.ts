import { t, SenderError } from 'spacetimedb/server';
import { spacetimedb } from '../schema/db.ts';
import { startResearchFor } from '../world/research.ts';

// Begin researching a Blacksmith tech. Authorize the caller owns the building,
// then delegate to the owner-parameterized helper (shared with the AI). Reading
// data still goes through subscriptions — this only mutates.
export const startResearch = spacetimedb.reducer(
  { buildingId: t.u64(), tech: t.u8() },
  (ctx, { buildingId, tech }) => {
    const b = ctx.db.building.entityId.find(buildingId);
    if (!b || !b.owner.equals(ctx.sender)) return;
    const err = startResearchFor(ctx, ctx.sender, b, tech);
    if (err) throw new SenderError(err);
  }
);
