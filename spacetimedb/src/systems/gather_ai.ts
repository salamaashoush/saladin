import { AI_DT, HARVEST_RANGE, DEPOSIT_RANGE, HARVEST_TIME } from '../../../shared/constants.ts';
import { UNIT_DEFS, BUILDING_DEFS } from '../../../shared/defs.ts';
import {
  UnitKind,
  BuildingKind,
  GatherState,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { nearestIndex } from '../../../shared/sim.ts';
import { spacetimedb } from '../schema/db.ts';
import { aiTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import { dist, buildNodes } from '../world/util.ts';
import { movePatch } from '../world/placement.ts';

// Gather AI state machine — runs every AI_TICK_MS, sets movement targets.
export const unitAi = spacetimedb.reducer({ timer: aiTimer.rowType }, (ctx) => {
  const nodes = buildNodes(ctx);

  // A gatherer whose node is gone heads to the nearest remaining node, and
  // only idles when the whole map is exhausted. Without this, peasants freeze
  // forever the moment their tree is chopped out.
  const retarget = (u: any, e: any) => {
    const idx = nearestIndex(e.x, e.y, nodes);
    if (idx < 0) {
      ctx.db.unit.entityId.update({
        ...u,
        gatherState: GatherState.Idle,
        hasTarget: false,
        targetNode: 0n,
      });
      return;
    }
    ctx.db.unit.entityId.update({
      ...u,
      gatherState: GatherState.ToResource,
      targetNode: nodes[idx].id,
      hasTarget: false,
    });
  };

  for (const u of [...ctx.db.unit.iter()]) {
    if (u.gatherState === GatherState.Idle) continue;
    const e = ctx.db.entity.entityId.find(u.entityId);
    if (!e) continue;

    if (u.gatherState === GatherState.ToResource) {
      const node = ctx.db.resourceNode.entityId.find(u.targetNode);
      const ne = node ? ctx.db.entity.entityId.find(node.entityId) : null;
      if (!node || !ne) {
        retarget(u, e);
        continue;
      }
      if (dist(e.x, e.y, ne.x, ne.y) <= HARVEST_RANGE) {
        ctx.db.unit.entityId.update({
          ...u,
          gatherState: GatherState.Harvesting,
          harvestTimer: 0,
          hasTarget: false,
        });
      } else if (!u.hasTarget) {
        ctx.db.unit.entityId.update({
          ...u,
          ...movePatch(ctx, e.x, e.y, ne.x, ne.y),
        });
      }
    } else if (u.gatherState === GatherState.Harvesting) {
      const node = ctx.db.resourceNode.entityId.find(u.targetNode);
      if (!node) {
        retarget(u, e);
        continue;
      }
      const timer = u.harvestTimer + AI_DT;
      if (timer < HARVEST_TIME) {
        ctx.db.unit.entityId.update({ ...u, harvestTimer: timer });
        continue;
      }
      const def = UNIT_DEFS[u.kind as UnitKindT] ?? UNIT_DEFS[UnitKind.Peasant];
      const take = Math.min(def.carry, node.remaining);
      const rem = node.remaining - take;
      if (rem <= 0) {
        ctx.db.resourceNode.entityId.delete(node.entityId);
        ctx.db.entity.entityId.delete(node.entityId);
      } else {
        ctx.db.resourceNode.entityId.update({ ...node, remaining: rem });
      }
      ctx.db.unit.entityId.update({
        ...u,
        carrying: take,
        harvestTimer: 0,
        gatherState: GatherState.ToStockpile,
      });
    } else if (u.gatherState === GatherState.ToStockpile) {
      const p = ctx.db.player.identity.find(u.owner);
      const keep = p ? ctx.db.entity.entityId.find(p.keepEntity) : null;
      if (!p || !keep) {
        ctx.db.unit.entityId.update({
          ...u,
          gatherState: GatherState.Idle,
          hasTarget: false,
        });
        continue;
      }
      // Keep blocks its own footprint, so the peasant stops at the wall edge;
      // accept deposits within the keep's radius too.
      const depositRange =
        DEPOSIT_RANGE + BUILDING_DEFS[BuildingKind.Keep].footprint / 2;
      if (dist(e.x, e.y, keep.x, keep.y) <= depositRange) {
        ctx.db.player.identity.update({ ...p, wood: p.wood + u.carrying });
        const node = ctx.db.resourceNode.entityId.find(u.targetNode);
        if (node) {
          ctx.db.unit.entityId.update({
            ...u,
            carrying: 0,
            hasTarget: false,
            gatherState: GatherState.ToResource,
          });
        } else {
          // Assigned tree is gone — pick the next nearest instead of idling.
          retarget({ ...u, carrying: 0 }, e);
        }
      } else if (!u.hasTarget) {
        ctx.db.unit.entityId.update({
          ...u,
          ...movePatch(ctx, e.x, e.y, keep.x, keep.y),
        });
      }
    }
  }
});

scheduleRefs.unitAi = unitAi;
