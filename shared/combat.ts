// Pure combat math — the damage-type × armor matrix and effective-damage rule.
// Shared by the module (authority) and tests. No SpacetimeDB/Three deps.
import { DamageType, ArmorClass, Stance } from './enums.ts';

// How far a Defensive unit will wander from its posted position before it breaks
// off and returns instead of chasing.
export const DEFENSIVE_LEASH = 7;

export type CombatAct = 'attack' | 'approach' | 'return' | 'hold';

// Decide what an out-of-range (or in-range) combatant does, given its stance and
// how far it has drifted from home. Pure so the posture rules are unit-testable.
export function combatAction(
  stance: Stance,
  inRange: boolean,
  distFromHome: number,
  leash = DEFENSIVE_LEASH
): CombatAct {
  if (inRange) return 'attack';
  if (stance === Stance.HoldGround) return 'hold';
  if (stance === Stance.Defensive && distFromHome >= leash) return 'return';
  return 'approach';
}

// Multiplier applied to base attack for each (damageType, armorClass) pair.
// Slash chews soft targets but glances off mail/stone; pierce punches leather
// but is blunted by mail; blunt ignores mail; siege is the only thing that
// truly cracks stone. Rows = DamageType, columns = ArmorClass.
export const DAMAGE_MATRIX: Record<DamageType, Record<ArmorClass, number>> = {
  [DamageType.Slash]: {
    [ArmorClass.Unarmored]: 1.25,
    [ArmorClass.Leather]: 1.0,
    [ArmorClass.Mail]: 0.6,
    [ArmorClass.Stone]: 0.25,
  },
  [DamageType.Pierce]: {
    [ArmorClass.Unarmored]: 1.0,
    [ArmorClass.Leather]: 1.15,
    [ArmorClass.Mail]: 0.55,
    [ArmorClass.Stone]: 0.2,
  },
  [DamageType.Blunt]: {
    [ArmorClass.Unarmored]: 0.9,
    [ArmorClass.Leather]: 1.0,
    [ArmorClass.Mail]: 1.25,
    [ArmorClass.Stone]: 0.5,
  },
  [DamageType.Siege]: {
    [ArmorClass.Unarmored]: 0.4,
    [ArmorClass.Leather]: 0.5,
    [ArmorClass.Mail]: 0.7,
    [ArmorClass.Stone]: 2.5,
  },
};

export interface Attacker {
  attack: number;
  damageType: DamageType;
  // Specialist multiplier vs specific armor (e.g. spearman braced vs mailed
  // cavalry). Stacks on top of the matrix.
  bonusVsArmor?: Partial<Record<ArmorClass, number>>;
}

// Damage one hit from `atk` deals to a target of `armor`, rounded down so hp
// stays integer and the math is deterministic across module and client.
export function effectiveDamage(atk: Attacker, armor: ArmorClass): number {
  const base = atk.attack * DAMAGE_MATRIX[atk.damageType][armor];
  const bonus = atk.bonusVsArmor?.[armor] ?? 1;
  return Math.max(1, Math.floor(base * bonus));
}
