// Deterministic biome terrain generated from a single seed. Shared by the
// module (authority: where land/resources are) and the client (render). No
// per-tile rows — both sides recompute from the seed.
import { fbm } from './noise.ts';
import { WORLD_SIZE } from './constants.ts';

export const Biome = {
  DeepWater: 0,
  ShallowWater: 1,
  Sand: 2,
  Desert: 3,
  Dunes: 4,
  Steppe: 5,
  Grassland: 6,
  Forest: 7,
  Hills: 8,
  Mountain: 9,
  Snow: 10,
} as const;
export type Biome = (typeof Biome)[keyof typeof Biome];

export const BIOME_LABEL: Record<Biome, string> = {
  [Biome.DeepWater]: 'Sea',
  [Biome.ShallowWater]: 'Shallows',
  [Biome.Sand]: 'Coast',
  [Biome.Desert]: 'Desert',
  [Biome.Dunes]: 'Dunes',
  [Biome.Steppe]: 'Steppe',
  [Biome.Grassland]: 'Grassland',
  [Biome.Forest]: 'Forest',
  [Biome.Hills]: 'Hills',
  [Biome.Mountain]: 'Mountain',
  [Biome.Snow]: 'Snow',
};

export const BIOME_COLOR: Record<Biome, number> = {
  [Biome.DeepWater]: 0x1f5673,
  [Biome.ShallowWater]: 0x3a86a8,
  [Biome.Sand]: 0xe2cf96,
  [Biome.Desert]: 0xdcb866,
  [Biome.Dunes]: 0xcaa257,
  [Biome.Steppe]: 0xb3ad6b,
  [Biome.Grassland]: 0x77a64a,
  [Biome.Forest]: 0x3f7d38,
  [Biome.Hills]: 0x8f7d54,
  [Biome.Mountain]: 0x7c7167,
  [Biome.Snow]: 0xeef2f5,
};

export interface TerrainSample {
  height: number;
  moisture: number;
  biome: Biome;
}

const H_SCALE = 0.045;
const M_SCALE = 0.03;
const SEA = 0.38;

// Continent falloff: water rings the map edge, land in the middle.
function radial(x: number, y: number): number {
  const c = WORLD_SIZE / 2;
  const d = Math.hypot(x - c, y - c) / (WORLD_SIZE * 0.5);
  return Math.max(0, 1.12 - Math.pow(d, 2.6) * 0.95);
}

export function sampleTerrain(seed: number, x: number, y: number): TerrainSample {
  let h = fbm(x * H_SCALE, y * H_SCALE, seed, 5);
  h = h * 0.78 + 0.18;
  h *= radial(x, y);
  const moisture = fbm(x * M_SCALE + 100, y * M_SCALE + 50, seed ^ 0x9e37, 4);
  return { height: h, moisture, biome: classify(h, moisture) };
}

function classify(h: number, m: number): Biome {
  if (h < SEA - 0.06) return Biome.DeepWater;
  if (h < SEA) return Biome.ShallowWater;
  if (h < SEA + 0.04) return Biome.Sand;
  if (h > 0.82) return Biome.Snow;
  if (h > 0.72) return Biome.Mountain;
  if (h > 0.62) return Biome.Hills;
  if (m < 0.3) return Biome.Desert;
  if (m < 0.42) return Biome.Dunes;
  if (m < 0.55) return Biome.Steppe;
  if (m < 0.72) return Biome.Grassland;
  return Biome.Forest;
}

// Render elevation in world units: water dips, land rises with height.
export function elevation(h: number): number {
  if (h < SEA) return -0.4 * ((SEA - h) / SEA) - 0.05;
  return (h - SEA) * 7;
}

export function isLand(seed: number, x: number, y: number): boolean {
  const b = sampleTerrain(seed, x, y).biome;
  return (
    b !== Biome.DeepWater &&
    b !== Biome.ShallowWater &&
    b !== Biome.Mountain &&
    b !== Biome.Snow
  );
}

// Probability a tree spawns on a given biome (used with a PRNG draw).
export function treeDensity(b: Biome): number {
  if (b === Biome.Forest) return 0.85;
  if (b === Biome.Grassland) return 0.32;
  if (b === Biome.Steppe) return 0.06;
  return 0;
}

// Stone outcrops cluster in the rocky uplands; quarries skirt the mountains.
export function rockDensity(b: Biome): number {
  if (b === Biome.Hills) return 0.55;
  if (b === Biome.Mountain) return 0.4;
  if (b === Biome.Steppe) return 0.12;
  if (b === Biome.Grassland) return 0.05;
  return 0;
}

// Wild game (food) grazes the open grass and steppe, never the bare desert.
export function gameDensity(b: Biome): number {
  if (b === Biome.Grassland) return 0.4;
  if (b === Biome.Steppe) return 0.28;
  if (b === Biome.Forest) return 0.12;
  return 0;
}

// Fishing only pays off on the shoreline; the caller pairs this with isCoastal
// so a fish node never lands out in open water or inland.
export function fishDensity(b: Biome): number {
  if (b === Biome.Sand) return 0.6;
  if (b === Biome.Grassland) return 0.15;
  if (b === Biome.Steppe) return 0.1;
  return 0;
}

// Gold veins are mined out of the rocky uplands skirting the mountains.
export function goldDensity(b: Biome): number {
  if (b === Biome.Mountain) return 0.35;
  if (b === Biome.Hills) return 0.18;
  return 0;
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
