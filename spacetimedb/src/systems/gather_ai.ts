import { AI_DT, HARVEST_RANGE, DEPOSIT_RANGE, HARVEST_TIME } from '../../../shared/constants.ts';
import { UNIT_DEFS } from '../../../shared/defs.ts';
import { addResource } from '../../../shared/economy.ts';
import {
  UnitKind,
  GatherState,
  type ResourceType as ResourceTypeT,
  type UnitKind as UnitKindT,
} from '../../../shared/enums.ts';
import { nearestIndex } from '../../../shared/sim.ts';
import { spacetimedb } from '../schema/db.ts';
import { aiTimer } from '../schema/tables.ts';
import { scheduleRefs } from '../schema/schedule_refs.ts';
import {
  dist,
  buildNodes,
  nearestDropoff,
  dropoffApproach,
  getSeed,
  passableWith,
} from '../world/util.ts';
import { movePatch, buildOccupancy } from '../world/placement.ts';
import { activeMatchIds } from '../world/scope.ts';

// Gather AI state machine — runs every AI_TICK_MS, sets movement targets. Only acts
// on units in Active matches; a node retarget stays within the unit's own match.
export const unitAi = spacetimedb.reducer({ timer: aiTimer.rowType }, (ctx) => {
  const active = activeMatchIds(ctx);
  const allNodes = buildNodes(ctx);
  const seed = getSeed(ctx);
  const occ = buildOccupancy(ctx);

  // A gatherer whose node is gone heads to the nearest remaining node IN ITS OWN
  // MATCH, and only idles when its match's forest is exhausted. Without this,
  // peasants freeze forever the moment their tree is chopped out.
  const retarget = (u: any, e: any, skipNode: bigint = 0n) => {
    const own = allNodes.filter((n) => n.matchId === u.matchId);
    const pool = skipNode ? own.filter((n) => n.id !== skipNode) : own;
    const idx = nearestIndex(e.x, e.y, pool);
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
      targetNode: pool[idx].id,
      hasTarget: false,
    });
  };

  // Drive off the matchId index (Rank 1, docs/STDB_PERF.md §3): only active-match
  // gatherers are decoded, not every unit in every match.
  for (const mid of active)
  for (const u of ctx.db.unit.matchId.filter(mid)) {
    if (u.garrisonedIn !== 0n) continue; // sheltered — off the field
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
        const patch = movePatch(ctx, e.x, e.y, ne.x, ne.y, occ);
        // No path to this node (it sits in a region the gatherer can't reach):
        // pick the next nearest node instead of retrying the same dead end every
        // tick. retarget skips the current target so we don't relock onto it.
        if (!patch.hasTarget) retarget(u, e, u.targetNode);
        else ctx.db.unit.entityId.update({ ...u, ...patch });
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
      // Stamp what we picked up from the node's own resType BEFORE the node row
      // can be deleted — otherwise the deposit always credits the default (wood).
      const carryType = node.resType;
      if (rem <= 0) {
        ctx.db.resourceNode.entityId.delete(node.entityId);
        ctx.db.entity.entityId.delete(node.entityId);
      } else {
        ctx.db.resourceNode.entityId.update({ ...node, remaining: rem });
      }
      ctx.db.unit.entityId.update({
        ...u,
        carrying: take,
        carryType,
        harvestTimer: 0,
        gatherState: GatherState.ToStockpile,
      });
    } else if (u.gatherState === GatherState.ToStockpile) {
      const p = ctx.db.player.identity.find(u.owner);
      // Route to the nearest valid deposit point for what's being carried — the
      // keep, or a food-dropoff (FishingHut/Granary) when carrying food.
      const drop = p
        ? nearestDropoff(ctx, u.owner, u.carryType, e.x, e.y)
        : null;
      if (!p || !drop) {
        ctx.db.unit.entityId.update({
          ...u,
          gatherState: GatherState.Idle,
          hasTarget: false,
        });
        continue;
      }
      // The building blocks its own footprint, and on a coastal/cramped keep the
      // only tile a gatherer can actually stand on may be a single perimeter tile
      // offset from the centre. Measure the deposit range against that REACHABLE
      // approach tile (not the blocked centre), so the carrier banks the moment it
      // has walked as close as the terrain allows instead of circling forever.
      const passable = passableWith(seed, occ);
      const approach = dropoffApproach(ctx, passable, e.x, e.y, drop);
      const atApproach = dist(e.x, e.y, approach.x, approach.y) <= DEPOSIT_RANGE;
      const atCentre =
        dist(e.x, e.y, drop.x, drop.y) <= DEPOSIT_RANGE + drop.footprint / 2;
      if (atApproach || atCentre) {
        ctx.db.player.identity.update({
          ...p,
          ...addResource(p, u.carryType as ResourceTypeT, u.carrying),
        });
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
          ...movePatch(ctx, e.x, e.y, drop.x, drop.y, occ),
        });
      }
    }
  }
});

scheduleRefs.unitAi = unitAi;
