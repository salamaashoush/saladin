import { ECONOMY_DT } from '../../../shared/constants.ts';
import { applyUpkeep } from '../../../shared/economy.ts';
import { UNIT_DEFS } from '../../../shared/defs.ts';
import { spacetimedb } from '../schema/db.ts';
import { economyTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { removeUnit } from '../world/garrison.ts';
import { activeMatchIds } from '../world/scope.ts';

// Economy upkeep — runs every ECONOMY_TICK_MS. Only COMBAT units draw rations;
// peasants and imams feed themselves, so a worker-only opening never starves and
// food instead caps how large an ARMY a player can sustain (real RTS pressure).
// A player whose stockpile runs dry starves and their soldiers bleed hp until
// fed. Defeated players are skipped. HP is written ONLY when it changes, so a
// well-fed army never touches the hot unit table.
export const economySystem = spacetimedb.reducer(
  { timer: economyTimer.rowType },
  (ctx) => {
    const active = activeMatchIds(ctx);
    // Players by match index (Rank 1) and each player's eaters by the owner index —
    // O(players × that player's units), not O(players × all units in all matches)
    // (docs/STDB_PERF.md §3 Rank 1).
    for (const mid of active)
    for (const p of ctx.db.player.matchId.filter(mid)) {
      if (p.defeated) continue;
      const eaters = [...ctx.db.unit.owner.filter(p.identity)].filter(
        (u) => (UNIT_DEFS[u.kind as 0]?.attack ?? 0) > 0
      );
      const { food, starving, hpDrain } = applyUpkeep(
        p.food,
        eaters.length,
        ECONOMY_DT
      );
      if (food !== p.food) ctx.db.player.identity.update({ ...p, food });
      if (!starving || hpDrain <= 0) continue;
      // Drain hp; a unit whose rations run out entirely starves to DEATH and is
      // removed (slot + unit + entity). Without this, zero-hp soldiers linger and
      // keep eating, deadlocking the larder so the army can never recover.
      for (const u of eaters) {
        const hp = Math.max(0, u.hp - hpDrain);
        if (hp === u.hp) continue;
        if (hp <= 0) removeUnit(ctx, u.entityId);
        else ctx.db.unit.entityId.update({ ...u, hp });
      }
    }
  }
);

scheduleRefs.economySystem = economySystem;
