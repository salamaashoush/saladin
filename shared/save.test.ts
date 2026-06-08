import { describe, it, expect } from "vitest";
import {
  SAVE_SCHEMA_VERSION,
  SAVED_TABLES,
  savedTablesMatchScoped,
  NO_REF,
  ENTITY_REF_COLUMNS,
  LIVE_COLUMNS,
  MIRROR_EXTRA_COLUMNS,
  expectedMirrorColumns,
  refColumnsAreLive,
  buildIdRemap,
  remapId,
  rewriteRow,
  rowRefsResolved,
  backfillRow,
} from "./save.ts";
import { MATCH_SCOPED_TABLES } from "./match.ts";

// ── schema descriptors ────────────────────────────────────────────────────────

describe("save schema descriptors", () => {
  it("SAVE_SCHEMA_VERSION is a positive integer", () => {
    expect(Number.isInteger(SAVE_SCHEMA_VERSION)).toBe(true);
    expect(SAVE_SCHEMA_VERSION).toBeGreaterThan(0);
  });

  it("the saved tables are exactly the match-scoped tables", () => {
    expect(savedTablesMatchScoped()).toBe(true);
    expect([...SAVED_TABLES].sort()).toEqual([...MATCH_SCOPED_TABLES].sort());
  });

  it("every saved table has a live column list", () => {
    for (const t of SAVED_TABLES) expect(LIVE_COLUMNS[t]).toBeDefined();
  });

  it("every ref column is a real column of its table (no dangling rewrite target)", () => {
    for (const t of Object.keys(ENTITY_REF_COLUMNS)) {
      expect(refColumnsAreLive(t)).toBe(true);
    }
  });
});

