// Owns the Three.js scene + the live SpacetimeDB connection, and SCOPES what the
// client replicates so the title screen never renders a running match.
//
// Two subscription scopes (see docs/STDB_PERF.md §3 Rank 1 — every world row carries
// a btree-indexed matchId, so a `WHERE matchId = M` subscription is an index lookup,
// not a full-table scan):
//
//   LOBBY scope (always, from the moment we connect): only the lightweight tables the
//   menu needs — match (lobby list), save_slot (own saves, owner-filtered server-side),
//   config (singleton: the menu backdrop's seed/preset), and the caller's own player/ai
//   rows so we can DETECT entering a match and learn its matchId. NO entity/unit/
//   building/resource_node/garrison/research/shot at the menu → nothing of another
//   match is ever replicated or drawn.
//
//   MATCH scope (only once the local player is in a match M): the world tables filtered
//   to matchId = M. Re-subscribed when M changes (load/rematch → new matchId), torn
//   down on leave (back to the menu). Subscribe-before-unsubscribe on every transition
//   so the cache never blinks empty (saladin-dev-gotchas).
import { useEffect, useRef, useState, type RefObject } from "react";
import { useSpacetimeDB } from "spacetimedb/react";
import type { Identity } from "spacetimedb";
import type { DbConnection, SubscriptionHandle } from "../module_bindings";
import { SaladinGame } from "../game/SaladinGame";
import { useMatchId } from "./useMatchId";

export interface GameSession {
  game: SaladinGame | null;
  identity?: Identity;
  isActive: boolean;
  matchId: bigint | null;
  ready: boolean; // lobby scope applied — the menu is safe to enable
  inMatch: boolean; // match scope applied — the world is replicated
}

// Lobby scope: lightweight identity/menu tables only. The player/ai queries are
// keyed to the caller (identity / host) so they stay index-backed point lookups
// rather than table scans, and they carry no other match's rows into the cache.
function lobbyQueries(identity: Identity): string[] {
  const me = `0x${identity.toHexString()}`;
  return [
    "SELECT * FROM match",
    "SELECT * FROM save_slot",
    "SELECT * FROM config",
    `SELECT * FROM player WHERE identity = ${me}`,
    `SELECT * FROM ai WHERE host = ${me}`,
  ];
}

// Match scope: the world tables filtered to one match. entity/unit/building/
// resource_node/ai/player are matchId-scoped; research has no matchId of its own,
// so it is scoped to the caller (the panel only shows the local player's research);
// shot is a broadcast-only event table (no rows to filter, just the firehose).
function matchQueries(matchId: bigint, identity: Identity): string[] {
  const m = matchId.toString();
  const me = `0x${identity.toHexString()}`;
  return [
    `SELECT * FROM entity WHERE match_id = ${m}`,
    `SELECT * FROM unit WHERE match_id = ${m}`,
    `SELECT * FROM building WHERE match_id = ${m}`,
    `SELECT * FROM resource_node WHERE match_id = ${m}`,
    `SELECT * FROM player WHERE match_id = ${m}`,
    `SELECT * FROM ai WHERE match_id = ${m}`,
    `SELECT * FROM garrison WHERE owner = ${me}`,
    `SELECT * FROM research WHERE owner = ${me}`,
    "SELECT * FROM shot",
  ];
}

export function useGameSession(
  containerRef: RefObject<HTMLDivElement | null>,
): GameSession {
  const { identity, isActive } = useSpacetimeDB();
  const { getConnection } = useSpacetimeDB() as unknown as {
    getConnection: () => DbConnection | null;
  };
  const matchId = useMatchId(identity);

  const gameRef = useRef<SaladinGame | null>(null);
  const [game, setGame] = useState<SaladinGame | null>(null);
  const [ready, setReady] = useState(false);
  const [inMatch, setInMatch] = useState(false);

  useEffect(() => {
    if (!containerRef.current) return;
    const g = new SaladinGame(containerRef.current);
    gameRef.current = g;
    setGame(g);
    return () => {
      g.dispose();
      gameRef.current = null;
      setGame(null);
    };
  }, [containerRef]);

  // Attach the connection to the renderer + open the LOBBY scope. This stays up the
  // whole session (the menu reads `match`/`save_slot`/`config`/own player from it);
  // the menu backdrop is just the config-seed terrain, with zero world rows.
  useEffect(() => {
    const conn = getConnection();
    const g = gameRef.current;
    if (!conn || !isActive || !identity || !g) return;

    g.setIdentity(identity.toHexString());
    g.attach(conn as never);

    setReady(false);
    const lobby = conn
      .subscriptionBuilder()
      .onApplied(() => setReady(true))
      .subscribe(lobbyQueries(identity));

    return () => {
      (lobby as SubscriptionHandle).unsubscribe();
      g.detach();
    };
  }, [isActive, identity, getConnection]);

  // Open the MATCH scope while the local player is in a match. Re-subscribes on a
  // matchId change (load/rematch); the new sub is created before the old is dropped
  // so the world cache hands off without a gap. Torn down (back to lobby-only) the
  // moment matchId goes null — the renderer's table deletes clear the meshes.
  useEffect(() => {
    const conn = getConnection();
    if (!conn || !isActive || !identity || matchId === null) {
      setInMatch(false);
      return;
    }

    setInMatch(false);
    const world = conn
      .subscriptionBuilder()
      .onApplied(() => setInMatch(true))
      .subscribe(matchQueries(matchId, identity));

    return () => {
      (world as SubscriptionHandle).unsubscribe();
    };
  }, [isActive, identity, getConnection, matchId]);

  return { game, identity, isActive, matchId, ready, inMatch };
}
