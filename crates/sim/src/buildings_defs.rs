use crate::economy::ResourceCost;
use crate::enums::{ArmorClass, BuildingKind, DamageType, ResourceType, UnitKind};
use crate::math::Fx;

/// The bit a resource occupies in `BuildingDef::accepts`.
pub const fn res_bit(r: ResourceType) -> u8 {
    1u8 << (r as u8)
}

/// Every resource — a universal drop-off.
pub const ACCEPTS_ALL: u8 = 0b1111;
pub const ACCEPTS_FOOD: u8 = res_bit(ResourceType::Food);

/// What a work aura speeds up. The Fishing Hut tends its fishery, the Granary
/// its fields — one mechanic, two rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuraTarget {
    WaterFood,
    Field,
}

/// A standing bonus a building projects over the resource nodes around it.
#[derive(Clone, Copy, Debug)]
pub struct WorkAura {
    pub radius: Fx,
    pub target: AuraTarget,
    /// Harvest speed multiplier for nodes in reach.
    pub harvest_mult: Fx,
    /// Units restocked per economy tick on each node in reach.
    pub regen: i32,
}

/// Stats, footprint, production roster and tech prereq for one structure.
/// Footprint MATH lives in `buildings.rs`; this is the DATA. Every capability a
/// system gates on is a FIELD here — a new role is a row, not a `kind == X`
/// branch in a system.
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
    /// The PRIMARY prerequisite — the short lock label. `prereqs` carries the
    /// rest; `has_prereq_all` reads both.
    pub requires: Option<BuildingKind>,
    /// Additional prerequisites beyond `requires`.
    pub prereqs: &'static [BuildingKind],
    pub enables_trade: bool,
    pub requires_water: bool,
    pub garrison_cap: i32,
    pub garrison_survives_death: bool,
    /// Soil this structure needs under it (0 = anywhere). Farms only.
    pub min_fertility: Fx,
    /// Seconds of one builder's labour to raise it (0 = stands instantly).
    pub build_time: Fx,
    pub upgrades_to: Option<BuildingKind>,
    pub upgrade_cost: ResourceCost,
    pub upgrade_time: Fx,
    /// Resource bitmask a gatherer may deposit here (`res_bit`).
    pub accepts: u8,
    pub aura: Option<WorkAura>,
    pub hosts_research: bool,
    /// Losing this structure loses the match.
    pub defeat_on_death: bool,
    /// Friendly units within this range recover morale faster (0 = none).
    pub morale_radius: Fx,
    /// Multiplier applied to SIEGE damage only, so hardening a wall never
    /// changes what a ram does to a spearman.
    pub siege_resist: Fx,
    /// Most one player may own (0 = unlimited).
    pub max_count: i32,
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
    prereqs: &[],
    enables_trade: false,
    requires_water: false,
    garrison_cap: 0,
    garrison_survives_death: false,
    min_fertility: crate::fx!("0"),
    build_time: crate::fx!("0"),
    upgrades_to: None,
    upgrade_cost: ResourceCost::ZERO,
    upgrade_time: crate::fx!("0"),
    accepts: 0,
    aura: None,
    hosts_research: false,
    defeat_on_death: false,
    morale_radius: crate::fx!("0"),
    siege_resist: crate::fx!("1"),
    max_count: 0,
};

