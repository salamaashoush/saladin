use crate::enums::{ArmorClass, DamageType, Stance};
use crate::math::{Fx, Located, V2, nearest_within};

/// How far a Defensive unit drifts from its post before breaking off to return.
pub const DEFENSIVE_LEASH: Fx = crate::fx!("7");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatAct {
    Attack,
    Approach,
    Return,
    Hold,
}

/// What an (in- or out-of-range) combatant does given its stance and drift from
/// home. Pure — the posture rules are unit-testable.
pub fn combat_action(stance: Stance, in_range: bool, dist_from_home: Fx, leash: Fx) -> CombatAct {
    if in_range {
        return CombatAct::Attack;
    }
    match stance {
        Stance::HoldGround => CombatAct::Hold,
        Stance::Defensive if dist_from_home >= leash => CombatAct::Return,
        _ => CombatAct::Approach,
    }
}

/// Base attack multiplier per (DamageType row, ArmorClass column). Slash chews
/// soft targets but glances off mail/stone; pierce punches leather but is
/// blunted by mail; blunt ignores mail; siege is the only thing that cracks
/// stone.
pub const DAMAGE_MATRIX: [[Fx; 4]; 4] = [
    // Unarmored          Leather            Mail               Stone
    [crate::fx!("1.25"), crate::fx!("1.0"), crate::fx!("0.6"), crate::fx!("0.25")], // Slash
    [crate::fx!("1.0"), crate::fx!("1.15"), crate::fx!("0.55"), crate::fx!("0.2")], // Pierce
    [crate::fx!("0.9"), crate::fx!("1.0"), crate::fx!("1.25"), crate::fx!("0.5")],  // Blunt
    [crate::fx!("0.4"), crate::fx!("0.5"), crate::fx!("0.7"), crate::fx!("2.5")],   // Siege
];

#[derive(Clone, Copy, Debug)]
pub struct Attacker {
    pub attack: Fx,
    pub damage_type: DamageType,
    /// Specialist multiplier vs each armor class (1.0 == none). Stacks on the
    /// matrix — e.g. a spearman braced vs mailed cavalry.
    pub bonus_vs_armor: [Fx; 4],
}

impl Attacker {
    pub fn new(attack: Fx, damage_type: DamageType) -> Self {
        Attacker { attack, damage_type, bonus_vs_armor: [Fx::ONE; 4] }
    }
}

/// Damage one hit deals to a target of `armor`, floored so hp stays integer and
/// the result is deterministic. Always at least 1.
pub fn effective_damage(atk: &Attacker, armor: ArmorClass) -> i32 {
    let base = atk.attack * DAMAGE_MATRIX[atk.damage_type as usize][armor as usize];
    let bonus = atk.bonus_vs_armor[armor as usize];
    let dealt = (base * bonus).floor().to_num::<i32>();
    dealt.max(1)
}

/// Auto-acquisition target for a combatant at `pos` with `aggro_range`. Siege
/// engines (`prefers_buildings`) lock onto the nearest enemy building first,
/// falling back to units; everyone else picks the nearest enemy unit.
pub fn acquire_target(
    pos: V2,
    aggro_range: Fx,
    enemy_units: &[Located],
    enemy_buildings: &[Located],
    prefers_buildings: bool,
) -> Option<Located> {
    if aggro_range <= Fx::ZERO {
        return None;
    }
    if prefers_buildings {
        if let Some(b) = nearest_within(pos, enemy_buildings, aggro_range) {
            return Some(b);
        }
    }
    nearest_within(pos, enemy_units, aggro_range)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(id: u64, x: &str, y: &str) -> Located {
        Located { id, pos: V2::new(Fx::lit(x), Fx::lit(y)) }
    }

    #[test]
    fn slash_chews_soft_glances_stone() {
        let a = Attacker::new(crate::fx!("20"), DamageType::Slash);
        assert_eq!(effective_damage(&a, ArmorClass::Unarmored), 25); // 20 * 1.25
        assert_eq!(effective_damage(&a, ArmorClass::Stone), 5); // 20 * 0.25
    }

    #[test]
    fn siege_cracks_stone() {
        let a = Attacker::new(crate::fx!("30"), DamageType::Siege);
        assert_eq!(effective_damage(&a, ArmorClass::Stone), 75); // 30 * 2.5
    }

    #[test]
    fn damage_floors_to_at_least_one() {
        let a = Attacker::new(crate::fx!("1"), DamageType::Slash);
        assert_eq!(effective_damage(&a, ArmorClass::Stone), 1); // floor(0.25) -> 1
    }

    #[test]
    fn bonus_vs_armor_stacks() {
        let mut a = Attacker::new(crate::fx!("10"), DamageType::Pierce);
        a.bonus_vs_armor[ArmorClass::Mail as usize] = crate::fx!("3");
        // 10 * 0.55 * 3 = 16.5 -> 16
        assert_eq!(effective_damage(&a, ArmorClass::Mail), 16);
    }

    #[test]
    fn stance_postures() {
        let leash = DEFENSIVE_LEASH;
        assert_eq!(combat_action(Stance::Aggressive, true, Fx::ZERO, leash), CombatAct::Attack);
        assert_eq!(combat_action(Stance::HoldGround, false, Fx::ZERO, leash), CombatAct::Hold);
        assert_eq!(combat_action(Stance::Defensive, false, crate::fx!("8"), leash), CombatAct::Return);
        assert_eq!(combat_action(Stance::Defensive, false, crate::fx!("2"), leash), CombatAct::Approach);
        assert_eq!(combat_action(Stance::Aggressive, false, crate::fx!("99"), leash), CombatAct::Approach);
    }

    #[test]
    fn siege_prefers_buildings_then_units() {
        let units = [loc(1, "5", "0")];
        let buildings = [loc(2, "6", "0")];
        let pos = V2::ZERO;
        let r = crate::fx!("10");
        assert_eq!(acquire_target(pos, r, &units, &buildings, true).unwrap().id, 2);
        assert_eq!(acquire_target(pos, r, &units, &buildings, false).unwrap().id, 1);
        // out of range -> none
        assert!(acquire_target(pos, crate::fx!("1"), &units, &buildings, true).is_none());
    }
}
