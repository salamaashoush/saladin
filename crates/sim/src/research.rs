use crate::buildings_defs::{BuildingDef, building_def};
use crate::economy::{ResourceCost, Stockpile};
use crate::enums::{ArmorClass, BuildingKind};
use crate::math::Fx;
use crate::tech::has_prereq;
use crate::enums::UnitRole;
use crate::units::{UnitDef, unit_def};
use crate::enums::UnitKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Blacksmith research. Each `Tech` is a bit position in the owner's u64
/// `tech_mask`. Bonuses are NEVER baked onto rows; they are DERIVED on read via
/// `effective_unit_def` / `effective_building_def`, so an upgrade applies to
/// every current and future unit of a kind automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Tech {
    ArmorMail = 0,
    ArmorPlate = 1,
    FletchedArrows = 2,
    SharpenedBlades = 3,
    Masonry = 4,
    Conscription = 5,
    SiegeEngineering = 6,
}

pub const ALL_TECHS: [Tech; 7] = [
    Tech::ArmorMail,
    Tech::ArmorPlate,
    Tech::FletchedArrows,
    Tech::SharpenedBlades,
    Tech::Masonry,
    Tech::Conscription,
    Tech::SiegeEngineering,
];

impl Tech {
    pub fn from_u8(v: u8) -> Option<Tech> {
        ALL_TECHS.iter().copied().find(|t| *t as u8 == v)
    }
}

/// Additive deltas folded onto a base def (0 == no change).
#[derive(Clone, Copy, Debug)]
pub struct UnitDelta {
    pub attack: i32,
    pub max_hp: i32,
    pub range: Fx,
    /// Flat damage soaked per incoming hit. Armour research USED to promote the
    /// unit's armour CLASS, which emptied the Leather column of DAMAGE_MATRIX
    /// halfway through every match and handed the one dominant unit a universal
    /// bonus exactly as the game progressed.
    pub damage_reduction: i32,
}

const NO_DELTA: UnitDelta =
    UnitDelta { attack: 0, max_hp: 0, range: crate::fx!("0"), damage_reduction: 0 };

