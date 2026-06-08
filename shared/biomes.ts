// DATA catalog of every biome. One row per biome holds its render colors, its
// gameplay flags (passable / buildable), a movement-cost multiplier, the
// per-resource spawn densities used as PRNG accept-probabilities by node
// placement, and the cosmetic decoration that scatters on it (client-only props,
// zero DB rows). This is the single source of truth: terrain.ts classifies a tile
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

// Cosmetic prop kinds the client scatters per biome. NOT gameplay rows — pure
// decoration recomputed from the seed (see src/game/Vegetation.ts). Resource
// trees/rocks/forage are separate DB-backed nodes; these are the dressing around
// them. New kinds need a mesh factory in Vegetation.ts.
export const Decoration = {
  None: 0,
  Shrub: 1, // dry brush — steppe / desert fringe
  Palm: 2, // oasis + coastal palms
  Rock: 3, // loose stones — hills / mountains / steppe
  DuneGrass: 4, // tufts on sand / dunes
  PineCluster: 5, // small cosmetic conifers thickening the forest
  Boulder: 6, // big snowy/mountain boulders
  Reeds: 7, // marsh reeds at the shallows' edge
} as const;
export type Decoration = (typeof Decoration)[keyof typeof Decoration];

export interface BiomeDef {
  label: string;
  color: number; // base vertex color
  shade: number; // darker facet color blended in by elevation (low-poly depth)
  passable: boolean; // a unit may stand/walk on this tile
  buildable: boolean; // a structure may be founded here
  moveCostMul: number; // pathing/movement multiplier (1 = baseline ground)
  heightEmphasis: number; // render-only relief multiplier for this biome's band
  // Cosmetic decoration: which prop scatters here and its accept-probability in
  // [0,1] against a per-tile deterministic roll. kind None = bare ground.
  decoration: { kind: Decoration; density: number };
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

// Decoration shorthand — None at 0 unless specified.
const D0 = { kind: Decoration.None, density: 0 } as const;
const deco = (kind: Decoration, density: number) => ({ kind, density });

export const BIOME_DEFS: Record<Biome, BiomeDef> = {
  [Biome.DeepWater]: {
    label: 'Sea',
    color: 0x1f5673,
    shade: 0x103a52,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    heightEmphasis: 1,
    decoration: D0,
    density: dens({}),
  },
  [Biome.ShallowWater]: {
    label: 'Shallows',
    color: 0x3a86a8,
    shade: 0x2a6f8f,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    heightEmphasis: 1,
    decoration: deco(Decoration.Reeds, 0.05),
    density: dens({}),
  },
  [Biome.Sand]: {
    label: 'Coast',
    color: 0xe2cf96,
    shade: 0xc9b377,
    passable: true,
    buildable: true,
    moveCostMul: 1.1,
    heightEmphasis: 0.6,
    decoration: deco(Decoration.DuneGrass, 0.1),
    density: dens({ fish: 0.6 }),
  },
  [Biome.Desert]: {
    label: 'Desert',
    color: 0xdcb866,
    shade: 0xc09a48,
    passable: true,
    buildable: true,
    moveCostMul: 1.2,
    heightEmphasis: 0.7,
    decoration: deco(Decoration.Shrub, 0.05),
    density: dens({}),
  },
  [Biome.Dunes]: {
    label: 'Dunes',
    color: 0xcaa257,
    shade: 0xa9853f,
    passable: true,
    buildable: true,
    moveCostMul: 1.5, // soft sand — slow going
    heightEmphasis: 1.2, // rolling dunes read taller
    decoration: deco(Decoration.DuneGrass, 0.12),
    density: dens({}),
  },
  [Biome.Steppe]: {
    label: 'Steppe',
    color: 0xb3ad6b,
    shade: 0x938c4f,
    passable: true,
    buildable: true,
    moveCostMul: 1.0,
    heightEmphasis: 0.9,
    decoration: deco(Decoration.Shrub, 0.1),
    density: dens({ tree: 0.06, rock: 0.12, game: 0.28, fish: 0.1 }),
  },
  [Biome.Grassland]: {
    label: 'Grassland',
    color: 0x77a64a,
    shade: 0x577f33,
    passable: true,
    buildable: true,
    moveCostMul: 1.0,
    heightEmphasis: 0.9,
    decoration: deco(Decoration.Shrub, 0.07),
    density: dens({ tree: 0.32, rock: 0.05, game: 0.4, fish: 0.15 }),
  },
  [Biome.Forest]: {
    label: 'Forest',
    color: 0x3f7d38,
    shade: 0x285626,
    passable: true,
    buildable: true,
    moveCostMul: 1.3, // dense timber slows a column
    heightEmphasis: 1.0,
    decoration: deco(Decoration.PineCluster, 0.22),
    density: dens({ tree: 0.85, game: 0.12 }),
  },
  [Biome.Hills]: {
    label: 'Hills',
    color: 0x8f7d54,
    shade: 0x6b5d3c,
    passable: true,
    buildable: true,
    moveCostMul: 1.4, // climbing
    heightEmphasis: 1.6, // hills should clearly rise
    decoration: deco(Decoration.Rock, 0.16),
    density: dens({ rock: 0.55, gold: 0.18 }),
  },
  [Biome.Mountain]: {
    label: 'Mountain',
    color: 0x7c7167,
    shade: 0x564e47,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    heightEmphasis: 2.4, // dramatic peaks
    decoration: deco(Decoration.Boulder, 0.14),
    density: dens({ rock: 0.4, gold: 0.35 }),
  },
  [Biome.Snow]: {
    label: 'Snow',
    color: 0xeef2f5,
    shade: 0xc7d3dc,
    passable: false,
    buildable: false,
    moveCostMul: Infinity,
    heightEmphasis: 2.6,
    decoration: deco(Decoration.Boulder, 0.08),
    density: dens({}),
  },
  [Biome.Oasis]: {
    label: 'Oasis',
    color: 0x4f9d6a,
    shade: 0x357a4c,
    passable: true,
    buildable: true,
    moveCostMul: 1.0,
    heightEmphasis: 0.7,
    decoration: deco(Decoration.Palm, 0.3),
    density: dens({ tree: 0.45, game: 0.35, fish: 0.2 }),
  },
};

export const BIOME_LABEL: Record<Biome, string> = Object.fromEntries(
  (Object.keys(BIOME_DEFS) as unknown as Biome[]).map((b) => [b, BIOME_DEFS[b].label])
) as Record<Biome, string>;

export const BIOME_COLOR: Record<Biome, number> = Object.fromEntries(
  (Object.keys(BIOME_DEFS) as unknown as Biome[]).map((b) => [b, BIOME_DEFS[b].color])
) as Record<Biome, number>;

export const BIOME_SHADE: Record<Biome, number> = Object.fromEntries(
  (Object.keys(BIOME_DEFS) as unknown as Biome[]).map((b) => [b, BIOME_DEFS[b].shade])
) as Record<Biome, number>;

export function biomeHeightEmphasis(b: Biome): number {
  return BIOME_DEFS[b].heightEmphasis;
}
export function biomeDecoration(b: Biome): { kind: Decoration; density: number } {
  return BIOME_DEFS[b].decoration;
}

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
