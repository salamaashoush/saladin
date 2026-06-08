import { describe, it, expect } from 'vitest';
import {
  findPathGrid,
  findPath,
  isPassable,
  nearestPassableGrid,
  type Passable,
} from '../shared/pathfinding.ts';
import { WORLD_SIZE } from '../shared/constants.ts';

const open: Passable = () => true;

describe('findPathGrid', () => {
  it('returns a single waypoint when start and goal share a tile', () => {
    const p = findPathGrid(open, 2.2, 2.2, 2.8, 2.4);
    expect(p).toHaveLength(1);
    expect(p[0]).toEqual({ x: 2.8, y: 2.4 });
  });

  it('string-pulls a clear path to a near-straight line', () => {
    const p = findPathGrid(open, 0.5, 0.5, 12.5, 0.5);
    expect(p.length).toBeLessThanOrEqual(2);
    expect(p[p.length - 1]).toEqual({ x: 12.5, y: 0.5 });
  });

  it('routes around a wall instead of through it', () => {
    // vertical wall at x=5 for y in [0,7]; gap at y>=8
    const wall: Passable = (x, y) => !(x === 5 && y <= 7);
    const p = findPathGrid(wall, 2.5, 2.5, 8.5, 2.5);
    expect(p.length).toBeGreaterThan(0);
    expect(p[p.length - 1]).toEqual({ x: 8.5, y: 2.5 });
    // it had to detour past the wall gap (some waypoint reaches y>=7)
    expect(p.some((wp) => wp.y >= 7)).toBe(true);
    // straight line is blocked, confirming the detour was necessary
    expect(wall(5, 2)).toBe(false);
  });

  it('returns [] when the goal is enclosed', () => {
    const enclosed: Passable = (x, y) => {
      // block the ring around (8,8); (8,8) itself stays open but unreachable
      if (x >= 7 && x <= 9 && y >= 7 && y <= 9 && !(x === 8 && y === 8))
        return false;
      return true;
    };
    const p = findPathGrid(enclosed, 2.5, 2.5, 8.5, 8.5);
    expect(p).toEqual([]);
  });

  it('does not cut corners through a diagonal pinch', () => {
    // blocks at (5,4) and (4,5) — a diagonal move 4,4 -> 5,5 must not slip through
    const pinch: Passable = (x, y) => !((x === 5 && y === 4) || (x === 4 && y === 5));
    const p = findPathGrid(pinch, 4.5, 4.5, 5.5, 5.5);
    // reachable around, but never via the illegal diagonal corner-cut
    expect(p.length).toBeGreaterThan(1);
  });
});

describe('nearestPassableGrid', () => {
  it('snaps a blocked point to the nearest open tile centre', () => {
    const blockedCol: Passable = (x) => x !== 5;
    const snapped = nearestPassableGrid(blockedCol, 5.5, 3.5);
    expect(Math.floor(snapped.x)).not.toBe(5);
  });
});

describe('terrain-backed pathfinding', () => {
  const seed = 12345;
  it('the map has both passable and impassable tiles', () => {
    let land = 0;
    let water = 0;
    for (let i = 0; i < WORLD_SIZE; i += 4) {
      if (isPassable(seed, WORLD_SIZE / 2, i)) land++;
      if (!isPassable(seed, 1, i)) water++; // edge column tends to be sea
    }
    expect(land).toBeGreaterThan(0);
    expect(water).toBeGreaterThan(0);
  });

  it('finds a route between two land tiles', () => {
    const c = WORLD_SIZE / 2;
    expect(isPassable(seed, c, c)).toBe(true);
    const p = findPath(seed, c + 0.5, c + 0.5, c + 6.5, c + 4.5);
    expect(p.length).toBeGreaterThan(0);
    expect(p[p.length - 1]).toEqual({ x: c + 6.5, y: c + 4.5 });
  });
});
