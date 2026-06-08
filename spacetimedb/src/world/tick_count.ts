// Singleton tick-rate instrument. One row (id=0) holds two monotonic counters;
// each hot scheduled system bumps its own counter exactly once per run (one cheap
// update/tick). Poll the row twice over a wall-clock window via `spacetime sql`
// and divide the delta by elapsed seconds to read the ACHIEVED tick rate.

const TICK_ROW_ID = 0;

// Create the singleton row if absent. Called from init and, defensively, from the
// bump helpers so a DB published before this table existed still starts counting.
export function ensureTickRow(ctx: any): void {
  if (!ctx.db.tickCount.id.find(TICK_ROW_ID))
    ctx.db.tickCount.insert({ id: TICK_ROW_ID, moveTicks: 0n, combatTicks: 0n });
}

export function bumpMoveTick(ctx: any): void {
  const row = ctx.db.tickCount.id.find(TICK_ROW_ID);
  if (!row) {
    ctx.db.tickCount.insert({ id: TICK_ROW_ID, moveTicks: 1n, combatTicks: 0n });
    return;
  }
  ctx.db.tickCount.id.update({ ...row, moveTicks: row.moveTicks + 1n });
}

export function bumpCombatTick(ctx: any): void {
  const row = ctx.db.tickCount.id.find(TICK_ROW_ID);
  if (!row) {
    ctx.db.tickCount.insert({ id: TICK_ROW_ID, moveTicks: 0n, combatTicks: 1n });
    return;
  }
  ctx.db.tickCount.id.update({ ...row, combatTicks: row.combatTicks + 1n });
}
