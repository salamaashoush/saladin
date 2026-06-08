import { describe, it, expect } from 'vitest';
import {
  hasPrereq,
  BUILDING_DEFS,
  UNIT_DEFS,
  BuildingKind,
  UnitKind,
  type ResourceCost,
} from './index.ts';

const owned = (...kinds: BuildingKind[]) => new Set<BuildingKind>(kinds);

describe('hasPrereq (tech-tree gate)', () => {
  it('lets through any def with no requires', () => {
    expect(hasPrereq(owned(), { requires: undefined })).toBe(true);
    expect(hasPrereq(owned(), BUILDING_DEFS[BuildingKind.House])).toBe(true);
    expect(hasPrereq(owned(), BUILDING_DEFS[BuildingKind.Barracks])).toBe(true);
  });

  it('gates the Stable behind a Barracks', () => {
    const stable = BUILDING_DEFS[BuildingKind.Stable];
    expect(stable.requires).toBe(BuildingKind.Barracks);
    expect(hasPrereq(owned(BuildingKind.Keep), stable)).toBe(false);
    expect(hasPrereq(owned(BuildingKind.Barracks), stable)).toBe(true);
  });

  it('gates the Blacksmith behind a Barracks', () => {
    const bs = BUILDING_DEFS[BuildingKind.Blacksmith];
    expect(bs.requires).toBe(BuildingKind.Barracks);
    expect(hasPrereq(owned(), bs)).toBe(false);
    expect(hasPrereq(owned(BuildingKind.Barracks), bs)).toBe(true);
  });

  it('gates the Siege Workshop behind a Blacksmith (two tiers deep)', () => {
    const sw = BUILDING_DEFS[BuildingKind.SiegeWorkshop];
    expect(sw.requires).toBe(BuildingKind.Blacksmith);
    expect(hasPrereq(owned(BuildingKind.Barracks), sw)).toBe(false);
    expect(hasPrereq(owned(BuildingKind.Barracks, BuildingKind.Blacksmith), sw)).toBe(true);
  });

  it('gates Market / Granary / FishingHut behind the Keep', () => {
    for (const k of [BuildingKind.Market, BuildingKind.Granary, BuildingKind.FishingHut]) {
      const def = BUILDING_DEFS[k];
      expect(def.requires).toBe(BuildingKind.Keep);
      expect(hasPrereq(owned(), def)).toBe(false);
      expect(hasPrereq(owned(BuildingKind.Keep), def)).toBe(true);
    }
  });

  it('gates cavalry behind the Stable and siege behind the Siege Workshop', () => {
    for (const k of [UnitKind.Knight, UnitKind.HorseArcher, UnitKind.Mamluk])
      expect(UNIT_DEFS[k].requires).toBe(BuildingKind.Stable);
    for (const k of [UnitKind.Ram, UnitKind.Mangonel])
      expect(UNIT_DEFS[k].requires).toBe(BuildingKind.SiegeWorkshop);

    const mamluk = UNIT_DEFS[UnitKind.Mamluk];
    expect(hasPrereq(owned(BuildingKind.Barracks), mamluk)).toBe(false);
    expect(hasPrereq(owned(BuildingKind.Stable), mamluk)).toBe(true);
  });

  it('places the building tech tree at the expected depths from the Keep', () => {
    // Depth 0: Keep. Depth 1: Barracks/Market/Granary/FishingHut (require Keep
    // or nothing). Depth 2: Stable/Blacksmith (require Barracks). Depth 3:
    // SiegeWorkshop (requires Blacksmith).
    const edges: Array<[BuildingKind, BuildingKind | undefined]> = [
      [BuildingKind.Barracks, undefined],
      [BuildingKind.Stable, BuildingKind.Barracks],
      [BuildingKind.Blacksmith, BuildingKind.Barracks],
      [BuildingKind.SiegeWorkshop, BuildingKind.Blacksmith],
      [BuildingKind.Market, BuildingKind.Keep],
      [BuildingKind.Granary, BuildingKind.Keep],
      [BuildingKind.FishingHut, BuildingKind.Keep],
    ];
    for (const [kind, requires] of edges)
      expect(BUILDING_DEFS[kind].requires).toBe(requires);
  });
});

const isValidCost = (c: ResourceCost): boolean => {
  const keys: Array<keyof ResourceCost> = ['wood', 'stone', 'food', 'gold'];
  if (Object.keys(c).some((k) => !keys.includes(k as keyof ResourceCost)))
    return false;
  // every named amount is a non-negative integer, and a buildable thing costs
  // SOMETHING (at least one positive resource).
  let total = 0;
  for (const k of keys) {
    const v = c[k];
    if (v === undefined) continue;
    if (!Number.isInteger(v) || v < 0) return false;
    total += v;
  }
  return total > 0;
};

describe('every new def has a valid ResourceCost', () => {
  const newUnits = [
    UnitKind.HorseArcher,
    UnitKind.Mamluk,
    UnitKind.Crossbowman,
    UnitKind.Ram,
    UnitKind.Mangonel,
  ];
  const newBuildings = [
    BuildingKind.Stable,
    BuildingKind.Blacksmith,
    BuildingKind.Market,
    BuildingKind.Granary,
    BuildingKind.FishingHut,
    BuildingKind.SiegeWorkshop,
  ];

  it.each(newUnits)('unit %i costs a valid multi-resource amount', (k) => {
    expect(isValidCost(UNIT_DEFS[k].cost)).toBe(true);
  });

  it.each(newBuildings)('building %i costs a valid multi-resource amount', (k) => {
    expect(isValidCost(BUILDING_DEFS[k].cost)).toBe(true);
  });

  it('all units and buildings (existing + new) have well-formed costs', () => {
    for (const def of Object.values(UNIT_DEFS)) expect(isValidCost(def.cost)).toBe(true);
    for (const def of Object.values(BUILDING_DEFS)) {
      // The Keep is free (cost { wood: 0 }) — it is never built by a player, so
      // it is the one allowed exception to "costs something".
      if (def === BUILDING_DEFS[BuildingKind.Keep]) continue;
      expect(isValidCost(def.cost)).toBe(true);
    }
  });
});
