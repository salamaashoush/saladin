// Deterministic biome terrain generated from a single seed. Shared by the
// module (authority: where land/resources are) and the client (render). No
// per-tile rows — both sides recompute from the seed.
//
// Generation: a domain-warped fbm height field (warp = offset sample coords by a
// second low-frequency fbm, which bends ridgelines and coastlines so they read
// as natural rather than blobby) shaped by a radial continent falloff, plus an
// independent moisture field. classify() bands height + moisture into biomes.
//
// The Biome enum and everything keyed on it (colors, labels, passability,
// densities) live in ./biomes.ts — this file owns generation and re-exports the
// catalog surface for back-compat with existing importers.
import { fbm } from './noise.ts';
import { mulberry32, hash2, mixSeed } from './rng.ts';
import { WORLD_SIZE } from './constants.ts';
import {
  Biome,
  BIOME_DEFS,
  biomePassable,
  biomeBuildable,
  moveCostMul,
} from './biomes.ts';

// Re-export the catalog surface so existing `from './terrain.ts'` importers keep
// working unchanged.
export {
  Biome,
  BIOME_DEFS,
  BIOME_LABEL,
  BIOME_COLOR,
  biomePassable,
  biomeBuildable,
  moveCostMul,
  treeDensity,
  rockDensity,
  gameDensity,
  fishDensity,
  goldDensity,
} from './biomes.ts';

export interface TerrainSample {
  height: number;
  moisture: number;
  biome: Biome;
}

const H_SCALE = 0.042;
const M_SCALE = 0.03;
const WARP_SCALE = 0.02; // low-frequency warp field
const WARP_AMP = 9; // tiles of coordinate displacement
const SEA = 0.38;

// Continent falloff: water rings the map edge, land in the middle. Independent
// of WORLD_SIZE shape so a bigger map keeps the same relative coastline ring.
function radial(x: number, y: number): number {
  const c = WORLD_SIZE / 2;
  const d = Math.hypot(x - c, y - c) / (WORLD_SIZE * 0.5);
  return Math.max(0, 1.12 - Math.pow(d, 2.6) * 0.95);
}

export function sampleTerrain(seed: number, x: number, y: number): TerrainSample {
  // Domain warp: displace the sample point by a second low-freq fbm so coastlines
  // and biome bands meander instead of forming concentric rings.
  const wx = (fbm(x * WARP_SCALE, y * WARP_SCALE, seed ^ 0x1b56, 3) - 0.5) * 2 * WARP_AMP;
  const wy =
    (fbm(x * WARP_SCALE + 31, y * WARP_SCALE + 17, seed ^ 0x77c1, 3) - 0.5) *
    2 *
    WARP_AMP;

  let h = fbm((x + wx) * H_SCALE, (y + wy) * H_SCALE, seed, 5);
  h = h * 0.78 + 0.18;
  h *= radial(x, y);

  const moisture = fbm(
    (x + wx) * M_SCALE + 100,
    (y + wy) * M_SCALE + 50,
    seed ^ 0x9e37,
    4
  );
  return { height: h, moisture, biome: classify(h, moisture) };
}

function classify(h: number, m: number): Biome {
  if (h < SEA - 0.06) return Biome.DeepWater;
  if (h < SEA) return Biome.ShallowWater;
  if (h < SEA + 0.04) return Biome.Sand;
  if (h > 0.82) return Biome.Snow;
  if (h > 0.72) return Biome.Mountain;
  if (h > 0.6) return Biome.Hills;
  // Lowlands banded by moisture: bone-dry desert → dunes → dry steppe →
  // grassland → wet forest, with a rare lush oasis where it is wettest at the
  // low, hot edge of the map.
  if (m < 0.26) return h < SEA + 0.12 ? Biome.Oasis : Biome.Desert;
  if (m < 0.4) return Biome.Dunes;
  if (m < 0.52) return Biome.Steppe;
  if (m < 0.72) return Biome.Grassland;
  return Biome.Forest;
}

