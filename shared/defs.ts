// Data-driven content barrel: resource stats, factions, AI tuning, match setup.
// Unit content lives in ./units.ts (UnitDef + UNIT_DEFS), building content in
// ./buildings_defs.ts (BuildingDef + BUILDING_DEFS + BUILD_CATEGORIES), and the
// tech-tree predicate in ./tech.ts. Those are re-exported here so existing
// imports (`from './defs.ts'`) keep working unchanged.

import {
  WORLD_SIZE,
  SPAWN_MARGIN,
  TREE_COUNT,
  TREE_WOOD,
  STONE_NODES,
  STONE_YIELD,
  FOOD_NODES,
  FOOD_YIELD,
  GOLD_NODES,
  GOLD_YIELD,
} from './constants.ts';
import {
  type Biome,
  treeDensity,
  rockDensity,
  gameDensity,
  fishDensity,
  goldDensity,
} from './terrain.ts';
import { ResourceType, type Faction } from './enums.ts';
import type { Vec2 } from './sim.ts';

export * from './units.ts';
export * from './buildings_defs.ts';
export * from './tech.ts';

export interface ResourceDef {
  label: string;
  color: number;
  icon: string; // emoji shown in HUD/cost badges
}

export const RESOURCE_DEFS: Record<ResourceType, ResourceDef> = {
  [ResourceType.Wood]: { label: 'Wood', color: 0x4b7f2f, icon: '🪵' },
  [ResourceType.Stone]: { label: 'Stone', color: 0x9a9a9a, icon: '🪨' },
  [ResourceType.Food]: { label: 'Food', color: 0xc9a227, icon: '🍞' },
  [ResourceType.Gold]: { label: 'Gold', color: 0xffd24a, icon: '🪙' },
};

// One scatter rule per resource node kind: how many to place, how much each
// holds, the per-biome density used as a PRNG accept-probability, and whether
// the node may only sit on the coast (fish). Data-driven so scatterNodes loops
// these — adding a node kind never touches the placement code.
export interface NodeKindDef {
  resType: ResourceType;
  count: number;
  yield: number;
  density: (b: Biome) => number;
  coastalOnly: boolean;
}

// Food comes from two sources sharing one balance: game grazing inland and fish
// on the shore (coastal-only so they stay reachable). The split keeps fish from
// crowding out land-locked maps.
const FOOD_FISH = Math.round(FOOD_NODES * 0.4);
const FOOD_GAME = FOOD_NODES - FOOD_FISH;

export const NODE_KINDS: NodeKindDef[] = [
  { resType: ResourceType.Wood, count: TREE_COUNT, yield: TREE_WOOD, density: treeDensity, coastalOnly: false },
  { resType: ResourceType.Stone, count: STONE_NODES, yield: STONE_YIELD, density: rockDensity, coastalOnly: false },
  { resType: ResourceType.Food, count: FOOD_GAME, yield: FOOD_YIELD, density: gameDensity, coastalOnly: false },
  { resType: ResourceType.Food, count: FOOD_FISH, yield: FOOD_YIELD, density: fishDensity, coastalOnly: true },
  { resType: ResourceType.Gold, count: GOLD_NODES, yield: GOLD_YIELD, density: goldDensity, coastalOnly: false },
];

export const FACTION_LABELS: Record<Faction, string> = {
  0: 'Ayyubid',
  1: 'Crusader',
};

// Skirmish AI behaviour, data-driven per difficulty. The brain reducer reads
// these numbers; tuning the opponent never touches systems code.
export interface AiProfile {
  label: string;
  peasantTarget: number; // economy size to reach before pushing military
  armyTarget: number; // soldiers to build toward
  waveSize: number; // soldiers on hand before launching an assault
  waveInterval: number; // seconds between assaults
  firstWaveDelay: number; // grace period before the first assault
  maxTowers: number; // defensive towers near the keep
  woodBuffer: number; // reserve kept before optional (tower) spends
  archerRatio: number; // 0..1 share of soldiers trained as archers
  knightRatio: number; // 0..1 share trained as (expensive) knights
  cavalryRatio: number; // 0..1 share of Stable production that is cavalry
  siegeTarget: number; // siege engines to build toward (0 = never go siege)
  imamTarget: number; // support Imams to keep around (0 = never train one)
}

