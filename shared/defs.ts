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
import { Tech } from './research.ts';
import type { Vec2 } from './sim.ts';

export * from './units.ts';
export * from './buildings_defs.ts';
export * from './tech.ts';
export * from './research.ts';

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
  // tactical layer — squad target priority, raids, defensive recall, scouting.
  // Decision QUALITY + reaction speed only, never a cheat.
  recallMargin: number; // extra attackers over home defenders before recalling
  recallFraction: number; // max share of the field army pulled home to defend
  raidFraction: number; // share of light cavalry peeled off to harass gatherers
  scouts: boolean; // sends an early scout toward the enemy (Hard only)
  defendReactDelay: number; // seconds of sustained threat before recalling
  raidReactDelay: number; // grace period before raids begin
  // research — Blacksmith techs the bot pursues, in priority order. Empty = never
  // researches (Easy). The brain starts the first affordable, prereq-met, not-yet-
  // owned tech via the SAME startResearchFor helper a human's reducer calls — no
  // free techs, no skipped costs/prereqs. Tech ids are shared/research.ts Tech.
  research: number[];
}

export const AiDifficulty = { Easy: 0, Normal: 1, Hard: 2 } as const;

export const AI_PROFILES: Record<number, AiProfile> = {
  // Easy: slow to decide, small army, no cavalry/siege — a gentle sparring foe.
  0: {
    label: 'Easy',
    decisionInterval: 2.0,
    waveSize: 4, waveInterval: 45, firstWaveDelay: 60,
    peasantTarget: 7, armyTarget: 6, coreArmy: 4, popBuffer: 2, foodFloorMult: 6,
    woodBuffer: 30, maxTowers: 1,
    wantsCavalry: false, wantsSiege: false, siegeTarget: 0, imamTarget: 0,
    defendThreat: 4, foodFloor: 12, reservePeasants: 2,
    // slow, blunt defense: recalls late and only a sliver of the army, never raids
    // or scouts — a gentle foe that mostly throws its main body at the keep.
    recallMargin: 2, recallFraction: 0.34, raidFraction: 0,
    scouts: false, defendReactDelay: 6, raidReactDelay: 9999,
    research: [], // Easy never visits the Blacksmith for upgrades
  },
  // Normal: steady tempo, mixed army with some cavalry and a single siege engine.
  1: {
    label: 'Normal',
    decisionInterval: 1.0,
    waveSize: 6, waveInterval: 35, firstWaveDelay: 45,
    peasantTarget: 9, armyTarget: 10, coreArmy: 6, popBuffer: 3, foodFloorMult: 6,
    woodBuffer: 40, maxTowers: 2,
    wantsCavalry: true, wantsSiege: true, siegeTarget: 1, imamTarget: 1,
    defendThreat: 3, foodFloor: 16, reservePeasants: 3,
    // a measured defender that peels a few raiders once the army is real, recalls
    // about half the field army on a real attack, but doesn't scout.
    recallMargin: 1, recallFraction: 0.5, raidFraction: 0.25,
    scouts: false, defendReactDelay: 3, raidReactDelay: 120,
    // armor first to survive the player's volleys, then a weapon edge.
    research: [Tech.ArmorMail, Tech.SharpenedBlades, Tech.FletchedArrows],
  },
  // Hard: thinks fast, teches the full tree quickly, counters precisely and
  // brings a siege train to crack the player's walls/keep.
  2: {
    label: 'Hard',
    decisionInterval: 0.6,
    waveSize: 8, waveInterval: 25, firstWaveDelay: 30,
    peasantTarget: 11, armyTarget: 14, coreArmy: 9, popBuffer: 4, foodFloorMult: 6,
    woodBuffer: 50, maxTowers: 3,
    wantsCavalry: true, wantsSiege: true, siegeTarget: 2, imamTarget: 1,
    defendThreat: 3, foodFloor: 20, reservePeasants: 4,
    // sharp tactics: raids the enemy economy early, recalls precisely and fast on
    // any threat, and scouts the map to react to what the player is actually doing.
    recallMargin: 0, recallFraction: 0.6, raidFraction: 0.34,
    scouts: true, defendReactDelay: 1, raidReactDelay: 75,
    // teches the full edge: armor, both weapons, then hp + masonry to harden base.
    research: [
      Tech.ArmorMail,
      Tech.SharpenedBlades,
      Tech.FletchedArrows,
      Tech.ArmorPlate,
      Tech.Conscription,
      Tech.Masonry,
    ],
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

// Build the tactical tuning view from a profile. Same one-place mapping pattern as
// plannerTuning — the brain reads this for recall / raid / scout decisions.
export function tacticalTuning(prof: AiProfile): import('./ai.ts').TacticalTuning {
  return {
    defendThreat: prof.defendThreat,
    recallMargin: prof.recallMargin,
    recallFraction: prof.recallFraction,
    raidFraction: prof.raidFraction,
    scouts: prof.scouts,
    defendReactDelay: prof.defendReactDelay,
    raidReactDelay: prof.raidReactDelay,
  };
}

// Themed commanders per faction, assigned to AIs in join order.
export const AI_NAMES_BY_FACTION: Record<Faction, string[]> = {
  0: [
    'Al-Afdal',
    'Al-Adil',
    'Taqi al-Din',
    'Gökböri',
    'Al-Mashtub',
    'Qaymaz al-Najmi',
    'Husam al-Din',
    'Badr al-Din',
  ],
  1: [
    'Reynald de Châtillon',
    'Guy de Lusignan',
    'Raymond of Tripoli',
    'Conrad of Montferrat',
    'Balian of Ibelin',
    'Gérard de Ridefort',
    'Humphrey of Toron',
    'Joscelin of Courtenay',
  ],
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
  {
    id: 'free-for-all',
    label: 'Free-for-All',
    description: '1 v 7 — the whole map against you.',
    enemies: [
      AiDifficulty.Hard,
      AiDifficulty.Hard,
      AiDifficulty.Normal,
      AiDifficulty.Normal,
      AiDifficulty.Normal,
      AiDifficulty.Easy,
      AiDifficulty.Easy,
    ],
  },
];

export const MAX_AI_OPPONENTS = 7; // 8 starts total, one is the player

// One distinct team colour per spawn slot (up to MAX_PLAYERS). Spaced around the
// hue wheel so all eight read apart at a glance on the minimap and unit tints.
export const PLAYER_COLORS = [
  0x2e7d32, // green
  0xb71c1c, // red
  0x1565c0, // blue
  0x6a1b9a, // purple
  0xef6c00, // orange
  0x00838f, // teal
  0xf9a825, // yellow
  0xad1457, // magenta
];

// Lowest free slot in [0, max) not present in `used`, else -1. Used to assign a
// STABLE spawn corner per player so a leaver freeing a slot never causes two
// players to share a corner (overlapping keeps).
export function allocSlot(used: ReadonlyArray<number>, max: number): number {
  const taken = new Set(used);
  for (let s = 0; s < max; s++) if (!taken.has(s)) return s;
  return -1;
}

// Deterministic spawn anchor per joining player index, for up to MAX_PLAYERS (8).
// Eight well-spread starts on the WORLD_SIZE map: the four corners followed by the
// four edge midpoints, so adjacent players are always ~half a map-side apart. The
// order interleaves so the FIRST few joiners (the common 1v1 / 1v2 / 1v3 case) take
// opposite corners, not neighbours. Each anchor sits inside SPAWN_MARGIN; the
// non-buildable ones are snapped onto land by findBuildableNear in foundPlayer.
export function spawnCorner(
  index: number,
  world = WORLD_SIZE,
  margin = SPAWN_MARGIN
): Vec2 {
  const lo = margin;
  const hi = world - margin;
  const mid = world / 2;
  const anchors: Vec2[] = [
    { x: lo, y: lo }, // 0 — corner SW
    { x: hi, y: hi }, // 1 — corner NE (opposite 0)
    { x: hi, y: lo }, // 2 — corner SE
    { x: lo, y: hi }, // 3 — corner NW (opposite 2)
    { x: mid, y: lo }, // 4 — edge S
    { x: mid, y: hi }, // 5 — edge N (opposite 4)
    { x: lo, y: mid }, // 6 — edge W
    { x: hi, y: mid }, // 7 — edge E (opposite 6)
  ];
  return anchors[index % anchors.length];
}
