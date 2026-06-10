use crate::economy::ResourceCost;
use crate::enums::{ArmorClass, BuildingKind, DamageType, UnitKind};
use crate::math::{Fx, ONE};

/// Stats + presentation for one trainable unit. The generic combat/gather/move
/// systems dispatch on `UnitKind`, so a new roster entry never touches systems.
#[derive(Clone, Copy, Debug)]
pub struct UnitDef {
    pub label: &'static str,
    pub icon: &'static str,
    pub speed: Fx,
    pub carry: i32,
    pub radius: Fx,
    pub height: Fx,
    pub max_hp: i32,
    pub attack: i32,
    pub damage_type: DamageType,
    pub armor_class: ArmorClass,
    /// Specialist multiplier vs each armor class (1.0 == none).
    pub bonus_vs_armor: [Fx; 4],
    pub range: Fx,
    pub attack_rate: Fx,
    pub aggro_range: Fx,
    pub cost: ResourceCost,
    pub tint: Option<u32>,
    pub requires: Option<BuildingKind>,
    pub prefers_buildings: bool,
    pub morale_aura: Fx,
    pub garrisonable: bool,
    pub ranged: bool,
}

const NO_BONUS: [Fx; 4] = [ONE; 4];

/// `bonus_vs_armor` array with one override at `ArmorClass` index.
const fn bonus(ac: ArmorClass, mult: Fx) -> [Fx; 4] {
    let mut b = [ONE; 4];
    b[ac as usize] = mult;
    b
}

