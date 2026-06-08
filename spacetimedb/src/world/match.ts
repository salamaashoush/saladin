import { MatchStatus } from '../../../shared/match.ts';
import { getSeed } from './util.ts';
import { scatterNodes } from './spawn.ts';

// Allocate the next matchId from config's monotonic counter (so ids never collide
// across resets/saves) and insert a fresh Active match row for `host`. Returns the
// new matchId. Resource nodes are scattered by the caller into this id.
export function createMatch(
  ctx: any,
  host: any,
  name: string,
  seed: number,
  preset: string
): bigint {
  const cfg = ctx.db.config.id.find(0);
  const id = cfg ? cfg.nextMatchId : 1n;
  if (cfg) ctx.db.config.id.update({ ...cfg, nextMatchId: id + 1n });
  ctx.db.match.insert({
    matchId: id,
    name,
    host,
    status: MatchStatus.Active,
    seed,
    preset,
    players: 0, // bumped by foundPlayer as each human/bot joins this match
  });
  return id;
}

// Scatter this match's forest from the SHARED map seed, stamped with its matchId so
// each match harvests only its own nodes even though they sit on one terrain.
export function scatterMatchNodes(ctx: any, matchId: bigint): void {
  scatterNodes(ctx, getSeed(ctx), matchId);
}
