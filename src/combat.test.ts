import { describe, it, expect } from 'vitest';
import {
  effectiveDamage,
  combatAction,
  acquireTarget,
  DAMAGE_MATRIX,
  UNIT_DEFS,
  UnitKind,
  DamageType,
  ArmorClass,
  Stance,
  type Located,
} from '../shared/index.ts';

describe('combatAction (stances)', () => {
  it('always attacks a target in range, whatever the stance', () => {
    for (const s of [Stance.Aggressive, Stance.Defensive, Stance.HoldGround])
      expect(combatAction(s, true, 999, 7)).toBe('attack');
  });

  it('aggressive chases regardless of distance from home', () => {
    expect(combatAction(Stance.Aggressive, false, 50, 7)).toBe('approach');
  });

  it('defensive chases within leash but returns once pulled past it', () => {
    expect(combatAction(Stance.Defensive, false, 3, 7)).toBe('approach');
    expect(combatAction(Stance.Defensive, false, 7, 7)).toBe('return');
  });

  it('hold ground never moves to engage', () => {
    expect(combatAction(Stance.HoldGround, false, 0, 7)).toBe('hold');
  });
});

describe('effectiveDamage', () => {
  it('applies the matrix multiplier and floors to an integer', () => {
    // pierce vs leather = 1.15 -> 10 * 1.15 = 11.5 -> 11
    const d = effectiveDamage(
      { attack: 10, damageType: DamageType.Pierce },
      ArmorClass.Leather
    );
    expect(d).toBe(11);
  });

  it('applies a specialist bonusVsArmor on top of the matrix', () => {
    // 12 * matrix[Pierce][Mail](0.55) * 2.6 = 17.16 -> 17
    const spear = {
      attack: 12,
      damageType: DamageType.Pierce,
      bonusVsArmor: { [ArmorClass.Mail]: 2.6 },
    };
    expect(effectiveDamage(spear, ArmorClass.Mail)).toBe(17);
  });

  it('never deals less than 1 (no fully immune target)', () => {
    expect(
      effectiveDamage({ attack: 1, damageType: DamageType.Slash }, ArmorClass.Stone)
    ).toBeGreaterThanOrEqual(1);
  });
});

describe('counter triangle (per-hit damage)', () => {
  const vsMail = (k: number) =>
    effectiveDamage(UNIT_DEFS[k as 0], ArmorClass.Mail);

  it('spearmen hard-counter mailed knights far better than archers do', () => {
    expect(vsMail(UnitKind.Spearman)).toBeGreaterThan(
      vsMail(UnitKind.Archer) * 2
    );
  });

  it('knights cut down leather-clad archers harder than they dent other knights', () => {
    const knight = UNIT_DEFS[UnitKind.Knight];
    expect(effectiveDamage(knight, ArmorClass.Leather)).toBeGreaterThan(
      effectiveDamage(knight, ArmorClass.Mail)
    );
  });
});

describe('expanded roster counters (per-hit damage)', () => {
  const vs = (k: number, armor: ArmorClass) =>
    effectiveDamage(UNIT_DEFS[k as 0], armor);

  it('crossbowmen punch mail far harder than archers — they counter knights at range', () => {
    expect(vs(UnitKind.Crossbowman, ArmorClass.Mail)).toBeGreaterThan(
      vs(UnitKind.Archer, ArmorClass.Mail) * 2
    );
  });

  it('mamluks butcher leather-clad troops harder than they dent mail', () => {
    expect(vs(UnitKind.Mamluk, ArmorClass.Leather)).toBeGreaterThan(
      vs(UnitKind.Mamluk, ArmorClass.Mail)
    );
    // and harder than a knight hits the same leather target
    expect(vs(UnitKind.Mamluk, ArmorClass.Leather)).toBeGreaterThan(
      vs(UnitKind.Knight, ArmorClass.Leather)
    );
  });

  it('rams and mangonels crack stone walls where melee barely scratches them', () => {
    for (const siege of [UnitKind.Ram, UnitKind.Mangonel]) {
      expect(vs(siege, ArmorClass.Stone)).toBeGreaterThan(
        vs(UnitKind.Knight, ArmorClass.Stone) * 4
      );
      // siege is poor against soft field troops — it's a building-breaker
      expect(vs(siege, ArmorClass.Stone)).toBeGreaterThan(
        vs(siege, ArmorClass.Leather)
      );
    }
  });

  it('siege units are flagged to prefer buildings', () => {
    expect(UNIT_DEFS[UnitKind.Ram].prefersBuildings).toBe(true);
    expect(UNIT_DEFS[UnitKind.Mangonel].prefersBuildings).toBe(true);
  });
});

describe('acquireTarget (siege building priority)', () => {
  const unit: Located = { id: 1n, x: 1, y: 0 };
  const bld: Located = { id: 2n, x: -1, y: 0 }; // equidistant from origin

  it('a prefersBuildings unit picks a building over an equidistant unit', () => {
    const t = acquireTarget(0, 0, 9, [unit], [bld], true);
    expect(t?.id).toBe(bld.id);
  });

  it('a non-siege unit picks the unit and ignores buildings', () => {
    const t = acquireTarget(0, 0, 9, [unit], [bld], false);
    expect(t?.id).toBe(unit.id);
  });

  it('siege falls back to units when no building is in aggro range', () => {
    const farBld: Located = { id: 3n, x: 50, y: 0 };
    const t = acquireTarget(0, 0, 9, [unit], [farBld], true);
    expect(t?.id).toBe(unit.id);
  });

  it('siege still prefers a building even when a unit is strictly closer', () => {
    const closeUnit: Located = { id: 4n, x: 0.5, y: 0 };
    const farishBld: Located = { id: 5n, x: 4, y: 0 };
    const t = acquireTarget(0, 0, 9, [closeUnit], [farishBld], true);
    expect(t?.id).toBe(farishBld.id);
  });

  it('returns null when nothing is in range or aggro is disabled', () => {
    expect(acquireTarget(0, 0, 0, [unit], [bld], true)).toBeNull();
    const farUnit: Located = { id: 6n, x: 100, y: 0 };
    expect(acquireTarget(0, 0, 5, [farUnit], [], false)).toBeNull();
  });
});

describe('damage matrix shape', () => {
  it('only siege meaningfully cracks stone', () => {
    expect(DAMAGE_MATRIX[DamageType.Siege][ArmorClass.Stone]).toBeGreaterThan(
      DAMAGE_MATRIX[DamageType.Slash][ArmorClass.Stone] * 4
    );
  });

  it('pierce is blunted by mail relative to leather', () => {
    expect(DAMAGE_MATRIX[DamageType.Pierce][ArmorClass.Mail]).toBeLessThan(
      DAMAGE_MATRIX[DamageType.Pierce][ArmorClass.Leather]
    );
  });
});
