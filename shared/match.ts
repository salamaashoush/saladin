// A match is a first-class entity: every game-object row carries the matchId of
// the match it belongs to, so a save is "all rows where matchId=X" and pause is
// "freeze the systems for that match". Stored as a u8 status column on the match
// row plus a u64 matchId stamped onto entity/unit/building/resource_node/player/ai.

// Lifecycle of a match. Scheduled systems simulate only Active matches, so Paused
// freezes a match in place and Ended marks it torn down (its rows are cleared).
export const MatchStatus = { Active: 0, Paused: 1, Ended: 2 } as const;
export type MatchStatus = (typeof MatchStatus)[keyof typeof MatchStatus];

// Only a match in this status advances under the scheduled simulation loops.
export function matchSimulates(status: number): boolean {
  return status === MatchStatus.Active;
}

// The tables whose rows carry a matchId — the set that a per-match teardown or
// save must sweep. Drives both the module-side scope helpers and the (future)
// save/load so neither can silently miss a table.
export const MATCH_SCOPED_TABLES = [
  'entity',
  'unit',
  'building',
  'resource_node',
  'player',
  'ai',
] as const;
export type MatchScopedTable = (typeof MATCH_SCOPED_TABLES)[number];

export function carriesMatchId(table: string): boolean {
  return (MATCH_SCOPED_TABLES as readonly string[]).includes(table);
}
