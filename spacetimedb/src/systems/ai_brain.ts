import { AI_BRAIN_DT, FOOD_PER_UNIT } from '../../../shared/constants.ts';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
  AI_PROFILES,
  plannerTuning,
} from '../../../shared/defs.ts';
import { canAfford } from '../../../shared/economy.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
  GatherState,
  type UnitKind as UnitKindT,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import {
  AiPhase,
  nextPhase,
  nextBuild,
  foodCrisis,
  type PlannerState,
  type UnitCensus,
} from '../../../shared/ai.ts';
import { spacetimedb } from '../schema/db.ts';
import { aiBrainTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { assignIdleGatherers, trainFrom } from '../world/economy.ts';
import { aiFindSpot, placeFor, movePatch } from '../world/placement.ts';
import { nearestEnemyKeep } from '../world/commands.ts';
import { dist } from '../world/util.ts';

const isCombat = (kind: number): boolean =>
  (UNIT_DEFS[kind as UnitKindT]?.attack ?? 0) > 0;
const isSiege = (kind: number): boolean =>
  !!UNIT_DEFS[kind as UnitKindT]?.prefersBuildings;

const HOME_THREAT_RADIUS = 24; // enemy combatants this close to the keep = threat

// Strategic skirmish AI. Each tick it builds ONE shared snapshot of the world
// (positions, every unit, every building) and then, per bot, derives a planner
// state from that snapshot — no nested table scans — and drives the pure planner
// in shared/ai.ts: nextPhase decides posture, nextBuild decides the single macro
// action, counterComposition (inside nextBuild) picks the unit that best answers
// the enemy's mix. It executes only through the SAME owner-parameterized helpers a
// human's reducers call (trainFrom / placeFor): no resource cheats, no vision
// cheats, no special powers.
export const aiBrain = spacetimedb.reducer(
  { timer: aiBrainTimer.rowType },
  (ctx) => {
    const bots = [...ctx.db.ai.iter()];
    if (bots.length === 0) return;

    // ── one snapshot per tick, hoisted out of the bot loop ──────────────────
    const pos = new Map<bigint, { x: number; y: number }>();
    for (const e of [...ctx.db.entity.iter()]) pos.set(e.entityId, e);
    const allUnits = [...ctx.db.unit.iter()];
    const allBuildings = [...ctx.db.building.iter()];
    // Owner identity (hex) → faction, so the per-bot enemy census never re-scans
    // the player table per unit/building.
    const factionOf = new Map<string, number>();
    for (const pl of [...ctx.db.player.iter()])
      factionOf.set(pl.identity.toHexString(), pl.faction);
    const facOf = (id: any): number | undefined =>
      factionOf.get(id.toHexString());

    for (const bot of bots) {
      const p = ctx.db.player.identity.find(bot.identity);
      if (!p || p.defeated) continue;
      const owner = bot.identity;
      const prof = AI_PROFILES[bot.difficulty] ?? AI_PROFILES[1];
      const tune = plannerTuning(prof);

      const keep = allBuildings.find(
        (b) => b.owner.equals(owner) && b.kind === BuildingKind.Keep
      );
      const ke = keep ? pos.get(keep.entityId) : null;
      if (!keep || !ke) continue; // keep gone — combatTick marks it defeated

      // ── census from the hoisted snapshot (no nested table iteration) ───────
      const myUnits = allUnits.filter((u) => u.owner.equals(owner));
      const armyComposition: UnitCensus = {};
      let peasants = 0;
      let soldiers = 0;
      let sieges = 0;
      for (const u of myUnits) {
        if (u.kind === UnitKind.Peasant) peasants++;
        if (isCombat(u.kind) || u.kind === UnitKind.Imam)
          armyComposition[u.kind] = (armyComposition[u.kind] ?? 0) + 1;
        if (isCombat(u.kind)) soldiers++;
        if (isSiege(u.kind)) sieges++;
      }

      const owned = new Set<BuildingKindT>();
      let towers = 0;
      let cap = 0;
      for (const b of allBuildings) {
        if (!b.owner.equals(owner)) continue;
        owned.add(b.kind as BuildingKindT);
        if (b.kind === BuildingKind.Tower) towers++;
        cap += (
          BUILDING_DEFS[b.kind as BuildingKindT] ?? BUILDING_DEFS[BuildingKind.Keep]
        ).pop;
      }
      const pop = myUnits.length;

      // Enemy census + wall detection + threat near home, all from the snapshot.
      // Hostile = any player on the opposing faction (the human or a rival bot).
      const enemy: UnitCensus = {};
      let threatNearHome = 0;
      for (const u of allUnits) {
        if (u.owner.equals(owner)) continue;
        const fac = facOf(u.owner);
        if (fac === undefined || fac === p.faction) continue;
        if (!isCombat(u.kind)) continue;
        enemy[u.kind] = (enemy[u.kind] ?? 0) + 1;
        const e = pos.get(u.entityId);
        if (e && dist(e.x, e.y, ke.x, ke.y) <= HOME_THREAT_RADIUS)
          threatNearHome++;
      }
      let enemyHasWalls = false;
      for (const b of allBuildings) {
        if (b.owner.equals(owner)) continue;
        const fac = facOf(b.owner);
        if (fac === undefined || fac === p.faction) continue;
        if (b.kind === BuildingKind.Wall || b.kind === BuildingKind.Gatehouse) {
          enemyHasWalls = true;
          break;
        }
      }

      const upkeep = soldiers * FOOD_PER_UNIT;

      const state: PlannerState = {
        peasants,
        pop,
        cap,
        food: p.food,
        wood: p.wood,
        stone: p.stone,
        gold: p.gold,
        upkeep,
        soldiers,
        armyComposition,
        sieges,
        towers,
        owned,
        enemy,
        enemyHasWalls,
        threatNearHome,
      };

      // ── economy: keep gatherers busy, steered to what the bot is short of ──
      // In a food crisis the army is out-eating the larder: forcibly pull EVERY
      // gatherer off other resources onto food (idle-reassign only moves idle
      // ones, which is too slow when food has already collapsed) so the shortfall
      // is closed before the army starves. Drop carried loads to food so the trip
      // banks immediately. Otherwise steer food-first when thin, then to the
      // scarcer building resource, else nearest node.
      const crisis = foodCrisis(state, tune);
      if (crisis) {
        for (const u of myUnits) {
          if (u.kind !== UnitKind.Peasant) continue;
          if (u.carryType === ResourceType.Food && u.gatherState !== GatherState.Idle)
            continue; // already working food
          ctx.db.unit.entityId.update({
            ...u,
            gatherState: GatherState.Idle,
            targetNode: 0n,
          });
        }
      }
      const prefer =
        crisis || p.food <= Math.max(1, upkeep) * tune.foodFloorMult
          ? ResourceType.Food
          : p.stone < p.wood
            ? ResourceType.Stone
            : undefined;
      assignIdleGatherers(ctx, owner, prefer);

      // ── phase + one macro decision per profile-paced window ───────────────
      const phase = nextPhase(state, tune);
      let decisionCd = bot.decisionCd - AI_BRAIN_DT;
      if (decisionCd <= 0) {
        decisionCd = prof.decisionInterval;
        const plan = nextBuild(state, tune);
        if (plan) {
          if (plan.isUnit) {
            const trainer = allBuildings.find(
              (b) =>
                b.owner.equals(owner) &&
                b.kind === (plan.trainer ?? BuildingKind.Keep)
            );
            if (trainer && pop < cap) trainFrom(ctx, owner, trainer, plan.kind);
          } else {
            // Defensive towers keep a wood reserve; structural buildings just
            // need to be affordable. placeFor re-checks afford + tech.
            const def = BUILDING_DEFS[plan.kind as BuildingKindT];
            const cost =
              plan.kind === BuildingKind.Tower
                ? { ...def.cost, wood: (def.cost.wood ?? 0) + tune.woodBuffer }
                : def.cost;
            if (canAfford(p, cost)) {
              const s = aiFindSpot(ctx, plan.kind, ke.x, ke.y);
              if (s) placeFor(ctx, owner, plan.kind, s.x, s.y);
            }
          }
        }
      }

      // ── threat timer (debug/telemetry) ────────────────────────────────────
      const threatTimer =
        threatNearHome > 0 ? bot.threatTimer + AI_BRAIN_DT : 0;

      // ── assault: muster then march on the nearest enemy keep ──────────────
      // Hold the army home while Defending; otherwise push when a wave is ready.
      let waveTimer = bot.waveTimer - AI_BRAIN_DT;
      const wantsAssault =
        phase !== AiPhase.Defend && soldiers >= prof.waveSize && waveTimer <= 0;
      if (wantsAssault) {
        const target = nearestEnemyKeep(ctx, owner, ke.x, ke.y);
        if (target) {
          for (const u of myUnits) {
            if (!isCombat(u.kind) && u.kind !== UnitKind.Imam) continue;
            const su = ctx.db.unit.entityId.find(u.entityId);
            const se = pos.get(u.entityId);
            if (!su || !se || su.routing) continue;
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

      ctx.db.ai.identity.update({
        ...bot,
        decisionCd,
        waveTimer,
        phase,
        threatTimer,
      });
    }
  }
);

scheduleRefs.aiBrain = aiBrain;
