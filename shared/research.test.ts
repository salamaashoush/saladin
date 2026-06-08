import { describe, it, expect } from 'vitest';
import {
  Tech,
  ALL_TECHS,
  UPGRADE_DEFS,
  effectiveUnitDef,
  effectiveBuildingDef,
  techBit,
  hasTech,
  setTech,
  techsInMask,
  UNIT_DEFS,
  BUILDING_DEFS,
  UnitKind,
  BuildingKind,
  ArmorClass,
  type ResourceCost,
} from './index.ts';

// Build a mask from a list of techs — exercises setTech as a fold.
const mask = (...techs: number[]): bigint =>
  techs.reduce((m, t) => setTech(m, t as Tech), 0n);

describe('techMask bit math', () => {
  it('techBit is a distinct power of two per tech', () => {
    const bits = ALL_TECHS.map(techBit);
    // all distinct
    expect(new Set(bits.map(String)).size).toBe(bits.length);
    // each is a single set bit (power of two)
    for (const b of bits) expect(b & (b - 1n)).toBe(0n);
  });

  it('bit index equals the Tech value', () => {
    for (const t of ALL_TECHS) expect(techBit(t)).toBe(1n << BigInt(t));
  });

  it('setTech then hasTech round-trips; unset techs read false', () => {
    let m = 0n;
    expect(hasTech(m, Tech.ArmorMail)).toBe(false);
    m = setTech(m, Tech.ArmorMail);
    expect(hasTech(m, Tech.ArmorMail)).toBe(true);
    expect(hasTech(m, Tech.SharpenedBlades)).toBe(false);
  });

  it('setTech is idempotent — re-setting a bit leaves the mask unchanged', () => {
    const once = setTech(0n, Tech.Conscription);
    expect(setTech(once, Tech.Conscription)).toBe(once);
  });

  it('techsInMask returns completed techs in canonical ascending order', () => {
    const m = mask(Tech.Masonry, Tech.ArmorMail, Tech.FletchedArrows);
    expect(techsInMask(m)).toEqual([
      Tech.ArmorMail,
      Tech.FletchedArrows,
      Tech.Masonry,
    ]);
  });

  it('an empty mask has no techs', () => {
    expect(techsInMask(0n)).toEqual([]);
  });
});

describe('effectiveUnitDef — additive composition', () => {
  it('returns the base def unchanged on an empty mask (same reference)', () => {
    for (const k of Object.values(UnitKind)) {
      expect(effectiveUnitDef(k, 0n)).toBe(UNIT_DEFS[k as 0]);
    }
  });

  it('does not mutate the base UNIT_DEFS entry', () => {
    const baseAttack = UNIT_DEFS[UnitKind.Spearman].attack;
    effectiveUnitDef(UnitKind.Spearman, mask(Tech.SharpenedBlades));
    expect(UNIT_DEFS[UnitKind.Spearman].attack).toBe(baseAttack);
  });

  it('SharpenedBlades adds attack to melee units only', () => {
    const d = UPGRADE_DEFS[Tech.SharpenedBlades].delta.attack ?? 0;
    expect(d).toBeGreaterThan(0);
    const spear = effectiveUnitDef(UnitKind.Spearman, mask(Tech.SharpenedBlades));
    expect(spear.attack).toBe(UNIT_DEFS[UnitKind.Spearman].attack + d);
    // an archer is ranged — sharpened blades must NOT touch it
    const archer = effectiveUnitDef(UnitKind.Archer, mask(Tech.SharpenedBlades));
    expect(archer.attack).toBe(UNIT_DEFS[UnitKind.Archer].attack);
  });

  it('FletchedArrows adds attack to ranged units only', () => {
    const d = UPGRADE_DEFS[Tech.FletchedArrows].delta.attack ?? 0;
    const archer = effectiveUnitDef(UnitKind.Archer, mask(Tech.FletchedArrows));
    expect(archer.attack).toBe(UNIT_DEFS[UnitKind.Archer].attack + d);
    // a knight is melee — fletched arrows must NOT touch it
    const knight = effectiveUnitDef(UnitKind.Knight, mask(Tech.FletchedArrows));
    expect(knight.attack).toBe(UNIT_DEFS[UnitKind.Knight].attack);
  });

  it('ArmorMail bumps a leather troop up one armor tier, capped at Mail', () => {
    const spear = effectiveUnitDef(UnitKind.Spearman, mask(Tech.ArmorMail));
    expect(UNIT_DEFS[UnitKind.Spearman].armorClass).toBe(ArmorClass.Leather);
    expect(spear.armorClass).toBe(ArmorClass.Mail);
    // a knight is already Mail — the tier is clamped, not pushed to Stone
    const knight = effectiveUnitDef(UnitKind.Knight, mask(Tech.ArmorMail));
    expect(knight.armorClass).toBe(ArmorClass.Mail);
  });

  it('Conscription adds hp to every combatant but not to non-combatants', () => {
    const d = UPGRADE_DEFS[Tech.Conscription].delta.maxHp ?? 0;
    const knight = effectiveUnitDef(UnitKind.Knight, mask(Tech.Conscription));
    expect(knight.maxHp).toBe(UNIT_DEFS[UnitKind.Knight].maxHp + d);
    // a peasant has attack 0 — never a combatant, gets no hp
    const peasant = effectiveUnitDef(UnitKind.Peasant, mask(Tech.Conscription));
    expect(peasant.maxHp).toBe(UNIT_DEFS[UnitKind.Peasant].maxHp);
    // an imam is a non-combatant support unit — also untouched
    const imam = effectiveUnitDef(UnitKind.Imam, mask(Tech.Conscription));
    expect(imam.maxHp).toBe(UNIT_DEFS[UnitKind.Imam].maxHp);
  });

  it('siege engines never gain troop armor from ArmorMail', () => {
    for (const k of [UnitKind.Ram, UnitKind.Mangonel]) {
      const d = effectiveUnitDef(k, mask(Tech.ArmorMail));
      expect(d.armorClass).toBe(UNIT_DEFS[k as 0].armorClass);
    }
  });

  it('Masonry has no effect on units (structures only)', () => {
    for (const k of Object.values(UnitKind)) {
      expect(effectiveUnitDef(k, mask(Tech.Masonry))).toEqual(UNIT_DEFS[k as 0]);
    }
  });

  it('a tech CHAIN folds additively and deterministically', () => {
    // Knight gets +hp from Plate AND Conscription, +attack from Sharpened Blades.
    const m = mask(Tech.ArmorPlate, Tech.Conscription, Tech.SharpenedBlades);
    const base = UNIT_DEFS[UnitKind.Knight];
    const plate = UPGRADE_DEFS[Tech.ArmorPlate].delta.maxHp ?? 0;
    const consc = UPGRADE_DEFS[Tech.Conscription].delta.maxHp ?? 0;
    const blade = UPGRADE_DEFS[Tech.SharpenedBlades].delta.attack ?? 0;
    const eff = effectiveUnitDef(UnitKind.Knight, m);
    expect(eff.maxHp).toBe(base.maxHp + plate + consc);
    expect(eff.attack).toBe(base.attack + blade);
    // order of folding never matters: a re-ordered mask yields the same def
    const m2 = mask(Tech.SharpenedBlades, Tech.Conscription, Tech.ArmorPlate);
    expect(effectiveUnitDef(UnitKind.Knight, m2)).toEqual(eff);
  });

  it('same input → same output (pure / deterministic)', () => {
    const m = mask(Tech.ArmorMail, Tech.FletchedArrows, Tech.Conscription);
    const a = effectiveUnitDef(UnitKind.Archer, m);
    const b = effectiveUnitDef(UnitKind.Archer, m);
    expect(a).toEqual(b);
  });
});

