import { t, SenderError } from 'spacetimedb/server';
import { MAX_PLAYERS } from '../../../shared/constants.ts';
import {
  MAX_AI_OPPONENTS,
  enemyFaction,
} from '../../../shared/defs.ts';
import { Faction } from '../../../shared/enums.ts';
import { spacetimedb } from '../schema/db.ts';
import { foundPlayer, spawnAi } from '../world/spawn.ts';
import {
  resetMatch,
  clearMatch,
  regenerateWorld,
  otherHumansPresent,
} from '../world/commands.ts';
import { assignIdleGatherers } from '../world/economy.ts';

// Join the shared world (multiplayer). Founds a base on first call; reconnects
// just flip online. Faction is the player's chosen side.
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
    if ([...ctx.db.player.iter()].length >= MAX_PLAYERS)
      throw new SenderError('the world is full');
    foundPlayer(ctx, ctx.sender, name || 'Amir', side);
  }
);

// Begin a fresh single-player skirmish: wipe the caller + all bots, regenerate
// the map from the chosen seed (0 = a fresh random one) and preset, then found
// the player and one bot per requested difficulty. Regeneration is skipped when
// another human is in the shared world so a skirmish can't reshape their match.
export const startSkirmish = spacetimedb.reducer(
  {
    name: t.string(),
    faction: t.u8(),
    enemies: t.array(t.u8()),
    seed: t.u32(),
    preset: t.string(),
  },
  (ctx, { name, faction, enemies, seed, preset }) => {
    resetMatch(ctx, ctx.sender);
    if (!otherHumansPresent(ctx, ctx.sender))
      regenerateWorld(ctx, seed, preset);
    const human =
      faction === Faction.Crusader ? Faction.Crusader : Faction.Ayyubid;
    foundPlayer(ctx, ctx.sender, name || 'Amir', human);
    const foe = enemyFaction(human as 0 | 1);
    const count = Math.min(enemies.length, MAX_AI_OPPONENTS);
    for (let i = 0; i < count; i++) spawnAi(ctx, ctx.sender, enemies[i], foe);
  }
);

// Leave the current match — tear down the caller and the bots they host, back to
// a clean slate so the client can return to the menu. Other matches untouched.
export const leaveGame = spacetimedb.reducer((ctx) => {
  clearMatch(ctx, ctx.sender);
});

// Add one bot to the caller's match, on the side opposing the caller.
export const addAi = spacetimedb.reducer(
  { difficulty: t.u8() },
  (ctx, { difficulty }) => {
    const p = ctx.db.player.identity.find(ctx.sender);
    if (!p) throw new SenderError('not in game');
    if ([...ctx.db.player.iter()].length >= MAX_PLAYERS)
      throw new SenderError('match is full');
    spawnAi(ctx, ctx.sender, difficulty, enemyFaction(p.faction as 0 | 1));
  }
);

// Send every idle gatherer owned by the caller to the nearest resource node.
export const autoGather = spacetimedb.reducer((ctx) => {
  assignIdleGatherers(ctx, ctx.sender);
});