const UNIT_DEFS: [UnitDef; 10] = [
    // Peasant
    UnitDef {
        label: "Peasant",
        icon: "🧑‍🌾",
        speed: crate::fx!("2.5"),
        carry: 8,
        radius: crate::fx!("0.22"),
        height: crate::fx!("0.7"),
        max_hp: 30,
        attack: 0,
        damage_type: DamageType::Blunt,
        armor_class: ArmorClass::Unarmored,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("0.8"),
        attack_rate: crate::fx!("1.2"),
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(20, 0, 0, 0),
        tint: None,
        requires: None,
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: false,
        ranged: false,
    },
    // Spearman
    UnitDef {
        label: "Spearman",
        icon: "🛡️",
        speed: crate::fx!("2.2"),
        carry: 0,
        radius: crate::fx!("0.26"),
        height: crate::fx!("0.85"),
        max_hp: 70,
        attack: 12,
        damage_type: DamageType::Pierce,
        armor_class: ArmorClass::Leather,
        bonus_vs_armor: bonus(ArmorClass::Mail, crate::fx!("2.6")),
        range: crate::fx!("1.2"),
        attack_rate: crate::fx!("1.0"),
        aggro_range: crate::fx!("6"),
        cost: ResourceCost::new(35, 0, 0, 0),
        tint: Some(0x3a3a3a),
        requires: None,
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: true,
        ranged: false,
    },
    // Archer
    UnitDef {
        label: "Archer",
        icon: "🏹",
        speed: crate::fx!("2.4"),
        carry: 0,
        radius: crate::fx!("0.24"),
        height: crate::fx!("0.8"),
        max_hp: 45,
        attack: 9,
        damage_type: DamageType::Pierce,
        armor_class: ArmorClass::Leather,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("5"),
        attack_rate: crate::fx!("1.4"),
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(45, 0, 0, 0),
        tint: Some(0x5a3a1a),
        requires: None,
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: true,
        ranged: true,
    },
    // Knight
    UnitDef {
        label: "Knight",
        icon: "🐎",
        speed: crate::fx!("3.4"),
        carry: 0,
        radius: crate::fx!("0.3"),
        height: crate::fx!("1.0"),
        max_hp: 130,
        attack: 17,
        damage_type: DamageType::Slash,
        armor_class: ArmorClass::Mail,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("1.0"),
        attack_rate: crate::fx!("1.1"),
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(90, 0, 0, 0),
        tint: Some(0x9a8050),
        requires: Some(BuildingKind::Stable),
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: false,
        ranged: false,
    },
    // HorseArcher
    UnitDef {
        label: "Horse Archer",
        icon: "🏇",
        speed: crate::fx!("4.0"),
        carry: 0,
        radius: crate::fx!("0.28"),
        height: crate::fx!("0.95"),
        max_hp: 60,
        attack: 8,
        damage_type: DamageType::Pierce,
        armor_class: ArmorClass::Leather,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("4.5"),
        attack_rate: crate::fx!("1.3"),
        aggro_range: crate::fx!("8"),
        cost: ResourceCost::new(40, 0, 0, 20),
        tint: Some(0x7a5a2a),
        requires: Some(BuildingKind::Stable),
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: false,
        ranged: true,
    },
    // Mamluk
    UnitDef {
        label: "Mamluk",
        icon: "🗡️",
        speed: crate::fx!("3.6"),
        carry: 0,
        radius: crate::fx!("0.31"),
        height: crate::fx!("1.05"),
        max_hp: 150,
        attack: 19,
        damage_type: DamageType::Slash,
        armor_class: ArmorClass::Mail,
        bonus_vs_armor: bonus(ArmorClass::Leather, crate::fx!("1.4")),
        range: crate::fx!("1.0"),
        attack_rate: crate::fx!("1.0"),
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(0, 0, 60, 50),
        tint: Some(0xc9a24a),
        requires: Some(BuildingKind::Stable),
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: false,
        ranged: false,
    },
    // Crossbowman
    UnitDef {
        label: "Crossbowman",
        icon: "🎯",
        speed: crate::fx!("2.0"),
        carry: 0,
        radius: crate::fx!("0.25"),
        height: crate::fx!("0.82"),
        max_hp: 55,
        attack: 14,
        damage_type: DamageType::Pierce,
        armor_class: ArmorClass::Leather,
        bonus_vs_armor: bonus(ArmorClass::Mail, crate::fx!("2.2")),
        range: crate::fx!("5.5"),
        attack_rate: crate::fx!("2.0"),
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(40, 0, 0, 20),
        tint: Some(0x4a3a2a),
        requires: None,
        prefers_buildings: false,
        morale_aura: crate::fx!("0"),
        garrisonable: true,
        ranged: true,
    },
    // Ram
    UnitDef {
        label: "Battering Ram",
        icon: "🪵",
        speed: crate::fx!("1.2"),
        carry: 0,
        radius: crate::fx!("0.5"),
        height: crate::fx!("1.1"),
        max_hp: 400,
        attack: 40,
        damage_type: DamageType::Siege,
        armor_class: ArmorClass::Mail,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("1.5"),
        attack_rate: crate::fx!("2.4"),
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(120, 0, 0, 0),
        tint: Some(0x6b4a2b),
        requires: Some(BuildingKind::SiegeWorkshop),
        prefers_buildings: true,
        morale_aura: crate::fx!("0"),
        garrisonable: false,
        ranged: false,
    },
    // Mangonel
    UnitDef {
        label: "Mangonel",
        icon: "💥",
        speed: crate::fx!("1.0"),
        carry: 0,
        radius: crate::fx!("0.45"),
        height: crate::fx!("1.0"),
        max_hp: 90,
        attack: 30,
        damage_type: DamageType::Siege,
        armor_class: ArmorClass::Unarmored,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("8"),
        attack_rate: crate::fx!("3.0"),
        aggro_range: crate::fx!("9"),
        cost: ResourceCost::new(100, 0, 0, 60),
        tint: Some(0x5a4632),
        requires: Some(BuildingKind::SiegeWorkshop),
        prefers_buildings: true,
        morale_aura: crate::fx!("0"),
        garrisonable: false,
        ranged: false,
    },
    // Imam
    UnitDef {
        label: "Imam",
        icon: "🕌",
        speed: crate::fx!("2.6"),
        carry: 0,
        radius: crate::fx!("0.24"),
        height: crate::fx!("0.85"),
        max_hp: 50,
        attack: 0,
        damage_type: DamageType::Blunt,
        armor_class: ArmorClass::Unarmored,
        bonus_vs_armor: NO_BONUS,
        range: crate::fx!("0"),
        attack_rate: crate::fx!("0"),
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(0, 0, 40, 0),
        tint: Some(0xe8e2d0),
        requires: None,
        prefers_buildings: false,
        morale_aura: crate::fx!("7"),
        garrisonable: true,
        ranged: false,
    },
];

pub fn unit_def(kind: UnitKind) -> &'static UnitDef {
    &UNIT_DEFS[kind as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_indexes() {
        for &k in UnitKind::ALL {
            assert_eq!(unit_def(k).label.is_empty(), false);
        }
    }

    #[test]
    fn spearman_braces_against_mail() {
        let d = unit_def(UnitKind::Spearman);
        assert_eq!(d.bonus_vs_armor[ArmorClass::Mail as usize], crate::fx!("2.6"));
        assert_eq!(d.bonus_vs_armor[ArmorClass::Leather as usize], ONE);
    }

    #[test]
    fn cavalry_requires_stable() {
        assert_eq!(unit_def(UnitKind::Knight).requires, Some(BuildingKind::Stable));
        assert_eq!(unit_def(UnitKind::Peasant).requires, None);
    }
}