#[derive(Clone, Copy, Debug)]
pub struct BuildingDelta {
    pub max_hp: i32,
    pub armor_tier: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct UpgradeDef {
    pub label: &'static str,
    pub icon: &'static str,
    pub cost: ResourceCost,
    pub research_time: Fx,
    pub requires: Option<BuildingKind>,
    pub applies_to: fn(&UnitDef) -> bool,
    pub delta: UnitDelta,
    pub building_delta: Option<BuildingDelta>,
    pub applies_to_buildings: bool,
}

// Upgrade eligibility reads the unit's ROLE. The shape-derived predicates it
// replaces (`attack > 0 && !ranged && range <= 2`) also matched a Battering Ram,
// which duly received Sharpened Blades and Plate Barding, while the Mangonel
// matched nothing at all and never got an attack upgrade in its life.
fn is_combatant(d: &UnitDef) -> bool {
    !matches!(d.role, UnitRole::Worker | UnitRole::Support)
}
fn is_ranged(d: &UnitDef) -> bool {
    matches!(d.role, UnitRole::Archer | UnitRole::HorseArcher)
}
fn is_melee(d: &UnitDef) -> bool {
    matches!(d.role, UnitRole::Foot | UnitRole::Cavalry)
}
fn is_mounted(d: &UnitDef) -> bool {
    matches!(d.role, UnitRole::Cavalry | UnitRole::HorseArcher)
}
fn is_siege(d: &UnitDef) -> bool {
    d.role == UnitRole::Siege
}
fn troops_not_siege(d: &UnitDef) -> bool {
    is_combatant(d) && !is_siege(d)
}
fn never(_: &UnitDef) -> bool {
    false
}

const UPGRADE_DEFS: [UpgradeDef; 7] = [
    // ArmorMail
    UpgradeDef {
        label: "Mail Armor",
        icon: "🥼",
        cost: ResourceCost::new(60, 0, 0, 40),
        research_time: crate::fx!("30"),
        requires: None,
        applies_to: troops_not_siege,
        delta: UnitDelta { damage_reduction: 2, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
    // ArmorPlate — barding is horse armour, and it is bought at the Stable.
    UpgradeDef {
        label: "Plate Barding",
        icon: "🛡️",
        cost: ResourceCost::new(40, 30, 0, 60),
        research_time: crate::fx!("45"),
        requires: Some(BuildingKind::Stable),
        applies_to: is_mounted,
        delta: UnitDelta { max_hp: 25, damage_reduction: 2, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
    // FletchedArrows
    UpgradeDef {
        label: "Fletched Arrows",
        icon: "🏹",
        cost: ResourceCost::new(50, 0, 0, 30),
        research_time: crate::fx!("30"),
        requires: None,
        applies_to: is_ranged,
        delta: UnitDelta { attack: 3, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
    // SharpenedBlades
    UpgradeDef {
        label: "Sharpened Blades",
        icon: "⚔️",
        cost: ResourceCost::new(50, 0, 0, 30),
        research_time: crate::fx!("30"),
        requires: None,
        applies_to: is_melee,
        delta: UnitDelta { attack: 3, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
    // Masonry
    UpgradeDef {
        label: "Masonry",
        icon: "🧱",
        cost: ResourceCost::new(40, 80, 0, 0),
        research_time: crate::fx!("40"),
        requires: None,
        applies_to: never,
        delta: NO_DELTA,
        building_delta: Some(BuildingDelta { max_hp: 250, armor_tier: 0 }),
        applies_to_buildings: true,
    },
    // Conscription
    UpgradeDef {
        label: "Conscription",
        icon: "🪖",
        cost: ResourceCost::new(0, 0, 60, 50),
        research_time: crate::fx!("50"),
        requires: Some(BuildingKind::Barracks),
        applies_to: is_combatant,
        delta: UnitDelta { max_hp: 15, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
    // SiegeEngineering — the engines were the one role no upgrade could reach.
    UpgradeDef {
        label: "Siege Engineering",
        icon: "🛠️",
        cost: ResourceCost::new(60, 40, 0, 40),
        research_time: crate::fx!("40"),
        requires: Some(BuildingKind::SiegeWorkshop),
        applies_to: is_siege,
        delta: UnitDelta { attack: 8, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
];

pub fn upgrade_def(tech: Tech) -> &'static UpgradeDef {
    &UPGRADE_DEFS[tech as usize]
}

// ── bitmask ─────────────────────────────────────────────────────────────────

pub fn tech_bit(tech: Tech) -> u64 {
    1u64 << (tech as u8)
}
pub fn has_tech(mask: u64, tech: Tech) -> bool {
    mask & tech_bit(tech) != 0
}
pub fn set_tech(mask: u64, tech: Tech) -> u64 {
    mask | tech_bit(tech)
}
pub fn techs_in_mask(mask: u64) -> Vec<Tech> {
    ALL_TECHS.iter().copied().filter(|t| has_tech(mask, *t)).collect()
}

fn clamp_tier(tier: i32, cap: ArmorClass) -> ArmorClass {
    let t = tier.clamp(0, cap as i32) as u8;
    ArmorClass::from_u8(t).unwrap_or(ArmorClass::Unarmored)
}

/// Fold the owner's completed techs into the base unit def as additive deltas.
/// Pure: same `(kind, mask)` → identical def. Predicate reads the BASE def.
pub fn effective_unit_def(kind: UnitKind, mask: u64) -> UnitDef {
    let base = *unit_def(kind);
    if mask == 0 {
        return base;
    }
    let mut out = base;
    for tech in techs_in_mask(mask) {
        let up = upgrade_def(tech);
        if !(up.applies_to)(&base) {
            continue;
        }
        out.attack += up.delta.attack;
        out.max_hp += up.delta.max_hp;
        out.range += up.delta.range;
        out.damage_reduction += up.delta.damage_reduction;
    }
    out
}

/// Fold structural techs (Masonry) into a base building def.
pub fn effective_building_def(kind: BuildingKind, mask: u64) -> BuildingDef {
    let base = *building_def(kind);
    if mask == 0 {
        return base;
    }
    let mut out = base;
    let mut tier = base.armor_class as i32;
    let mut changed = false;
    for tech in techs_in_mask(mask) {
        let up = upgrade_def(tech);
        let Some(d) = up.building_delta else { continue };
        if !up.applies_to_buildings {
            continue;
        }
        out.max_hp += d.max_hp;
        tier += d.armor_tier;
        changed = true;
    }
    if !changed {
        return base;
    }
    if tier != base.armor_class as i32 {
        out.armor_class = clamp_tier(tier, ArmorClass::Stone);
    }
    out
}

/// Health a building of `kind` gains when the owner's tech mask moves from
/// `before` to `after` — what a completed structural tech RETRO-APPLIES to
/// everything already standing.
pub fn building_hp_delta(before: u64, after: u64, kind: BuildingKind) -> i32 {
    effective_building_def(kind, after).max_hp - effective_building_def(kind, before).max_hp
}

// ── research panel (UI-facing, pure) ─────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResearchStatus {
    Done,
    InProgress,
    Locked,
    Unaffordable,
    Available,
}

/// Minimal shape of a research-table row the panel needs.
#[derive(Clone, Copy, Debug)]
pub struct ResearchProgressRow {
    pub tech: u8,
    pub progress: Fx,
    pub done: bool,
}

#[derive(Clone, Debug)]
pub struct ResearchRowState {
    pub tech: Tech,
    pub label: &'static str,
    pub icon: &'static str,
    pub cost: ResourceCost,
    pub status: ResearchStatus,
    pub progress: Fx,
    pub lock_note: Option<String>,
}

/// One descriptor per tech for the Blacksmith research panel. Status precedence:
/// done > in_progress > locked > unaffordable > available.
pub fn research_panel_state(
    mask: u64,
    rows: &[ResearchProgressRow],
    stock: &Stockpile,
    owned_buildings: &HashSet<BuildingKind>,
) -> Vec<ResearchRowState> {
    ALL_TECHS
        .iter()
        .copied()
        .map(|tech| {
            let up = upgrade_def(tech);
            let row = rows.iter().find(|r| r.tech == tech as u8);
            let mk = |status, progress, lock_note| ResearchRowState {
                tech,
                label: up.label,
                icon: up.icon,
                cost: up.cost,
                status,
                progress,
                lock_note,
            };

            if has_tech(mask, tech) {
                return mk(ResearchStatus::Done, Fx::ONE, None);
            }
            if let Some(r) = row {
                if !r.done {
                    let p = r.progress.clamp(Fx::ZERO, Fx::ONE);
                    return mk(ResearchStatus::InProgress, p, None);
                }
            }
            if let Some(req) = up.requires {
                if !has_prereq(owned_buildings, Some(req)) {
                    let note = format!("Requires {}", building_def(req).label);
                    return mk(ResearchStatus::Locked, Fx::ZERO, Some(note));
                }
            }
            if !stock.can_afford(&up.cost) {
                return mk(ResearchStatus::Unaffordable, Fx::ZERO, None);
            }
            mk(ResearchStatus::Available, Fx::ZERO, None)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_bits() {
        let m = set_tech(0, Tech::FletchedArrows);
        assert!(has_tech(m, Tech::FletchedArrows));
        assert!(!has_tech(m, Tech::ArmorMail));
        assert_eq!(techs_in_mask(set_tech(m, Tech::ArmorMail)), vec![Tech::ArmorMail, Tech::FletchedArrows]);
    }

    #[test]
    fn fletched_arrows_boosts_only_ranged() {
        let m = set_tech(0, Tech::FletchedArrows);
        let base = unit_def(UnitKind::Archer).attack;
        assert_eq!(effective_unit_def(UnitKind::Archer, m).attack, base + 3);
        assert_eq!(
            effective_unit_def(UnitKind::Spearman, m).attack,
            unit_def(UnitKind::Spearman).attack,
            "melee gains nothing from fletching"
        );
    }

    /// Armour research must never move a unit between DAMAGE_MATRIX columns:
    /// promoting every Leather unit to Mail deleted a whole column of the matrix
    /// mid-match and handed the anti-mail specialist a universal bonus exactly
    /// as the game progressed.
    #[test]
    fn mail_armor_soaks_damage_and_leaves_the_column_alone() {
        let m = set_tech(0, Tech::ArmorMail);
        for &k in UnitKind::ALL {
            assert_eq!(
                effective_unit_def(k, m).armor_class,
                unit_def(k).armor_class,
                "{k:?} changed armour column"
            );
        }
        assert_eq!(effective_unit_def(UnitKind::Archer, m).damage_reduction, 2);
        // engines and workers are not issued mail
        assert_eq!(effective_unit_def(UnitKind::Ram, m).damage_reduction, 0);
        assert_eq!(effective_unit_def(UnitKind::Peasant, m).damage_reduction, 0);
    }

    /// The shipped mis-issue: the Ram matched a SHAPE-derived `is_melee`, so a
    /// battering ram drew Sharpened Blades AND Plate Barding, while the Mangonel
    /// matched nothing at all and could never be upgraded.
    #[test]
    fn every_fighting_kind_gets_an_attack_upgrade_and_no_engine_gets_barding() {
        let all: u64 = ALL_TECHS.iter().fold(0, |m, t| set_tech(m, *t));
        for &k in UnitKind::ALL {
            let base = unit_def(k);
            if base.attack <= 0 {
                continue;
            }
            assert!(
                effective_unit_def(k, all).attack > base.attack,
                "{k:?} can never sharpen anything"
            );
        }
        let barding = upgrade_def(Tech::ArmorPlate);
        for &k in UnitKind::ALL {
            let d = unit_def(k);
            if d.role == crate::enums::UnitRole::Siege {
                assert!(!(barding.applies_to)(d), "{k:?} was issued horse armour");
                assert!(
                    !(upgrade_def(Tech::SharpenedBlades).applies_to)(d),
                    "{k:?} was issued a whetstone"
                );
            }
        }
        assert!((barding.applies_to)(unit_def(UnitKind::Knight)));
        assert!((upgrade_def(Tech::SiegeEngineering).applies_to)(unit_def(UnitKind::Mangonel)));
    }

    #[test]
    fn masonry_hardens_buildings_only() {
        let m = set_tech(0, Tech::Masonry);
        let keep = effective_building_def(BuildingKind::Keep, m);
        assert_eq!(keep.max_hp, 1750); // 1500 + 250
        assert_eq!(keep.armor_class, building_def(BuildingKind::Keep).armor_class);
        // unit unaffected
        assert_eq!(
            effective_unit_def(UnitKind::Spearman, m).max_hp,
            unit_def(UnitKind::Spearman).max_hp
        );
    }

    /// The old `+1 armor tier` was a measured NO-OP on every Stone kind and a
    /// NET DOWNGRADE on the Siege Workshop (Leather -> Mail moves it into a
    /// column siege hits HARDER). Masonry must help every structure.
    #[test]
    fn masonry_helps_every_structure() {
        use crate::combat::{Attacker, building_damage};
        let m = set_tech(0, Tech::Masonry);
        let ram = crate::units::unit_def(UnitKind::Ram);
        let atk = Attacker::new(Fx::from_num(ram.attack), ram.damage_type);
        for &k in BuildingKind::ALL {
            let hits = |d: &BuildingDef| {
                let per = building_damage(&atk, d);
                (d.max_hp + per - 1) / per
            };
            let before = hits(building_def(k));
            let after = hits(&effective_building_def(k, m));
            assert!(after > before, "masonry left {k:?} at {before} hits");
        }
        assert_eq!(building_hp_delta(0, m, BuildingKind::Keep), 250);
        assert_eq!(building_hp_delta(m, m, BuildingKind::Keep), 0);
    }

    #[test]
    fn panel_precedence() {
        let owned: HashSet<BuildingKind> = HashSet::new();
        let rich = Stockpile { wood: 999, stone: 999, food: 999, gold: 999 };
        let rows: Vec<ResearchProgressRow> = vec![];
        let panel = research_panel_state(0, &rows, &rich, &owned);
        // ArmorPlate requires Stable (not owned) -> Locked
        let plate = panel.iter().find(|r| r.tech == Tech::ArmorPlate).unwrap();
        assert_eq!(plate.status, ResearchStatus::Locked);
        // ArmorMail has no prereq, affordable -> Available
        let mail = panel.iter().find(|r| r.tech == Tech::ArmorMail).unwrap();
        assert_eq!(mail.status, ResearchStatus::Available);
    }
}
