import { describe, it, expect } from 'vitest';
import { spawnCorner, UNIT_DEFS, UnitKind, WORLD_SIZE } from '../shared/index.ts';

describe('shared sim data', () => {
  it('assigns distinct spawn corners per player index', () => {
    const a = spawnCorner(0);
    const b = spawnCorner(1);
    expect(a).not.toEqual(b);
    for (const v of [a, b]) {
      expect(v.x).toBeGreaterThan(0);
      expect(v.x).toBeLessThan(WORLD_SIZE);
      expect(v.y).toBeGreaterThan(0);
      expect(v.y).toBeLessThan(WORLD_SIZE);
    }
  });

  it('wraps spawn corners after the 4th player', () => {
    expect(spawnCorner(4)).toEqual(spawnCorner(0));
  });

  it('defines peasant stats used by the module and renderer', () => {
    const def = UNIT_DEFS[UnitKind.Peasant];
    expect(def.speed).toBeGreaterThan(0);
    expect(def.carry).toBeGreaterThan(0);
  });
});