export const AiDifficulty = { Easy: 0, Normal: 1, Hard: 2 } as const;

export const AI_PROFILES: Record<number, AiProfile> = {
  0: { label: 'Easy', peasantTarget: 6, armyTarget: 6, waveSize: 4, waveInterval: 45, firstWaveDelay: 60, maxTowers: 1, woodBuffer: 30, archerRatio: 0.3, knightRatio: 0.0, cavalryRatio: 0.0, siegeTarget: 0, imamTarget: 0 },
  1: { label: 'Normal', peasantTarget: 8, armyTarget: 10, waveSize: 6, waveInterval: 35, firstWaveDelay: 45, maxTowers: 2, woodBuffer: 40, archerRatio: 0.35, knightRatio: 0.2, cavalryRatio: 0.3, siegeTarget: 1, imamTarget: 1 },
  2: { label: 'Hard', peasantTarget: 10, armyTarget: 14, waveSize: 8, waveInterval: 25, firstWaveDelay: 30, maxTowers: 3, woodBuffer: 50, archerRatio: 0.4, knightRatio: 0.3, cavalryRatio: 0.4, siegeTarget: 2, imamTarget: 1 },
};

// Themed commanders per faction, assigned to AIs in join order.
export const AI_NAMES_BY_FACTION: Record<Faction, string[]> = {
  0: ['Al-Afdal', 'Al-Adil', 'Taqi al-Din', 'Gökböri'],
  1: ['Reynald de Châtillon', 'Guy de Lusignan', 'Raymond of Tripoli', 'Conrad of Montferrat'],
};

export function aiName(faction: Faction, index: number): string {
  const pool = AI_NAMES_BY_FACTION[faction] ?? AI_NAMES_BY_FACTION[1];
  return pool[index % pool.length];
}

export function enemyFaction(faction: Faction): Faction {
  return faction === 0 ? 1 : 0;
}

// Skirmish match presets — the setup screen offers these, then lets the player
// tweak per-opponent difficulty. `enemies` holds one AiDifficulty per opponent.
export interface MatchPreset {
  id: string;
  label: string;
  description: string;
  enemies: number[];
}

export const MATCH_PRESETS: MatchPreset[] = [
  { id: 'duel', label: 'Duel', description: '1 v 1 — a single rival keep.', enemies: [AiDifficulty.Normal] },
  { id: 'skirmish', label: 'Skirmish', description: '1 v 2 — outnumbered on open ground.', enemies: [AiDifficulty.Normal, AiDifficulty.Easy] },
  { id: 'last-stand', label: 'Last Stand', description: '1 v 3 — every corner against you.', enemies: [AiDifficulty.Hard, AiDifficulty.Normal, AiDifficulty.Easy] },
];

export const MAX_AI_OPPONENTS = 3; // 4 corners total, one is the player

export const PLAYER_COLORS = [0x2e7d32, 0xb71c1c, 0x1565c0, 0x6a1b9a];

// Lowest free slot in [0, max) not present in `used`, else -1. Used to assign a
// STABLE spawn corner per player so a leaver freeing a slot never causes two
// players to share a corner (overlapping keeps).
export function allocSlot(used: ReadonlyArray<number>, max: number): number {
  const taken = new Set(used);
  for (let s = 0; s < max; s++) if (!taken.has(s)) return s;
  return -1;
}

// Deterministic spawn corner per joining player index.
export function spawnCorner(
  index: number,
  world = WORLD_SIZE,
  margin = SPAWN_MARGIN
): Vec2 {
  const lo = margin;
  const hi = world - margin;
  const corners: Vec2[] = [
    { x: lo, y: lo },
    { x: hi, y: hi },
    { x: hi, y: lo },
    { x: lo, y: hi },
  ];
  return corners[index % corners.length];
}
