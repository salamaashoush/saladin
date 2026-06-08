import { describe, it, expect } from 'vitest';
import {
  effectiveDamage,
  combatAction,
  DAMAGE_MATRIX,
  UNIT_DEFS,
  UnitKind,
  DamageType,
  ArmorClass,
  Stance,
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