const BUILDING_DEFS: [BuildingDef; 16] = [
    // 0 Keep
    BuildingDef {
        label: "Keep",
        blurb: "Town heart: trains peasants, takes every haul, and its fall ends the war.",
        icon: "🏰",
        footprint: 3,
        height: crate::fx!("1.8"),
        // Never paid: the Keep is `buildable: false` and `demolish` refuses it.
        // This is what MENDING one is priced against — `repair_charge` scales
        // the build cost, so a costless keep healed from a wreck for nothing.
        cost: ResourceCost::new(200, 300, 0, 0),
        max_hp: 1500,
        buildable: false,
        pop: 8,
        trains: &[UnitKind::Peasant],
        attack: 11,
        range: crate::fx!("8"),
        attack_rate: crate::fx!("1.0"),
        garrison_cap: 10,
        garrison_survives_death: true,
        accepts: ACCEPTS_ALL,
        defeat_on_death: true,
        morale_radius: crate::fx!("5"),
        siege_resist: crate::fx!("0.3"),
        ..DEFAULT
    },
    // 1 Barracks
    BuildingDef {
        label: "Barracks",
        blurb: "Infantry hall, and the root of the whole military tree.",
        icon: "🏛️",
        footprint: 2,
        height: crate::fx!("1.4"),
        cost: ResourceCost::new(70, 20, 0, 0),
        max_hp: 500,
        trains: &[
            UnitKind::Spearman,
            UnitKind::Archer,
            UnitKind::Crossbowman,
            UnitKind::Sergeant,
            UnitKind::Naffatun,
        ],
        armor_class: ArmorClass::Leather,
        build_time: crate::fx!("25"),
        ..DEFAULT
    },
    // 2 Tower
    BuildingDef {
        label: "Tower",
        blurb: "Cheap picket. Garrison archers, then raise it into a Watchtower in place.",
        icon: "🗼",
        footprint: 1,
        height: crate::fx!("2.6"),
        cost: ResourceCost::new(40, 50, 0, 0),
        max_hp: 550,
        attack: 9,
        range: crate::fx!("7"),
        attack_rate: crate::fx!("0.9"),
        garrison_cap: 5,
        garrison_survives_death: true,
        build_time: crate::fx!("20"),
        upgrades_to: Some(BuildingKind::Watchtower),
        upgrade_cost: ResourceCost::new(60, 90, 0, 0),
        upgrade_time: crate::fx!("25"),
        siege_resist: crate::fx!("0.4"),
        ..DEFAULT
    },
    // 3 Wall
    BuildingDef {
        label: "Wall",
        blurb: "Shapes the battle. Garrison 2 to fire from the parapet.",
        icon: "🧱",
        footprint: 1,
        height: crate::fx!("1.2"),
        cost: ResourceCost::new(5, 8, 0, 0),
        max_hp: 420,
        garrison_cap: 2,
        garrison_survives_death: false,
        build_time: crate::fx!("3"),
        siege_resist: crate::fx!("0.45"),
        ..DEFAULT
    },
    // 4 Gatehouse
    BuildingDef {
        label: "Gatehouse",
        blurb: "A wall your own units walk through and the enemy does not.",
        icon: "🚪",
        footprint: 1,
        height: crate::fx!("1.5"),
        cost: ResourceCost::new(20, 25, 0, 0),
        max_hp: 500,
        passable: true,
        garrison_cap: 3,
        garrison_survives_death: true,
        prereqs: &[BuildingKind::Wall],
        build_time: crate::fx!("10"),
        siege_resist: crate::fx!("0.4"),
        ..DEFAULT
    },
    // 5 House
    BuildingDef {
        label: "House",
        blurb: "Houses 6, and shelters 3 peasants when the raiders come.",
        icon: "🏠",
        footprint: 2,
        height: crate::fx!("1.2"),
        cost: ResourceCost::new(30, 0, 20, 0),
        max_hp: 250,
        pop: 6,
        armor_class: ArmorClass::Leather,
        garrison_cap: 3,
        garrison_survives_death: true,
        build_time: crate::fx!("12"),
        ..DEFAULT
    },
    // 6 Stable
    BuildingDef {
        label: "Stable",
        blurb: "Cavalry hall: the mounted arm, whichever one your banner fields.",
        icon: "🐴",
        footprint: 2,
        height: crate::fx!("1.4"),
        cost: ResourceCost::new(120, 40, 0, 0),
        max_hp: 500,
        trains: &[UnitKind::Knight, UnitKind::HorseArcher, UnitKind::Mamluk],
        armor_class: ArmorClass::Leather,
        requires: Some(BuildingKind::Barracks),
        prereqs: &[BuildingKind::Blacksmith],
        build_time: crate::fx!("30"),
        ..DEFAULT
    },
    // 7 Blacksmith
    BuildingDef {
        label: "Blacksmith",
        blurb: "Research hall: every weapon, armor and masonry upgrade.",
        icon: "⚒️",
        footprint: 2,
        height: crate::fx!("1.5"),
        cost: ResourceCost::new(90, 60, 0, 0),
        max_hp: 550,
        armor_class: ArmorClass::Leather,
        requires: Some(BuildingKind::Barracks),
        hosts_research: true,
        build_time: crate::fx!("30"),
        ..DEFAULT
    },
    // 8 Market
    BuildingDef {
        label: "Market",
        blurb: "The gold engine: sell a glut, buy a shortage.",
        icon: "🏪",
        footprint: 2,
        height: crate::fx!("1.3"),
        cost: ResourceCost::new(70, 30, 0, 0),
        max_hp: 450,
        armor_class: ArmorClass::Leather,
        enables_trade: true,
        build_time: crate::fx!("25"),
        ..DEFAULT
    },
    // 9 Granary
    BuildingDef {
        label: "Granary",
        blurb: "Farm hub: fields in reach are worked and regrow far faster.",
        icon: "🌾",
        footprint: 2,
        height: crate::fx!("1.3"),
        cost: ResourceCost::new(50, 10, 0, 0),
        max_hp: 400,
        armor_class: ArmorClass::Leather,
        prereqs: &[BuildingKind::Farm],
        aura: Some(WorkAura {
            radius: crate::GRANARY_RANGE,
            target: AuraTarget::Field,
            harvest_mult: crate::fx!("1.5"),
            regen: 3,
        }),
        build_time: crate::fx!("20"),
        ..DEFAULT
    },
    // 10 FishingHut
    BuildingDef {
        label: "Fishing Hut",
        blurb: "Shore camp: nets double the catch and restock the fishery.",
        icon: "🎣",
        footprint: 1,
        height: crate::fx!("1.0"),
        cost: ResourceCost::new(40, 0, 0, 0),
        max_hp: 250,
        armor_class: ArmorClass::Leather,
        accepts: ACCEPTS_FOOD,
        requires_water: true,
        aura: Some(WorkAura {
            radius: crate::FISHING_HUT_RANGE,
            target: AuraTarget::WaterFood,
            harvest_mult: crate::fx!("2"),
            regen: crate::FISH_REGEN_PER_TICK,
        }),
        build_time: crate::fx!("12"),
        ..DEFAULT
    },
    // 11 SiegeWorkshop
    BuildingDef {
        label: "Siege Workshop",
        blurb: "Siege hall: rams and mangonels, the answer to a walled enemy.",
        icon: "🛠️",
        footprint: 2,
        height: crate::fx!("1.5"),
        cost: ResourceCost::new(160, 80, 0, 0),
        max_hp: 600,
        trains: &[UnitKind::Ram, UnitKind::Mangonel],
        armor_class: ArmorClass::Leather,
        requires: Some(BuildingKind::Blacksmith),
        prereqs: &[BuildingKind::Barracks],
        build_time: crate::fx!("40"),
        ..DEFAULT
    },
    // 12 Watchtower — never built, only upgraded into from a standing Tower.
    // `cost` is what the finished tower represents (Tower + upgrade), so a
    // demolition refunds against what was actually spent.
    BuildingDef {
        label: "Watchtower",
        blurb: "What a Tower becomes: longest reach, garrison 8.",
        icon: "🛡️",
        footprint: 1,
        height: crate::fx!("3.4"),
        cost: ResourceCost::new(100, 140, 0, 0),
        max_hp: 950,
        buildable: false,
        attack: 13,
        range: crate::fx!("9"),
        attack_rate: crate::fx!("0.8"),
        garrison_cap: 8,
        garrison_survives_death: true,
        requires: Some(BuildingKind::Tower),
        siege_resist: crate::fx!("0.35"),
        ..DEFAULT
    },
    // 13 Farm
    BuildingDef {
        label: "Farm",
        blurb: "Sown field: peasants harvest it forever, and rich soil regrows it faster.",
        icon: "🌾",
        footprint: 2,
        height: crate::fx!("0.35"),
        cost: ResourceCost::new(50, 0, 0, 0),
        max_hp: 220,
        armor_class: ArmorClass::Unarmored,
        min_fertility: crate::FARM_MIN_FERTILITY,
        build_time: crate::fx!("12"),
        ..DEFAULT
    },
    // 14 Storehouse
    BuildingDef {
        label: "Storehouse",
        blurb: "Outpost drop-off for every resource: how a town reaches the far quarries.",
        icon: "📦",
        footprint: 2,
        height: crate::fx!("1.25"),
        cost: ResourceCost::new(70, 20, 0, 0),
        max_hp: 400,
        armor_class: ArmorClass::Leather,
        accepts: ACCEPTS_ALL,
        build_time: crate::fx!("16"),
        ..DEFAULT
    },
    // 15 Mosque
    BuildingDef {
        label: "Mosque",
        blurb: "Faith hall: trains your banner's preacher and steadies the ground.",
        icon: "🕌",
        footprint: 2,
        height: crate::fx!("1.9"),
        cost: ResourceCost::new(90, 40, 0, 60),
        max_hp: 600,
        trains: &[UnitKind::Imam, UnitKind::Chaplain],
        requires: Some(BuildingKind::Barracks),
        morale_radius: crate::MOSQUE_MORALE_RANGE,
        build_time: crate::fx!("30"),
        siege_resist: crate::fx!("0.75"),
        max_count: 1,
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

/// Four tabs, every one of them worth opening. The Watchtower appears in NONE:
/// it is an upgrade of a standing Tower, not a purchase.
pub const BUILD_CATEGORIES: [BuildCategory; 4] = [
    BuildCategory {
        label: "Economy",
        icon: "🏠",
        kinds: &[
            BuildingKind::House,
            BuildingKind::Farm,
            BuildingKind::Granary,
            BuildingKind::FishingHut,
            BuildingKind::Market,
        ],
    },
    BuildCategory {
        label: "Town",
        icon: "📦",
        kinds: &[BuildingKind::Storehouse, BuildingKind::Blacksmith, BuildingKind::Mosque],
    },
    BuildCategory {
        label: "Defense",
        icon: "🛡️",
        kinds: &[BuildingKind::Wall, BuildingKind::Gatehouse, BuildingKind::Tower],
    },
    BuildCategory {
        label: "Military",
        icon: "⚔️",
        kinds: &[BuildingKind::Barracks, BuildingKind::Stable, BuildingKind::SiegeWorkshop],
    },
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

    /// `repair_charge` scales the BUILD cost, so any structure a hammer can
    /// reach has to carry one — a costless def mends for free forever.
    #[test]
    fn nothing_repairable_is_priced_at_nothing() {
        for &k in BuildingKind::ALL {
            let d = building_def(k);
            let paid = d.cost.wood + d.cost.stone + d.cost.food + d.cost.gold;
            assert!(paid > 0, "{k:?} has no cost, so mending it is free");
        }
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

    /// Capabilities are DATA, spread across the roster — the fields that
    /// replaced the hardcoded `kind == X` branches in gather/economy/combat.
    #[test]
    fn role_fields_are_the_single_source_of_truth() {
        let dropoffs: Vec<_> =
            BuildingKind::ALL.iter().filter(|k| building_def(**k).accepts != 0).collect();
        assert!(dropoffs.len() >= 3, "a town needs more than one place to haul to");
        assert_eq!(building_def(BuildingKind::Keep).accepts, ACCEPTS_ALL);
        assert_eq!(building_def(BuildingKind::Storehouse).accepts, ACCEPTS_ALL);
        assert_eq!(building_def(BuildingKind::FishingHut).accepts, ACCEPTS_FOOD);
        assert_eq!(building_def(BuildingKind::Farm).accepts, 0, "a farm is not a warehouse");

        let auras: Vec<_> =
            BuildingKind::ALL.iter().filter(|k| building_def(**k).aura.is_some()).collect();
        assert_eq!(auras.len(), 2, "one aura mechanic, two rows");
        assert_eq!(
            building_def(BuildingKind::Granary).aura.unwrap().target,
            AuraTarget::Field
        );

        for &k in BuildingKind::ALL {
            let d = building_def(k);
            assert_eq!(d.hosts_research, k == BuildingKind::Blacksmith, "{k:?} research");
            assert_eq!(d.defeat_on_death, k == BuildingKind::Keep, "{k:?} defeat");
            assert!(d.siege_resist > Fx::ZERO, "{k:?} siege resist");
            assert!(d.build_time >= Fx::ZERO);
        }
        // morale is no longer the Keep's private hardcoded trick
        assert!(building_def(BuildingKind::Mosque).morale_radius > Fx::ZERO);
    }

    /// The delete test, mechanised: if two structures answer "what do I give up
    /// by not building this?" identically, one of them should not exist.
    #[test]
    fn no_two_buildable_kinds_share_a_role() {
        #[derive(PartialEq, Debug)]
        struct Sig {
            accepts: u8,
            trains: Vec<u8>,
            aura: Option<AuraTarget>,
            research: bool,
            pop: i32,
            attack: i32,
            trade: bool,
            morale: bool,
            water: bool,
            farmland: bool,
            garrison: i32,
            passable: bool,
            upgrades: bool,
        }
        let sig = |k: BuildingKind| -> Sig {
            let d = building_def(k);
            Sig {
                accepts: d.accepts,
                trains: d.trains.iter().map(|u| *u as u8).collect(),
                aura: d.aura.map(|a| a.target),
                research: d.hosts_research,
                pop: d.pop,
                attack: d.attack,
                trade: d.enables_trade,
                morale: d.morale_radius > Fx::ZERO,
                water: d.requires_water,
                farmland: d.min_fertility > Fx::ZERO,
                garrison: d.garrison_cap,
                passable: d.passable,
                upgrades: d.upgrades_to.is_some(),
            }
        };
        let buildable: Vec<BuildingKind> =
            BuildingKind::ALL.iter().copied().filter(|k| building_def(*k).buildable).collect();
        for (i, a) in buildable.iter().enumerate() {
            for b in &buildable[i + 1..] {
                assert_ne!(sig(*a), sig(*b), "{a:?} and {b:?} do the same job");
            }
        }
    }

    #[test]
    fn every_build_tab_is_worth_opening() {
        let mut seen: Vec<BuildingKind> = Vec::new();
        for c in BUILD_CATEGORIES.iter() {
            assert!(c.kinds.len() >= 3, "tab {} holds {} cards", c.label, c.kinds.len());
            for &k in c.kinds {
                assert!(building_def(k).buildable, "{k:?} is on the bar but cannot be built");
                assert!(!seen.contains(&k), "{k:?} appears in two tabs");
                seen.push(k);
            }
        }
        for &k in BuildingKind::ALL {
            assert_eq!(
                building_def(k).buildable,
                seen.contains(&k),
                "{k:?} buildable/on-the-bar mismatch"
            );
        }
    }

    #[test]
    fn the_watchtower_is_earned_not_bought() {
        assert!(!building_def(BuildingKind::Watchtower).buildable);
        let upgrades: Vec<BuildingKind> = BuildingKind::ALL
            .iter()
            .copied()
            .filter_map(|k| building_def(k).upgrades_to)
            .collect();
        assert_eq!(upgrades, vec![BuildingKind::Watchtower]);
        let t = building_def(BuildingKind::Tower);
        assert!(t.upgrade_cost.wood + t.upgrade_cost.stone > 0 && t.upgrade_time > Fx::ZERO);
    }

    #[test]
    fn the_prereq_graph_has_real_depth() {
        use crate::tech::all_prereqs;
        let edges: i32 = BuildingKind::ALL
            .iter()
            .filter(|k| building_def(**k).buildable)
            .map(|k| all_prereqs(building_def(*k)).len() as i32)
            .sum();
        assert!(edges >= 7, "only {edges} prereq edges in the whole tree");
        // the two genuine multi-prereq gates
        for k in [BuildingKind::Stable, BuildingKind::SiegeWorkshop] {
            assert_eq!(all_prereqs(building_def(k)).len(), 2, "{k:?}");
        }
        // no vacuous "requires the Keep you always own" edges
        for &k in BuildingKind::ALL {
            assert!(
                !all_prereqs(building_def(k)).contains(&BuildingKind::Keep),
                "{k:?} gates on the Keep, which every player owns"
            );
        }
    }

    /// Hits a Battering Ram needs per structure. Stone works are the hard
    /// things; the halls burn.
    #[test]
    fn a_ram_finds_stone_hard_and_timber_soft() {
        let ram = crate::units::unit_def(crate::enums::UnitKind::Ram);
        let atk = crate::combat::Attacker::new(Fx::from_num(ram.attack), ram.damage_type);
        let hits = |k: BuildingKind| {
            let d = building_def(k);
            let per = crate::combat::building_damage(&atk, d);
            (d.max_hp + per - 1) / per
        };
        let keep = hits(BuildingKind::Keep);
        for &k in BuildingKind::ALL {
            if k == BuildingKind::Keep {
                continue;
            }
            assert!(keep > hits(k), "the Keep must outlast {k:?} ({keep} vs {})", hits(k));
        }
        // the shipped inversion: a Keep used to fall in half the hits a Siege
        // Workshop did
        assert!(hits(BuildingKind::Keep) > hits(BuildingKind::SiegeWorkshop) * 2);
        // and the softest things are the ones made of wood and thatch
        let soft = hits(BuildingKind::Farm).max(hits(BuildingKind::House));
        for k in [BuildingKind::Tower, BuildingKind::Wall, BuildingKind::Gatehouse] {
            assert!(hits(k) > soft, "{k:?} is no sturdier than a hut");
        }
    }

    /// The ordering over EVERY kind, which is what the old two-clause check
    /// missed: it compared the Keep against everything and the three
    /// fortifications against a hut, so a Stone-classed Blacksmith carrying
    /// `siege_resist: 2.0` — 200 damage a hit, three hits, softer than a house,
    /// and the Siege Workshop's own prerequisite — shipped green.
    #[test]
    fn every_structure_sits_where_it_belongs_under_a_ram() {
        let ram = crate::units::unit_def(crate::enums::UnitKind::Ram);
        let atk = crate::combat::Attacker::new(Fx::from_num(ram.attack), ram.damage_type);
        let hits = |k: BuildingKind| {
            let d = building_def(k);
            let per = crate::combat::building_damage(&atk, d).max(1);
            (d.max_hp + per - 1) / per
        };
        const FORTS: [BuildingKind; 5] = [
            BuildingKind::Keep,
            BuildingKind::Watchtower,
            BuildingKind::Tower,
            BuildingKind::Gatehouse,
            BuildingKind::Wall,
        ];
        let weakest_fort = FORTS.iter().map(|k| hits(*k)).min().unwrap();
        for &k in BuildingKind::ALL {
            if FORTS.contains(&k) {
                continue;
            }
            assert!(
                hits(k) < weakest_fort,
                "{k:?} takes {} ram hits, no less than the weakest fortification ({weakest_fort})",
                hits(k)
            );
        }
        // fortifications rank in the order their walls look
        for pair in FORTS.windows(2) {
            assert!(
                hits(pair[0]) > hits(pair[1]),
                "{:?} ({}) does not outlast {:?} ({})",
                pair[0],
                hits(pair[0]),
                pair[1],
                hits(pair[1])
            );
        }
        // the forge that unlocks the siege shed is not the softest thing in town
        assert!(hits(BuildingKind::Blacksmith) > hits(BuildingKind::House));
        for &k in BuildingKind::ALL {
            assert!(hits(k) >= 2, "{k:?} falls to a single ram blow");
        }
    }

    #[test]
    fn every_unit_takes_time_to_train() {
        for &k in crate::enums::UnitKind::ALL {
            assert!(crate::units::unit_def(k).train_time > Fx::ZERO, "{k:?} trains instantly");
        }
    }

    #[test]
    fn tech_prereqs() {
        assert_eq!(building_def(BuildingKind::Stable).requires, Some(BuildingKind::Barracks));
        assert_eq!(building_def(BuildingKind::SiegeWorkshop).requires, Some(BuildingKind::Blacksmith));
    }
}
