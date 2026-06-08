import { describe, it, expect } from 'vitest';
import {
  MatchStatus,
  matchSimulates,
  carriesMatchId,
  MATCH_SCOPED_TABLES,
} from './match.ts';
import { clearMatchRows } from '../spacetimedb/src/world/scope.ts';

describe('MatchStatus enum', () => {
  it('has the three lifecycle states with distinct values', () => {
    expect(MatchStatus.Active).toBe(0);
    expect(MatchStatus.Paused).toBe(1);
    expect(MatchStatus.Ended).toBe(2);
    const vals = [MatchStatus.Active, MatchStatus.Paused, MatchStatus.Ended];
    expect(new Set(vals).size).toBe(3);
  });
});

describe('matchSimulates (scope predicate)', () => {
  it('only an Active match advances under the scheduled systems', () => {
    expect(matchSimulates(MatchStatus.Active)).toBe(true);
    expect(matchSimulates(MatchStatus.Paused)).toBe(false);
    expect(matchSimulates(MatchStatus.Ended)).toBe(false);
  });

  it('an unknown status never simulates (fail closed)', () => {
    expect(matchSimulates(99)).toBe(false);
  });
});

describe('carriesMatchId (which tables are match-scoped)', () => {
  it('the six game-object tables carry a matchId', () => {
    for (const tbl of MATCH_SCOPED_TABLES) expect(carriesMatchId(tbl)).toBe(true);
    expect(MATCH_SCOPED_TABLES).toContain('unit');
    expect(MATCH_SCOPED_TABLES).toContain('building');
    expect(MATCH_SCOPED_TABLES).toContain('player');
    expect(MATCH_SCOPED_TABLES).toContain('ai');
    expect(MATCH_SCOPED_TABLES).toContain('entity');
    expect(MATCH_SCOPED_TABLES).toContain('resource_node');
  });

  it('tables without a matchId column are not scoped', () => {
    expect(carriesMatchId('garrison')).toBe(false); // scoped via its unit
    expect(carriesMatchId('research')).toBe(false); // scoped via its owner
    expect(carriesMatchId('config')).toBe(false);
    expect(carriesMatchId('match')).toBe(false); // the match row IS the scope
    expect(carriesMatchId('shot')).toBe(false);
  });
});

// ── teardown isolation: clearMatchRows(A) must leave match B fully intact ──────

// A throwaway Identity stand-in: hex-keyed, with the .equals/.toHexString the
// module helpers call. Enough to drive clearMatchRows without a live database.
class Id {
  constructor(public readonly hex: string) {}
  equals(o: Id): boolean {
    return o instanceof Id && o.hex === this.hex;
  }
  toHexString(): string {
    return this.hex;
  }
}

// Minimal in-memory table: insert/iter and the pk-keyed find/delete the helpers use.
class FakeTable<T> {
  rows = new Map<unknown, T>();
  constructor(private pk: (r: T) => unknown) {}
  insert(r: T): void {
    this.rows.set(this.pk(r), r);
  }
  iter(): IterableIterator<T> {
    return this.rows.values();
  }
  find(key: unknown): T | undefined {
    return this.rows.get(key);
  }
  delete(key: unknown): void {
    this.rows.delete(key);
  }
}

// Wrap a FakeTable so it exposes the pk accessor name the module code uses, e.g.
// ctx.db.unit.entityId.find / .delete and ctx.db.unit.iter().
function accessor<T>(table: FakeTable<T>, pkName: string) {
  return {
    iter: () => table.iter(),
    insert: (r: T) => table.insert(r),
    [pkName]: {
      find: (k: unknown) => table.find(k) ?? null,
      delete: (k: unknown) => table.delete(k),
    },
  };
}

function makeCtx() {
  const entity = new FakeTable<any>((r) => r.entityId);
  const unit = new FakeTable<any>((r) => r.entityId);
  const building = new FakeTable<any>((r) => r.entityId);
  const resourceNode = new FakeTable<any>((r) => r.entityId);
  const player = new FakeTable<any>((r) => r.identity.hex);
  const ai = new FakeTable<any>((r) => r.identity.hex);
  const research = new FakeTable<any>((r) => r.researchId);
  const garrison = new FakeTable<any>((r) => r.slotId);
  const match = new FakeTable<any>((r) => r.matchId);

  // player/ai are keyed by identity hex but the module calls .identity.find(id)
  // with an Id object — translate via a thin custom accessor.
  const idAccessor = (table: FakeTable<any>) => ({
    iter: () => table.iter(),
    insert: (r: any) => table.insert(r),
    identity: {
      find: (id: Id) => table.find(id.hex) ?? null,
      delete: (id: Id) => table.delete(id.hex),
    },
  });

  const db = {
    entity: accessor(entity, 'entityId'),
    unit: accessor(unit, 'entityId'),
    building: accessor(building, 'entityId'),
    resourceNode: accessor(resourceNode, 'entityId'),
    player: idAccessor(player),
    ai: idAccessor(ai),
    research: accessor(research, 'researchId'),
    garrison: accessor(garrison, 'slotId'),
    match: accessor(match, 'matchId'),
  };
  return { db };
}

