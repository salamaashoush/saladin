// Numeric enums stored as u8 columns in the module, shared with the client.

export const UnitKind = { Peasant: 0, Spearman: 1, Archer: 2 } as const;
export type UnitKind = (typeof UnitKind)[keyof typeof UnitKind];

export const BuildingKind = {
  Keep: 0,
  Barracks: 1,
  Tower: 2,
  Wall: 3,
  Gatehouse: 4,
  House: 5,
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
