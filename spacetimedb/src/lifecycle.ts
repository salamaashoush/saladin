import { ScheduleAt } from 'spacetimedb';
import {
  WORLD_SIZE,
  MOVE_TICK_MS,
  AI_TICK_MS,
  COMBAT_TICK_MS,
  AI_BRAIN_TICK_MS,
  ECONOMY_TICK_MS,
  RESEARCH_TICK_MS,
} from '../../shared/constants.ts';
import { spacetimedb } from './schema/db.ts';
import { scatterNodes } from './world/spawn.ts';

export const init = spacetimedb.init((ctx) => {
  const seed = ctx.random.integerInRange(1, 2_000_000_000);
  ctx.db.config.insert({
    id: 0,
    worldSize: WORLD_SIZE,
    seed,
    preset: 'continental',
    initialized: true,
    nextBotId: 1n,
  });

  scatterNodes(ctx, seed);

  ctx.db.moveTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(MOVE_TICK_MS) * 1000n),
  });
  ctx.db.aiTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(AI_TICK_MS) * 1000n),
  });
  ctx.db.combatTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(COMBAT_TICK_MS) * 1000n),
  });
  ctx.db.aiBrainTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(AI_BRAIN_TICK_MS) * 1000n),
  });
  ctx.db.economyTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(ECONOMY_TICK_MS) * 1000n),
  });
  ctx.db.researchTimer.insert({
    scheduledId: 0n,
    scheduledAt: ScheduleAt.interval(BigInt(RESEARCH_TICK_MS) * 1000n),
  });
});

export const onConnect = spacetimedb.clientConnected((ctx) => {
  const p = ctx.db.player.identity.find(ctx.sender);
  if (p) ctx.db.player.identity.update({ ...p, online: true });
});

export const onDisconnect = spacetimedb.clientDisconnected((ctx) => {
  const p = ctx.db.player.identity.find(ctx.sender);
  if (p) ctx.db.player.identity.update({ ...p, online: false });
});
