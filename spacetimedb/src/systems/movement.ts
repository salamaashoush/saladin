import { MOVE_DT, ARRIVE_EPS } from '../../../shared/constants.ts';
import { stepToward } from '../../../shared/sim.ts';
import { cellOf } from '../../../shared/spatial.ts';
import { spacetimedb } from '../schema/db.ts';
import { moveTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { activeMatchIds } from '../world/scope.ts';
import { bumpMoveTick } from '../world/tick_count.ts';

// Movement integration — runs every MOVE_TICK_MS. Only touches movers in Active
// matches (a Paused/Ended match's units freeze in place). Drives off the matchId
// btree index (Rank 1, docs/STDB_PERF.md §3) so it decodes only active-match units,
// not every row in every match.
export const moveUnits = spacetimedb.reducer(
  { timer: moveTimer.rowType },
  (ctx) => {
    bumpMoveTick(ctx);
    const active = activeMatchIds(ctx);
    for (const mid of active)
    for (const u of ctx.db.unit.matchId.filter(mid)) {
      if (u.garrisonedIn !== 0n) continue; // sheltered — off the field
      if (!u.hasTarget) continue;
      const e = ctx.db.entity.entityId.find(u.entityId);
      if (!e) continue;

      const r = stepToward(
        e.x,
        e.y,
        u.targetX,
        u.targetY,
        u.speed * MOVE_DT,
        ARRIVE_EPS
      );
      // Keep the spatial-grid column current, but ONLY when the unit crosses a cell
      // boundary (Rank 2): most ticks the cell is unchanged, so the entity row is
      // updated with the same cost as before — no extra write on the common path.
      const newCell = cellOf(r.x, r.y);
      ctx.db.entity.entityId.update({
        ...e,
        x: r.x,
        y: r.y,
        facing: r.facing,
        cell: newCell === e.cell ? e.cell : newCell,
      });
      if (!r.arrived) continue;
      const next = u.pathIdx + 1;
      if (next < u.path.length) {
        const wp = u.path[next];
        ctx.db.unit.entityId.update({
          ...u,
          pathIdx: next,
          targetX: wp.x,
          targetY: wp.y,
        });
      } else {
        ctx.db.unit.entityId.update({ ...u, hasTarget: false });
      }
    }
  }
);

scheduleRefs.moveUnits = moveUnits;
