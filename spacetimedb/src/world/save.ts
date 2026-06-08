// Impure save/load: the DB read/write half of the save system. The pure transforms
// (id remap, ref rewrite, backfill) live in shared/save.ts; this file walks the
// live tables into the mirror tables and back, scoped to one match. ctx is `any`
// here (typed at the reducer boundary) so the snapshot loops stay terse.
import {
  SAVE_SCHEMA_VERSION,
  buildIdRemap,
  rewriteRow,
  backfillRow,
} from "../../../shared/save.ts";
import { MatchStatus } from "../../../shared/match.ts";
import {
  unitsOfMatch,
  buildingsOfMatch,
  nodesOfMatch,
  playersOfMatch,
  aisOfMatch,
} from "./scope.ts";

// Strip the live-row primary/auto columns the mirror table adds back itself, and
// stamp the saveId. A mirror row is the live row's columns verbatim PLUS saveId;
// saveRowId is the mirror's own autoInc, inserted as 0n.
function mirror(saveId: bigint, row: any): any {
  return { ...row, saveRowId: 0n, saveId };
}

// Snapshot every row of `matchId` into the mirror tables under `saveId`. Reads
// each live table in a stable order (the table's own iteration); the mirror rows
// preserve the live entityIds/matchId untouched so a load can remap them.
export function snapshotMatch(ctx: any, saveId: bigint, matchId: bigint): void {
  // entity rows are scoped by membership: the union of every unit/building/node's
  // entity row (entity has a matchId column, so filter directly).
  for (const e of [...ctx.db.entity.iter()])
    if (e.matchId === matchId) ctx.db.saveEntity.insert(mirror(saveId, e));
  for (const u of unitsOfMatch(ctx, matchId))
    ctx.db.saveUnit.insert(mirror(saveId, u));
  for (const b of buildingsOfMatch(ctx, matchId))
    ctx.db.saveBuilding.insert(mirror(saveId, b));
  for (const n of nodesOfMatch(ctx, matchId))
    ctx.db.saveResourceNode.insert(mirror(saveId, n));
  for (const p of playersOfMatch(ctx, matchId))
    ctx.db.savePlayer.insert(mirror(saveId, p));
  for (const a of aisOfMatch(ctx, matchId))
    ctx.db.saveAi.insert(mirror(saveId, a));
  // garrison rows carry no matchId — scope by their sheltered unit being in-match.
  const unitIds = new Set<bigint>(
    unitsOfMatch(ctx, matchId).map((u: any) => u.entityId),
  );
  for (const g of [...ctx.db.garrison.iter()])
    if (unitIds.has(g.unit)) ctx.db.saveGarrison.insert(mirror(saveId, g));
  const m = ctx.db.match.matchId.find(matchId);
  if (m) ctx.db.saveMatchRow.insert(mirror(saveId, m));
}

// Drop every mirror row belonging to `saveId` (and the slot itself is dropped by
// the caller). Used when overwriting a same-name save or deleting one.
export function dropSaveRows(ctx: any, saveId: bigint): void {
  for (const r of [...ctx.db.saveEntity.saveId.filter(saveId)])
    ctx.db.saveEntity.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.saveUnit.saveId.filter(saveId)])
    ctx.db.saveUnit.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.saveBuilding.saveId.filter(saveId)])
    ctx.db.saveBuilding.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.saveResourceNode.saveId.filter(saveId)])
    ctx.db.saveResourceNode.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.savePlayer.saveId.filter(saveId)])
    ctx.db.savePlayer.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.saveAi.saveId.filter(saveId)])
    ctx.db.saveAi.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.saveGarrison.saveId.filter(saveId)])
    ctx.db.saveGarrison.saveRowId.delete(r.saveRowId);
  for (const r of [...ctx.db.saveMatchRow.saveId.filter(saveId)])
    ctx.db.saveMatchRow.saveRowId.delete(r.saveRowId);
}

// Strip the mirror-only columns (saveRowId, saveId) off a mirror row, leaving the
// live row shape. The remaining columns are exactly the live table's.
function unmirror(row: any): any {
  const { saveRowId: _s, saveId: _i, ...live } = row;
  return live;
}

// Largest entityId currently live across entity/unit/building/resource_node, so
// the remap can allocate fresh ids strictly past every existing one (loaded ids
// never collide with another match still in the world). entity covers them all
// (every game object has an entity row) but scan the typed tables too for safety.
function maxLiveEntityId(ctx: any): bigint {
  let max = 0n;
  for (const e of [...ctx.db.entity.iter()])
    if (e.entityId > max) max = e.entityId;
  return max;
}

