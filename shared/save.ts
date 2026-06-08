// Pure, deterministic save/load core. The module's saveMatch/loadMatch reducers
// are thin shells around the helpers here: snapshot a match's rows into mirror
// tables, then on load remap the saved (autoInc) entityIds onto fresh ids and
// rewrite every cross-reference so a loaded world has no dangling pointers.
//
// Nothing in this file touches a database, a clock, or randomness — it is a set
// of plain transforms over rows, so it is fully unit-testable and shared by the
// module. The actual table reads/writes live in the reducer.

import { MATCH_SCOPED_TABLES } from "./match.ts";

// Bump when a mirror table's column set changes in a way older saves can't be
// rehydrated verbatim. loadMatch reads save_slot.schemaVersion and runs the
// backfill ladder up to SAVE_SCHEMA_VERSION before rehydrating.
export const SAVE_SCHEMA_VERSION = 1;

// The live tables a save copies, in dependency order: `entity` first (positions
// the others reference), then the typed rows. Drives the snapshot loop and the
// parity test. These are exactly the match-scoped tables (every row carries the
// caller's matchId) — keep this list in lockstep with MATCH_SCOPED_TABLES.
export const SAVED_TABLES = [
  "entity",
  "unit",
  "building",
  "resource_node",
  "player",
  "ai",
] as const;
export type SavedTable = (typeof SAVED_TABLES)[number];

// Every column of each LIVE table that a save must carry, mirrored from
// schema/tables.ts. This is the contract the snapshot/rehydrate code copies and
// the parity test checks: a mirror table must hold exactly these columns PLUS the
// save bookkeeping ones (saveRowId, saveId). Adding a live column means adding it
// here, to the mirror table, and (if it changes the wire shape) bumping
// SAVE_SCHEMA_VERSION. `garrison` is included because it is saved too (scoped via
// its unit) even though it is not match-scoped.
export const LIVE_COLUMNS: Record<string, readonly string[]> = {
  entity: ["entityId", "x", "y", "facing", "matchId"],
  unit: [
    "entityId",
    "owner",
    "kind",
    "targetX",
    "targetY",
    "hasTarget",
    "speed",
    "gatherState",
    "targetNode",
    "carrying",
    "carryType",
    "harvestTimer",
    "hp",
    "attackTarget",
    "attackCooldown",
    "stance",
    "morale",
    "routing",
    "homeX",
    "homeY",
    "garrisonedIn",
    "path",
    "pathIdx",
    "matchId",
  ],
  building: [
    "entityId",
    "owner",
    "kind",
    "hp",
    "cooldown",
    "rallyX",
    "rallyY",
    "matchId",
  ],
  resource_node: ["entityId", "resType", "remaining", "matchId"],
  player: [
    "identity",
    "playerId",
    "name",
    "faction",
    "wood",
    "stone",
    "food",
    "gold",
    "color",
    "online",
    "keepEntity",
    "defeated",
    "slot",
    "techMask",
    "matchId",
  ],
  ai: [
    "identity",
    "host",
    "difficulty",
    "decisionCd",
    "waveTimer",
    "phase",
    "scoutId",
    "threatTimer",
    "matchId",
  ],
  garrison: ["slotId", "building", "unit", "owner"],
  match: ["matchId", "name", "host", "status", "seed", "preset"],
};

// Columns a mirror table adds on top of its live columns: its own autoInc id and
// the saveId that groups one save's rows. The parity check expects a mirror's
// column set to be exactly LIVE_COLUMNS[table] ∪ MIRROR_EXTRA_COLUMNS.
export const MIRROR_EXTRA_COLUMNS = ["saveRowId", "saveId"] as const;

// The expected full column set of a mirror table for `table`: every live column
// plus the save bookkeeping columns. A test asserts this equals the columns the
// schema actually declares, so the mirror can never silently drop a live column.
export function expectedMirrorColumns(table: string): string[] {
  const live = LIVE_COLUMNS[table];
  if (!live) return [];
  return [...MIRROR_EXTRA_COLUMNS, ...live];
}

// Every entityId-bearing reference column named in ENTITY_REF_COLUMNS for a table
// must be a real column of that table — otherwise a rewrite would target nothing.
// Asserted by the parity test so the ref map can't drift from the schema.
export function refColumnsAreLive(table: string): boolean {
  const cols = ENTITY_REF_COLUMNS[table];
  const live = LIVE_COLUMNS[table];
  if (!cols) return true;
  if (!live) return false;
  return cols.every((c) => live.includes(c));
}

