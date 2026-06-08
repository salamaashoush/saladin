import { describe, it, expect } from 'vitest';
import {
  Biome,
  sampleTerrain,
  isLand,
  isCoastal,
  treeDensity,
  rockDensity,
  gameDensity,
  fishDensity,
  goldDensity,
  WORLD_SIZE,
} from '../shared/index.ts';

const SEED = 12345;

describe('terrain determinism', () => {
  it('sampleTerrain is a pure function of (seed, x, y)', () => {
    for (let i = 0; i < 50; i++) {
      const x = (i * 7.3) % WORLD_SIZE;
      const y = (i * 3.1) % WORLD_SIZE;
      const a = sampleTerrain(SEED, x, y);
      const b = sampleTerrain(SEED, x, y);
      expect(a).toEqual(b);
    }
  });

  it('different seeds generally yield different biome maps', () => {
    let diff = 0;
    for (let i = 0; i < 200; i++) {
      const x = (i * 0.47) % WORLD_SIZE;
      const y = (i * 0.91) % WORLD_SIZE;
      if (sampleTerrain(1, x, y).biome !== sampleTerrain(2, x, y).biome) diff++;
    }
    expect(diff).toBeGreaterThan(0);
  });

  it('isCoastal is deterministic and reproducible', () => {
    for (let x = 4; x < WORLD_SIZE - 4; x += 5)
      for (let y = 4; y < WORLD_SIZE - 4; y += 5)
        expect(isCoastal(SEED, x, y)).toBe(isCoastal(SEED, x, y));
  });
});

describe('biome densities', () => {
  it('tree density peaks in forest, zero on bare/water biomes', () => {
    expect(treeDensity(Biome.Forest)).toBeGreaterThan(treeDensity(Biome.Grassland));
    expect(treeDensity(Biome.Desert)).toBe(0);
    expect(treeDensity(Biome.DeepWater)).toBe(0);
  });

  it('rock density favors the rocky uplands', () => {
    expect(rockDensity(Biome.Hills)).toBeGreaterThan(0);
    expect(rockDensity(Biome.Mountain)).toBeGreaterThan(0);
    expect(rockDensity(Biome.Grassland)).toBeLessThan(rockDensity(Biome.Hills));
    expect(rockDensity(Biome.DeepWater)).toBe(0);
  });

  it('game density grazes the open grass/steppe, never the desert', () => {
    expect(gameDensity(Biome.Grassland)).toBeGreaterThan(0);
    expect(gameDensity(Biome.Steppe)).toBeGreaterThan(0);
    expect(gameDensity(Biome.Desert)).toBe(0);
    expect(gameDensity(Biome.DeepWater)).toBe(0);
  });

  it('fish density is highest on the sandy shore', () => {
    expect(fishDensity(Biome.Sand)).toBeGreaterThan(fishDensity(Biome.Grassland));
    expect(fishDensity(Biome.Desert)).toBe(0);
  });

  it('gold density is confined to the mountains and hills', () => {
    expect(goldDensity(Biome.Mountain)).toBeGreaterThan(0);
    expect(goldDensity(Biome.Hills)).toBeGreaterThan(0);
    expect(goldDensity(Biome.Grassland)).toBe(0);
    expect(goldDensity(Biome.Sand)).toBe(0);
  });
});

describe('isCoastal', () => {
  it('returns false on open water (not land)', () => {
    // Map center is forced to land by the radial falloff; the corners are water.
    // Scan the outer ring: any water tile must not be coastal (it is not land).
    for (let i = 0; i < WORLD_SIZE; i += 3) {
      const onWater = !isLand(SEED, i, 0);
      if (onWater) expect(isCoastal(SEED, i, 0)).toBe(false);
    }
  });

  it('every coastal tile is land with a water neighbour', () => {
    let coastalFound = 0;
    for (let x = 1; x < WORLD_SIZE - 1; x += 1) {
      for (let y = 1; y < WORLD_SIZE - 1; y += 1) {
        if (!isCoastal(SEED, x, y)) continue;
        coastalFound++;
        expect(isLand(SEED, x, y)).toBe(true);
        const neighbours = [
          [1, 0],
          [-1, 0],
          [0, 1],
          [0, -1],
        ];
        const hasWaterAdj = neighbours.some(([dx, dy]) => {
          const b = sampleTerrain(SEED, x + dx, y + dy).biome;
          return b === Biome.DeepWater || b === Biome.ShallowWater;
        });
        expect(hasWaterAdj).toBe(true);
      }
    }
    // A continent with water rings the edge must produce some shoreline.
    expect(coastalFound).toBeGreaterThan(0);
  });
});
