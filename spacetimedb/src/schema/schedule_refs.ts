// Late-bound reducer references for scheduled tables. Scheduled `table()` defs
// live in tables.ts (which must not import the system reducers — doing so would
// pull the schema's dependency graph through `spacetimedb.reducer(...)` and read
// `spacetimedb` before db.ts assigns it). The system files populate these slots
// at module-load time; the SDK invokes each scheduled table's thunk only during
// schedule resolution, long after every module has finished evaluating, so the
// slots are always filled by the time a thunk reads one.
export const scheduleRefs: {
  moveUnits?: unknown;
  unitAi?: unknown;
  combatTick?: unknown;
  aiBrain?: unknown;
  economySystem?: unknown;
  researchSystem?: unknown;
} = {};
