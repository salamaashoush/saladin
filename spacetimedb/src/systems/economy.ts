import { ECONOMY_DT } from '../../../shared/constants.ts';
import { applyUpkeep } from '../../../shared/economy.ts';
import { spacetimedb } from '../schema/db.ts';
import { economyTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';

// Economy upkeep — runs every ECONOMY_TICK_MS. Each player's units eat food; a
// player whose stockpile runs dry starves, and their units bleed hp until fed.
// Defeated players are skipped. HP is written ONLY when it actually changes, so a
// well-fed army never touches the hot unit table.
export const economySystem = spacetimedb.reducer(
  { timer: economyTimer.rowType },
  (ctx) => {
    for (const p of [...ctx.db.player.iter()]) {
      if (p.defeated) continue;
      const owned = [...ctx.db.unit.iter()].filter((u) =>
        u.owner.equals(p.identity)
      );
      const { food, starving, hpDrain } = applyUpkeep(
        p.food,
        owned.length,
        ECONOMY_DT
      );
      if (food !== p.food) ctx.db.player.identity.update({ ...p, food });
      if (!starving || hpDrain <= 0) continue;
      for (const u of owned) {
        const hp = Math.max(0, u.hp - hpDrain);
        if (hp !== u.hp) ctx.db.unit.entityId.update({ ...u, hp });
      }
    }
  }
);

scheduleRefs.economySystem = economySystem;
