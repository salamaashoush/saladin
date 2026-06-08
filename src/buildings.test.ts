import { describe, it, expect } from 'vitest';
import {
  footprintTiles,
  footprintCenter,
  canPlace,
  BuildingKind,
  findPathGrid,
  type Passable,
} from '../shared/index.ts';

describe('footprints', () => {
  it('covers 1 / 4 / 9 tiles for footprint 1 / 2 / 3', () => {
    expect(footprintTiles(1, 10.6, 10.2)).toHaveLength(1);
    expect(footprintTiles(2, 10.6, 10.2)).toHaveLength(4);
    expect(footprintTiles(3, 10.6, 10.2)).toHaveLength(9);
  });

  it('footprint 1 sits on the clicked tile', () => {
    expect(footprintTiles(1, 10.6, 10.2)[0]).toEqual({ tx: 10, ty: 10 });
  });

  it('odd footprint centres on the tile centre', () => {
    expect(footprintCenter(3, 10.6, 10.2)).toEqual({ x: 10.5, y: 10.5 });
    expect(footprintCenter(1, 10.6, 10.2)).toEqual({ x: 10.5, y: 10.5 });
  });
});

describe('canPlace', () => {
  const free = () => false;
  const allLand: Passable = () => true;

  it('allows placement on clear passable ground', () => {
    expect(canPlace(BuildingKind.Tower, 10.5, 10.5, allLand, free)).toBe(true);
  });

  it('rejects when any footprint tile is water', () => {
    // barracks (footprint 2) at (10.5,10.5) covers tiles {9,10}x{9,10}
    const oneWater: Passable = (x, y) => !(x === 10 && y === 10);
    expect(canPlace(BuildingKind.Barracks, 10.5, 10.5, oneWater, free)).toBe(
      false
    );
  });

  it('rejects when a footprint tile is already occupied', () => {
    const occupied = (x: number, y: number) => x === 10 && y === 10;
    expect(canPlace(BuildingKind.Wall, 10.5, 10.5, allLand, occupied)).toBe(
      false
    );
  });
});

describe('occupancy-aware routing', () => {
  it('units route around a placed wall line', () => {
    // wall along x=6 for y in [0,7], gap below — built from footprint tiles
    const occ = new Set<number>();
    const W = 96;
    for (let y = 0; y <= 7; y++)
      for (const t of footprintTiles(1, 6.5, y + 0.5))
        occ.add(t.ty * W + t.tx);
    const passable: Passable = (x, y) =>
      x >= 0 && y >= 0 && x < W && y < W && !occ.has(y * W + x);

    const path = findPathGrid(passable, 3.5, 3.5, 9.5, 3.5);
    expect(path.length).toBeGreaterThan(0);
    expect(path[path.length - 1]).toEqual({ x: 9.5, y: 3.5 });
    expect(path.some((wp) => wp.y >= 7)).toBe(true); // detoured past the wall
  });
});