// Rehydrate `saveId` into the LIVE tables under a brand-new matchId, controlled by
// `caller`. Returns the new matchId. Steps: backfill older saves → build a stable
// old→new entityId remap (fresh ids past every live id) → re-insert each table
// with remapped ids/refs and the new matchId. The saved player whose identity is
// the caller is restored verbatim (identity is stable across reconnect), so the
// caller owns the loaded match; bot identities are preserved as-is.
export function rehydrateSave(
  ctx: any,
  saveId: bigint,
  version: number,
  caller: any,
): bigint {
  const cfg = ctx.db.config.id.find(0);
  const newMatchId = cfg ? cfg.nextMatchId : 1n;

  const savedEntities = [...ctx.db.saveEntity.saveId.filter(saveId)].map(
    (r: any) => backfillRow("entity", unmirror(r), version),
  );
  const { map, nextId } = buildIdRemap(
    savedEntities.map((e: any) => e.entityId),
    maxLiveEntityId(ctx) + 1n,
  );

  // Reserve the consumed entityId range + the new matchId so nothing collides
  // with a future spawn. nextBotId is left alone (saved bot identities are kept).
  void nextId;
  if (cfg) ctx.db.config.id.update({ ...cfg, nextMatchId: newMatchId + 1n });

  for (const e of savedEntities)
    ctx.db.entity.insert(
      rewriteRow("entity", { ...e, matchId: newMatchId }, map),
    );
  for (const r of ctx.db.saveUnit.saveId.filter(saveId)) {
    const row = backfillRow("unit", unmirror(r), version);
    ctx.db.unit.insert(
      rewriteRow("unit", { ...row, matchId: newMatchId }, map),
    );
  }
  for (const r of ctx.db.saveBuilding.saveId.filter(saveId)) {
    const row = backfillRow("building", unmirror(r), version);
    ctx.db.building.insert(
      rewriteRow("building", { ...row, matchId: newMatchId }, map),
    );
  }
  for (const r of ctx.db.saveResourceNode.saveId.filter(saveId)) {
    const row = backfillRow("resource_node", unmirror(r), version);
    ctx.db.resourceNode.insert(
      rewriteRow("resource_node", { ...row, matchId: newMatchId }, map),
    );
  }
  for (const r of ctx.db.savePlayer.saveId.filter(saveId)) {
    const row = backfillRow("player", unmirror(r), version);
    // playerId is unique-autoInc on the live table — re-insert as 0 so the host
    // assigns a fresh one rather than colliding with another match's player.
    ctx.db.player.insert(
      rewriteRow("player", { ...row, playerId: 0, matchId: newMatchId }, map),
    );
  }
  for (const r of ctx.db.saveAi.saveId.filter(saveId)) {
    const row = backfillRow("ai", unmirror(r), version);
    // Re-home the bot under the live caller so teardown stays scoped to them.
    ctx.db.ai.insert(
      rewriteRow("ai", { ...row, host: caller, matchId: newMatchId }, map),
    );
  }

  for (const r of ctx.db.saveGarrison.saveId.filter(saveId)) {
    const row = backfillRow("garrison", unmirror(r), version);
    // slotId is autoInc on the live table — re-insert as 0 for a fresh id; both
    // building/unit refs remap onto the freshly-inserted rows above.
    ctx.db.garrison.insert(rewriteRow("garrison", { ...row, slotId: 0n }, map));
  }

  // Restore the match row itself under the new id, Active and hosted by the caller.
  // `players` is recomputed from the players actually restored above (a load may
  // drop bots whose identities collide, etc.), so the lobby count stays honest.
  const savedMatch = [...ctx.db.saveMatchRow.saveId.filter(saveId)][0];
  if (savedMatch) {
    const m = unmirror(savedMatch);
    const restoredPlayers = [...ctx.db.player.matchId.filter(newMatchId)]
      .length;
    ctx.db.match.insert({
      matchId: newMatchId,
      name: m.name,
      host: caller,
      status: MatchStatus.Active,
      seed: m.seed,
      preset: m.preset,
      players: restoredPlayers,
    });
  }
  return newMatchId;
}

export { SAVE_SCHEMA_VERSION };
