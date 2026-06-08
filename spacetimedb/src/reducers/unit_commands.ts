import { t } from 'spacetimedb/server';
import { UNIT_DEFS } from '../../../shared/defs.ts';
import {
  UnitKind,
  GatherState,
  Stance,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { spacetimedb } from '../schema/db.ts';
import { clampWorld } from '../world/util.ts';
import { movePatch } from '../world/placement.ts';

// Benign stale clicks (unit died, target gone, not yours after a desync) return
// silently — they are races, not user errors, and must not flood the error log.
export const moveUnit = spacetimedb.reducer(
  { entityId: t.u64(), x: t.f32(), y: t.f32() },
  (ctx, { entityId, x, y }) => {
    const u = ctx.db.unit.entityId.find(entityId);
    if (!u || !u.owner.equals(ctx.sender)) return;
    const e = ctx.db.entity.entityId.find(entityId);
    if (!e) return;
    const hx = clampWorld(x);
    const hy = clampWorld(y);
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.Idle,
      targetNode: 0n,
      attackTarget: 0n,
      homeX: hx,
      homeY: hy,
      ...movePatch(ctx, e.x, e.y, hx, hy),
    });
  }
);

export const gatherResource = spacetimedb.reducer(
  { entityId: t.u64(), nodeId: t.u64() },
  (ctx, { entityId, nodeId }) => {
    const u = ctx.db.unit.entityId.find(entityId);
    if (!u || !u.owner.equals(ctx.sender)) return;
    const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
    if (def.carry <= 0) return;
    if (!ctx.db.resourceNode.entityId.find(nodeId)) return;
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.ToResource,
      targetNode: nodeId,
      attackTarget: 0n,
      hasTarget: false,
    });
  }
);

export const setRally = spacetimedb.reducer(
  { entityId: t.u64(), x: t.f32(), y: t.f32() },
  (ctx, { entityId, x, y }) => {
    const b = ctx.db.building.entityId.find(entityId);
    if (!b || !b.owner.equals(ctx.sender)) return;
    ctx.db.building.entityId.update({
      ...b,
      rallyX: clampWorld(x),
      rallyY: clampWorld(y),
    });
  }
);

// Set combat posture for a batch of the caller's units; posts each unit's home
// at its current position so Defensive units leash to where they were set.
export const setStance = spacetimedb.reducer(
  { entityIds: t.array(t.u64()), stance: t.u8() },
  (ctx, { entityIds, stance }) => {
    const s = stance > Stance.HoldGround ? Stance.Aggressive : stance;
    for (const id of entityIds) {
      const u = ctx.db.unit.entityId.find(id);
      if (!u || !u.owner.equals(ctx.sender)) continue;
      const e = ctx.db.entity.entityId.find(id);
      ctx.db.unit.entityId.update({
        ...u,
        stance: s,
        homeX: e ? e.x : u.homeX,
        homeY: e ? e.y : u.homeY,
      });
    }
  }
);

export const attackUnit = spacetimedb.reducer(
  { entityId: t.u64(), targetId: t.u64() },
  (ctx, { entityId, targetId }) => {
    const u = ctx.db.unit.entityId.find(entityId);
    if (!u || !u.owner.equals(ctx.sender)) return;
    const tu = ctx.db.unit.entityId.find(targetId);
    const tb = tu ? null : ctx.db.building.entityId.find(targetId);
    const target = tu ?? tb;
    if (!target || target.owner.equals(ctx.sender)) return;
    ctx.db.unit.entityId.update({
      ...u,
      attackTarget: targetId,
      gatherState: GatherState.Idle,
      targetNode: 0n,
      hasTarget: false,
    });
  }
);