// The save list IS the match-scoped table list — a save must cover every table
// that carries a matchId, no more, no less. Guards against the two drifting.
export function savedTablesMatchScoped(): boolean {
  const a = [...SAVED_TABLES].sort();
  const b = [...MATCH_SCOPED_TABLES].sort();
  return a.length === b.length && a.every((x, i) => x === b[i]);
}

// Sentinel meaning "no reference" across every entityId pointer column. A 0 id
// always remaps to 0 so an absent target stays absent.
export const NO_REF = 0n;

// Which columns on each table hold an entityId that must be rewritten on load.
// `entityId` itself is the row's own id (remapped to its fresh value); the rest
// are pointers into other rows. attackTarget/targetNode/garrisonedIn can point at
// a unit, resource node, or building respectively; keepEntity/scoutId at a
// building/unit. Every value is looked up in the SAME old→new map because all
// entityIds share one global autoInc space.
export const ENTITY_REF_COLUMNS: Record<string, readonly string[]> = {
  entity: ["entityId"],
  unit: ["entityId", "targetNode", "attackTarget", "garrisonedIn"],
  building: ["entityId"],
  resource_node: ["entityId"],
  player: ["keepEntity"],
  ai: ["scoutId"],
  // garrison is saved (not match-scoped, scoped via its unit) but both its refs
  // are entityIds into building/unit rows that DO get remapped on load.
  garrison: ["building", "unit"],
};

// A consistent old→new entityId map. Allocate fresh ids by walking the saved
// `entity` rows in a STABLE order (sorted by old id) so the mapping is
// deterministic and reproducible. `nextId` is where fresh allocation starts
// (the module passes a value past every live id so loaded ids never collide).
// Returns the map plus the next free id after the batch.
export function buildIdRemap(
  savedEntityIds: bigint[],
  nextId: bigint,
): { map: Map<bigint, bigint>; nextId: bigint } {
  const map = new Map<bigint, bigint>([[NO_REF, NO_REF]]);
  let cursor = nextId;
  for (const oldId of [...savedEntityIds].sort((a, b) =>
    a < b ? -1 : a > b ? 1 : 0,
  )) {
    if (map.has(oldId)) continue;
    map.set(oldId, cursor);
    cursor += 1n;
  }
  return { map, nextId: cursor };
}

// Translate one entityId through the map. An id with no mapping (a dangling
// pointer in the save) collapses to NO_REF rather than leaking a stale id — so a
// rehydrated world can never reference a row that wasn't restored.
export function remapId(map: Map<bigint, bigint>, id: bigint): bigint {
  return map.get(id) ?? NO_REF;
}

// Rewrite every entityId-bearing column of a saved row of `table` through the
// remap, returning a NEW row (the input is not mutated). Columns the table does
// not declare as references pass through untouched. Used for both the row's own
// id and its cross-references in one pass.
export function rewriteRow<T extends Record<string, unknown>>(
  table: string,
  row: T,
  map: Map<bigint, bigint>,
): T {
  const cols = ENTITY_REF_COLUMNS[table];
  if (!cols || cols.length === 0) return { ...row };
  const out: Record<string, unknown> = { ...row };
  for (const c of cols) {
    if (typeof out[c] === "bigint") out[c] = remapId(map, out[c] as bigint);
  }
  return out as T;
}

// True if `row` (of `table`) has no dangling entityId pointer after a remap: every
// reference column is either NO_REF or present as a value in the map. Used by the
// round-trip test to assert referential integrity.
export function rowRefsResolved(
  table: string,
  row: Record<string, unknown>,
  map: Map<bigint, bigint>,
): boolean {
  const cols = ENTITY_REF_COLUMNS[table];
  if (!cols) return true;
  const live = new Set<bigint>(map.values());
  for (const c of cols) {
    const v = row[c];
    if (typeof v !== "bigint") continue;
    if (v === NO_REF) continue;
    if (!live.has(v)) return false;
  }
  return true;
}

// Backfill a saved row to the current schema. Older saves (schemaVersion < V)
// may be missing columns added since; fill them with the same defaults a fresh
// row gets so rehydration always produces a complete, valid row. Adding a column
// to a live table means adding its default here AND bumping SAVE_SCHEMA_VERSION.
export function backfillRow(
  table: string,
  row: Record<string, unknown>,
  version: number,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...row };
  // v1 is the first versioned schema; every current column already exists in it,
  // so there is nothing to backfill yet. The ladder below is where future
  // `if (version < N) { … }` blocks go, newest last.
  void version;
  void table;
  return out;
}
