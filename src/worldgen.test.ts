import { describe, it, expect } from 'vitest';
import {
  mulberry32,
  hash2,
  mixSeed,
  sampleTerrain,
  scatterNodes,
  type ScatterRule,
  Biome,
  BIOME_DEFS,
  biomePassable,
  biomeBuildable,
  moveCostMul,
  treeDensity,
  rockDensity,
  gameDensity,
  fishDensity,
  goldDensity,
  elevation,
  elevationRangeBonus,
  ELEV_BONUS_MAX,
  findPathGrid,
  findPath,
  isPassable,
  type Passable,
  WORLD_SIZE,
  NODE_KINDS,
  renderHeight,
  biomeHeightEmphasis,
  biomeDecoration,
  Decoration,
  MAP_PRESETS,
  mapPresetById,
  mapPresetByIndex,
  biasOf,
  NEUTRAL_BIAS,
} from '../shared/index.ts';

const SEED = 12345;

// ── PRNG determinism ──────────────────────────────────────────────────────────

describe('mulberry32', () => {
  it('same seed yields the same sequence', () => {
    const a = mulberry32(42);
    const b = mulberry32(42);
    const seqA = Array.from({ length: 32 }, () => a());
    const seqB = Array.from({ length: 32 }, () => b());
    expect(seqA).toEqual(seqB);
  });

  it('different seeds diverge', () => {
    const a = mulberry32(1);
    const b = mulberry32(2);
    let diff = 0;
    for (let i = 0; i < 32; i++) if (a() !== b()) diff++;
    expect(diff).toBeGreaterThan(20);
  });

  it('stays within [0,1)', () => {
    const r = mulberry32(7);
    for (let i = 0; i < 1000; i++) {
      const v = r();
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThan(1);
    }
  });
});

describe('hash2', () => {
  it('is a pure stable function of (x, y, seed)', () => {
    for (let i = 0; i < 100; i++) {
      const x = (i * 13) % 200;
      const y = (i * 7) % 200;
      expect(hash2(x, y, SEED)).toBe(hash2(x, y, SEED));
    }
  });

  it('floors coordinates so any point in a tile hashes identically', () => {
    expect(hash2(5, 9, SEED)).toBe(hash2(5.4, 9.99, SEED));
  });

  it('stays within [0,1) and varies across the lattice', () => {
    const seen = new Set<number>();
    for (let x = 0; x < 20; x++) {
      for (let y = 0; y < 20; y++) {
        const v = hash2(x, y, SEED);
        expect(v).toBeGreaterThanOrEqual(0);
        expect(v).toBeLessThan(1);
        seen.add(v);
      }
    }
    // 400 lattice points should produce many distinct values (no collapse).
    expect(seen.size).toBeGreaterThan(300);
  });

  it('decorrelates neighbouring tiles', () => {
    // adjacent tiles should rarely be near-equal — proves it is not a smooth ramp
    let close = 0;
    for (let x = 0; x < 50; x++)
      if (Math.abs(hash2(x, 0, SEED) - hash2(x + 1, 0, SEED)) < 0.01) close++;
    expect(close).toBeLessThan(5);
  });
});

describe('mixSeed', () => {
  it('is deterministic and decorrelates derived streams', () => {
    expect(mixSeed(SEED, 1)).toBe(mixSeed(SEED, 1));
    expect(mixSeed(SEED, 1)).not.toBe(mixSeed(SEED, 2));
    const a = mulberry32(mixSeed(SEED, 1));
    const b = mulberry32(mixSeed(SEED, 2));
    expect(a()).not.toBe(b());
  });
});

// ── terrain reproducibility ───────────────────────────────────────────────────

