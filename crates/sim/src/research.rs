use crate::buildings_defs::{BuildingDef, building_def};
use crate::economy::{ResourceCost, Stockpile};
use crate::enums::{ArmorClass, BuildingKind};
use crate::math::Fx;
use crate::tech::has_prereq;
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
}

pub const ALL_TECHS: [Tech; 6] = [
    Tech::ArmorMail,
    Tech::ArmorPlate,
    Tech::FletchedArrows,
    Tech::SharpenedBlades,
    Tech::Masonry,
    Tech::Conscription,
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
    pub armor_tier: i32,
}

const NO_DELTA: UnitDelta = UnitDelta { attack: 0, max_hp: 0, range: crate::fx!("0"), armor_tier: 0 };

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

fn is_combatant(d: &UnitDef) -> bool {
    d.attack > 0
}
fn is_ranged(d: &UnitDef) -> bool {
    d.ranged
}
fn is_melee(d: &UnitDef) -> bool {
    d.attack > 0 && !d.ranged && d.range <= crate::fx!("2")
}
fn troops_not_siege(d: &UnitDef) -> bool {
    is_combatant(d) && !d.prefers_buildings
}
fn never(_: &UnitDef) -> bool {
    false
}

const UPGRADE_DEFS: [UpgradeDef; 6] = [
    // ArmorMail
    UpgradeDef {
        label: "Mail Armor",
        icon: "🥼",
        cost: ResourceCost::new(60, 0, 0, 40),
        research_time: crate::fx!("30"),
        requires: None,
        applies_to: troops_not_siege,
        delta: UnitDelta { armor_tier: 1, ..NO_DELTA },
        building_delta: None,
        applies_to_buildings: false,
    },
    // ArmorPlate
    UpgradeDef {
        label: "Plate Barding",
        icon: "🛡️",
        cost: ResourceCost::new(40, 30, 0, 60),
        research_time: crate::fx!("45"),
        requires: Some(BuildingKind::Stable),
        applies_to: is_melee,
        delta: UnitDelta { max_hp: 25, ..NO_DELTA },
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
        building_delta: Some(BuildingDelta { max_hp: 150, armor_tier: 1 }),
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
    let mut tier = base.armor_class as i32;
    let mut changed = false;
    for tech in techs_in_mask(mask) {
        let up = upgrade_def(tech);
        if !(up.applies_to)(&base) {
            continue;
        }
        out.attack += up.delta.attack;
        out.max_hp += up.delta.max_hp;
        out.range += up.delta.range;
        tier += up.delta.armor_tier;
        changed = true;
    }
    if !changed {
        return base;
    }
    out.armor_class = clamp_tier(tier, ArmorClass::Mail);
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
    out.armor_class = clamp_tier(tier, ArmorClass::Stone);
    out
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
        assert_eq!(effective_unit_def(UnitKind::Archer, m).attack, 12); // 9 + 3
        assert_eq!(effective_unit_def(UnitKind::Spearman, m).attack, 12); // unchanged (melee)
    }

    #[test]
    fn mail_armor_bumps_tier_clamped() {
        let m = set_tech(0, Tech::ArmorMail);
        // Archer Leather(1) -> Mail(2)
        assert_eq!(effective_unit_def(UnitKind::Archer, m).armor_class, ArmorClass::Mail);
        // Knight already Mail(2) -> clamp keeps Mail
        assert_eq!(effective_unit_def(UnitKind::Knight, m).armor_class, ArmorClass::Mail);
    }

    #[test]
    fn masonry_hardens_buildings_only() {
        let m = set_tech(0, Tech::Masonry);
        let keep = effective_building_def(BuildingKind::Keep, m);
        assert_eq!(keep.max_hp, 1650); // 1500 + 150
        // unit unaffected
        assert_eq!(effective_unit_def(UnitKind::Spearman, m).max_hp, 70);
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
