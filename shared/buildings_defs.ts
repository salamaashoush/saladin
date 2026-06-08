// Data-driven building content: stats, footprints, production rosters, tech
// prerequisites and presentation for every structure. New buildings slot in by
// adding one BUILDING_DEFS entry plus an enum value — placement, occupancy,
// training and the tech gate all dispatch on the numeric kind.
//
// NOTE: footprint MATH lives in shared/buildings.ts. This file is the DATA.

import { BuildingKind, UnitKind, DamageType, ArmorClass } from './enums.ts';
import type { ResourceCost } from './economy.ts';

export interface BuildingDef {
  label: string;
  icon: string; // emoji shown in the build UI
  footprint: number; // tiles per side (integer)
  height: number;
  cost: ResourceCost;
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
  requires?: BuildingKind; // tech prereq: owner must already have this building
  enablesTrade?: boolean; // gates the marketTrade reducer (Market)
  foodDropoff?: boolean; // alternate food deposit point (FishingHut)
  requiresWater?: boolean; // must be placed water-adjacent (FishingHut)
}

const B = (
  label: string,
  footprint: number,
  height: number,
  cost: ResourceCost,
  maxHp: number,
  buildable: boolean,
  extra: Partial<BuildingDef> = {}
): BuildingDef => ({
  label,
  icon: '🏗️',
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
  [BuildingKind.Keep]: B('Keep', 3, 1.8, { wood: 0 }, 1500, false, {
    icon: '🏰',
    pop: 8,
    trains: [UnitKind.Peasant],
  }),
  [BuildingKind.Barracks]: B('Barracks', 2, 1.4, { wood: 70, stone: 20 }, 500, true, {
    icon: '🏛️',
    trains: [UnitKind.Spearman, UnitKind.Archer, UnitKind.Crossbowman],
    armorClass: ArmorClass.Leather, // timber hall — chops faster than stone
  }),
  [BuildingKind.Tower]: B('Tower', 1, 2.6, { wood: 40, stone: 30 }, 400, true, {
    icon: '🗼',
    attack: 9,
    range: 7,
    attackRate: 0.9,
  }),
  [BuildingKind.Wall]: B('Wall', 1, 1.2, { wood: 6, stone: 6 }, 300, true, { icon: '🧱' }),
  [BuildingKind.Gatehouse]: B('Gatehouse', 1, 1.5, { wood: 15, stone: 15 }, 400, true, {
    icon: '🚪',
    passable: true,
  }),
  [BuildingKind.House]: B('House', 2, 1.2, { wood: 40 }, 250, true, {
    icon: '🏠',
    pop: 6,
    armorClass: ArmorClass.Leather,
  }),
  [BuildingKind.Stable]: B('Stable', 2, 1.4, { wood: 80, stone: 20 }, 500, true, {
    icon: '🐴',
    trains: [UnitKind.Knight, UnitKind.HorseArcher, UnitKind.Mamluk],
    armorClass: ArmorClass.Leather,
    requires: BuildingKind.Barracks,
  }),
  [BuildingKind.Blacksmith]: B('Blacksmith', 2, 1.5, { wood: 60, stone: 40 }, 550, true, {
    icon: '⚒️',
    requires: BuildingKind.Barracks,
  }),
  [BuildingKind.Market]: B('Market', 2, 1.3, { wood: 60, stone: 20 }, 450, true, {
    icon: '🏪',
    armorClass: ArmorClass.Leather,
    enablesTrade: true,
    requires: BuildingKind.Keep,
  }),
  [BuildingKind.Granary]: B('Granary', 2, 1.3, { wood: 50, stone: 10 }, 400, true, {
    icon: '🌾',
    pop: 4, // simple data-driven effect: extra population headroom
    armorClass: ArmorClass.Leather,
    foodDropoff: true,
    requires: BuildingKind.Keep,
  }),
  [BuildingKind.FishingHut]: B('Fishing Hut', 1, 1.0, { wood: 35 }, 250, true, {
    icon: '🎣',
    armorClass: ArmorClass.Leather,
    foodDropoff: true, // alternate food deposit point on the shore
    requiresWater: true,
    requires: BuildingKind.Keep,
  }),
  [BuildingKind.SiegeWorkshop]: B('Siege Workshop', 2, 1.5, { wood: 100, stone: 40 }, 600, true, {
    icon: '🛠️',
    trains: [UnitKind.Ram, UnitKind.Mangonel],
    armorClass: ArmorClass.Leather,
    requires: BuildingKind.Blacksmith,
  }),
};

export const BUILD_CATEGORIES: { label: string; icon: string; kinds: BuildingKind[] }[] = [
  {
    label: 'Defense',
    icon: '🛡️',
    kinds: [BuildingKind.Wall, BuildingKind.Gatehouse, BuildingKind.Tower],
  },
  {
    label: 'Economy',
    icon: '🏠',
    kinds: [
      BuildingKind.House,
      BuildingKind.Market,
      BuildingKind.Granary,
      BuildingKind.FishingHut,
    ],
  },
  {
    label: 'Military',
    icon: '⚔️',
    kinds: [BuildingKind.Barracks],
  },
  {
    label: 'Cavalry',
    icon: '🐴',
    kinds: [BuildingKind.Stable],
  },
  {
    label: 'Siege',
    icon: '🛠️',
    kinds: [BuildingKind.SiegeWorkshop],
  },
  {
    label: 'Tech',
    icon: '⚒️',
    kinds: [BuildingKind.Blacksmith],
  },
];
