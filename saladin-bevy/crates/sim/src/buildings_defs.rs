use crate::economy::ResourceCost;
use crate::enums::{ArmorClass, BuildingKind, DamageType, UnitKind};
use crate::math::Fx;

/// Stats, footprint, production roster and tech prereq for one structure.
/// Footprint MATH lives in `buildings.rs`; this is the DATA.
#[derive(Clone, Copy, Debug)]
pub struct BuildingDef {
    pub label: &'static str,
    /// One-line role shown on the command card — what this building is FOR.
    pub blurb: &'static str,
    pub icon: &'static str,
    pub footprint: i32,
    pub height: Fx,
    pub cost: ResourceCost,
    pub max_hp: i32,
    pub buildable: bool,
    pub pop: i32,
    pub attack: i32,
    pub damage_type: DamageType,
    pub armor_class: ArmorClass,
    pub range: Fx,
    pub attack_rate: Fx,
    pub passable: bool,
    pub trains: &'static [UnitKind],
    pub requires: Option<BuildingKind>,
    pub enables_trade: bool,
    pub food_dropoff: bool,
    pub requires_water: bool,
    pub garrison_cap: i32,
    pub garrison_survives_death: bool,
}

/// Defaults mirroring the TS `B()` helper; entries override fields after spread.
const DEFAULT: BuildingDef = BuildingDef {
    label: "",
    blurb: "",
    icon: "🏗️",
    footprint: 1,
    height: crate::fx!("1"),
    cost: ResourceCost::ZERO,
    max_hp: 0,
    buildable: true,
    pop: 0,
    attack: 0,
    damage_type: DamageType::Pierce,
    armor_class: ArmorClass::Stone,
    range: crate::fx!("0"),
    attack_rate: crate::fx!("0"),
    passable: false,
    trains: &[],
    requires: None,
    enables_trade: false,
    food_dropoff: false,
    requires_water: false,
    garrison_cap: 0,
    garrison_survives_death: false,
};

const BUILDING_DEFS: [BuildingDef; 13] = [
    // 0 Keep
    BuildingDef {
        label: "Keep",
        blurb: "Town heart: trains peasants, drops off all resources, fires on raiders.",
        icon: "🏰",
        footprint: 3,
        height: crate::fx!("1.8"),
        max_hp: 1500,
        buildable: false,
        pop: 8,
        trains: &[UnitKind::Peasant, UnitKind::Imam],
        attack: 11,
        range: crate::fx!("8"),
        attack_rate: crate::fx!("1.0"),
        garrison_cap: 10,
        garrison_survives_death: true,
        ..DEFAULT
    },
    // 1 Barracks
    BuildingDef {
        label: "Barracks",
        blurb: "Trains infantry: spearmen, archers, crossbowmen.",
        icon: "🏛️",
        footprint: 2,
        height: crate::fx!("1.4"),
        cost: ResourceCost::new(70, 20, 0, 0),
        max_hp: 500,
        trains: &[UnitKind::Spearman, UnitKind::Archer, UnitKind::Crossbowman],
        armor_class: ArmorClass::Leather,
        ..DEFAULT
    },
    // 2 Tower
    BuildingDef {
        label: "Tower",
        blurb: "Defensive tower; garrison archers to stack its volleys.",
        icon: "🗼",
        footprint: 1,
        height: crate::fx!("2.6"),
        cost: ResourceCost::new(40, 30, 0, 0),
        max_hp: 400,
        attack: 9,
        range: crate::fx!("7"),
        attack_rate: crate::fx!("0.9"),
        garrison_cap: 5,
        garrison_survives_death: true,
        ..DEFAULT
    },
    // 3 Wall
    BuildingDef {
        label: "Wall",
        blurb: "Blocks movement. Garrison 2 to fire from the parapet.",
        icon: "🧱",
        footprint: 1,
        height: crate::fx!("1.2"),
        cost: ResourceCost::new(6, 6, 0, 0),
        max_hp: 300,
        garrison_cap: 2,
        garrison_survives_death: false,
        ..DEFAULT
    },
    // 4 Gatehouse
    BuildingDef {
        label: "Gatehouse",
        blurb: "A wall your own units walk through.",
        icon: "🚪",
        footprint: 1,
        height: crate::fx!("1.5"),
        cost: ResourceCost::new(15, 15, 0, 0),
        max_hp: 400,
        passable: true,
        garrison_cap: 3,
        ..DEFAULT
    },
    // 5 House
    BuildingDef {
        label: "House",
        blurb: "Houses 6 population.",
        icon: "🏠",
        footprint: 2,
        height: crate::fx!("1.2"),
        cost: ResourceCost::new(40, 0, 0, 0),
        max_hp: 250,
        pop: 6,
        armor_class: ArmorClass::Leather,
        ..DEFAULT
    },
    // 6 Stable
    BuildingDef {
        label: "Stable",
        blurb: "Trains cavalry: knights, horse archers, mamluks.",
        icon: "🐴",
        footprint: 2,
        height: crate::fx!("1.4"),
        cost: ResourceCost::new(80, 20, 0, 0),
        max_hp: 500,
        trains: &[UnitKind::Knight, UnitKind::HorseArcher, UnitKind::Mamluk],
        armor_class: ArmorClass::Leather,
        requires: Some(BuildingKind::Barracks),
        ..DEFAULT
    },
    // 7 Blacksmith
    BuildingDef {
        label: "Blacksmith",
        blurb: "Researches weapon, armor and economy upgrades.",
        icon: "⚒️",
        footprint: 2,
        height: crate::fx!("1.5"),
        cost: ResourceCost::new(60, 40, 0, 0),
        max_hp: 550,
        requires: Some(BuildingKind::Barracks),
        ..DEFAULT
    },
    // 8 Market
    BuildingDef {
        label: "Market",
        blurb: "Sell surplus wood and stone for gold.",
        icon: "🏪",
        footprint: 2,
        height: crate::fx!("1.3"),
        cost: ResourceCost::new(60, 20, 0, 0),
        max_hp: 450,
        armor_class: ArmorClass::Leather,
        enables_trade: true,
        requires: Some(BuildingKind::Keep),
        ..DEFAULT
    },
    // 9 Granary
    BuildingDef {
        label: "Granary",
        blurb: "Food drop-off: shortens hunting hauls.",
        icon: "🌾",
        footprint: 2,
        height: crate::fx!("1.3"),
        cost: ResourceCost::new(50, 10, 0, 0),
        max_hp: 400,
        armor_class: ArmorClass::Leather,
        food_dropoff: true,
        requires: Some(BuildingKind::Keep),
        ..DEFAULT
    },
    // 10 FishingHut
    BuildingDef {
        label: "Fishing Hut",
        blurb: "Shoreline food drop-off: build beside water, near fish.",
        icon: "🎣",
        footprint: 1,
        height: crate::fx!("1.0"),
        cost: ResourceCost::new(35, 0, 0, 0),
        max_hp: 250,
        armor_class: ArmorClass::Leather,
        food_dropoff: true,
        requires_water: true,
        requires: Some(BuildingKind::Keep),
        ..DEFAULT
    },
    // 11 SiegeWorkshop
    BuildingDef {
        label: "Siege Workshop",
        blurb: "Builds rams and mangonels.",
        icon: "🛠️",
        footprint: 2,
        height: crate::fx!("1.5"),
        cost: ResourceCost::new(100, 40, 0, 0),
        max_hp: 600,
        trains: &[UnitKind::Ram, UnitKind::Mangonel],
        armor_class: ArmorClass::Leather,
        requires: Some(BuildingKind::Blacksmith),
        ..DEFAULT
    },
    // 12 Watchtower
    BuildingDef {
        label: "Watchtower",
        blurb: "Heavy tower: longest reach, garrison 8.",
        icon: "🛡️",
        footprint: 1,
        height: crate::fx!("3.4"),
        cost: ResourceCost::new(80, 70, 0, 0),
        max_hp: 700,
        attack: 13,
        range: crate::fx!("9"),
        attack_rate: crate::fx!("0.8"),
        garrison_cap: 8,
        garrison_survives_death: true,
        requires: Some(BuildingKind::Tower),
        ..DEFAULT
    },
];

