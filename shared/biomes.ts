// DATA catalog of every biome. One row per biome holds its render color, its
// gameplay flags (passable / buildable), a movement-cost multiplier, and the
// per-resource spawn densities used as PRNG accept-probabilities by node
// placement. This is the single source of truth: terrain.ts classifies a tile
// into a Biome and reads everything else from here; the density helper functions
// that used to live in terrain.ts are now thin lookups into this catalog.

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
  Oasis: 11,
} as const;
export type Biome = (typeof Biome)[keyof typeof Biome];

export interface BiomeDef {
  label: string;
  color: number;
  passable: boolean; // a unit may stand/walk on this tile
  buildable: boolean; // a structure may be founded here
  moveCostMul: number; // pathing/movement multiplier (1 = baseline ground)
  // Per-resource spawn density in [0,1], used as an accept-probability against a
  // deterministic per-tile roll. 0 means the resource never spawns on this biome.
  density: {
    tree: number;
    rock: number;
    game: number;
    fish: number;
    gold: number;
  };
}

// Density shorthand — omitted resources default to 0.
type D = Partial<BiomeDef['density']>;
const dens = (d: D): BiomeDef['density'] => ({
  tree: 0,
  rock: 0,
  game: 0,
  fish: 0,
  gold: 0,
  ...d,
});

export const BIOME_DEFS: Record<Biome, BiomeDef> = {
  [Biome.DeepWater]: {
    label: 'Sea',
    color: 0x1f5673,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    density: dens({}),
  },
  [Biome.ShallowWater]: {
    label: 'Shallows',
    color: 0x3a86a8,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    density: dens({}),
  },
  [Biome.Sand]: {
    label: 'Coast',
    color: 0xe2cf96,
    passable: true,
    buildable: true,
    moveCostMul: 1.1,
    density: dens({ fish: 0.6 }),
  },
  [Biome.Desert]: {
    label: 'Desert',
    color: 0xdcb866,
    passable: true,
    buildable: true,
    moveCostMul: 1.2,
    density: dens({}),
  },
  [Biome.Dunes]: {
    label: 'Dunes',
    color: 0xcaa257,
    passable: true,
    buildable: true,
    moveCostMul: 1.5, // soft sand — slow going
    density: dens({}),
  },
  [Biome.Steppe]: {
    label: 'Steppe',
    color: 0xb3ad6b,
    passable: true,
    buildable: true,
    moveCostMul: 1.0,
    density: dens({ tree: 0.06, rock: 0.12, game: 0.28, fish: 0.1 }),
  },
  [Biome.Grassland]: {
    label: 'Grassland',
    color: 0x77a64a,
    passable: true,
    buildable: true,
    moveCostMul: 1.0,
    density: dens({ tree: 0.32, rock: 0.05, game: 0.4, fish: 0.15 }),
  },
  [Biome.Forest]: {
    label: 'Forest',
    color: 0x3f7d38,
    passable: true,
    buildable: true,
    moveCostMul: 1.3, // dense timber slows a column
    density: dens({ tree: 0.85, game: 0.12 }),
  },
  [Biome.Hills]: {
    label: 'Hills',
    color: 0x8f7d54,
    passable: true,
    buildable: true,
    moveCostMul: 1.4, // climbing
    density: dens({ rock: 0.55, gold: 0.18 }),
  },
  [Biome.Mountain]: {
    label: 'Mountain',
    color: 0x7c7167,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    density: dens({ rock: 0.4, gold: 0.35 }),
  },
  [Biome.Snow]: {
    label: 'Snow',
    color: 0xeef2f5,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    density: dens({}),
  },
  [Biome.Oasis]: {
    label: 'Oasis',
    color: 0x4f9d6a,
    passable: true,
    buildable: true,
    moveCostMul: 1.0,
    density: dens({ tree: 0.45, game: 0.35, fish: 0.2 }),
  },
};

export const BIOME_LABEL: Record<Biome, string> = Object.fromEntries(
  (Object.keys(BIOME_DEFS) as unknown as Biome[]).map((b) => [b, BIOME_DEFS[b].label])
) as Record<Biome, string>;

export const BIOME_COLOR: Record<Biome, number> = Object.fromEntries(
  (Object.keys(BIOME_DEFS) as unknown as Biome[]).map((b) => [b, BIOME_DEFS[b].color])
) as Record<Biome, number>;

export function biomePassable(b: Biome): boolean {
  return BIOME_DEFS[b].passable;
}
export function biomeBuildable(b: Biome): boolean {
  return BIOME_DEFS[b].buildable;
}
export function moveCostMul(b: Biome): number {
  return BIOME_DEFS[b].moveCostMul;
}

// Density accessors — thin lookups into the catalog. Kept as named functions so
// NODE_KINDS in defs.ts can reference them directly and terrain.ts can re-export
// them under their historical names.
export function treeDensity(b: Biome): number {
  return BIOME_DEFS[b].density.tree;
}
export function rockDensity(b: Biome): number {
  return BIOME_DEFS[b].density.rock;
}
export function gameDensity(b: Biome): number {
  return BIOME_DEFS[b].density.game;
}
export function fishDensity(b: Biome): number {
  return BIOME_DEFS[b].density.fish;
}
export function goldDensity(b: Biome): number {
  return BIOME_DEFS[b].density.gold;
}
