// The multiplayer lobby's open-match list. Reads the `match` table — already in
// the always-on lobby subscription scope (useGameSession) — and exposes only the
// joinable matches: status Active with room left (players < MAX_PLAYERS). The
// player count comes from the denormalized `match.players` column the module
// maintains (foundPlayer/clearMatchRows), so the lobby never has to subscribe to
// the player table across every running match — the match row alone carries it.
import { useTable } from "spacetimedb/react";
import { tables } from "../module_bindings";
import { MAX_PLAYERS, MatchStatus, mapPresetById } from "../../shared/index.ts";

export interface LobbyMatch {
  id: string; // matchId as a decimal string (bigint-safe for keys + reducer args)
  name: string;
  host: string; // host identity hex, to mark "your match" in the UI
  players: number;
  maxPlayers: number;
  preset: string; // map preset id
  presetLabel: string; // human label for the preset
}

export function useLobby(): LobbyMatch[] {
  const [matches] = useTable(
    tables.match.where((r) => r.status.eq(MatchStatus.Active)),
  );
  return matches
    .filter((m) => m.players < MAX_PLAYERS)
    .map((m) => ({
      id: m.matchId.toString(),
      name: m.name,
      host: m.host.toHexString(),
      players: m.players,
      maxPlayers: MAX_PLAYERS,
      preset: m.preset,
      presetLabel: mapPresetById(m.preset).label,
    }))
    .sort((a, b) => (a.id < b.id ? 1 : a.id > b.id ? -1 : 0)); // newest first
}
