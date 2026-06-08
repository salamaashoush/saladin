// Numeric enums stored as u8 columns in the module, shared with the client.

export const UnitKind = {
  Peasant: 0,
  Spearman: 1,
  Archer: 2,
  Knight: 3,
  HorseArcher: 4,
  Mamluk: 5,
  Crossbowman: 6,
  Ram: 7,
  Mangonel: 8,
  Imam: 9,
} as const;
export type UnitKind = (typeof UnitKind)[keyof typeof UnitKind];

// How an attack interacts with armor. The damage matrix turns these into the
// rock-paper-scissors of the battlefield.
export const DamageType = { Slash: 0, Pierce: 1, Blunt: 2, Siege: 3 } as const;
export type DamageType = (typeof DamageType)[keyof typeof DamageType];

export const ArmorClass = { Unarmored: 0, Leather: 1, Mail: 2, Stone: 3 } as const;
export type ArmorClass = (typeof ArmorClass)[keyof typeof ArmorClass];

export const BuildingKind = {
  Keep: 0,
  Barracks: 1,
  Tower: 2,
  Wall: 3,
  Gatehouse: 4,
  House: 5,
  Stable: 6,
  Blacksmith: 7,
  Market: 8,
  Granary: 9,
  FishingHut: 10,
  SiegeWorkshop: 11,
  Watchtower: 12,
} as const;
export type BuildingKind = (typeof BuildingKind)[keyof typeof BuildingKind];

export const ResourceType = { Wood: 0, Stone: 1, Food: 2, Gold: 3 } as const;
export type ResourceType = (typeof ResourceType)[keyof typeof ResourceType];

export const GatherState = {
  Idle: 0,
  ToResource: 1,
  Harvesting: 2,
  ToStockpile: 3,
} as const;
export type GatherState = (typeof GatherState)[keyof typeof GatherState];

export const Faction = { Ayyubid: 0, Crusader: 1 } as const;
export type Faction = (typeof Faction)[keyof typeof Faction];

// Combat posture. Aggressive: hunt within aggro range. Defensive: engage nearby
// foes but never stray far from where you were posted. HoldGround: never move to
// fight — strike only what comes within reach.
export const Stance = { Aggressive: 0, Defensive: 1, HoldGround: 2 } as const;
export type Stance = (typeof Stance)[keyof typeof Stance];
