import { t, SenderError } from "spacetimedb/server";
import { spacetimedb } from "../schema/db.ts";
import { callerMatchId, clearMatchRows } from "../world/scope.ts";
import {
  snapshotMatch,
  dropSaveRows,
  rehydrateSave,
  SAVE_SCHEMA_VERSION,
} from "../world/save.ts";

// save_slot is public but row-filtered: a connection sees only its own slots, so
// the save/load menu can subscribe to the whole table yet never leak another
// player's saves. Exported by name so the host registers the rule.
export const saveSlotVisibility = spacetimedb.clientVisibilityFilter.sql(
  "SELECT * FROM save_slot WHERE owner = :sender",
);

// Find the caller's existing save by name (the unique (owner,name) slot). Returns
// the slot row or null. Used by saveMatch to overwrite a same-name save in place.
function findSaveByName(ctx: any, owner: any, name: string): any | null {
  for (const s of ctx.db.saveSlot.by_owner.filter(owner))
    if (s.name === name) return s;
  return null;
}

// Snapshot the caller's current match into a named save slot. Overwrites a save of
// the same name (drops its old mirror rows first) so re-saving "before the siege"
// keeps one slot, not a pile. Deterministic: reads the live tables in table order
// and writes the mirror rows; timestamp comes from ctx (not a wall clock).
export const saveMatch = spacetimedb.reducer(
  { name: t.string() },
  (ctx, { name }) => {
    const matchId = callerMatchId(ctx, ctx.sender);
    if (matchId === null) throw new SenderError("not in a match");
    const label = name.trim() || "Save";

    const existing = findSaveByName(ctx, ctx.sender, label);
    if (existing) {
      dropSaveRows(ctx, existing.saveId);
      ctx.db.saveSlot.saveId.delete(existing.saveId);
    }

    const slot = ctx.db.saveSlot.insert({
      saveId: 0n,
      owner: ctx.sender,
      name: label,
      createdAt: ctx.timestamp,
      schemaVersion: SAVE_SCHEMA_VERSION,
    });
    snapshotMatch(ctx, slot.saveId, matchId);
  },
);

// Load a save into a fresh match the caller controls. Authorizes ownership, clears
// the caller's current match, then rehydrates the saved rows into the LIVE tables
// with remapped entityIds + rewritten cross-refs under a brand-new matchId. The
// saved player whose identity is the caller is restored verbatim, so the caller
// owns the loaded match (reconnect-resume: same identity/token loads their save).
export const loadMatch = spacetimedb.reducer(
  { saveId: t.u64() },
  (ctx, { saveId }) => {
    const slot = ctx.db.saveSlot.saveId.find(saveId);
    if (!slot) throw new SenderError("save not found");
    if (!slot.owner.equals(ctx.sender)) throw new SenderError("not your save");

    const prior = callerMatchId(ctx, ctx.sender);
    if (prior !== null) clearMatchRows(ctx, prior);

    rehydrateSave(ctx, saveId, slot.schemaVersion, ctx.sender);
  },
);

// Drop a save slot and every mirror row under it. Owner-only.
export const deleteSave = spacetimedb.reducer(
  { saveId: t.u64() },
  (ctx, { saveId }) => {
    const slot = ctx.db.saveSlot.saveId.find(saveId);
    if (!slot) return;
    if (!slot.owner.equals(ctx.sender)) throw new SenderError("not your save");
    dropSaveRows(ctx, saveId);
    ctx.db.saveSlot.saveId.delete(saveId);
  },
);
