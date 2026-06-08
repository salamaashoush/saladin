// Derives the current player's match state from the live tables. The cache IS
// the source of truth (TanStack-style) — no local mirrors. `inGame` and
// `outcome` drive the top-level phase routing in App.
import { useTable } from 'spacetimedb/react';
import type { Identity } from 'spacetimedb';
import { tables } from '../module_bindings';
import { UnitKind, BUILDING_DEFS, FACTION_LABELS } from '../../shared/index.ts';

export type Outcome = 'victory' | 'defeat' | null;

export interface MatchState {
  inGame: boolean;
  name: string;
  faction: string;
  wood: number;
  peasants: number;
  soldiers: number;
  pop: number;
  cap: number;
  outcome: Outcome;
}

export function useMatch(identity?: Identity): MatchState {
  const [players] = useTable(tables.player);
  const [units] = useTable(tables.unit);
  const [buildings] = useTable(tables.building);

  const me = identity
    ? players.find((p) => p.identity.isEqual(identity))
    : undefined;
  const myUnits = identity
    ? units.filter((u) => u.owner.isEqual(identity))
    : [];
  const peasants = myUnits.filter((u) => u.kind === UnitKind.Peasant).length;
  const cap = identity
    ? buildings
        .filter((b) => b.owner.isEqual(identity))
        .reduce((s, b) => s + (BUILDING_DEFS[b.kind as 0]?.pop ?? 0), 0)
    : 0;

  const rivals = identity
    ? players.filter((p) => !p.identity.isEqual(identity))
    : [];
  const outcome: Outcome = me?.defeated
    ? 'defeat'
    : rivals.length > 0 && rivals.every((p) => p.defeated)
      ? 'victory'
      : null;

  return {
    inGame: !!me,
    name: me?.name || 'Commander',
    faction: me ? (FACTION_LABELS[me.faction as 0 | 1] ?? 'Ayyubid') : 'Ayyubid',
    wood: me?.wood ?? 0,
    peasants,
    soldiers: myUnits.length - peasants,
    pop: myUnits.length,
    cap,
    outcome,
  };
}
