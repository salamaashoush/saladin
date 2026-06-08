import { MatchStatus, matchSimulates } from '../../../shared/match.ts';

// matchId-scoped row access + teardown. A match is the unit of isolation: every
// game-object row carries the matchId it belongs to, so "a match" is literally the
// set of rows sharing one id. These helpers REPLACE the old owner-only teardown
// (clearOwner/clearMatch) with id-scoped sweeps, while keeping a host fallback for
// legacy rows stamped matchId=0 before this iteration existed.

export function unitsOfMatch(ctx: any, matchId: bigint): any[] {
  return [...ctx.db.unit.iter()].filter((u: any) => u.matchId === matchId);
}

export function buildingsOfMatch(ctx: any, matchId: bigint): any[] {
  return [...ctx.db.building.iter()].filter((b: any) => b.matchId === matchId);
}

export function playersOfMatch(ctx: any, matchId: bigint): any[] {
  return [...ctx.db.player.iter()].filter((p: any) => p.matchId === matchId);
}

export function nodesOfMatch(ctx: any, matchId: bigint): any[] {
  return [...ctx.db.resourceNode.iter()].filter((n: any) => n.matchId === matchId);
}

export function aisOfMatch(ctx: any, matchId: bigint): any[] {
  return [...ctx.db.ai.iter()].filter((a: any) => a.matchId === matchId);
}

// True if a HUMAN (no ai row) other than `caller` shares `matchId` — used to guard
// world regeneration so one player's reset can't reshape a co-tenant's match.
export function otherHumansInMatch(
  ctx: any,
  matchId: bigint,
  caller: any
): boolean {
  for (const p of playersOfMatch(ctx, matchId))
    if (!p.identity.equals(caller) && !ctx.db.ai.identity.find(p.identity))
      return true;
  return false;
}

// Drop every garrison slot belonging to a unit in `unitIds` (match teardown). The
// garrison table has no matchId of its own — its scope is its unit's.
function clearGarrisonsForUnits(ctx: any, unitIds: Set<bigint>): void {
  for (const g of [...ctx.db.garrison.iter()])
    if (unitIds.has(g.unit)) ctx.db.garrison.slotId.delete(g.slotId);
}

// Drop every research row owned by a player in `owners` (match teardown).
function clearResearchForOwners(ctx: any, owners: Set<string>): void {
  for (const r of [...ctx.db.research.iter()])
    if (owners.has(r.owner.toHexString())) ctx.db.research.researchId.delete(r.researchId);
}

// Tear down a whole match: every unit, building, resource node, their entity rows,
// garrison slots, research rows, ai rows, player rows, and the match row itself.
// Idempotent — safe to call on an already-cleared match. Scoped purely by matchId
// so another match's rows are never touched.
export function clearMatchRows(ctx: any, matchId: bigint): void {
  const units = unitsOfMatch(ctx, matchId);
  const unitIds = new Set<bigint>(units.map((u) => u.entityId));
  clearGarrisonsForUnits(ctx, unitIds);
  for (const u of units) {
    ctx.db.unit.entityId.delete(u.entityId);
    ctx.db.entity.entityId.delete(u.entityId);
  }
  for (const b of buildingsOfMatch(ctx, matchId)) {
    ctx.db.building.entityId.delete(b.entityId);
    ctx.db.entity.entityId.delete(b.entityId);
  }
  for (const n of nodesOfMatch(ctx, matchId)) {
    ctx.db.resourceNode.entityId.delete(n.entityId);
    ctx.db.entity.entityId.delete(n.entityId);
  }
  const owners = new Set<string>(
    playersOfMatch(ctx, matchId).map((p) => p.identity.toHexString())
  );
  clearResearchForOwners(ctx, owners);
  for (const a of aisOfMatch(ctx, matchId)) ctx.db.ai.identity.delete(a.identity);
  for (const p of playersOfMatch(ctx, matchId))
    ctx.db.player.identity.delete(p.identity);
  if (ctx.db.match.matchId.find(matchId)) ctx.db.match.matchId.delete(matchId);
}

// The match a caller belongs to: their player row's matchId, else the match they
// host (covers the moment before a player row exists, and legacy host scoping).
export function callerMatchId(ctx: any, caller: any): bigint | null {
  const p = ctx.db.player.identity.find(caller);
  if (p) return p.matchId;
  for (const m of [...ctx.db.match.iter()])
    if (m.host.equals(caller)) return m.matchId;
  return null;
}

// The set of matchIds currently simulating (status Active). Resolved once per tick
// so the scheduled systems can cheaply skip rows in Paused/Ended matches. Legacy
// rows (matchId 0) have no match row; they are treated as Active so a pre-iteration
// world that predates first-class matches keeps simulating.
export function activeMatchIds(ctx: any): Set<bigint> {
  // 0n is always Active (legacy/global rows from before first-class matches); a real
  // match row never has id 0, so seeding it here can't collide with a paused match.
  const active = new Set<bigint>([0n]);
  for (const m of [...ctx.db.match.iter()])
    if (matchSimulates(m.status)) active.add(m.matchId);
  return active;
}

export { MatchStatus };