// Seed one full match (owner human + a bot + a keep + a unit + a node + research +
// a garrison slot + the match row), all stamped with `matchId`.
function seedMatch(ctx: any, matchId: bigint, base: bigint) {
  const human = new Id(`h${matchId}`);
  const bot = new Id(`b${matchId}`);
  ctx.db.match.insert({ matchId, name: `m${matchId}`, host: human, status: 0, seed: 1, preset: 'x' });
  ctx.db.player.insert({ identity: human, matchId, slot: 0 });
  ctx.db.player.insert({ identity: bot, matchId, slot: 1 });
  ctx.db.ai.insert({ identity: bot, host: human, matchId });
  // keep (building) + its entity
  ctx.db.entity.insert({ entityId: base + 0n, x: 0, y: 0, facing: 0, matchId });
  ctx.db.building.insert({ entityId: base + 0n, owner: human, kind: 0, matchId });
  // a unit + its entity
  ctx.db.entity.insert({ entityId: base + 1n, x: 1, y: 1, facing: 0, matchId });
  ctx.db.unit.insert({ entityId: base + 1n, owner: human, kind: 0, matchId });
  // a resource node + its entity
  ctx.db.entity.insert({ entityId: base + 2n, x: 2, y: 2, facing: 0, matchId });
  ctx.db.resourceNode.insert({ entityId: base + 2n, resType: 0, remaining: 100, matchId });
  // research owned by the human + a garrison slot sheltering the unit
  ctx.db.research.insert({ researchId: base + 3n, owner: human, tech: 0, progress: 0.5, done: false });
  ctx.db.garrison.insert({ slotId: base + 4n, building: base + 0n, unit: base + 1n, owner: human });
}

describe('clearMatchRows teardown isolation', () => {
  it('tears down match A entirely while leaving match B untouched', () => {
    const ctx = makeCtx();
    seedMatch(ctx, 10n, 1000n);
    seedMatch(ctx, 20n, 2000n);

    const count = (t: any) => [...t.iter()].length;
    // both matches fully present up front
    expect(count(ctx.db.match)).toBe(2);
    expect(count(ctx.db.player)).toBe(4);
    expect(count(ctx.db.unit)).toBe(2);
    expect(count(ctx.db.building)).toBe(2);
    expect(count(ctx.db.resourceNode)).toBe(2);
    expect(count(ctx.db.entity)).toBe(6);
    expect(count(ctx.db.research)).toBe(2);
    expect(count(ctx.db.ai)).toBe(2);
    expect(count(ctx.db.garrison)).toBe(2);

    clearMatchRows(ctx, 10n);

    // match A is gone…
    expect(ctx.db.match.matchId.find(10n)).toBeNull();
    const remainingMatch = [...ctx.db.match.iter()];
    expect(remainingMatch).toHaveLength(1);
    expect(remainingMatch[0].matchId).toBe(20n);

    // …and every row it owned with it
    const allMatchIds = (t: any) => [...t.iter()].map((r: any) => r.matchId);
    expect(allMatchIds(ctx.db.unit)).toEqual([20n]);
    expect(allMatchIds(ctx.db.building)).toEqual([20n]);
    expect(allMatchIds(ctx.db.resourceNode)).toEqual([20n]);
    expect(allMatchIds(ctx.db.player)).toEqual([20n, 20n]);
    expect(allMatchIds(ctx.db.ai)).toEqual([20n]);
    expect(allMatchIds(ctx.db.entity)).toEqual([20n, 20n, 20n]);

    // research + garrison (no matchId column) cleared via their owner/unit
    expect([...ctx.db.research.iter()].map((r: any) => r.researchId)).toEqual([2003n]);
    expect([...ctx.db.garrison.iter()].map((g: any) => g.slotId)).toEqual([2004n]);

    // match B's rows are byte-for-byte intact
    expect(ctx.db.match.matchId.find(20n)).not.toBeNull();
    expect(ctx.db.building.entityId.find(2000n)).not.toBeNull();
    expect(ctx.db.unit.entityId.find(2001n)).not.toBeNull();
    expect(ctx.db.resourceNode.entityId.find(2002n)).not.toBeNull();
  });

  it('is idempotent — clearing an already-empty match is a no-op', () => {
    const ctx = makeCtx();
    seedMatch(ctx, 20n, 2000n);
    clearMatchRows(ctx, 10n); // never existed
    expect([...ctx.db.match.iter()]).toHaveLength(1);
    expect([...ctx.db.unit.iter()]).toHaveLength(1);
  });
});
