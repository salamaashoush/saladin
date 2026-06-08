import { describe, it, expect } from 'vitest';
import {
  canGarrison,
  canHostGarrison,
  garrisonFreeSlots,
  garrisonFirePower,
  type GarrisonOccupant,
} from './garrison.ts';
import { UNIT_DEFS, BUILDING_DEFS } from './defs.ts';
import { UnitKind, BuildingKind } from './enums.ts';

const tower = BUILDING_DEFS[BuildingKind.Tower];
const wall = BUILDING_DEFS[BuildingKind.Wall];
const keep = BUILDING_DEFS[BuildingKind.Keep];
const stable = BUILDING_DEFS[BuildingKind.Stable]; // no garrisonCap

describe('canGarrison (data-driven roster rule)', () => {
  it('accepts foot soldiers and missile troops', () => {
    expect(canGarrison(UNIT_DEFS[UnitKind.Archer])).toBe(true);
    expect(canGarrison(UNIT_DEFS[UnitKind.Crossbowman])).toBe(true);
    expect(canGarrison(UNIT_DEFS[UnitKind.Spearman])).toBe(true);
    expect(canGarrison(UNIT_DEFS[UnitKind.Imam])).toBe(true); // protected, no fire
  });

  it('rejects cavalry — mounted units cannot man a parapet', () => {
    expect(canGarrison(UNIT_DEFS[UnitKind.Knight])).toBe(false);
    expect(canGarrison(UNIT_DEFS[UnitKind.HorseArcher])).toBe(false);
    expect(canGarrison(UNIT_DEFS[UnitKind.Mamluk])).toBe(false);
  });

  it('rejects siege engines', () => {
    expect(canGarrison(UNIT_DEFS[UnitKind.Ram])).toBe(false);
    expect(canGarrison(UNIT_DEFS[UnitKind.Mangonel])).toBe(false);
  });

  it('rejects an undefined unit', () => {
    expect(canGarrison(undefined)).toBe(false);
  });
});

describe('garrison capacity', () => {
  it('towers, the keep and walls advertise a positive capacity', () => {
    expect(canHostGarrison(tower)).toBe(true);
    expect(canHostGarrison(keep)).toBe(true);
    expect(canHostGarrison(wall)).toBe(true);
    expect(tower.garrisonCap!).toBeGreaterThan(0);
    expect(keep.garrisonCap!).toBeGreaterThan(wall.garrisonCap!); // keep holds more than a wall stretch
  });

  it('non-defensive structures cannot host', () => {
    expect(canHostGarrison(stable)).toBe(false);
    expect(canHostGarrison(undefined)).toBe(false);
  });

  it('free slots shrink as occupants fill and clamp at zero', () => {
    expect(garrisonFreeSlots(tower, 0)).toBe(tower.garrisonCap);
    expect(garrisonFreeSlots(tower, 2)).toBe(tower.garrisonCap! - 2);
    expect(garrisonFreeSlots(tower, tower.garrisonCap!)).toBe(0);
    expect(garrisonFreeSlots(tower, tower.garrisonCap! + 5)).toBe(0); // never negative
    expect(garrisonFreeSlots(undefined, 0)).toBe(0);
  });
});

describe('garrisonFirePower', () => {
  const archer = (): GarrisonOccupant => ({
    attack: UNIT_DEFS[UnitKind.Archer].attack,
    ranged: true,
  });
  const spear = (): GarrisonOccupant => ({
    attack: UNIT_DEFS[UnitKind.Spearman].attack,
    ranged: false,
  });

  it('sums the attack of ranged occupants', () => {
    const power = garrisonFirePower([archer(), archer(), archer()], tower);
    expect(power).toBe(UNIT_DEFS[UnitKind.Archer].attack * 3);
  });

  it('ignores non-shooters (protected but lend no fire)', () => {
    const power = garrisonFirePower([archer(), spear(), spear()], tower);
    expect(power).toBe(UNIT_DEFS[UnitKind.Archer].attack);
  });

  it('counts only up to the host capacity (limited firing slits)', () => {
    const many = Array.from({ length: tower.garrisonCap! + 4 }, archer);
    const power = garrisonFirePower(many, tower);
    expect(power).toBe(UNIT_DEFS[UnitKind.Archer].attack * tower.garrisonCap!);
  });

  it('a host with no capacity contributes no fire', () => {
    expect(garrisonFirePower([archer(), archer()], stable)).toBe(0);
    expect(garrisonFirePower([archer()], undefined)).toBe(0);
  });

  it('an empty garrison contributes no fire', () => {
    expect(garrisonFirePower([], tower)).toBe(0);
  });

  it('mixes ranged and melee correctly under the cap', () => {
    // cap=2 wall, fill with 3 archers + 2 spears: only 2 archers fire.
    const power = garrisonFirePower(
      [archer(), spear(), archer(), archer(), spear()],
      wall
    );
    expect(power).toBe(UNIT_DEFS[UnitKind.Archer].attack * wall.garrisonCap!);
  });
});
