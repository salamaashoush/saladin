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
  DEFENSIVE_LEASH,
} from '../../../shared/combat.ts';
import {
  elevation,
  elevationRangeBonus,
  ELEV_BONUS_MAX,
} from '../../../shared/elevation.ts';
import { spacetimedb } from '../schema/db.ts';
import { combatTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { dist, getSeed } from '../world/util.ts';
import { movePatch } from '../world/placement.ts';
import { markDefeated } from '../world/spawn.ts';

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
    if (!ctx.db.unit.entityId.find(o.entityId)) continue; // dead this tick
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

// Combat — runs every COMBAT_TICK_MS. Soldiers auto-acquire nearby enemies,
// close to range, and strike on cooldown. Dead units are removed.
export const combatTick = spacetimedb.reducer(
  { timer: combatTimer.rowType },
  (ctx) => {
    const seed = getSeed(ctx);
    const units = [...ctx.db.unit.iter()];
    for (const snap of units) {
      // Re-fetch every iteration: a unit (or its target) killed earlier this
      // same tick must never act, nor be acted on, via a stale snapshot row.
      const u = ctx.db.unit.entityId.find(snap.entityId);
      if (!u) continue;
      const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
      if (def.attack <= 0) continue; // non-combatants never fight

      const e = ctx.db.entity.entityId.find(u.entityId);
      if (!e) continue;

      let targetId = u.attackTarget;
      // No current target: auto-acquire the nearest LIVE enemy in range. Siege
      // engines (prefersBuildings) acquire enemy structures first, falling back
      // to units only when no building is near.
      if (targetId === 0n && def.aggroRange > 0) {
        if (def.prefersBuildings) {
          const blds = enemyBuildingsAround(ctx, u.owner);
          const nearB = nearestWithin(e.x, e.y, blds, def.aggroRange);
          if (nearB) targetId = nearB.id;
        }
        if (targetId === 0n) {
          const enemies = enemyUnitsAround(ctx, units, u.owner, u.entityId);
          const near = nearestWithin(e.x, e.y, enemies, def.aggroRange);
          if (near) targetId = near.id;
        }
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
          if (tu) ctx.db.unit.entityId.update({ ...tu, hp: newHp });
          else ctx.db.building.entityId.update({ ...tb!, hp: newHp });
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

    // Towers auto-fire at the nearest enemy unit within range.
    for (const b of [...ctx.db.building.iter()]) {
      const bdef =
        BUILDING_DEFS[b.kind as BuildingKindT] ?? BUILDING_DEFS[BuildingKind.Keep];
      if (bdef.attack <= 0) continue;
      const be = ctx.db.entity.entityId.find(b.entityId);
      if (!be) continue;
      const cd = Math.max(0, b.cooldown - COMBAT_DT);
      const enemies = enemyUnitsAround(ctx, units, b.owner, 0n);
      // Search out to the best-case elevation reach, then confirm the chosen
      // target is within this tower's range adjusted for THIS target's elevation.
      const towerElev = elevation(seed, be.x, be.y);
      const near = nearestWithin(be.x, be.y, enemies, bdef.range * (1 + ELEV_BONUS_MAX));
      const inElevRange =
        near != null &&
        dist(be.x, be.y, near.x, near.y) <=
          bdef.range * elevationRangeBonus(towerElev, elevation(seed, near.x, near.y));
      const fresh = near && inElevRange ? ctx.db.unit.entityId.find(near.id) : null;
      if (near && fresh && cd <= 0) {
        ctx.db.shot.insert({ fromX: be.x, fromY: be.y, toX: near.x, toY: near.y });
        const newHp = applyDamage(
          fresh.hp,
          effectiveDamage(
            { attack: bdef.attack, damageType: bdef.damageType },
            UNIT_DEFS[fresh.kind as UnitKindT].armorClass
          )
        );
        if (newHp <= 0) {
          ctx.db.unit.entityId.delete(fresh.entityId);
          ctx.db.entity.entityId.delete(fresh.entityId);
        } else {
          ctx.db.unit.entityId.update({ ...fresh, hp: newHp });
        }
        ctx.db.building.entityId.update({ ...b, cooldown: bdef.attackRate });
      } else if (b.cooldown !== cd) {
        ctx.db.building.entityId.update({ ...b, cooldown: cd });
      }
    }
  }
);

scheduleRefs.combatTick = combatTick;
