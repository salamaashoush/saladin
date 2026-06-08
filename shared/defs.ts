// Data-driven content: unit/building/resource stats + presentation. New unit
// types, buildings, factions slot in here without touching systems code.

import { WORLD_SIZE, SPAWN_MARGIN } from './constants.ts';
import {
  UnitKind,
  BuildingKind,
  ResourceType,
  DamageType,
  ArmorClass,
  type Faction,
} from './enums.ts';
import type { Vec2 } from './sim.ts';

export interface UnitDef {
  label: string;
  speed: number; // world units / second
  carry: number; // wood per gather trip (0 = non-gatherer)
  radius: number;
  height: number;
  maxHp: number;
  attack: number; // base damage per hit (0 = non-combatant)
  damageType: DamageType;
  armorClass: ArmorClass;
  bonusVsArmor?: Partial<Record<ArmorClass, number>>; // specialist counter
  range: number; // attack reach in world units
  attackRate: number; // seconds between hits
  aggroRange: number; // auto-acquire enemies within (0 = never auto-aggro)
  cost: number; // wood to train
  tint?: number; // mesh tint override; otherwise owner color
}

export const UNIT_DEFS: Record<UnitKind, UnitDef> = {
  [UnitKind.Peasant]: {
    label: 'Peasant',
    speed: 2.5,
    carry: 8,
    radius: 0.22,
    height: 0.7,
    maxHp: 30,
    attack: 0,
    damageType: DamageType.Blunt,
    armorClass: ArmorClass.Unarmored,
    range: 0.8,
    attackRate: 1.2,
    aggroRange: 0,
    cost: 20,
  },
  [UnitKind.Spearman]: {
    label: 'Spearman',
    speed: 2.2,
    carry: 0,
    radius: 0.26,
    height: 0.85,
    maxHp: 70,
    attack: 12,
    damageType: DamageType.Pierce,
    armorClass: ArmorClass.Leather,
    bonusVsArmor: { [ArmorClass.Mail]: 2.6 }, // braced against mailed cavalry
    range: 1.2, // long reach — outranges a knight's blade
    attackRate: 1.0,
    aggroRange: 6,
    cost: 35,
    tint: 0x3a3a3a,
  },
  [UnitKind.Archer]: {
    label: 'Archer',
    speed: 2.4,
    carry: 0,
    radius: 0.24,
    height: 0.8,
    maxHp: 45,
    attack: 9,
    damageType: DamageType.Pierce,
    armorClass: ArmorClass.Leather,
    range: 5,
    attackRate: 1.4,
    aggroRange: 7,
    cost: 45,
    tint: 0x5a3a1a,
  },
  [UnitKind.Knight]: {
    label: 'Knight',
    speed: 3.4, // fast — runs down archers
    carry: 0,
    radius: 0.3,
    height: 1.0,
    maxHp: 130,
    attack: 17,
    damageType: DamageType.Slash, // shreds unarmored/leather, glances mail
    armorClass: ArmorClass.Mail, // shrugs off arrows, but spears punch through
    range: 1.0,
    attackRate: 1.1,
    aggroRange: 7,
    cost: 90,
    tint: 0x9a8050,
  },
};

export interface BuildingDef {
  label: string;
  footprint: number; // tiles per side (integer)
  height: number;
  cost: number;
  maxHp: number;
  buildable: boolean;
  pop: number; // population capacity provided
  attack: number; // tower fire damage (0 = not a shooter)
  damageType: DamageType; // tower fire type
  armorClass: ArmorClass; // how the structure resists damage
  range: number; // tower fire range
  attackRate: number; // seconds between shots
  passable: boolean; // units may walk through (gatehouse)
  trains: number[]; // UnitKinds this building can produce
}

const B = (
  label: string,
  footprint: number,
  height: number,
  cost: number,
  maxHp: number,
  buildable: boolean,
  extra: Partial<BuildingDef> = {}
): BuildingDef => ({
  label,
  footprint,
  height,
  cost,
  maxHp,
  buildable,
  pop: 0,
  attack: 0,
  damageType: DamageType.Pierce,
  armorClass: ArmorClass.Stone,
  range: 0,
  attackRate: 0,
  passable: false,
  trains: [],
  ...extra,
});

export const BUILDING_DEFS: Record<BuildingKind, BuildingDef> = {
  [BuildingKind.Keep]: B('Keep', 3, 1.8, 0, 1500, false, {
    pop: 8,
    trains: [UnitKind.Peasant],
  }),
  [BuildingKind.Barracks]: B('Barracks', 2, 1.4, 80, 500, true, {
    trains: [UnitKind.Spearman, UnitKind.Archer, UnitKind.Knight],
    armorClass: ArmorClass.Leather, // timber hall — chops faster than stone
  }),
  [BuildingKind.Tower]: B('Tower', 1, 2.6, 60, 400, true, {
    attack: 9,
    range: 7,
    attackRate: 0.9,
  }),
  [BuildingKind.Wall]: B('Wall', 1, 1.2, 12, 300, true),
  [BuildingKind.Gatehouse]: B('Gatehouse', 1, 1.5, 25, 400, true, {
    passable: true,
  }),
  [BuildingKind.House]: B('House', 2, 1.2, 40, 250, true, {
    pop: 6,
    armorClass: ArmorClass.Leather,
  }),
};

export const BUILD_CATEGORIES: { label: string; icon: string; kinds: BuildingKind[] }[] = [
  {
    label: 'Defense',
    icon: '🛡️',
    kinds: [BuildingKind.Wall, BuildingKind.Gatehouse, BuildingKind.Tower],
  },
  { label: 'Economy', icon: '🏠', kinds: [BuildingKind.House] },
  { label: 'Military', icon: '⚔️', kinds: [BuildingKind.Barracks] },
];

export interface ResourceDef {
  label: string;
  color: number;
}

export const RESOURCE_DEFS: Record<ResourceType, ResourceDef> = {
  [ResourceType.Wood]: { label: 'Wood', color: 0x4b7f2f },
  [ResourceType.Stone]: { label: 'Stone', color: 0x9a9a9a },
  [ResourceType.Food]: { label: 'Food', color: 0xc9a227 },
  [ResourceType.Gold]: { label: 'Gold', color: 0xffd24a },
};

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
}

export const AiDifficulty = { Easy: 0, Normal: 1, Hard: 2 } as const;

export const AI_PROFILES: Record<number, AiProfile> = {
  0: { label: 'Easy', peasantTarget: 6, armyTarget: 6, waveSize: 4, waveInterval: 45, firstWaveDelay: 60, maxTowers: 1, woodBuffer: 30, archerRatio: 0.3, knightRatio: 0.0 },
  1: { label: 'Normal', peasantTarget: 8, armyTarget: 10, waveSize: 6, waveInterval: 35, firstWaveDelay: 45, maxTowers: 2, woodBuffer: 40, archerRatio: 0.35, knightRatio: 0.2 },
  2: { label: 'Hard', peasantTarget: 10, armyTarget: 14, waveSize: 8, waveInterval: 25, firstWaveDelay: 30, maxTowers: 3, woodBuffer: 50, archerRatio: 0.4, knightRatio: 0.3 },
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
