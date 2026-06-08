import { describe, it, expect } from 'vitest';
import {
  stepToward,
  nearestIndex,
  dist,
  applyDamage,
  inRange,
} from '../shared/sim.ts';

describe('stepToward', () => {
  it('advances exactly `step` units toward a far target', () => {
    const r = stepToward(0, 0, 10, 0, 0.5, 0.05);
    expect(r.arrived).toBe(false);
    expect(r.x).toBeCloseTo(0.5, 6);
    expect(r.y).toBeCloseTo(0, 6);
    expect(dist(0, 0, r.x, r.y)).toBeCloseTo(0.5, 6);
  });

  it('snaps to target and reports arrival when within one step', () => {
    const r = stepToward(0, 0, 0.1, 0, 0.5, 0.05);
    expect(r.arrived).toBe(true);
    expect(r.x).toBe(0.1);
    expect(r.y).toBe(0);
  });

  it('arrives when already within epsilon', () => {
    const r = stepToward(5, 5, 5.01, 5, 0.5, 0.05);
    expect(r.arrived).toBe(true);
  });

  it('computes facing toward the target', () => {
    const r = stepToward(0, 0, 0, 5, 1, 0.05);
    expect(r.facing).toBeCloseTo(Math.PI / 2, 6);
  });

  it('moves diagonally with correct step length', () => {
    const r = stepToward(0, 0, 10, 10, 1, 0.05);
    expect(dist(0, 0, r.x, r.y)).toBeCloseTo(1, 6);
  });

  it('does not overshoot — repeated steps converge to target', () => {
    let x = 0;
    let y = 0;
    for (let i = 0; i < 100; i++) {
      const r = stepToward(x, y, 3, 4, 0.5, 0.05);
      x = r.x;
      y = r.y;
      if (r.arrived) break;
    }
    expect(dist(x, y, 3, 4)).toBeLessThan(1e-6);
  });
});

describe('nearestIndex', () => {
  it('returns the index of the closest point', () => {
    const pts = [
      { x: 10, y: 10 },
      { x: 1, y: 1 },
      { x: 5, y: 5 },
    ];
    expect(nearestIndex(0, 0, pts)).toBe(1);
  });

  it('returns -1 for an empty list', () => {
    expect(nearestIndex(0, 0, [])).toBe(-1);
  });

  it('breaks ties toward the earlier index', () => {
    const pts = [
      { x: 1, y: 0 },
      { x: -1, y: 0 },
    ];
    expect(nearestIndex(0, 0, pts)).toBe(0);
  });
});

describe('combat', () => {
  it('applyDamage subtracts and clamps at zero', () => {
    expect(applyDamage(70, 12)).toBe(58);
    expect(applyDamage(10, 12)).toBe(0);
    expect(applyDamage(0, 5)).toBe(0);
  });

  it('inRange is inclusive of the boundary', () => {
    expect(inRange(1.0, 1.0)).toBe(true);
    expect(inRange(0.9, 1.0)).toBe(true);
    expect(inRange(1.1, 1.0)).toBe(false);
  });

  it('a unit dies after enough hits', () => {
    let hp = 70;
    let hits = 0;
    while (hp > 0) {
      hp = applyDamage(hp, 12);
      hits++;
    }
    expect(hits).toBe(6);
    expect(hp).toBe(0);
  });
});
