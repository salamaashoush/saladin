// The single detection point for "am I in a match, and which one." Derived purely
// from the local player's own row via a lobby-scoped subscription (player WHERE
// identity = me) — an index-backed query that costs one point lookup, NOT a scan
// of every player in every running match. The returned matchId drives the world
// subscription scope in useGameSession; null means the player is at the menu.
import { useTable } from "spacetimedb/react";
import type { Identity } from "spacetimedb";
import { tables } from "../module_bindings";

export function useMatchId(identity?: Identity): bigint | null {
  // `enabled` gates the subscription so we open nothing until we know who we are.
  const [mine] = useTable(
    identity
      ? tables.player.where((r) => r.identity.eq(identity))
      : tables.player,
    { enabled: !!identity },
  );
  return mine[0]?.matchId ?? null;
}
