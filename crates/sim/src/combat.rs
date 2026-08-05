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

/// Damage one hit deals to a unit, after its armour class AND the flat
/// reduction its armour research bought. Never below 1.
pub fn effective_damage_vs(atk: &Attacker, target: &crate::units::UnitDef) -> i32 {
    (effective_damage(atk, target.armor_class) - target.damage_reduction).max(1)
}

/// What a siege engine does to each STRUCTURAL material. This is a
/// building-only column: `DAMAGE_MATRIX` cannot carry it, because its Leather
/// and Mail cells are shared with archers and knights, and a ram that burned
/// timber halls properly through that table would also be anti-infantry.
/// Thatch and timber burn; masonry has to be broken.
pub const SIEGE_VS_STRUCTURE: [Fx; 4] = [
    crate::fx!("5"),    // Unarmored — thatch and standing crop
    crate::fx!("3"),    // Leather   — timber halls
    crate::fx!("2.75"), // Mail      — reinforced
    crate::fx!("2.5"),  // Stone     — masonry
];

/// Damage one hit deals to a STRUCTURE. `siege_resist` is HARDENING ONLY
/// (0 < r <= 1) — it used to do two opposite jobs at once, so a stone Wall was
/// the softest thing on the map at 10 ram hits while a Barracks took 13, and a
/// Stone-classed Blacksmith at `siege_resist: 2.0` fell in three. Timber
/// softness now lives in the material column alone.
pub fn building_damage(atk: &Attacker, def: &crate::buildings_defs::BuildingDef) -> i32 {
    if atk.damage_type != DamageType::Siege {
        return effective_damage(atk, def.armor_class);
    }
    let material = SIEGE_VS_STRUCTURE[def.armor_class as usize];
    let bonus = atk.bonus_vs_armor[def.armor_class as usize];
    (atk.attack * material * bonus * def.siege_resist).floor().to_num::<i32>().max(1)
}

/// The multiplier a charging rider actually lands. A braced spear taken
/// FRONTALLY cancels it outright — that is what makes the charge a position
/// problem rather than a damage stat.
pub fn charge_multiplier(charge_mult: Fx, target_braced: bool, frontal: bool) -> Fx {
    if target_braced && frontal { Fx::ONE } else { charge_mult.max(Fx::ONE) }
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
    fn siege_resist_hardens_walls_without_touching_units() {
        use crate::buildings_defs::building_def;
        use crate::enums::BuildingKind;
        let ram = Attacker::new(crate::fx!("40"), DamageType::Siege);
        // 40 * 2.5 vs stone = 100, then the wall's own resistance
        let wall = building_def(BuildingKind::Wall);
        assert_eq!(
            building_damage(&ram, wall),
            (crate::fx!("100") * wall.siege_resist).floor().to_num::<i32>()
        );
        // a spearman's pierce is untouched by any of it
        let spear = Attacker::new(crate::fx!("12"), DamageType::Pierce);
        assert_eq!(building_damage(&spear, wall), effective_damage(&spear, wall.armor_class));
        // and never below one
        let pebble = Attacker::new(crate::fx!("1"), DamageType::Siege);
        assert!(building_damage(&pebble, wall) >= 1);
    }

    /// The shipped inversion: `siege_resist` above 1 SOFTENED, so the Stone
    /// column of a shared unit matrix decided how a hall burned. Hardening only,
    /// materials in their own column.
    #[test]
    fn hardening_only_and_the_material_column_carries_the_softness() {
        use crate::buildings_defs::building_def;
        use crate::enums::BuildingKind;
        for &k in BuildingKind::ALL {
            let r = building_def(k).siege_resist;
            assert!(r > Fx::ZERO && r <= Fx::ONE, "{k:?} siege_resist {r} is not hardening");
        }
        // thatch burns fastest, masonry burns slowest, per point of attack
        let mut last = Fx::MAX;
        for ac in [ArmorClass::Unarmored, ArmorClass::Leather, ArmorClass::Mail, ArmorClass::Stone] {
            let m = SIEGE_VS_STRUCTURE[ac as usize];
            assert!(m < last, "{ac:?} is no softer than the material above it");
            last = m;
        }
    }

    #[test]
    fn a_braced_spear_cancels_a_charge_only_from_the_front() {
        let knight = crate::fx!("3");
        assert_eq!(charge_multiplier(knight, true, true), Fx::ONE);
        assert_eq!(charge_multiplier(knight, true, false), knight, "the flank is open");
        assert_eq!(charge_multiplier(knight, false, true), knight, "unset spears are trampled");
        // a unit with no charge never gets one from the geometry
        assert_eq!(charge_multiplier(Fx::ONE, false, false), Fx::ONE);
    }

    #[test]
    fn armour_research_is_flat_reduction_not_a_class_promotion() {
        use crate::enums::UnitKind;
        use crate::research::{Tech, effective_unit_def, set_tech};
        let mail = set_tech(0, Tech::ArmorMail);
        let plain = *crate::units::unit_def(UnitKind::Archer);
        let armoured = effective_unit_def(UnitKind::Archer, mail);
        assert_eq!(armoured.armor_class, plain.armor_class, "the Leather column must survive");
        assert!(armoured.damage_reduction > plain.damage_reduction);
        let a = Attacker::new(crate::fx!("9"), DamageType::Pierce);
        assert!(effective_damage_vs(&a, &armoured) < effective_damage_vs(&a, &plain));
        // and never to nothing
        let pebble = Attacker::new(crate::fx!("1"), DamageType::Slash);
        assert_eq!(effective_damage_vs(&pebble, &armoured), 1);
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