describe('sampleTerrain reproducibility', () => {
  it('returns identical samples for the same (seed, x, y)', () => {
    for (let i = 0; i < 100; i++) {
      const x = (i * 11.7) % WORLD_SIZE;
      const y = (i * 5.3) % WORLD_SIZE;
      expect(sampleTerrain(SEED, x, y)).toEqual(sampleTerrain(SEED, x, y));
    }
  });

  it('produces the new biomes somewhere on the map', () => {
    const found = new Set<number>();
    for (let x = 0; x < WORLD_SIZE; x += 2)
      for (let y = 0; y < WORLD_SIZE; y += 2)
        found.add(sampleTerrain(SEED, x, y).biome);
    // The richer generator should surface variety beyond water + grass.
    expect(found.has(Biome.Dunes)).toBe(true);
    expect(found.has(Biome.Steppe)).toBe(true);
    expect(found.has(Biome.Hills)).toBe(true);
    expect(found.size).toBeGreaterThanOrEqual(6);
  });

  it('different seeds yield different biome maps', () => {
    let diff = 0;
    for (let i = 0; i < 300; i++) {
      const x = (i * 0.43) % WORLD_SIZE;
      const y = (i * 0.89) % WORLD_SIZE;
      if (sampleTerrain(1, x, y).biome !== sampleTerrain(2, x, y).biome) diff++;
    }
    expect(diff).toBeGreaterThan(0);
  });
});

// ── node placement reproducibility ────────────────────────────────────────────

describe('scatterNodes', () => {
  it('places the same nodes for the same seed (reproducible, no ctx.random)', () => {
    const a = scatterNodes(SEED, NODE_KINDS as ScatterRule[]);
    const b = scatterNodes(SEED, NODE_KINDS as ScatterRule[]);
    expect(a).toEqual(b);
    expect(a.length).toBeGreaterThan(0);
  });

  it('different seeds place nodes at different positions', () => {
    const a = scatterNodes(1, NODE_KINDS as ScatterRule[]);
    const b = scatterNodes(2, NODE_KINDS as ScatterRule[]);
    // Compare first wood node position — overwhelmingly likely to differ.
    expect(a[0]).not.toEqual(b[0]);
  });

  it('every node lands on a passable biome (reachable ground)', () => {
    const nodes = scatterNodes(SEED, NODE_KINDS as ScatterRule[]);
    for (const n of nodes) {
      expect(biomePassable(sampleTerrain(SEED, n.x, n.y).biome)).toBe(true);
    }
  });

  it('respects a per-kind density of zero', () => {
    const rule: ScatterRule = {
      resType: 0,
      count: 50,
      yield: 10,
      density: () => 0,
      coastalOnly: false,
    };
    expect(scatterNodes(SEED, [rule])).toEqual([]);
  });

  it('carries the rule yield onto each placed node', () => {
    const nodes = scatterNodes(SEED, NODE_KINDS as ScatterRule[]);
    const woodYield = (NODE_KINDS[0] as ScatterRule).yield;
    const wood = nodes.find((n) => n.resType === (NODE_KINDS[0] as ScatterRule).resType);
    expect(wood?.yield).toBe(woodYield);
  });
});

// ── biome catalog validity ────────────────────────────────────────────────────

describe('biome catalog', () => {
  it('has a complete, well-formed def for every biome value', () => {
    for (const b of Object.values(Biome)) {
      const def = BIOME_DEFS[b as Biome];
      expect(def).toBeDefined();
      expect(typeof def.label).toBe('string');
      expect(def.label.length).toBeGreaterThan(0);
      expect(typeof def.color).toBe('number');
      expect(typeof def.passable).toBe('boolean');
      expect(typeof def.buildable).toBe('boolean');
      for (const k of ['tree', 'rock', 'game', 'fish', 'gold'] as const) {
        expect(def.density[k]).toBeGreaterThanOrEqual(0);
        expect(def.density[k]).toBeLessThanOrEqual(1);
      }
    }
  });

  it('impassable biomes are never buildable, and have infinite move cost', () => {
    for (const b of Object.values(Biome)) {
      const def = BIOME_DEFS[b as Biome];
      if (!def.passable) {
        expect(def.buildable).toBe(false);
        expect(def.moveCostMul).toBe(Infinity);
      } else {
        expect(def.moveCostMul).toBeGreaterThanOrEqual(1);
        expect(Number.isFinite(def.moveCostMul)).toBe(true);
      }
    }
  });

  it('water and mountains are impassable; lowlands are passable', () => {
    expect(biomePassable(Biome.DeepWater)).toBe(false);
    expect(biomePassable(Biome.Mountain)).toBe(false);
    expect(biomePassable(Biome.Snow)).toBe(false);
    expect(biomePassable(Biome.Grassland)).toBe(true);
    expect(biomeBuildable(Biome.Grassland)).toBe(true);
    expect(moveCostMul(Biome.Dunes)).toBeGreaterThan(moveCostMul(Biome.Grassland));
  });

  it('density helpers match the catalog (single source of truth)', () => {
    for (const b of Object.values(Biome)) {
      const bb = b as Biome;
      expect(treeDensity(bb)).toBe(BIOME_DEFS[bb].density.tree);
      expect(rockDensity(bb)).toBe(BIOME_DEFS[bb].density.rock);
      expect(gameDensity(bb)).toBe(BIOME_DEFS[bb].density.game);
      expect(fishDensity(bb)).toBe(BIOME_DEFS[bb].density.fish);
      expect(goldDensity(bb)).toBe(BIOME_DEFS[bb].density.gold);
    }
  });
});

