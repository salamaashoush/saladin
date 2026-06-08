import { t, SenderError } from 'spacetimedb/server';
import { MAX_PLAYERS } from '../../../shared/constants.ts';
import { MAX_AI_OPPONENTS, enemyFaction } from '../../../shared/defs.ts';
import { Faction, ResourceType } from '../../../shared/enums.ts';
import { MatchStatus } from '../../../shared/match.ts';
import { foodLow } from '../../../shared/economy.ts';
import { spacetimedb } from '../schema/db.ts';
import { foundPlayer, spawnAi } from '../world/spawn.ts';
import { regenerateWorld } from '../world/commands.ts';
import { createMatch, scatterMatchNodes } from '../world/match.ts';
import {
  clearMatchRows,
  callerMatchId,
  playersOfMatch,
  otherHumansInMatch,
} from '../world/scope.ts';
import { assignIdleGatherers, popInfo } from '../world/economy.ts';

// A multiplayer-joinable match: Active, hosted by a human, with room left. enterGame
// drops into the most recent such match so several humans can share one world; if
// none exists it founds its own. Skirmish matches are joinable too — a friend can
// hop into a started skirmish.
function joinableMatch(ctx: any): any | null {
  let best: any = null;
  for (const m of [...ctx.db.match.iter()]) {
    if (m.status !== MatchStatus.Active) continue;
    if (playersOfMatch(ctx, m.matchId).length >= MAX_PLAYERS) continue;
    if (best === null || m.matchId > best.matchId) best = m;
  }
  return best;
}

// Join the world. Founds a base on first call (joining an open match or creating
// one), reconnects just flip online. Faction is the player's chosen side.
export const enterGame = spacetimedb.reducer(
  { name: t.string(), faction: t.u8() },
  (ctx, { name, faction }) => {
    const side =
      faction === Faction.Crusader ? Faction.Crusader : Faction.Ayyubid;
    const existing = ctx.db.player.identity.find(ctx.sender);
    if (existing) {
      ctx.db.player.identity.update({
        ...existing,
        online: true,
        name: name || existing.name,
        faction: side,
      });
      return;
    }
    const open = joinableMatch(ctx);
    let matchId: bigint;
    if (open) {
      matchId = open.matchId;
    } else {
      const cfg = ctx.db.config.id.find(0);
      matchId = createMatch(
        ctx,
        ctx.sender,
        name || 'Skirmish',
        cfg?.seed ?? 1,
        cfg?.preset ?? 'continental'
      );
      scatterMatchNodes(ctx, matchId);
    }
    foundPlayer(ctx, ctx.sender, name || 'Amir', side, matchId);
  }
);

// Begin a fresh single-player skirmish. Ends the caller's current match (cleaning up
// its rows), then creates a brand-new match: regenerate the shared map from the
// chosen seed (0 = a random one) and preset when the caller isn't sharing a world,
// scatter the new match's forest, and found the human plus one bot per requested
// difficulty — all stamped with the new matchId.
export const startSkirmish = spacetimedb.reducer(
  {
    name: t.string(),
    faction: t.u8(),
    enemies: t.array(t.u8()),
    seed: t.u32(),
    preset: t.string(),
  },
  (ctx, { name, faction, enemies, seed, preset }) => {
    const prior = callerMatchId(ctx, ctx.sender);
    const sharing =
      prior !== null && otherHumansInMatch(ctx, prior, ctx.sender);
    if (prior !== null) clearMatchRows(ctx, prior);

    const cfg = ctx.db.config.id.find(0);
    // Only roll the shared terrain when the caller isn't co-tenant with another
    // human; otherwise keep the current map so a skirmish can't reshape it.
    const world = sharing
      ? { seed: cfg?.seed ?? 1, preset: cfg?.preset ?? 'continental' }
      : regenerateWorld(ctx, seed, preset);

    const matchId = createMatch(ctx, ctx.sender, name || 'Skirmish', world.seed, world.preset);
    scatterMatchNodes(ctx, matchId);

    const human =
      faction === Faction.Crusader ? Faction.Crusader : Faction.Ayyubid;
    foundPlayer(ctx, ctx.sender, name || 'Amir', human, matchId);
    const foe = enemyFaction(human as 0 | 1);
    const count = Math.min(enemies.length, MAX_AI_OPPONENTS);
    for (let i = 0; i < count; i++)
      spawnAi(ctx, ctx.sender, enemies[i], foe, matchId);
  }
);

// Leave the current match — end it and tear down every row scoped to its matchId so
// the client returns to a clean menu. Other matches untouched.
export const leaveGame = spacetimedb.reducer((ctx) => {
  const matchId = callerMatchId(ctx, ctx.sender);
  if (matchId !== null) clearMatchRows(ctx, matchId);
});

// Add one bot to the caller's match, on the side opposing the caller.
export const addAi = spacetimedb.reducer(
  { difficulty: t.u8() },
  (ctx, { difficulty }) => {
    const p = ctx.db.player.identity.find(ctx.sender);
    if (!p) throw new SenderError('not in game');
    if (playersOfMatch(ctx, p.matchId).length >= MAX_PLAYERS)
      throw new SenderError('match is full');
    spawnAi(ctx, ctx.sender, difficulty, enemyFaction(p.faction as 0 | 1), p.matchId);
  }
);

// Pause the caller's match: scheduled systems skip every Paused/Ended match, so its
// units freeze in place while other matches keep simulating. Idempotent.
export const pauseMatch = spacetimedb.reducer((ctx) => {
  const matchId = callerMatchId(ctx, ctx.sender);
  if (matchId === null) return;
  const m = ctx.db.match.matchId.find(matchId);
  if (m && m.status === MatchStatus.Active)
    ctx.db.match.matchId.update({ ...m, status: MatchStatus.Paused });
});

// Resume the caller's paused match — the systems pick it up again next tick.
export const resumeMatch = spacetimedb.reducer((ctx) => {
  const matchId = callerMatchId(ctx, ctx.sender);
  if (matchId === null) return;
  const m = ctx.db.match.matchId.find(matchId);
  if (m && m.status === MatchStatus.Paused)
    ctx.db.match.matchId.update({ ...m, status: MatchStatus.Active });
});

// Send every idle gatherer to work — balanced food-first, but all-in on food when
// the larder is running low so a "Gather" click can't starve the base.
export const autoGather = spacetimedb.reducer((ctx) => {
  const p = ctx.db.player.identity.find(ctx.sender);
  const prefer =
    p && foodLow(p.food, popInfo(ctx, ctx.sender).pop)
      ? ResourceType.Food
      : undefined;
  assignIdleGatherers(ctx, ctx.sender, prefer);
});
