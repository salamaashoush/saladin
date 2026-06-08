import { AI_BRAIN_DT, FOOD_PER_UNIT } from '../../../shared/constants.ts';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
  AI_PROFILES,
  plannerTuning,
  tacticalTuning,
} from '../../../shared/defs.ts';
import { canAfford } from '../../../shared/economy.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
  GatherState,
  Stance,
  type UnitKind as UnitKindT,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import {
  AiPhase,
  nextPhase,
  nextBuild,
  foodCrisis,
  squadRole,
  targetForRole,
  raidQuota,
  mustered,
  shouldRecall,
  recallCount,
  SquadRole,
  type PlannerState,
  type UnitCensus,
  type ThreatState,
  type AssaultIntel,
} from '../../../shared/ai.ts';
import { spacetimedb } from '../schema/db.ts';
import { aiBrainTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { assignIdleGatherers, trainFrom } from '../world/economy.ts';
import { startResearchFor } from '../world/research.ts';
import { aiFindSpot, placeFor, movePatch } from '../world/placement.ts';
import { nearestEnemyKeep, assaultIntel } from '../world/commands.ts';
import { dist } from '../world/util.ts';

const isCombat = (kind: number): boolean =>
  (UNIT_DEFS[kind as UnitKindT]?.attack ?? 0) > 0;
const isSiege = (kind: number): boolean =>
  !!UNIT_DEFS[kind as UnitKindT]?.prefersBuildings;

const HOME_THREAT_RADIUS = 24; // enemy combatants this close to home = a threat
const HOME_RADIUS = 18; // own combat units this close to a building count as "home"

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
      const tac = tacticalTuning(prof);

      const keep = allBuildings.find(
        (b) => b.owner.equals(owner) && b.kind === BuildingKind.Keep
      );
      const ke = keep ? pos.get(keep.entityId) : null;
      if (!keep || !ke) continue; // keep gone — combatTick marks it defeated

      // Positions of every owned building — threat is measured against ALL of them
      // (a tower or barracks under attack is still an attack on home), not the keep
      // alone, so the bot reacts to a base raid even away from its keep.
      const ownedBuildingPos: { x: number; y: number }[] = [];
      for (const b of allBuildings) {
        if (!b.owner.equals(owner)) continue;
        const e = pos.get(b.entityId);
        if (e) ownedBuildingPos.push(e);
      }

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
      // Threat = an enemy combatant within HOME_THREAT_RADIUS of ANY owned building.
      const enemy: UnitCensus = {};
      let threatNearHome = 0;
      const nearAnyOwnedBuilding = (x: number, y: number): boolean => {
        for (const b of ownedBuildingPos)
          if (dist(x, y, b.x, b.y) <= HOME_THREAT_RADIUS) return true;
        return false;
      };
      for (const u of allUnits) {
        if (u.owner.equals(owner)) continue;
        const fac = facOf(u.owner);
        if (fac === undefined || fac === p.faction) continue;
        if (!isCombat(u.kind)) continue;
        enemy[u.kind] = (enemy[u.kind] ?? 0) + 1;
        const e = pos.get(u.entityId);
        if (e && nearAnyOwnedBuilding(e.x, e.y)) threatNearHome++;
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

      // ── economy: steer gatherers to what the bot is short of ──────────────
      // Two distinct levers, deliberately not conflated:
      //
      //  (a) idle assignment — what NEW idle peasants pick up, in three tiers:
      //      FOOD emergency (crisis, or food below the upkeep*foodFloorMult cushion
      //      while an army eats) → everyone on food until the larder recovers.
      //      Food in deep SURPLUS (well past the cushion) → the scarcer BUILDING
      //      resource, because a balanced round-robin keeps ~¼ of workers piling
      //      onto food the bot can't spend while wood/stone — what it actually
      //      builds with — stays starved. Otherwise → balanced food-first.
      //
      //  (b) committed re-steer — assignIdleGatherers only moves IDLE peasants, so
      //      a worker locked onto a fat node mines it for many trips and a bias
      //      never takes hold (a bot sits on 200 stone / 20 wood for minutes). So
      //      we also pull a FEW already-committed peasants off the glut resource
      //      onto the scarce one. Food emergency pulls everyone; otherwise a capped
      //      handful so income on the others holds. The scout stays out scouting.
      const crisis = foodCrisis(state, tune);
      // Cushion = a working reserve scaled by the army's appetite: enough that the
      // larder never crashes between decisions (an army out-eats the slow gather
      // round-robin), but not so deep the bot hoards food and never banks wood to
      // tech. ~a dozen seconds of upkeep plus a flat floor strikes the balance.
      const cushion = 40 + upkeep * tune.foodFloorMult * 2;
      const foodEmergency = crisis || p.food <= cushion;
      // Only call food "surplus" (free to pour labour into the build bottleneck)
      // when it's deep past the cushion AND there's no army eating into it — never
      // strip food workers while soldiers are in the field, that was the starve bug.
      const foodSurplus =
        !foodEmergency && upkeep === 0 && p.food > cushion + 200;
      const scarceBuild =
        p.wood <= p.stone ? ResourceType.Wood : ResourceType.Stone;

      const idleBias = foodEmergency
        ? ResourceType.Food
        : foodSurplus
          ? scarceBuild
          : undefined;

      // What resource a peasant is CURRENTLY working, by its target NODE's type
      // (carryType lags — it holds the last DEPOSITED load, so a peasant harvesting
      // a food node still reads as its previous wood trip). Re-steering on that lag
      // was the starve bug: it kept resetting peasants who were already on food.
      const nodeType = (u: any): number | null => {
        if (u.targetNode === 0n) return null;
        const n = ctx.db.resourceNode.entityId.find(u.targetNode);
        return n ? n.resType : null;
      };
      // Pull peasants OFF a resource and idle them so they reassign to `want`.
      // Skips: the scout, idle ones, anyone whose load is in transit to the
      // stockpile (let the trip bank), and — crucially — anyone whose TARGET NODE
      // already matches `want` (never interrupt a peasant already working it). Drop
      // a mid-harvest load only when switching to a different resource.
      const steerTo = (want: number, fromTypes: number[] | null, max: number) => {
        let n = 0;
        for (const u of myUnits) {
          if (n >= max) break;
          if (u.kind !== UnitKind.Peasant) continue;
          if (u.entityId === bot.scoutId) continue;
          if (u.gatherState === GatherState.Idle) continue;
          if (u.gatherState === GatherState.ToStockpile) continue;
          const nt = nodeType(u);
          if (nt === want) continue; // already working the wanted resource
          if (fromTypes !== null && (nt === null || !fromTypes.includes(nt)))
            continue; // only pull off the named glut resource(s)
          ctx.db.unit.entityId.update({
            ...u,
            gatherState: GatherState.Idle,
            targetNode: 0n,
          });
          n++;
        }
      };
      if (foodEmergency) {
        // pull EVERY non-food gatherer onto food
        steerTo(ResourceType.Food, null, peasants);
      } else if (foodSurplus) {
        steerTo(scarceBuild, [ResourceType.Food], 3);
      } else if (Math.abs(p.wood - p.stone) > 80) {
        const glut = p.wood > p.stone ? ResourceType.Wood : ResourceType.Stone;
        steerTo(scarceBuild, [glut], 3);
      }
      assignIdleGatherers(ctx, owner, idleBias);

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

        // ── research: start the highest-priority Blacksmith tech the bot can
        //    afford. Runs through the SAME startResearchFor a human uses — it
        //    rejects already-done/in-flight/unaffordable/prereq-missing techs and
        //    only succeeds (returns null) on a real start, so this is no cheat:
        //    the bot pays full cost and waits out the research timer. One start
        //    per decision window keeps the spend paced like the rest of the macro.
        if (prof.research.length > 0) {
          const smith = allBuildings.find(
            (b) => b.owner.equals(owner) && b.kind === BuildingKind.Blacksmith
          );
          if (smith) {
            for (const tech of prof.research) {
              if (startResearchFor(ctx, owner, smith, tech) === null) break;
            }
          }
        }
      }

      // ── threat timer: seconds of SUSTAINED threat near home ───────────────
      // Recall reacts only once the threat has held for the profile's react delay
      // (Hard ~1s, Easy ~6s) — that's reaction SPEED, not a cheat. Resets the
      // instant the base is clear so a fleeting probe doesn't pull the army back.
      const threatTimer =
        threatNearHome > 0 ? bot.threatTimer + AI_BRAIN_DT : 0;

      // The fielded combat units: live rows + position, classified by squad role.
      // "Home" units are those already near an owned building (the standing
      // garrison); the rest are the field army that can be recalled or sent out.
      interface FieldUnit {
        row: any;
        x: number;
        y: number;
        role: SquadRole;
        atHome: boolean;
      }
      const army: FieldUnit[] = [];
      for (const u of myUnits) {
        if (!isCombat(u.kind) && u.kind !== UnitKind.Imam) continue;
        const su = ctx.db.unit.entityId.find(u.entityId);
        const se = pos.get(u.entityId);
        if (!su || !se || su.routing) continue; // routing units flee on their own
        let atHome = false;
        for (const b of ownedBuildingPos)
          if (dist(se.x, se.y, b.x, b.y) <= HOME_RADIUS) {
            atHome = true;
            break;
          }
        army.push({ row: su, x: se.x, y: se.y, role: squadRole(u.kind), atHome });
      }

      const recallHome = (fu: FieldUnit) => {
        ctx.db.unit.entityId.update({
          ...fu.row,
          attackTarget: 0n,
          stance: Stance.Defensive,
          gatherState: GatherState.Idle,
          targetNode: 0n,
          homeX: ke.x,
          homeY: ke.y,
          ...movePatch(ctx, fu.x, fu.y, ke.x, ke.y),
        });
      };

      // ── defensive recall: pull part of the field army home under sustained
      //    attack. shouldRecall/recallCount (shared/ai.ts) make the QUALITY call;
      //    threatTimer gates the SPEED. Closest field units come back first so the
      //    relief arrives soonest; when the threat clears, the regular assault
      //    block below resumes the plan. Units already home stay and defend.
      const fieldArmy = army.filter((a) => !a.atHome);
      const homeArmy = army.length - fieldArmy.length;
      const th: ThreatState = {
        attackers: threatNearHome,
        fieldArmy: fieldArmy.length,
        homeArmy,
      };
      const underAttack =
        threatTimer >= tac.defendReactDelay && shouldRecall(th, tac);
      if (underAttack) {
        const n = recallCount(th, tac);
        const byClosest = [...fieldArmy].sort(
          (a, b) =>
            dist(a.x, a.y, ke.x, ke.y) - dist(b.x, b.y, ke.x, ke.y)
        );
        for (let i = 0; i < n && i < byClosest.length; i++)
          recallHome(byClosest[i]);
      }

      // ── assault: muster to waveSize, then march squads onto role targets ──
      // Hold while Defending or while a recall is in progress; otherwise commit a
      // full mustered wave at once (never tiny dribbles). Siege leads onto the
      // fortifications, the main body besieges the keep, and a fraction of the
      // light cavalry peels off to raid the enemy economy.
      let waveTimer = bot.waveTimer - AI_BRAIN_DT;
      const wantsAssault =
        phase !== AiPhase.Defend &&
        !underAttack &&
        mustered(soldiers, prof.waveSize) &&
        waveTimer <= 0;
      let launched = false;
      if (wantsAssault) {
        const isEnemy = (id: any): boolean => {
          const fac = facOf(id);
          return fac !== undefined && fac !== p.faction;
        };
        const intel: AssaultIntel = assaultIntel(
          allUnits,
          allBuildings,
          pos,
          isEnemy,
          ke.x,
          ke.y
        );
        if (intel.keep || intel.buildings.length > 0) {
          // Carve the fastest raider-class units off to harass; the rest of the
          // raiders fold back into the main body so the assault keeps its punch.
          const raiders = army
            .filter((a) => a.role === SquadRole.Raider)
            .sort(
              (a, b) =>
                (UNIT_DEFS[b.row.kind as UnitKindT]?.speed ?? 0) -
                (UNIT_DEFS[a.row.kind as UnitKindT]?.speed ?? 0)
            );
          const raids = raidQuota(raiders.length, tac.raidFraction);
          const raidSet = new Set<bigint>();
          for (let i = 0; i < raids; i++) raidSet.add(raiders[i].row.entityId);

          for (const fu of army) {
            const raiding = raidSet.has(fu.row.entityId);
            // A raider not picked for the raid marches as Main so the assault keeps
            // its punch; siege keeps its Siege role; everything else is Main.
            const effRole = raiding
              ? SquadRole.Raider
              : fu.role === SquadRole.Raider
                ? SquadRole.Main
                : fu.role;
            const target =
              targetForRole(effRole, fu.x, fu.y, intel) ?? intel.keep;
            if (!target) continue;
            ctx.db.unit.entityId.update({
              ...fu.row,
              attackTarget: target.id,
              stance: Stance.Aggressive,
              gatherState: GatherState.Idle,
              targetNode: 0n,
              ...movePatch(ctx, fu.x, fu.y, target.x, target.y),
            });
          }
          waveTimer = prof.waveInterval;
          launched = true;
        }
      }

      // ── scouting (Hard): once early on, send the cheapest expendable unit
      //    toward the enemy so the bot reacts to the real map, not just its home
      //    sightlines. Deterministic: lowest-id idle peasant, sent to the nearest
      //    enemy keep. Lower difficulties don't scout (tac.scouts false).
      let scoutId = bot.scoutId;
      const scoutAlive = scoutId !== 0n && ctx.db.unit.entityId.find(scoutId);
      if (tac.scouts && !scoutAlive && !launched) {
        const target = nearestEnemyKeep(ctx, owner, ke.x, ke.y);
        if (target) {
          let best: any = null;
          for (const u of myUnits) {
            if (u.kind !== UnitKind.Peasant) continue;
            if (best === null || u.entityId < best.entityId) best = u;
          }
          if (best) {
            const se = pos.get(best.entityId);
            const su = ctx.db.unit.entityId.find(best.entityId);
            if (se && su) {
              ctx.db.unit.entityId.update({
                ...su,
                gatherState: GatherState.Idle,
                targetNode: 0n,
                ...movePatch(ctx, se.x, se.y, target.x, target.y),
              });
              scoutId = best.entityId;
            }
          }
        }
      } else if (scoutId !== 0n && !scoutAlive) {
        scoutId = 0n; // scout died — clear so a fresh one can go out later
      }

      ctx.db.ai.identity.update({
        ...bot,
        decisionCd,
        waveTimer,
        phase,
        threatTimer,
        scoutId,
      });
    }
  }
);

scheduleRefs.aiBrain = aiBrain;