// ── elevation gameplay ────────────────────────────────────────────────────────

describe('elevation', () => {
  it('is deterministic and clamped to [0,1]', () => {
    for (let i = 0; i < 100; i++) {
      const x = (i * 9.1) % WORLD_SIZE;
      const y = (i * 4.7) % WORLD_SIZE;
      const v = elevation(SEED, x, y);
      expect(v).toBe(elevation(SEED, x, y));
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
    }
  });
});

describe('elevationRangeBonus', () => {
  it('is neutral on level ground', () => {
    expect(elevationRangeBonus(0.5, 0.5)).toBe(1);
  });

  it('rewards high ground and penalizes shooting uphill, symmetrically', () => {
    const high = elevationRangeBonus(0.8, 0.5);
    const low = elevationRangeBonus(0.5, 0.8);
    expect(high).toBeGreaterThan(1);
    expect(low).toBeLessThan(1);
    expect(high - 1).toBeCloseTo(1 - low, 6);
  });

  it('saturates at ±ELEV_BONUS_MAX for large deltas', () => {
    expect(elevationRangeBonus(1, 0)).toBeCloseTo(1 + ELEV_BONUS_MAX, 6);
    expect(elevationRangeBonus(0, 1)).toBeCloseTo(1 - ELEV_BONUS_MAX, 6);
  });

  it('is monotonic in the elevation delta', () => {
    let prev = -Infinity;
    for (let d = -0.4; d <= 0.4; d += 0.05) {
      const v = elevationRangeBonus(0.5 + d / 2, 0.5 - d / 2);
      expect(v).toBeGreaterThanOrEqual(prev);
      prev = v;
    }
  });
});

// ── long-route pathfinding on the bigger map ──────────────────────────────────

describe('long-route pathfinding (144² map)', () => {
  const open: Passable = () => true;

  it('routes corner-to-corner across the full open grid', () => {
    const p = findPathGrid(open, 1.5, 1.5, WORLD_SIZE - 1.5, WORLD_SIZE - 1.5);
    expect(p.length).toBeGreaterThan(0);
    expect(p[p.length - 1]).toEqual({ x: WORLD_SIZE - 1.5, y: WORLD_SIZE - 1.5 });
  });

  it('completes a long detour around a near-full vertical wall', () => {
    // wall down the middle column for almost the whole height; one gap at the top.
    const gapY = WORLD_SIZE - 2;
    const mid = Math.floor(WORLD_SIZE / 2);
    const wall: Passable = (x, y) => {
      if (x < 0 || y < 0 || x >= WORLD_SIZE || y >= WORLD_SIZE) return false;
      return !(x === mid && y < gapY);
    };
    const p = findPathGrid(wall, 4.5, 4.5, WORLD_SIZE - 4.5, 4.5);
    // The only way across is up to the gap and back down — a long route that
    // must NOT be abandoned by maxExpansions on the bigger map.
    expect(p.length).toBeGreaterThan(0);
    expect(p[p.length - 1]).toEqual({ x: WORLD_SIZE - 4.5, y: 4.5 });
    expect(p.some((wp) => wp.y >= gapY - 1)).toBe(true);
  });

  it('finds a real long route between two distant land tiles on the seeded map', () => {
    // Pick two passable tiles far apart and confirm a path exists end-to-end.
    let start: { x: number; y: number } | null = null;
    let goal: { x: number; y: number } | null = null;
    for (let i = 6; i < WORLD_SIZE - 6 && !start; i++)
      if (isPassable(SEED, i, i)) start = { x: i + 0.5, y: i + 0.5 };
    for (let i = WORLD_SIZE - 7; i > 6 && !goal; i--)
      if (isPassable(SEED, i, WORLD_SIZE - i)) goal = { x: i + 0.5, y: WORLD_SIZE - i + 0.5 };
    expect(start).not.toBeNull();
    expect(goal).not.toBeNull();
    const p = findPath(SEED, start!.x, start!.y, goal!.x, goal!.y);
    expect(p.length).toBeGreaterThan(0);
    expect(p[p.length - 1]).toEqual({ x: goal!.x, y: goal!.y });
  });
});

