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

// Skirmish AI behaviour, data-driven per difficulty. The strategic planner
// (shared/ai.ts) reads the economy/army/tech knobs; the brain reducer reads the
// assault cadence + decision interval. Difficulty is DECISION QUALITY and CADENCE
// only — never a resource, vision or production handicap. Easy reacts slowly with
// a small army and no siege; Hard teches fast, counters precisely, and uses
// siege. Tuning the opponent never touches systems code.
export interface AiProfile {
  label: string;
  // cadence — how fast the bot thinks and how aggressively it pushes out
  decisionInterval: number; // seconds between macro decisions (lower = sharper)
  waveSize: number; // soldiers on hand before launching an assault
  waveInterval: number; // seconds between assaults
  firstWaveDelay: number; // grace period before the first assault
  // planner tuning — fed straight into PlannerTuning (shared/ai.ts)
  peasantTarget: number; // economy size to reach before pushing military
  armyTarget: number; // soldiers to build toward
  coreArmy: number; // standing army kept WHILE teching (defensive core)
  popBuffer: number; // free pop headroom to keep before building a House
  foodFloorMult: number; // bias to food while food <= upkeep * this
  woodBuffer: number; // reserve kept before optional (tower) spends
  maxTowers: number; // defensive towers near the keep
  wantsCavalry: boolean; // teches a Stable and fields cavalry
  wantsSiege: boolean; // teches Blacksmith→SiegeWorkshop and fields siege
  siegeTarget: number; // siege engines to build toward (0 = never go siege)
  imamTarget: number; // support Imams to keep around (0 = never train one)
  defendThreat: number; // enemy combatants near home that trigger Defend
  foodFloor: number; // food balance below which the bot stops adding upkeep
  reservePeasants: number; // extra gatherers added during a food crisis
}

export const AiDifficulty = { Easy: 0, Normal: 1, Hard: 2 } as const;

export const AI_PROFILES: Record<number, AiProfile> = {
  // Easy: slow to decide, small army, no cavalry/siege — a gentle sparring foe.
  0: {
    label: 'Easy',
    decisionInterval: 2.0,
    waveSize: 4, waveInterval: 45, firstWaveDelay: 60,
    peasantTarget: 6, armyTarget: 6, coreArmy: 3, popBuffer: 1, foodFloorMult: 4,
    woodBuffer: 30, maxTowers: 1,
    wantsCavalry: false, wantsSiege: false, siegeTarget: 0, imamTarget: 0,
    defendThreat: 4, foodFloor: 12, reservePeasants: 2,
  },
  // Normal: steady tempo, mixed army with some cavalry and a single siege engine.
  1: {
    label: 'Normal',
    decisionInterval: 1.0,
    waveSize: 6, waveInterval: 35, firstWaveDelay: 45,
    peasantTarget: 8, armyTarget: 10, coreArmy: 4, popBuffer: 2, foodFloorMult: 4,
    woodBuffer: 40, maxTowers: 2,
    wantsCavalry: true, wantsSiege: true, siegeTarget: 1, imamTarget: 1,
    defendThreat: 3, foodFloor: 16, reservePeasants: 3,
  },
  // Hard: thinks fast, teches the full tree quickly, counters precisely and
  // brings a siege train to crack the player's walls/keep.
  2: {
    label: 'Hard',
    decisionInterval: 0.6,
    waveSize: 8, waveInterval: 25, firstWaveDelay: 30,
    peasantTarget: 10, armyTarget: 14, coreArmy: 5, popBuffer: 3, foodFloorMult: 5,
    woodBuffer: 50, maxTowers: 3,
    wantsCavalry: true, wantsSiege: true, siegeTarget: 2, imamTarget: 1,
    defendThreat: 3, foodFloor: 20, reservePeasants: 4,
  },
};

// Build the planner's tuning view from a profile. Pure mapping, kept here so the
// profile shape and the planner stay in one place.
export function plannerTuning(prof: AiProfile): import('./ai.ts').PlannerTuning {
  return {
    peasantTarget: prof.peasantTarget,
    armyTarget: prof.armyTarget,
    coreArmy: prof.coreArmy,
    popBuffer: prof.popBuffer,
    foodFloorMult: prof.foodFloorMult,
    woodBuffer: prof.woodBuffer,
    maxTowers: prof.maxTowers,
    wantsCavalry: prof.wantsCavalry,
    wantsSiege: prof.wantsSiege,
    siegeTarget: prof.siegeTarget,
    imamTarget: prof.imamTarget,
    defendThreat: prof.defendThreat,
    foodFloor: prof.foodFloor,
    reservePeasants: prof.reservePeasants,
  };
}

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
