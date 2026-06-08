import { describe, it, expect } from 'vitest';
import {
  nearestPassableGrid,
  nearestReachablePassableGrid,
  findPathGrid,
  type Passable,
} from './pathfinding.ts';
import {
  hasPassableApproach,
  findBuildableNear,
  occupancySet,
  footprintTiles,
} from './buildings.ts';
import { isPassable } from './pathfinding.ts';
import { BUILDING_DEFS } from './buildings_defs.ts';
import { BuildingKind } from './enums.ts';
import { spawnCorner } from './defs.ts';
import { WORLD_SIZE } from './constants.ts';

const KEEP_F = BUILDING_DEFS[BuildingKind.Keep].footprint;

describe('nearestReachablePassableGrid (deposit-stall fix)', () => {
  it('never returns a tile in a disconnected pocket the mover cannot reach', () => {
    // A vertical wall of impassable tiles at column 5 splits the world. The mover
    // sits at x=2; the geometric nearest-passable to a target at x=8 is across the
    // wall, but the reachable approach must stay on the mover's side.
    const passable: Passable = (x, y) =>
      x >= 0 && y >= 0 && x < WORLD_SIZE && y < WORLD_SIZE && x !== 5;

    const target = { x: 8.5, y: 10.5 }; // on the far side
    const from = { x: 2.5, y: 10.5 }; // near side

    // Plain nearest-passable jumps the wall (picks x=6/7/8 area) — the old bug.
    const naive = nearestPassableGrid(passable, target.x, target.y);
    expect(naive.x).toBeGreaterThan(5);

    // Reachable variant stays on the mover's side and is path-connected.
    const approach = nearestReachablePassableGrid(
      passable,
      from.x,
      from.y,
      target.x,
      target.y
    )!;
    expect(approach).not.toBeNull();
    expect(Math.floor(approach.x)).toBeLessThan(5);
    // And a real path to it must exist (no [] => no freeze).
    const path = findPathGrid(passable, from.x, from.y, approach.x, approach.y);
    expect(path.length).toBeGreaterThan(0);
  });

  it('returns the goal tile itself when it is directly reachable', () => {
    const passable: Passable = (x, y) =>
      x >= 0 && y >= 0 && x < WORLD_SIZE && y < WORLD_SIZE;
    const a = nearestReachablePassableGrid(passable, 10.5, 10.5, 20.5, 20.5)!;
    expect(Math.floor(a.x)).toBe(20);
    expect(Math.floor(a.y)).toBe(20);
  });

  it('returns null only when the mover is fully boxed in', () => {
    // Single passable tile, everything else blocked.
    const passable: Passable = (x, y) => x === 10 && y === 10;
    const a = nearestReachablePassableGrid(passable, 10.5, 10.5, 50.5, 50.5)!;
    // The mover's region is just its own tile — best approach is that tile.
    expect(Math.floor(a.x)).toBe(10);
    expect(Math.floor(a.y)).toBe(10);

    const boxed: Passable = () => false;
    expect(nearestReachablePassableGrid(boxed, 10.5, 10.5, 0, 0)).toBeNull();
  });
});

describe('hasPassableApproach', () => {
  it('true when a footprint has an open orthogonal neighbour', () => {
    const open: Passable = () => true;
    expect(hasPassableApproach(KEEP_F, 20, 20, open)).toBe(true);
  });

  it('false when water rings the whole footprint', () => {
    // Footprint tiles passable, every bordering tile blocked.
    const tiles = new Set(
      footprintTiles(KEEP_F, 20, 20).map((t) => t.ty * WORLD_SIZE + t.tx)
    );
    const ringed: Passable = (x, y) => tiles.has(y * WORLD_SIZE + x);
    expect(hasPassableApproach(KEEP_F, 20, 20, ringed)).toBe(false);
  });
});

// The end-to-end invariant the fix guarantees: on EVERY seed/slot, a carrier
// returning from anywhere reachable can always reach a deposit approach beside
// the keep — the economy can never stall. Reproduces the original failing case
// (seed 98, slot 1) plus a deterministic sample of seeds, kept small for speed.
describe('keep deposit is always reachable (economy never stalls)', () => {
  const DEPOSIT_RANGE = 1.1;

  function carriersCanDeposit(seed: number, slot: number): boolean {
    const corner = spawnCorner(slot);
    const base = findBuildableNear(seed, corner.x, corner.y, KEEP_F);
    const occ = occupancySet(
      [{ kind: BuildingKind.Keep, x: base.x, y: base.y }],
      false
    );
    const passable: Passable = (px, py) =>
      isPassable(seed, px, py) && !occ.has(py * WORLD_SIZE + px);

    // Sample carrier origins on a ring of passable tiles around the keep.
    for (let r = 2; r <= 12; r += 2)
      for (let a = 0; a < 8; a++) {
        const ang = (a / 8) * Math.PI * 2;
        const ox = Math.round(base.x + Math.cos(ang) * r);
        const oy = Math.round(base.y + Math.sin(ang) * r);
        if (ox < 0 || oy < 0 || ox >= WORLD_SIZE || oy >= WORLD_SIZE) continue;
        if (!passable(ox, oy)) continue;
        const fromX = ox + 0.5;
        const fromY = oy + 0.5;
        const approach = nearestReachablePassableGrid(
          passable,
          fromX,
          fromY,
          base.x,
          base.y
        );
        if (!approach) return false;
        const already =
          Math.hypot(fromX - approach.x, fromY - approach.y) <= DEPOSIT_RANGE;
        const path = findPathGrid(passable, fromX, fromY, approach.x, approach.y);
        if (path.length === 0 && !already) return false;
      }
    return true;
  }

  it('original failing seed 98 slot 1 now banks resources', () => {
    expect(carriersCanDeposit(98, 1)).toBe(true);
  });

  it('holds across a sample of seeds (was ~1 in 5 stalled before)', () => {
    // Mix of seeds/slots that stalled before the fix plus a few healthy ones.
    const seeds = [9, 16, 44, 55, 87, 94, 98, 7, 21, 42];
    const fails = seeds.flatMap((seed) =>
      [0, 1, 2, 3]
        .filter((slot) => !carriersCanDeposit(seed, slot))
        .map((slot) => `seed=${seed} slot=${slot}`)
    );
    expect(fails).toEqual([]);
  });
});
