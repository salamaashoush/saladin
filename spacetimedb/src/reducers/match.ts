import { t, SenderError } from 'spacetimedb/server';
import { MAX_PLAYERS } from '../../../shared/constants.ts';
import { MAX_AI_OPPONENTS, enemyFaction } from '../../../shared/defs.ts';
import { Faction, ResourceType } from '../../../shared/enums.ts';
import { MatchStatus } from '../../../shared/match.ts';
import { foodLow } from '../../../shared/economy.ts';
import { spacetimedb } from '../schema/db.ts';
import { foundPlayer, spawnAi } from '../world/spawn.ts';
import { regenerateWorld } from '../world/commands.ts';
import {
  createMatch as createMatchRow,
  scatterMatchNodes,
} from '../world/match.ts';
import {
  clearMatchRows,
  callerMatchId,
  playersOfMatch,
  otherHumansInMatch,
} from '../world/scope.ts';
import { assignIdleGatherers, popInfo } from '../world/economy.ts';

function normalizeFaction(faction: number): number {
  return faction === Faction.Crusader ? Faction.Crusader : Faction.Ayyubid;
}

// Found a brand-new multiplayer match the caller hosts, and join it. The match is
// Active, gets a fresh per-match forest on the shared map, and starts with just the
// host — others find it in the lobby (status Active, players < MAX_PLAYERS) and
// join by its matchId. Returns the new matchId so enterGame(0) can reuse it.
function createNewMatch(
  ctx: any,
  name: string,
  preset: string,
  side: number
): bigint {
  const cfg = ctx.db.config.id.find(0);
  const presetId = preset || cfg?.preset || 'continental';
  const matchId = createMatchRow(
    ctx,
    ctx.sender,
    name || 'Open Match',
    cfg?.seed ?? 1,
    presetId
  );
  scatterMatchNodes(ctx, matchId);
  foundPlayer(ctx, ctx.sender, name || 'Amir', side, matchId);
  return matchId;
}

// Create a fresh multiplayer match hosted by the caller and drop them into it. The
// lobby's "Create Match" calls this; `preset` is the map flavor shown in the list.
// A caller already in a match is rejected — leave first.
export const createMatch = spacetimedb.reducer(
  { name: t.string(), faction: t.u8(), preset: t.string() },
  (ctx, { name, faction, preset }) => {
    if (ctx.db.player.identity.find(ctx.sender))
      throw new SenderError('already in a match — leave first');
    createNewMatch(ctx, name, preset, normalizeFaction(faction));
  }
);

// Join an EXPLICIT match. `matchId = 0` means "create a new multiplayer match and
// join it" (the lobby's Create path). A real id joins THAT match if it is Active
// and has room — erroring on a missing/ended/full match rather than silently
// dropping the player into some other world. A reconnecting player (already has a
// row) just flips online; their matchId is unchanged.
export const enterGame = spacetimedb.reducer(
  { matchId: t.u64(), name: t.string(), faction: t.u8() },
  (ctx, { matchId, name, faction }) => {
    const side = normalizeFaction(faction);
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
    if (matchId === 0n) {
      createNewMatch(ctx, name, '', side);
      return;
    }
    const m = ctx.db.match.matchId.find(matchId);
    if (!m) throw new SenderError('match no longer exists');
    if (m.status !== MatchStatus.Active)
      throw new SenderError('match is not open to join');
    if (playersOfMatch(ctx, matchId).length >= MAX_PLAYERS)
      throw new SenderError('match is full');
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

    const matchId = createMatchRow(ctx, ctx.sender, name || 'Skirmish', world.seed, world.preset);
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
