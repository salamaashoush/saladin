import { AI_BRAIN_DT, FOOD_PER_UNIT } from '../../../shared/constants.ts';
import { UNIT_DEFS, BUILDING_DEFS, AI_PROFILES } from '../../../shared/defs.ts';
import { canAfford } from '../../../shared/economy.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
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
      const stable = myBuildings.find((b) => b.kind === BuildingKind.Stable);
      const blacksmith = myBuildings.find(
        (b) => b.kind === BuildingKind.Blacksmith
      );
      const siegeWorkshop = myBuildings.find(
        (b) => b.kind === BuildingKind.SiegeWorkshop
      );
      const towers = myBuildings.filter(
        (b) => b.kind === BuildingKind.Tower
      ).length;
      const sieges = myUnits.filter(
        (u) => UNIT_DEFS[u.kind as UnitKindT]?.prefersBuildings
      ).length;
      const pop = popInfo(ctx, owner);

      // Keep the economy busy every tick, steering toward what the bot is short
      // of: food first if starvation looms (it must out-gather upkeep), then the
      // scarcest of the raw building resources, otherwise the nearest node.
      const upkeep = myUnits.length * FOOD_PER_UNIT;
      const prefer =
        p.food <= upkeep * 4
          ? ResourceType.Food
          : p.stone < p.wood
            ? ResourceType.Stone
            : undefined;
      assignIdleGatherers(ctx, owner, prefer);

      const wantsCavalry = prof.cavalryRatio > 0;
      const wantsSiege = prof.siegeTarget > 0;
      const placeNear = (kind: BuildingKind) => {
        if (!canAfford(p, BUILDING_DEFS[kind].cost)) return false;
        const s = aiFindSpot(ctx, kind, ke.x, ke.y);
        if (s) placeFor(ctx, owner, kind, s.x, s.y);
        return true;
      };

      // One macro action per decision window. Follows the tech tree: economy →
      // barracks → stable (cavalry) → blacksmith → siege workshop, mustering an
      // army the whole way. Each branch enforces its own prereqs via placeFor.
      let decisionCd = bot.decisionCd - AI_BRAIN_DT;
      if (decisionCd <= 0) {
        decisionCd = 1.0;
        if (peasants < prof.peasantTarget && pop.pop < pop.cap) {
          trainFrom(ctx, owner, keep, UnitKind.Peasant);
        } else if (
          pop.cap - pop.pop <= 1 &&
          canAfford(p, BUILDING_DEFS[BuildingKind.House].cost)
        ) {
          placeNear(BuildingKind.House);
        } else if (!barracks) {
          placeNear(BuildingKind.Barracks);
        } else if (wantsCavalry && !stable) {
          placeNear(BuildingKind.Stable);
        } else if (wantsSiege && !blacksmith) {
          placeNear(BuildingKind.Blacksmith);
        } else if (wantsSiege && blacksmith && !siegeWorkshop) {
          placeNear(BuildingKind.SiegeWorkshop);
        } else if (
          siegeWorkshop &&
          sieges < prof.siegeTarget &&
          pop.pop < pop.cap
        ) {
          const kind =
            ctx.random() < 0.5 ? UnitKind.Mangonel : UnitKind.Ram;
          trainFrom(ctx, owner, siegeWorkshop, kind);
        } else if (soldiers.length < prof.armyTarget && pop.pop < pop.cap) {
          // Split production between the barracks (infantry) and the stable
          // (cavalry) by the profile's cavalryRatio.
          const roll = ctx.random();
          if (stable && roll < prof.cavalryRatio) {
            const cav =
              roll < prof.cavalryRatio * 0.4
                ? UnitKind.Mamluk
                : roll < prof.cavalryRatio * 0.7
                  ? UnitKind.Knight
                  : UnitKind.HorseArcher;
            trainFrom(ctx, owner, stable, cav);
          } else {
            const r2 = ctx.random();
            const inf =
              r2 < prof.archerRatio
                ? UnitKind.Archer
                : r2 < prof.archerRatio + prof.knightRatio
                  ? UnitKind.Crossbowman
                  : UnitKind.Spearman;
            trainFrom(ctx, owner, barracks, inf);
          }
        } else if (
          towers < prof.maxTowers &&
          // Keep a wood reserve before optional defensive spends.
          canAfford(p, {
            ...BUILDING_DEFS[BuildingKind.Tower].cost,
            wood:
              (BUILDING_DEFS[BuildingKind.Tower].cost.wood ?? 0) +
              prof.woodBuffer,
          })
        ) {
          placeNear(BuildingKind.Tower);
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