// ── render height (client visuals only) ───────────────────────────────────────

describe('renderHeight (render-only relief)', () => {
  it('dips below zero for water and rises for land', () => {
    expect(renderHeight(0.2)).toBeLessThan(0);
    expect(renderHeight(0.7)).toBeGreaterThan(0);
  });

  it('amplifies high-emphasis biomes above plains at the same raw height', () => {
    const h = 0.75; // a hills/mountain band height
    const mountain = renderHeight(h, Biome.Mountain);
    const grass = renderHeight(h, Biome.Grassland);
    expect(biomeHeightEmphasis(Biome.Mountain)).toBeGreaterThan(
      biomeHeightEmphasis(Biome.Grassland)
    );
    expect(mountain).toBeGreaterThan(grass);
  });

  it('scales with a preset elevGain bias but never sinks land below sea', () => {
    const flat = renderHeight(0.7, Biome.Hills, NEUTRAL_BIAS);
    const tall = renderHeight(0.7, Biome.Hills, biasOf('highlands'));
    expect(tall).toBeGreaterThan(flat); // highlands lift relief
    expect(renderHeight(0.7, Biome.Hills, biasOf('verdant'))).toBeGreaterThan(0);
  });
});

// ── biome decoration catalog (cosmetic props, zero DB rows) ───────────────────

describe('biome decoration catalog', () => {
  it('gives every biome a well-formed decoration entry', () => {
    for (const b of Object.values(Biome)) {
      const d = biomeDecoration(b as Biome);
      expect(Object.values(Decoration)).toContain(d.kind);
      expect(d.density).toBeGreaterThanOrEqual(0);
      expect(d.density).toBeLessThanOrEqual(1);
      // A non-None kind must have a non-zero density and vice versa.
      if (d.kind === Decoration.None) expect(d.density).toBe(0);
      else expect(d.density).toBeGreaterThan(0);
    }
  });

  it('places palms at the oasis and rocks in the hills', () => {
    expect(biomeDecoration(Biome.Oasis).kind).toBe(Decoration.Palm);
    expect(biomeDecoration(Biome.Hills).kind).toBe(Decoration.Rock);
    expect(biomeDecoration(Biome.DeepWater).kind).toBe(Decoration.None);
  });
});

// ── map presets (data-driven render flavor) ───────────────────────────────────

describe('map presets', () => {
  it('exposes a non-empty catalog with unique ids and a neutral default', () => {
    expect(MAP_PRESETS.length).toBeGreaterThanOrEqual(3);
    const ids = new Set(MAP_PRESETS.map((p) => p.id));
    expect(ids.size).toBe(MAP_PRESETS.length);
    expect(mapPresetById(MAP_PRESETS[0].id).bias).toEqual(NEUTRAL_BIAS);
  });

  it('falls back to the first preset for unknown ids', () => {
    expect(mapPresetById('nope').id).toBe(MAP_PRESETS[0].id);
    expect(biasOf('nope')).toEqual(NEUTRAL_BIAS);
  });

  it('indexes presets with wrap-around (stable for the AI/round-robin paths)', () => {
    expect(mapPresetByIndex(0).id).toBe(MAP_PRESETS[0].id);
    expect(mapPresetByIndex(MAP_PRESETS.length).id).toBe(MAP_PRESETS[0].id);
    expect(mapPresetByIndex(-1).id).toBe(
      MAP_PRESETS[MAP_PRESETS.length - 1].id
    );
  });

  it('highlands raises relief and desert dries the moisture', () => {
    expect(biasOf('highlands').elevGain).toBeGreaterThan(1);
    expect(biasOf('desert').moistShift).toBeLessThan(0);
  });
});
