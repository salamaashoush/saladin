import { AI_BRAIN_DT } from '../../../shared/constants.ts';
import { UNIT_DEFS, BUILDING_DEFS, AI_PROFILES } from '../../../shared/defs.ts';
import {
  UnitKind,
  BuildingKind,
  GatherState,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { spacetimedb } from '../schema/db.ts';
import { aiBrainTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { popInfo, assignIdleGatherers, trainFrom } from '../world/economy.ts';
import { aiFindSpot, placeFor, movePatch } from '../world/placement.ts';
import { nearestEnemyKeep } from '../world/commands.ts';

// Skirmish AI — runs every AI_BRAIN_TICK_MS. Each bot keeps peasants gathering,
// follows a build-order priority (economy → military → defense), and launches
// attack waves. It only calls the same owner-parameterized command logic a human
// goes through; no special powers.
export const aiBrain = spacetimedb.reducer(
  { timer: aiBrainTimer.rowType },
  (ctx) => {
    for (const bot of [...ctx.db.ai.iter()]) {
      const p = ctx.db.player.identity.find(bot.identity);
      if (!p || p.defeated) continue;
      const owner = bot.identity;
      const prof = AI_PROFILES[bot.difficulty] ?? AI_PROFILES[1];

      const keep = ctx.db.building.entityId.find(p.keepEntity);
      const ke = keep ? ctx.db.entity.entityId.find(keep.entityId) : null;
      if (!keep || !ke) continue; // keep gone — combatTick marks it defeated

      // Census of this bot's holdings.
      const myUnits = [...ctx.db.unit.iter()].filter((u) =>
        u.owner.equals(owner)
      );
      const peasants = myUnits.filter((u) => u.kind === UnitKind.Peasant).length;
      const soldiers = myUnits.filter(
        (u) => (UNIT_DEFS[u.kind as UnitKindT]?.attack ?? 0) > 0
      );
      const myBuildings = [...ctx.db.building.iter()].filter((b) =>
        b.owner.equals(owner)
      );
      const barracks = myBuildings.find((b) => b.kind === BuildingKind.Barracks);
      const towers = myBuildings.filter(
        (b) => b.kind === BuildingKind.Tower
      ).length;
      const pop = popInfo(ctx, owner);

      // Keep the economy busy every tick.
      assignIdleGatherers(ctx, owner);

      // One macro action per decision window.
      let decisionCd = bot.decisionCd - AI_BRAIN_DT;
      if (decisionCd <= 0) {
        decisionCd = 1.0;
        if (peasants < prof.peasantTarget && pop.pop < pop.cap) {
          trainFrom(ctx, owner, keep, UnitKind.Peasant);
        } else if (
          pop.cap - pop.pop <= 1 &&
          p.wood >= BUILDING_DEFS[BuildingKind.House].cost
        ) {
          const s = aiFindSpot(ctx, BuildingKind.House, ke.x, ke.y);
          if (s) placeFor(ctx, owner, BuildingKind.House, s.x, s.y);
        } else if (
          !barracks &&
          p.wood >= BUILDING_DEFS[BuildingKind.Barracks].cost
        ) {
          const s = aiFindSpot(ctx, BuildingKind.Barracks, ke.x, ke.y);
          if (s) placeFor(ctx, owner, BuildingKind.Barracks, s.x, s.y);
        } else if (
          barracks &&
          soldiers.length < prof.armyTarget &&
          pop.pop < pop.cap
        ) {
          const roll = ctx.random();
          const kind =
            roll < prof.knightRatio
              ? UnitKind.Knight
              : roll < prof.knightRatio + prof.archerRatio
                ? UnitKind.Archer
                : UnitKind.Spearman;
          trainFrom(ctx, owner, barracks, kind);
        } else if (
          towers < prof.maxTowers &&
          p.wood >= BUILDING_DEFS[BuildingKind.Tower].cost + prof.woodBuffer
        ) {
          const s = aiFindSpot(ctx, BuildingKind.Tower, ke.x, ke.y);
          if (s) placeFor(ctx, owner, BuildingKind.Tower, s.x, s.y);
        }
      }

      // Assault: once an army is mustered and the timer is up, throw everyone at
      // the nearest enemy keep. movePatch routes them; combat auto-aggro fights.
      let waveTimer = bot.waveTimer - AI_BRAIN_DT;
      if (soldiers.length >= prof.waveSize && waveTimer <= 0) {
        const target = nearestEnemyKeep(ctx, owner, ke.x, ke.y);
        if (target) {
          for (const s of soldiers) {
            const su = ctx.db.unit.entityId.find(s.entityId);
            const se = ctx.db.entity.entityId.find(s.entityId);
            if (!su || !se) continue;
            ctx.db.unit.entityId.update({
              ...su,
              attackTarget: target.id,
              gatherState: GatherState.Idle,
              targetNode: 0n,
              ...movePatch(ctx, se.x, se.y, target.x, target.y),
            });
          }
          waveTimer = prof.waveInterval;
        }
      }

      ctx.db.ai.identity.update({ ...bot, decisionCd, waveTimer });
    }
  }
);

scheduleRefs.aiBrain = aiBrain;
