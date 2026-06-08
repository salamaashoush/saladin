// Data-driven unit content: stats + presentation for every trainable unit. New
// unit types slot in here by adding one UNIT_DEFS entry — the generic combat,
// gather and movement systems already dispatch on the numeric kind, so no
// systems code changes when the roster grows.

import {
  UnitKind,
  BuildingKind,
  DamageType,
  ArmorClass,
} from './enums.ts';
import type { ResourceCost } from './economy.ts';

export interface UnitDef {
  label: string;
  icon: string; // emoji shown in the train/selection UI
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
  cost: ResourceCost; // resources to train
  tint?: number; // mesh tint override; otherwise owner color
  requires?: BuildingKind; // extra tech prereq beyond the training building
  prefersBuildings?: boolean; // siege: hunt structures over soft targets
}

export const UNIT_DEFS: Record<UnitKind, UnitDef> = {
  [UnitKind.Peasant]: {
    label: 'Peasant',
    icon: '🧑‍🌾',
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
    cost: { wood: 20 },
  },
  [UnitKind.Spearman]: {
    label: 'Spearman',
    icon: '🛡️',
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
    cost: { wood: 35 },
    tint: 0x3a3a3a,
  },
  [UnitKind.Archer]: {
    label: 'Archer',
    icon: '🏹',
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
    cost: { wood: 45 },
    tint: 0x5a3a1a,
  },
  [UnitKind.Knight]: {
    label: 'Knight',
    icon: '🐎',
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
    cost: { wood: 90 },
    tint: 0x9a8050,
    requires: BuildingKind.Stable, // moved out of the Barracks into the Stable
  },
  [UnitKind.HorseArcher]: {
    label: 'Horse Archer',
    icon: '🏇',
    speed: 4.0, // fastest on the field — kites everything
    carry: 0,
    radius: 0.28,
    height: 0.95,
    maxHp: 60, // fragile for cavalry
    attack: 8,
    damageType: DamageType.Pierce,
    armorClass: ArmorClass.Leather,
    range: 4.5,
    attackRate: 1.3,
    aggroRange: 8,
    cost: { wood: 40, gold: 20 },
    tint: 0x7a5a2a,
    requires: BuildingKind.Stable,
  },
  [UnitKind.Mamluk]: {
    label: 'Mamluk',
    icon: '🗡️',
    speed: 3.6,
    carry: 0,
    radius: 0.31,
    height: 1.05,
    maxHp: 150, // elite — tougher than a knight
    attack: 19,
    damageType: DamageType.Slash, // strong vs leather/unarmored
    armorClass: ArmorClass.Mail,
    bonusVsArmor: { [ArmorClass.Leather]: 1.4 }, // hunts archers/spearmen
    range: 1.0,
    attackRate: 1.0,
    aggroRange: 7,
    cost: { food: 60, gold: 50 },
    tint: 0xc9a24a,
    requires: BuildingKind.Stable,
  },
  [UnitKind.Crossbowman]: {
    label: 'Crossbowman',
    icon: '🎯',
    speed: 2.0, // slow, heavy
    carry: 0,
    radius: 0.25,
    height: 0.82,
    maxHp: 55,
    attack: 14,
    damageType: DamageType.Pierce,
    armorClass: ArmorClass.Leather,
    bonusVsArmor: { [ArmorClass.Mail]: 2.2 }, // bolts punch mail — counters knights at range
    range: 5.5,
    attackRate: 2.0, // slow reload
    aggroRange: 7,
    cost: { wood: 40, gold: 20 },
    tint: 0x4a3a2a,
  },
  [UnitKind.Ram]: {
    label: 'Battering Ram',
    icon: '🪵',
    speed: 1.2, // crawls
    carry: 0,
    radius: 0.5,
    height: 1.1,
    maxHp: 400, // very high hp — soaks fire while it works the gate
    attack: 40,
    damageType: DamageType.Siege,
    armorClass: ArmorClass.Mail, // timber roof shrugs arrows, melee chips it
    range: 1.5,
    attackRate: 2.4,
    aggroRange: 0, // never auto-aggro — driven onto targets
    cost: { wood: 120 },
    tint: 0x6b4a2b,
    requires: BuildingKind.SiegeWorkshop,
    prefersBuildings: true,
  },
  [UnitKind.Mangonel]: {
    label: 'Mangonel',
    icon: '💥',
    speed: 1.0,
    carry: 0,
    radius: 0.45,
    height: 1.0,
    maxHp: 90, // fragile — must be screened
    attack: 30,
    damageType: DamageType.Siege,
    armorClass: ArmorClass.Unarmored,
    range: 8, // long siege reach
    attackRate: 3.0,
    aggroRange: 9,
    cost: { wood: 100, gold: 60 },
    tint: 0x5a4632,
    requires: BuildingKind.SiegeWorkshop,
    prefersBuildings: true,
  },
};