// ── mirror ↔ live column parity ───────────────────────────────────────────────
//
// The mirror tables are declared by hand in schema/tables.ts; these expectations
// are the contract. The list below is the column set each MIRROR table actually
// declares (kept in lockstep with schema/tables.ts). The parity assertion proves
// every live column is mirrored plus exactly the save bookkeeping columns — so a
// new live column can't silently drop out of a save.
const MIRROR_COLUMNS: Record<string, string[]> = {
  entity: ["saveRowId", "saveId", "entityId", "x", "y", "facing", "matchId", "cell"],
  unit: [
    "saveRowId",
    "saveId",
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
    "saveRowId",
    "saveId",
    "entityId",
    "owner",
    "kind",
    "hp",
    "cooldown",
    "rallyX",
    "rallyY",
    "matchId",
  ],
  resource_node: [
    "saveRowId",
    "saveId",
    "entityId",
    "resType",
    "remaining",
    "matchId",
  ],
  player: [
    "saveRowId",
    "saveId",
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
    "saveRowId",
    "saveId",
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
  garrison: ["saveRowId", "saveId", "slotId", "building", "unit", "owner"],
  match: [
    "saveRowId",
    "saveId",
    "matchId",
    "name",
    "host",
    "status",
    "seed",
    "preset",
    "players",
  ],
};

describe("mirror table parity (mirror columns == live columns + bookkeeping)", () => {
  for (const table of Object.keys(LIVE_COLUMNS)) {
    it(`save_${table} mirrors every live column of ${table}, plus exactly saveRowId+saveId`, () => {
      const mirror = MIRROR_COLUMNS[table];
      expect(mirror, `MIRROR_COLUMNS missing ${table}`).toBeDefined();

      // every live column is present in the mirror
      for (const col of LIVE_COLUMNS[table]) expect(mirror).toContain(col);

      // the mirror's extra columns are exactly the bookkeeping pair
      const extra = mirror.filter((c) => !LIVE_COLUMNS[table].includes(c));
      expect(extra.sort()).toEqual([...MIRROR_EXTRA_COLUMNS].sort());

      // and the full set matches expectedMirrorColumns (order-independent)
      expect([...mirror].sort()).toEqual(
        [...expectedMirrorColumns(table)].sort(),
      );

      // no stray columns: mirror has live + 2 bookkeeping, nothing more
      expect(mirror.length).toBe(
        LIVE_COLUMNS[table].length + MIRROR_EXTRA_COLUMNS.length,
      );
    });
  }
});

// ── entityId remap ────────────────────────────────────────────────────────────

describe("buildIdRemap", () => {
  it("maps NO_REF to itself and assigns fresh sequential ids past nextId", () => {
    const { map, nextId } = buildIdRemap([10n, 11n, 12n], 100n);
    expect(remapId(map, NO_REF)).toBe(NO_REF);
    expect(map.get(10n)).toBe(100n);
    expect(map.get(11n)).toBe(101n);
    expect(map.get(12n)).toBe(102n);
    expect(nextId).toBe(103n);
  });

  it("is deterministic regardless of input order (sorted allocation)", () => {
    const a = buildIdRemap([12n, 10n, 11n], 100n).map;
    const b = buildIdRemap([11n, 12n, 10n], 100n).map;
    expect(a.get(10n)).toBe(b.get(10n));
    expect(a.get(11n)).toBe(b.get(11n));
    expect(a.get(12n)).toBe(b.get(12n));
    // smallest old id always gets the smallest fresh id
    expect(a.get(10n)).toBe(100n);
    expect(a.get(12n)).toBe(102n);
  });

  it("dedupes repeated ids and never reuses a fresh id", () => {
    const { map, nextId } = buildIdRemap([5n, 5n, 7n], 50n);
    expect(map.get(5n)).toBe(50n);
    expect(map.get(7n)).toBe(51n);
    expect(nextId).toBe(52n);
    // every mapped (non-NO_REF) target is unique
    const targets = [...map.entries()]
      .filter(([k]) => k !== NO_REF)
      .map(([, v]) => v);
    expect(new Set(targets).size).toBe(targets.length);
  });

  it("fresh ids never collide with the live ids they start past", () => {
    const liveMax = 999n;
    const { map } = buildIdRemap([1n, 2n, 3n], liveMax + 1n);
    for (const v of map.values())
      if (v !== NO_REF) expect(v).toBeGreaterThan(liveMax);
  });
});

// ── cross-reference rewrite ───────────────────────────────────────────────────

describe("rewriteRow (cross-reference rewrite)", () => {
  it("rewrites a unit row id and all of its entityId pointers consistently", () => {
    const { map } = buildIdRemap([1n, 2n, 3n, 4n], 1000n);
    // unit#1 targets node#2, attacks unit#3, garrisoned in building#4
    const saved = {
      entityId: 1n,
      targetNode: 2n,
      attackTarget: 3n,
      garrisonedIn: 4n,
      owner: "h",
      kind: 0,
    };
    const out = rewriteRow("unit", saved, map);
    expect(out.entityId).toBe(map.get(1n));
    expect(out.targetNode).toBe(map.get(2n));
    expect(out.attackTarget).toBe(map.get(3n));
    expect(out.garrisonedIn).toBe(map.get(4n));
    // non-ref columns untouched, input not mutated
    expect(out.owner).toBe("h");
    expect(out.kind).toBe(0);
    expect(saved.entityId).toBe(1n);
  });

  it("keeps NO_REF pointers as NO_REF (idle unit, no targets)", () => {
    const { map } = buildIdRemap([1n], 1000n);
    const out = rewriteRow(
      "unit",
      {
        entityId: 1n,
        targetNode: NO_REF,
        attackTarget: NO_REF,
        garrisonedIn: NO_REF,
      },
      map,
    );
    expect(out.entityId).toBe(map.get(1n));
    expect(out.targetNode).toBe(NO_REF);
    expect(out.attackTarget).toBe(NO_REF);
    expect(out.garrisonedIn).toBe(NO_REF);
  });

  it("rewrites player.keepEntity and ai.scoutId and garrison refs", () => {
    const { map } = buildIdRemap([7n, 8n, 9n], 1000n);
    expect(rewriteRow("player", { keepEntity: 7n }, map).keepEntity).toBe(
      map.get(7n),
    );
    expect(rewriteRow("ai", { scoutId: 8n }, map).scoutId).toBe(map.get(8n));
    const g = rewriteRow(
      "garrison",
      { building: 7n, unit: 9n, slotId: 0n },
      map,
    );
    expect(g.building).toBe(map.get(7n));
    expect(g.unit).toBe(map.get(9n));
  });

  it("a dangling saved pointer collapses to NO_REF, never a stale id", () => {
    // unit#1 saved with attackTarget=99 which was NOT in the save (its target
    // died/was never captured). After remap it must not reference id 99.
    const { map } = buildIdRemap([1n], 1000n);
    const out = rewriteRow(
      "unit",
      {
        entityId: 1n,
        targetNode: NO_REF,
        attackTarget: 99n,
        garrisonedIn: NO_REF,
      },
      map,
    );
    expect(out.attackTarget).toBe(NO_REF);
    expect(out.attackTarget).not.toBe(99n);
  });
});

// ── referential integrity over a whole save ───────────────────────────────────

describe("rowRefsResolved (no dangling refs after rehydrate)", () => {
  it("every rewritten row in a self-consistent save resolves", () => {
    const ids = [1n, 2n, 3n, 4n];
    const { map } = buildIdRemap(ids, 1000n);
    const rows = [
      rewriteRow("entity", { entityId: 1n }, map),
      rewriteRow(
        "unit",
        { entityId: 1n, targetNode: 2n, attackTarget: 3n, garrisonedIn: 4n },
        map,
      ),
      rewriteRow("resource_node", { entityId: 2n }, map),
      rewriteRow("building", { entityId: 4n }, map),
      rewriteRow("player", { keepEntity: 4n }, map),
      rewriteRow("garrison", { building: 4n, unit: 1n, slotId: 0n }, map),
    ];
    const tables = [
      "entity",
      "unit",
      "resource_node",
      "building",
      "player",
      "garrison",
    ];
    rows.forEach((r, i) =>
      expect(rowRefsResolved(tables[i], r, map)).toBe(true),
    );
  });

  it("flags a row that still points outside the remapped id set", () => {
    const { map } = buildIdRemap([1n], 1000n);
    // a hand-built row that bypassed rewriteRow and kept a stale pointer
    const bad = {
      entityId: map.get(1n),
      attackTarget: 12345n,
      targetNode: NO_REF,
      garrisonedIn: NO_REF,
    };
    expect(rowRefsResolved("unit", bad, map)).toBe(false);
  });
});

// ── schema-version backfill ───────────────────────────────────────────────────

describe("backfillRow (older-save default backfill)", () => {
  it("passes a current-version row through unchanged", () => {
    const row = { entityId: 1n, x: 5, y: 6, facing: 0, matchId: 2n };
    expect(backfillRow("entity", row, SAVE_SCHEMA_VERSION)).toEqual(row);
  });

  it("is a no-op clone (does not mutate the input)", () => {
    const row = { entityId: 1n, hp: 10 };
    const out = backfillRow("unit", row, 1);
    expect(out).not.toBe(row);
    expect(out).toEqual(row);
  });
});

// ── round-trip: serialize (mirror) → rehydrate (remap + rewrite) ───────────────
//
// Simulate the save/load core without a database: take a small match, "snapshot"
// it (copy rows + add saveId), then "rehydrate" (remap ids, rewrite refs, restamp
// matchId) and assert state is preserved with full referential integrity.

interface World {
  entity: any[];
  unit: any[];
  building: any[];
  resource_node: any[];
  player: any[];
  ai: any[];
  garrison: any[];
}

function sampleWorld(): World {
  // ids: 1 keep(building), 2 peasant(unit) gathering node 3, 3 tree(node),
  // 4 archer(unit) garrisoned in keep#1, scouted by ai (scoutId=4)
  return {
    entity: [
      { entityId: 1n, x: 10, y: 10, facing: 0, matchId: 5n },
      { entityId: 2n, x: 12, y: 11, facing: 1, matchId: 5n },
      { entityId: 3n, x: 30, y: 30, facing: 0, matchId: 5n },
      { entityId: 4n, x: 10, y: 10, facing: 0, matchId: 5n },
    ],
    building: [{ entityId: 1n, owner: "h", kind: 0, hp: 2000, matchId: 5n }],
    unit: [
      {
        entityId: 2n,
        owner: "h",
        kind: 0,
        hp: 25,
        targetNode: 3n,
        attackTarget: NO_REF,
        garrisonedIn: NO_REF,
        matchId: 5n,
      },
      {
        entityId: 4n,
        owner: "h",
        kind: 3,
        hp: 30,
        targetNode: NO_REF,
        attackTarget: NO_REF,
        garrisonedIn: 1n,
        matchId: 5n,
      },
    ],
    resource_node: [{ entityId: 3n, resType: 0, remaining: 80, matchId: 5n }],
    player: [
      {
        identity: "h",
        keepEntity: 1n,
        wood: 100,
        stone: 50,
        food: 200,
        gold: 0,
        matchId: 5n,
      },
    ],
    ai: [{ identity: "bot", host: "h", scoutId: 4n, matchId: 5n }],
    garrison: [{ slotId: 7n, building: 1n, unit: 4n, owner: "h" }],
  };
}

function rehydrate(saved: World, nextId: bigint, newMatchId: bigint): World {
  const { map } = buildIdRemap(
    saved.entity.map((e) => e.entityId),
    nextId,
  );
  const stamp = (t: string, r: any) =>
    rewriteRow(t, { ...r, matchId: newMatchId }, map);
  return {
    entity: saved.entity.map((r) => stamp("entity", r)),
    unit: saved.unit.map((r) => stamp("unit", r)),
    building: saved.building.map((r) => stamp("building", r)),
    resource_node: saved.resource_node.map((r) => stamp("resource_node", r)),
    player: saved.player.map((r) => stamp("player", r)),
    ai: saved.ai.map((r) => stamp("ai", r)),
    // garrison keeps no matchId; slotId reassigned on insert (0n here)
    garrison: saved.garrison.map((r) =>
      rewriteRow("garrison", { ...r, slotId: 0n }, map),
    ),
  };
}

describe("round-trip serialize→rehydrate", () => {
  it("preserves non-ref state and remaps every entityId to fresh ids", () => {
    const w = sampleWorld();
    const loaded = rehydrate(w, 1000n, 42n);

    // fresh ids: all entity ids are past 999 and unique
    const newIds = loaded.entity.map((e) => e.entityId);
    for (const id of newIds) expect(id).toBeGreaterThan(999n);
    expect(new Set(newIds).size).toBe(newIds.length);

    // stockpiles / hp / counts preserved
    expect(loaded.player[0].wood).toBe(100);
    expect(loaded.player[0].food).toBe(200);
    expect(loaded.unit.find((u) => u.hp === 25)).toBeTruthy();
    expect(loaded.building[0].hp).toBe(2000);
    expect(loaded.resource_node[0].remaining).toBe(80);

    // every row restamped to the new match
    for (const r of [
      ...loaded.entity,
      ...loaded.unit,
      ...loaded.building,
      ...loaded.resource_node,
      ...loaded.player,
      ...loaded.ai,
    ])
      expect(r.matchId).toBe(42n);
  });

  it("rewrites every cross-reference with NO dangling old ids", () => {
    const w = sampleWorld();
    const { map } = buildIdRemap(
      w.entity.map((e) => e.entityId),
      1000n,
    );
    const loaded = rehydrate(w, 1000n, 42n);
    const liveIds = new Set<bigint>(loaded.entity.map((e) => e.entityId));

    // peasant#2 still targets the SAME node (now node's fresh id)
    const peasant = loaded.unit.find((u) => u.hp === 25)!;
    expect(peasant.targetNode).toBe(map.get(3n));
    expect(liveIds.has(peasant.targetNode)).toBe(true);

    // archer#4 still garrisoned in the keep (keep's fresh id)
    const archer = loaded.unit.find((u) => u.hp === 30)!;
    expect(archer.garrisonedIn).toBe(map.get(1n));
    expect(liveIds.has(archer.garrisonedIn)).toBe(true);

    // player's keep ref points at the fresh keep id
    expect(loaded.player[0].keepEntity).toBe(map.get(1n));
    expect(liveIds.has(loaded.player[0].keepEntity)).toBe(true);

    // ai scout ref points at the fresh archer id
    expect(loaded.ai[0].scoutId).toBe(map.get(4n));
    expect(liveIds.has(loaded.ai[0].scoutId)).toBe(true);

    // garrison row connects the fresh keep + fresh archer
    expect(loaded.garrison[0].building).toBe(map.get(1n));
    expect(loaded.garrison[0].unit).toBe(map.get(4n));

    // no old id (1..4) survives anywhere as a reference
    const allRefs = [
      ...loaded.unit.flatMap((u) => [
        u.targetNode,
        u.attackTarget,
        u.garrisonedIn,
      ]),
      loaded.player[0].keepEntity,
      loaded.ai[0].scoutId,
      loaded.garrison[0].building,
      loaded.garrison[0].unit,
    ];
    for (const ref of allRefs)
      if (ref !== NO_REF) expect(ref).toBeGreaterThan(999n);
  });

  it("every rehydrated row passes the referential-integrity check", () => {
    const w = sampleWorld();
    const { map } = buildIdRemap(
      w.entity.map((e) => e.entityId),
      1000n,
    );
    const loaded = rehydrate(w, 1000n, 42n);
    for (const r of loaded.entity)
      expect(rowRefsResolved("entity", r, map)).toBe(true);
    for (const r of loaded.unit)
      expect(rowRefsResolved("unit", r, map)).toBe(true);
    for (const r of loaded.building)
      expect(rowRefsResolved("building", r, map)).toBe(true);
    for (const r of loaded.resource_node)
      expect(rowRefsResolved("resource_node", r, map)).toBe(true);
    for (const r of loaded.player)
      expect(rowRefsResolved("player", r, map)).toBe(true);
    for (const r of loaded.ai) expect(rowRefsResolved("ai", r, map)).toBe(true);
    for (const r of loaded.garrison)
      expect(rowRefsResolved("garrison", r, map)).toBe(true);
  });

  it("loading twice yields disjoint fresh id ranges (no collision across loads)", () => {
    const w = sampleWorld();
    const first = rehydrate(w, 1000n, 42n);
    // second load starts past the first batch's ids
    const second = rehydrate(w, 2000n, 43n);
    const a = new Set(first.entity.map((e) => e.entityId));
    const b = new Set(second.entity.map((e) => e.entityId));
    for (const id of b) expect(a.has(id)).toBe(false);
  });
});
