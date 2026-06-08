// The caller's save slots, straight from the save_slot table — the subscription
// cache IS the source of truth (no local mirror). The table is row-filtered
// server-side to owner = :sender, so the client only ever sees its own saves.
import { useTable } from "spacetimedb/react";
import { tables } from "../module_bindings";

export interface SaveEntry {
  id: string; // saveId as a decimal string (bigint-safe for keys + reducer args)
  name: string;
  createdAt: number; // ms since epoch, for display
  schemaVersion: number;
}

export function useSaves(): SaveEntry[] {
  const [slots] = useTable(tables.saveSlot);
  return slots
    .map((s) => ({
      id: s.saveId.toString(),
      name: s.name,
      createdAt: Number(s.createdAt.microsSinceUnixEpoch / 1000n),
      schemaVersion: s.schemaVersion,
    }))
    .sort((a, b) => b.createdAt - a.createdAt); // newest first
}