describe('effectiveBuildingDef — Masonry', () => {
  it('returns the base def unchanged on an empty mask', () => {
    for (const k of Object.values(BuildingKind))
      expect(effectiveBuildingDef(k, 0n)).toBe(BUILDING_DEFS[k as 0]);
  });

  it('Masonry adds structure hp and bumps armor toward Stone', () => {
    const d = UPGRADE_DEFS[Tech.Masonry].buildingDelta;
    expect(d).toBeDefined();
    const barracks = effectiveBuildingDef(BuildingKind.Barracks, mask(Tech.Masonry));
    expect(barracks.maxHp).toBe(
      BUILDING_DEFS[BuildingKind.Barracks].maxHp + (d?.maxHp ?? 0)
    );
    expect(barracks.armorClass).toBe(ArmorClass.Mail); // Leather + 1 tier
    // a Keep is already Stone — clamped, not overflowed
    const keep = effectiveBuildingDef(BuildingKind.Keep, mask(Tech.Masonry));
    expect(keep.armorClass).toBe(ArmorClass.Stone);
    expect(keep.maxHp).toBe(BUILDING_DEFS[BuildingKind.Keep].maxHp + (d?.maxHp ?? 0));
  });

  it('non-Masonry techs do not affect buildings', () => {
    for (const t of [Tech.ArmorMail, Tech.SharpenedBlades, Tech.Conscription]) {
      const b = effectiveBuildingDef(BuildingKind.Tower, mask(t));
      expect(b).toEqual(BUILDING_DEFS[BuildingKind.Tower]);
    }
  });
});

const isValidCost = (c: ResourceCost): boolean => {
  const keys: Array<keyof ResourceCost> = ['wood', 'stone', 'food', 'gold'];
  if (Object.keys(c).some((k) => !keys.includes(k as keyof ResourceCost)))
    return false;
  let total = 0;
  for (const k of keys) {
    const v = c[k];
    if (v === undefined) continue;
    if (!Number.isInteger(v) || v < 0) return false;
    total += v;
  }
  return total > 0;
};

describe('UPGRADE_DEFS data validity', () => {
  it('every tech has a defined upgrade with a valid ResourceCost', () => {
    for (const t of ALL_TECHS) {
      const up = UPGRADE_DEFS[t];
      expect(up).toBeDefined();
      expect(isValidCost(up.cost)).toBe(true);
    }
  });

  it('every tech has a positive research time', () => {
    for (const t of ALL_TECHS)
      expect(UPGRADE_DEFS[t].researchTime).toBeGreaterThan(0);
  });

  it('every tech carries a non-empty label and icon', () => {
    for (const t of ALL_TECHS) {
      expect(UPGRADE_DEFS[t].label.length).toBeGreaterThan(0);
      expect(UPGRADE_DEFS[t].icon.length).toBeGreaterThan(0);
    }
  });

  it('a unit-targeting tech actually applies to at least one unit', () => {
    for (const t of ALL_TECHS) {
      const up = UPGRADE_DEFS[t];
      if (up.appliesToBuildings) continue; // structural tech (Masonry)
      const anyUnit = Object.values(UnitKind).some((k) =>
        up.appliesTo(UNIT_DEFS[k as 0])
      );
      expect(anyUnit).toBe(true);
    }
  });

  it('ALL_TECHS covers exactly the keys of UPGRADE_DEFS with no duplicate bits', () => {
    expect(ALL_TECHS.length).toBe(Object.keys(UPGRADE_DEFS).length);
    expect(new Set(ALL_TECHS).size).toBe(ALL_TECHS.length);
  });
});
