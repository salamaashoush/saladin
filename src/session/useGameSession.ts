// Owns the Three.js scene + the live SpacetimeDB connection: mounts the renderer
// into a container, attaches the connection, and subscribes to the world tables.
// Returns the pieces the UI layer needs — it never derives game state itself.
import { useEffect, useRef, useState, type RefObject } from "react";
import { useSpacetimeDB } from "spacetimedb/react";
import type { Identity } from "spacetimedb";
import { tables, type DbConnection } from "../module_bindings";
import { SaladinGame } from "../game/SaladinGame";

export interface GameSession {
  game: SaladinGame | null;
  identity?: Identity;
  isActive: boolean;
  ready: boolean;
}

const WORLD_TABLES = [
  tables.entity,
  tables.unit,
  tables.building,
  tables.garrison,
  tables.resourceNode,
  tables.player,
  tables.config,
  tables.shot,
  tables.research,
  tables.saveSlot, // the caller's save slots (row-filtered to owner server-side)
];

export function useGameSession(
  containerRef: RefObject<HTMLDivElement | null>,
): GameSession {
  const { identity, isActive } = useSpacetimeDB();
  const { getConnection } = useSpacetimeDB() as unknown as {
    getConnection: () => DbConnection | null;
  };

  const gameRef = useRef<SaladinGame | null>(null);
  const [game, setGame] = useState<SaladinGame | null>(null);
  const [ready, setReady] = useState(false);

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

  useEffect(() => {
    const conn = getConnection();
    const g = gameRef.current;
    if (!conn || !isActive || !identity || !g) return;

    g.setIdentity(identity.toHexString());
    g.attach(conn as never);

    const sub = conn
      .subscriptionBuilder()
      .onApplied(() => setReady(true))
      .subscribe(WORLD_TABLES);

    return () => {
      (sub as { unsubscribe?: () => void }).unsubscribe?.();
      g.detach();
    };
  }, [isActive, identity, getConnection]);

  return { game, identity, isActive, ready };
}