pub fn building_def(kind: BuildingKind) -> &'static BuildingDef {
    &BUILDING_DEFS[kind as usize]
}

pub struct BuildCategory {
    pub label: &'static str,
    pub icon: &'static str,
    pub kinds: &'static [BuildingKind],
}

pub const BUILD_CATEGORIES: [BuildCategory; 6] = [
    BuildCategory {
        label: "Defense",
        icon: "🛡️",
        kinds: &[
            BuildingKind::Wall,
            BuildingKind::Gatehouse,
            BuildingKind::Tower,
            BuildingKind::Watchtower,
        ],
    },
    BuildCategory {
        label: "Economy",
        icon: "🏠",
        kinds: &[
            BuildingKind::House,
            BuildingKind::Market,
            BuildingKind::Granary,
            BuildingKind::FishingHut,
        ],
    },
    BuildCategory { label: "Military", icon: "⚔️", kinds: &[BuildingKind::Barracks] },
    BuildCategory { label: "Cavalry", icon: "🐴", kinds: &[BuildingKind::Stable] },
    BuildCategory { label: "Siege", icon: "🛠️", kinds: &[BuildingKind::SiegeWorkshop] },
    BuildCategory { label: "Tech", icon: "⚒️", kinds: &[BuildingKind::Blacksmith] },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_is_unbuildable_fortress() {
        let k = building_def(BuildingKind::Keep);
        assert!(!k.buildable);
        assert_eq!(k.max_hp, 1500);
        assert_eq!(k.garrison_cap, 10);
        assert!(k.trains.contains(&UnitKind::Peasant));
    }

    #[test]
    fn every_kind_indexes_in_order() {
        for &k in BuildingKind::ALL {
            assert_eq!(building_def(k).label.is_empty(), false);
        }
        // index order sanity: enum discriminant maps to the right entry
        assert_eq!(building_def(BuildingKind::Watchtower).label, "Watchtower");
        assert_eq!(building_def(BuildingKind::SiegeWorkshop).label, "Siege Workshop");
    }

    #[test]
    fn tech_prereqs() {
        assert_eq!(building_def(BuildingKind::Stable).requires, Some(BuildingKind::Barracks));
        assert_eq!(building_def(BuildingKind::SiegeWorkshop).requires, Some(BuildingKind::Blacksmith));
    }
}