// Render elevation in world units: water dips, land rises with height. Named
// `renderHeight` so it does not collide with elevation(seed,x,y) in
// ./elevation.ts (the gameplay elevation layer). Client mesh code calls this.
export function renderHeight(h: number): number {
  if (h < SEA) return -0.4 * ((SEA - h) / SEA) - 0.05;
  return (h - SEA) * 7;
}

export function isLand(seed: number, x: number, y: number): boolean {
  return biomePassable(sampleTerrain(seed, x, y).biome);
}

// True when (x,y) is buildable land with open water on at least one of its four
// orthogonal neighbours — i.e. a reachable shore. Deterministic from the seed.
export function isCoastal(seed: number, x: number, y: number): boolean {
  if (!isLand(seed, x, y)) return false;
  const adj = [
    [1, 0],
    [-1, 0],
    [0, 1],
    [0, -1],
  ];
  for (const [dx, dy] of adj) {
    const b = sampleTerrain(seed, x + dx, y + dy).biome;
    if (b === Biome.DeepWater || b === Biome.ShallowWater) return true;
  }
  return false;
}

// One placed resource node. Positions are world-unit floats at tile centres so
// the module can drop them straight into entity rows.
export interface ScatteredNode {
  x: number;
  y: number;
  resType: number;
  yield: number;
}

// One scatter rule: how many of a resource to place, how much each holds, the
// per-biome accept-probability, and whether it may only sit on the coast (fish).
export interface ScatterRule {
  resType: number;
  count: number;
  yield: number;
  density: (b: Biome) => number;
  coastalOnly: boolean;
}

// Deterministically place all resource nodes for a seed. Pure — same seed +
// rules always yield the same positions, so the module's placement is
// reproducible and testable, and never relies on ctx.random. Each rule draws
// from its own mulberry32 stream (derived via mixSeed) so adding/removing a kind
// does not shift the others. A node is accepted when a per-tile density roll
// passes; reachability (land, or shore for coastal) is checked first.
export function scatterNodes(seed: number, rules: ScatterRule[]): ScatteredNode[] {
  const out: ScatteredNode[] = [];
  rules.forEach((rule, ri) => {
    const rand = mulberry32(mixSeed(seed, 1013 * (ri + 1)));
    let placed = 0;
    let attempts = 0;
    const budget = Math.max(60, rule.count) * 80;
    while (placed < rule.count && attempts < budget) {
      attempts++;
      const x = 3 + rand() * (WORLD_SIZE - 6);
      const y = 3 + rand() * (WORLD_SIZE - 6);
      const reachable = rule.coastalOnly
        ? isCoastal(seed, x, y)
        : isLand(seed, x, y);
      if (!reachable) continue;
      const roll = hash2(Math.floor(x), Math.floor(y), mixSeed(seed, ri + 1));
      if (roll < rule.density(sampleTerrain(seed, x, y).biome)) {
        out.push({ x, y, resType: rule.resType, yield: rule.yield });
        placed++;
      }
    }
  });
  return out;
}

// Spiral outward to the nearest buildable land near (x,y).
export function findLandNear(
  seed: number,
  x: number,
  y: number
): { x: number; y: number } {
  if (isLand(seed, x, y)) return { x, y };
  for (let r = 1; r < WORLD_SIZE; r += 1) {
    for (let a = 0; a < 24; a++) {
      const ang = (a / 24) * Math.PI * 2;
      const nx = Math.max(3, Math.min(WORLD_SIZE - 3, x + Math.cos(ang) * r));
      const ny = Math.max(3, Math.min(WORLD_SIZE - 3, y + Math.sin(ang) * r));
      if (isLand(seed, nx, ny)) return { x: nx, y: ny };
    }
  }
  return { x: WORLD_SIZE / 2, y: WORLD_SIZE / 2 };
}
