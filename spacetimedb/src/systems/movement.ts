import { MOVE_DT, ARRIVE_EPS } from '../../../shared/constants.ts';
import { stepToward } from '../../../shared/sim.ts';
import { spacetimedb } from '../schema/db.ts';
import { moveTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';

// Movement integration — runs every MOVE_TICK_MS. Only touches movers.
export const moveUnits = spacetimedb.reducer(
  { timer: moveTimer.rowType },
  (ctx) => {
    for (const u of [...ctx.db.unit.iter()]) {
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
