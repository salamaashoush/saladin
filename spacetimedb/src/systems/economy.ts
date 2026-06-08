import { ECONOMY_DT } from '../../../shared/constants.ts';
import { applyUpkeep } from '../../../shared/economy.ts';
import { UNIT_DEFS } from '../../../shared/defs.ts';
import { spacetimedb } from '../schema/db.ts';
import { economyTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';

// Economy upkeep — runs every ECONOMY_TICK_MS. Only COMBAT units draw rations;
// peasants and imams feed themselves, so a worker-only opening never starves and
// food instead caps how large an ARMY a player can sustain (real RTS pressure).
// A player whose stockpile runs dry starves and their soldiers bleed hp until
// fed. Defeated players are skipped. HP is written ONLY when it changes, so a
// well-fed army never touches the hot unit table.
export const economySystem = spacetimedb.reducer(
  { timer: economyTimer.rowType },
  (ctx) => {
    for (const p of [...ctx.db.player.iter()]) {
      if (p.defeated) continue;
      const eaters = [...ctx.db.unit.iter()].filter(
        (u) =>
          u.owner.equals(p.identity) && (UNIT_DEFS[u.kind as 0]?.attack ?? 0) > 0
      );
      const { food, starving, hpDrain } = applyUpkeep(
        p.food,
        eaters.length,
        ECONOMY_DT
      );
      if (food !== p.food) ctx.db.player.identity.update({ ...p, food });
      if (!starving || hpDrain <= 0) continue;
      for (const u of eaters) {
        const hp = Math.max(0, u.hp - hpDrain);
        if (hp !== u.hp) ctx.db.unit.entityId.update({ ...u, hp });
      }
    }
  }
);

scheduleRefs.economySystem = economySystem;
