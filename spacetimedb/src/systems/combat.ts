import { COMBAT_DT } from '../../../shared/constants.ts';
import { UNIT_DEFS, BUILDING_DEFS } from '../../../shared/defs.ts';
import {
  UnitKind,
  BuildingKind,
  Stance,
  type UnitKind as UnitKindT,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import {
  applyDamage,
  nearestWithin,
  type Located,
} from '../../../shared/sim.ts';
import {
  effectiveDamage,
  combatAction,
  acquireTarget,
  DEFENSIVE_LEASH,
} from '../../../shared/combat.ts';
import {
  moraleAfterHit,
  moraleRecover,
  isRouting,
  MORALE_MAX,
} from '../../../shared/morale.ts';
import {
  elevation,
  elevationRangeBonus,
  ELEV_BONUS_MAX,
} from '../../../shared/elevation.ts';
import { garrisonFirePower } from '../../../shared/garrison.ts';
import { spacetimedb } from '../schema/db.ts';
import { combatTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { dist, getSeed } from '../world/util.ts';
import { movePatch } from '../world/placement.ts';
import { markDefeated } from '../world/spawn.ts';
import {
  occupantFireProfile,
  garrisonRangeRate,
  evacuateOnDeath,
} from '../world/garrison.ts';

// Live enemy units (as Located) for target acquisition. `units` is the tick
// snapshot; each is re-fetched so a unit already killed this tick is excluded.
// Shared by the soldier and tower loops so acquisition logic lives in one place.
function enemyUnitsAround(
  ctx: any,
  units: any[],
  owner: any,
  selfId: bigint
): Located[] {
  const out: Located[] = [];
  for (const o of units) {
    if (o.entityId === selfId || o.owner.equals(owner)) continue;
    const fresh = ctx.db.unit.entityId.find(o.entityId);
    if (!fresh) continue; // dead this tick
    if (fresh.garrisonedIn !== 0n) continue; // sheltered — not a field target
    const oe = ctx.db.entity.entityId.find(o.entityId);
    if (oe) out.push({ id: o.entityId, x: oe.x, y: oe.y });
  }
  return out;
}

// Live enemy buildings (as Located). Siege engines (prefersBuildings) auto-
// acquire these so they hammer structures rather than chasing soft targets.
function enemyBuildingsAround(ctx: any, owner: any): Located[] {
  const out: Located[] = [];
  for (const b of [...ctx.db.building.iter()]) {
    if (b.owner.equals(owner)) continue;
    const be = ctx.db.entity.entityId.find(b.entityId);
    if (be) out.push({ id: b.entityId, x: be.x, y: be.y });
  }
  return out;
}

// Radius within which own units count as "nearby allies" for morale recovery.
const ALLY_RADIUS = 5;

// Count own LIVE units (excluding self) within ALLY_RADIUS of (x,y). Steadies a
// unit's morale — soldiers hold the line better in formation than in isolation.
function nearbyAllies(
  ctx: any,
  units: any[],
  owner: any,
  selfId: bigint,
  x: number,
  y: number
): number {
  let n = 0;
  for (const o of units) {
    if (o.entityId === selfId || !o.owner.equals(owner)) continue;
    const fresh = ctx.db.unit.entityId.find(o.entityId);
    if (!fresh || fresh.garrisonedIn !== 0n) continue; // dead or sheltered
    const oe = ctx.db.entity.entityId.find(o.entityId);
    if (oe && dist(x, y, oe.x, oe.y) <= ALLY_RADIUS) n++;
  }
  return n;
}

// True if (x,y) is within an own keep's steadying presence or an allied Imam's
// morale aura — both let nearby troops recover morale and resist rout faster.
function nearSupport(
  ctx: any,
  units: any[],
  owner: any,
  x: number,
  y: number
): boolean {
  for (const b of [...ctx.db.building.iter()]) {
    if (b.kind !== BuildingKind.Keep || !b.owner.equals(owner)) continue;
    const be = ctx.db.entity.entityId.find(b.entityId);
    if (be && dist(x, y, be.x, be.y) <= ALLY_RADIUS) return true;
  }
  for (const o of units) {
    const aura = UNIT_DEFS[o.kind as UnitKindT]?.moraleAura ?? 0;
    if (aura <= 0 || !o.owner.equals(owner)) continue;
    const fresh = ctx.db.unit.entityId.find(o.entityId);
    if (!fresh || fresh.garrisonedIn !== 0n) continue; // dead or sheltered
    const oe = ctx.db.entity.entityId.find(o.entityId);
    if (oe && dist(x, y, oe.x, oe.y) <= aura) return true;
  }
  return false;
}

// Where a routing unit flees: back toward its posted home, falling through to
// the nearest own keep when home is unset/overrun. Pure-ish — reads tables only.
function routDestination(
  ctx: any,
  owner: any,
  u: any
): { x: number; y: number } {
  let best: { x: number; y: number } | null = null;
  let bestD = Infinity;
  for (const b of [...ctx.db.building.iter()]) {
    if (b.kind !== BuildingKind.Keep || !b.owner.equals(owner)) continue;
    const be = ctx.db.entity.entityId.find(b.entityId);
    if (!be) continue;
    const d = dist(u.homeX, u.homeY, be.x, be.y);
    if (d < bestD) {
      bestD = d;
      best = { x: be.x, y: be.y };
    }
  }
  return best ?? { x: u.homeX, y: u.homeY };
}

// Combat — runs every COMBAT_TICK_MS. Soldiers auto-acquire nearby enemies,
// close to range, and strike on cooldown. Dead units are removed.
export const combatTick = spacetimedb.reducer(
  { timer: combatTimer.rowType },
  (ctx) => {
    const seed = getSeed(ctx);
    const units = [...ctx.db.unit.iter()];
    // Units that took damage this tick: they dent morale here and skip the
    // end-of-tick recovery pass (you don't catch your breath while being hit).
    const hitThisTick = new Set<bigint>();

    // Dent a defender's morale by the fraction of its max hp the blow removed,
    // and remember it was hit so it can't recover morale this same tick.
    const dentMorale = (defId: bigint, oldHp: number, newHp: number) => {
      const du = ctx.db.unit.entityId.find(defId);
      if (!du) return; // building or already dead — no morale
      const ddef = UNIT_DEFS[du.kind as UnitKindT];
      const maxHp = ddef?.maxHp ?? du.hp;
      const dmgFrac = maxHp > 0 ? (oldHp - newHp) / maxHp : 0;
      hitThisTick.add(defId);
      ctx.db.unit.entityId.update({
        ...du,
        morale: moraleAfterHit(du.morale, dmgFrac),
      });
    };

    for (const snap of units) {
      // Re-fetch every iteration: a unit (or its target) killed earlier this
      // same tick must never act, nor be acted on, via a stale snapshot row.
      const u = ctx.db.unit.entityId.find(snap.entityId);
      if (!u) continue;
      if (u.garrisonedIn !== 0n) continue; // sheltered — fights via its host, not here
      const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
      if (def.attack <= 0) continue; // non-combatants never fight

      const e = ctx.db.entity.entityId.find(u.entityId);
      if (!e) continue;

      // Latch routing with hysteresis from this unit's current morale. A routing
      // unit drops its target, suppresses attacks, and flees toward home/keep
      // until it rallies back above RALLY_THRESHOLD.
      const routing = isRouting(u.routing, u.morale);
      if (routing) {
        const dest = routDestination(ctx, u.owner, u);
        ctx.db.unit.entityId.update({
          ...u,
          routing: true,
          attackTarget: 0n,
          attackCooldown: Math.max(0, u.attackCooldown - COMBAT_DT),
          ...(u.hasTarget ? {} : movePatch(ctx, e.x, e.y, dest.x, dest.y)),
        });
        continue;
      }
      // Not routing: ensure the latch is clear so later `...u` spreads carry it.
      u.routing = false;

      let targetId = u.attackTarget;
      // No current target: auto-acquire via the shared priority rule. Siege
      // engines (prefersBuildings) lock onto enemy structures first, falling back
      // to units only when none is in range; everyone else picks nearest unit.
      if (targetId === 0n && def.aggroRange > 0) {
        const near = acquireTarget(
          e.x,
          e.y,
          def.aggroRange,
          enemyUnitsAround(ctx, units, u.owner, u.entityId),
          def.prefersBuildings ? enemyBuildingsAround(ctx, u.owner) : [],
          !!def.prefersBuildings
        );
        if (near) targetId = near.id;
      }

      // Resolve the target fresh — stale hp can't double-hit or "revive".
      const tu = targetId !== 0n ? ctx.db.unit.entityId.find(targetId) : null;
      const tb =
        !tu && targetId !== 0n ? ctx.db.building.entityId.find(targetId) : null;
      const te = tu || tb ? ctx.db.entity.entityId.find(targetId) : null;

      const cd = Math.max(0, u.attackCooldown - COMBAT_DT);
      if (!te || (!tu && !tb)) {
        if (u.attackTarget !== 0n || u.attackCooldown !== cd)
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: cd,
          });
        continue;
      }

      // Big buildings can be hit from anywhere on their footprint edge.
      const targetR = tb
        ? (BUILDING_DEFS[tb.kind as BuildingKindT]?.footprint ?? 1) / 2
        : 0;
      const d = dist(e.x, e.y, te.x, te.y);
      // High ground extends a ranged unit's reach (and shortens it shooting
      // uphill). Melee reach (range ~1) is barely affected; archers gain the most.
      const elevMul = elevationRangeBonus(
        elevation(seed, e.x, e.y),
        elevation(seed, te.x, te.y)
      );
      if (d <= def.range * elevMul + targetR) {
        if (cd > 0) {
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: targetId,
            attackCooldown: cd,
            hasTarget: false,
          });
          continue;
        }
        const targetArmor = tu
          ? UNIT_DEFS[tu.kind as UnitKindT].armorClass
          : BUILDING_DEFS[tb!.kind as BuildingKindT].armorClass;
        const newHp = applyDamage(
          tu ? tu.hp : tb!.hp,
          effectiveDamage(def, targetArmor)
        );
        if (newHp <= 0) {
          if (tu) ctx.db.unit.entityId.delete(targetId);
          else {
            // Empty the garrison first (eject survivors or kill them per the
            // host def) so no garrison row outlives its building.
            evacuateOnDeath(ctx, tb!, te);
            if (tb!.kind === BuildingKind.Keep) markDefeated(ctx, tb!.owner);
            ctx.db.building.entityId.delete(targetId);
          }
          ctx.db.entity.entityId.delete(targetId);
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: def.attackRate,
            hasTarget: false,
          });
        } else {
          if (tu) {
            ctx.db.unit.entityId.update({ ...tu, hp: newHp });
            dentMorale(targetId, tu.hp, newHp); // a survived blow shakes resolve
          } else ctx.db.building.entityId.update({ ...tb!, hp: newHp });
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: targetId,
            attackCooldown: def.attackRate,
            hasTarget: false,
          });
        }
      } else {
        // Out of range — posture decides whether to chase, fall back, or hold.
        const act = combatAction(
          u.stance as Stance,
          false,
          dist(e.x, e.y, u.homeX, u.homeY),
          DEFENSIVE_LEASH
        );
        if (act === 'approach') {
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: targetId,
            attackCooldown: cd,
            ...(u.hasTarget ? {} : movePatch(ctx, e.x, e.y, te.x, te.y)),
          });
        } else if (act === 'return') {
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: cd,
            ...(u.hasTarget ? {} : movePatch(ctx, e.x, e.y, u.homeX, u.homeY)),
          });
        } else {
          // hold: stand fast, do not chase.
          ctx.db.unit.entityId.update({
            ...u,
            attackTarget: 0n,
            attackCooldown: cd,
          });
        }
      }
    }

    // Towers (and manned walls) auto-fire at the nearest enemy unit within range.
    // A garrisoned host's ranged occupants stack their bows onto its volley; a
    // structure that cannot shoot on its own still fires once archers man it.
    for (const b of [...ctx.db.building.iter()]) {
      const bdef =
        BUILDING_DEFS[b.kind as BuildingKindT] ?? BUILDING_DEFS[BuildingKind.Keep];
      const garrisonFire = garrisonFirePower(
        occupantFireProfile(ctx, b.entityId),
        bdef
      );
      const fireAttack = bdef.attack + garrisonFire;
      if (fireAttack <= 0) continue; // neither the host nor its garrison can fire
      // Walls/gatehouses have no fire stats of their own — borrow the occupants'
      // reach and reload; a self-shooting host keeps its own range/cadence.
      const gr = bdef.attack <= 0 ? garrisonRangeRate(ctx, b.entityId) : null;
      const fireRange = bdef.range > 0 ? bdef.range : gr?.range ?? 0;
      const fireRate = bdef.attackRate > 0 ? bdef.attackRate : gr?.rate ?? 1;
      if (fireRange <= 0) continue;
      const be = ctx.db.entity.entityId.find(b.entityId);
      if (!be) continue;
      const cd = Math.max(0, b.cooldown - COMBAT_DT);
      const enemies = enemyUnitsAround(ctx, units, b.owner, 0n);
      // Search out to the best-case elevation reach, then confirm the chosen
      // target is within this tower's range adjusted for THIS target's elevation.
      const towerElev = elevation(seed, be.x, be.y);
      const near = nearestWithin(be.x, be.y, enemies, fireRange * (1 + ELEV_BONUS_MAX));
      const inElevRange =
        near != null &&
        dist(be.x, be.y, near.x, near.y) <=
          fireRange * elevationRangeBonus(towerElev, elevation(seed, near.x, near.y));
      const fresh = near && inElevRange ? ctx.db.unit.entityId.find(near.id) : null;
      if (near && fresh && cd <= 0) {
        ctx.db.shot.insert({ fromX: be.x, fromY: be.y, toX: near.x, toY: near.y });
        const newHp = applyDamage(
          fresh.hp,
          effectiveDamage(
            { attack: fireAttack, damageType: bdef.damageType },
            UNIT_DEFS[fresh.kind as UnitKindT].armorClass
          )
        );
        if (newHp <= 0) {
          ctx.db.unit.entityId.delete(fresh.entityId);
          ctx.db.entity.entityId.delete(fresh.entityId);
        } else {
          ctx.db.unit.entityId.update({ ...fresh, hp: newHp });
          dentMorale(fresh.entityId, fresh.hp, newHp); // tower fire rattles too
        }
        ctx.db.building.entityId.update({ ...b, cooldown: fireRate });
      } else if (b.cooldown !== cd) {
        ctx.db.building.entityId.update({ ...b, cooldown: cd });
      }
    }

    // Morale recovery pass: every surviving combat unit NOT hit this tick steadies
    // its resolve — faster among nearby allies and within an own keep's presence
    // or an allied Imam's aura. Deterministic (no clocks/random). The latched
    // routing flag is refreshed so the next tick acts on up-to-date morale.
    for (const snap of units) {
      if (hitThisTick.has(snap.entityId)) continue;
      const u = ctx.db.unit.entityId.find(snap.entityId);
      if (!u || u.garrisonedIn !== 0n) continue; // gone or sheltered (safe)
      const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
      if (def.attack <= 0) continue; // non-combatants never rout
      if (u.morale >= MORALE_MAX && !u.routing) continue; // already full + steady
      const e = ctx.db.entity.entityId.find(u.entityId);
      if (!e) continue;
      const allies = nearbyAllies(ctx, units, u.owner, u.entityId, e.x, e.y);
      const support = nearSupport(ctx, units, u.owner, e.x, e.y);
      const morale = moraleRecover(u.morale, COMBAT_DT, allies, support);
      ctx.db.unit.entityId.update({
        ...u,
        morale,
        routing: isRouting(u.routing, morale),
      });
    }
  }
);

scheduleRefs.combatTick = combatTick;
