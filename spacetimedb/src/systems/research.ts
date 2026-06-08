import { RESEARCH_DT } from '../../../shared/constants.ts';
import {
  UPGRADE_DEFS,
  setTech,
  type Tech as TechT,
} from '../../../shared/research.ts';
import { spacetimedb } from '../schema/db.ts';
import { researchTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { activeMatchIds } from '../world/scope.ts';

// Research progress — runs every RESEARCH_TICK_MS. Advances each in-flight tech by
// one tick of its researchTime; on completion it flips the owner's player.techMask
// bit (so combat math reads a single number) and marks the row done. Deterministic:
// progress is a fixed fraction per tick, no clocks/random. Done rows are kept so
// the HUD can show a tech as complete; the techMask is the authority for combat.
export const researchSystem = spacetimedb.reducer(
  { timer: researchTimer.rowType },
  (ctx) => {
    const active = activeMatchIds(ctx);
    for (const r of [...ctx.db.research.iter()]) {
      if (r.done) continue;
      // A research row carries no matchId of its own — its scope is its owner's
      // player row. Freeze the bar while that match is paused/ended.
      const owner = ctx.db.player.identity.find(r.owner);
      if (!owner || !active.has(owner.matchId)) continue;
      const up = UPGRADE_DEFS[r.tech as TechT];
      if (!up) continue;
      // researchTime is in seconds; a single tick adds DT/time of the bar.
      const step = up.researchTime > 0 ? RESEARCH_DT / up.researchTime : 1;
      const progress = r.progress + step;
      if (progress < 1) {
        ctx.db.research.researchId.update({ ...r, progress });
        continue;
      }
      // Completed: stamp the bit onto the owner's mask, then mark the row done.
      const p = ctx.db.player.identity.find(r.owner);
      if (p) {
        ctx.db.player.identity.update({
          ...p,
          techMask: setTech(p.techMask, r.tech as TechT),
        });
      }
      ctx.db.research.researchId.update({ ...r, progress: 1, done: true });
    }
  }
);

scheduleRefs.researchSystem = researchSystem;
