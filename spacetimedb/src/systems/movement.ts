import { MOVE_DT, ARRIVE_EPS } from '../../../shared/constants.ts';
import { stepToward } from '../../../shared/sim.ts';
import { spacetimedb } from '../schema/db.ts';
import { moveTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { activeMatchIds } from '../world/scope.ts';

// Movement integration — runs every MOVE_TICK_MS. Only touches movers in Active
// matches (a Paused/Ended match's units freeze in place).
export const moveUnits = spacetimedb.reducer(
  { timer: moveTimer.rowType },
  (ctx) => {
    const active = activeMatchIds(ctx);
    for (const u of [...ctx.db.unit.iter()]) {
      if (!active.has(u.matchId)) continue; // paused/ended match — frozen
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
      ctx.db.entity.entityId.update({ ...e, x: r.x, y: r.y, facing: r.facing });
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
