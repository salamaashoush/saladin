use crate::buildings_defs::{BUILD_CATEGORIES, BuildingDef, building_def};
use crate::economy::{ResourceCost, Stockpile};
use crate::enums::BuildingKind;
use crate::math::Fx;
use crate::tech::all_prereqs;
use std::collections::HashSet;

/// Why a build card is or is not clickable. Precedence: locked > at-limit >
/// unaffordable > available — a card you cannot unlock yet says so first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildStatus {
    Available,
    Unaffordable,
    Locked { missing: Vec<BuildingKind> },
    AtLimit,
}

/// One build-bar card. The COST rides on every row, locked or not: a player has
/// to be able to learn what a Siege Workshop costs before he can build one.
#[derive(Clone, Debug)]
pub struct BuildRowState {
    pub kind: BuildingKind,
    pub label: &'static str,
    pub icon: &'static str,
    pub cost: ResourceCost,
    pub build_time: Fx,
    pub status: BuildStatus,
    pub note: Option<String>,
}

fn label_list(kinds: &[BuildingKind]) -> String {
    let names: Vec<&str> = kinds.iter().map(|k| building_def(*k).label).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].to_string(),
        _ => format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
    }
}

fn row(kind: BuildingKind, def: &BuildingDef, status: BuildStatus, note: Option<String>) -> BuildRowState {
    BuildRowState {
        kind,
        label: def.label,
        icon: def.icon,
        cost: def.cost,
        build_time: def.build_time,
        status,
        note,
    }
}

/// Status of ONE build card, for a player owning `owned` with `counts` of each
/// kind standing and `stock` in the bank.
pub fn build_row_state(
    kind: BuildingKind,
    owned: &HashSet<BuildingKind>,
    counts: &[i32],
    stock: &Stockpile,
) -> BuildRowState {
    let def = building_def(kind);
    let missing: Vec<BuildingKind> =
        all_prereqs(def).into_iter().filter(|k| !owned.contains(k)).collect();
    if !missing.is_empty() {
        let note = format!("Needs {}", label_list(&missing));
        return row(kind, def, BuildStatus::Locked { missing }, Some(note));
    }
    if def.max_count > 0 && counts.get(kind as usize).copied().unwrap_or(0) >= def.max_count {
        let note = if def.max_count == 1 {
            "One per town".to_string()
        } else {
            format!("Limit {}", def.max_count)
        };
        return row(kind, def, BuildStatus::AtLimit, Some(note));
    }
    if !stock.can_afford(&def.cost) {
        return row(kind, def, BuildStatus::Unaffordable, None);
    }
    row(kind, def, BuildStatus::Available, None)
}

/// Every card on build-bar tab `tab`, in bar order.
pub fn build_panel_state(
    tab: usize,
    owned: &HashSet<BuildingKind>,
    counts: &[i32],
    stock: &Stockpile,
) -> Vec<BuildRowState> {
    let cat = &BUILD_CATEGORIES[tab.min(BUILD_CATEGORIES.len() - 1)];
    cat.kinds.iter().map(|k| build_row_state(*k, owned, counts, stock)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich() -> Stockpile {
        Stockpile { wood: 999, stone: 999, food: 999, gold: 999 }
    }

    #[test]
    fn a_locked_card_still_states_its_price() {
        let owned: HashSet<BuildingKind> = HashSet::new();
        let r = build_row_state(BuildingKind::SiegeWorkshop, &owned, &[], &rich());
        assert!(matches!(r.status, BuildStatus::Locked { .. }));
        assert!(r.cost.wood > 0 && r.build_time > Fx::ZERO, "a locked card is still a price tag");
        let BuildStatus::Locked { missing } = &r.status else { unreachable!() };
        assert_eq!(missing.len(), 2, "the note lists the FULL prereq set");
        let note = r.note.unwrap();
        assert!(note.is_ascii() && note.contains("Blacksmith") && note.contains("Barracks"), "{note}");
    }

    #[test]
    fn status_precedence_runs_locked_limit_afford_available() {
        let mut owned: HashSet<BuildingKind> = HashSet::new();
        owned.insert(BuildingKind::Keep);
        let broke = Stockpile::default();
        // locked beats broke
        assert!(matches!(
            build_row_state(BuildingKind::Stable, &owned, &[], &broke).status,
            BuildStatus::Locked { .. }
        ));
        // unlocked but broke
        assert_eq!(
            build_row_state(BuildingKind::House, &owned, &[], &broke).status,
            BuildStatus::Unaffordable
        );
        assert_eq!(
            build_row_state(BuildingKind::House, &owned, &[], &rich()).status,
            BuildStatus::Available
        );
        // the one-per-town structure
        owned.insert(BuildingKind::Barracks);
        let mut counts = [0i32; 16];
        counts[BuildingKind::Mosque as usize] = 1;
        assert_eq!(
            build_row_state(BuildingKind::Mosque, &owned, &counts, &rich()).status,
            BuildStatus::AtLimit
        );
    }

    #[test]
    fn every_tab_renders_and_every_note_is_ascii() {
        let owned: HashSet<BuildingKind> = HashSet::new();
        for (t, cat) in BUILD_CATEGORIES.iter().enumerate() {
            let rows = build_panel_state(t, &owned, &[], &rich());
            assert_eq!(rows.len(), cat.kinds.len());
            for r in rows {
                assert!(r.label.is_ascii(), "{} label", r.label);
                if let Some(n) = r.note {
                    assert!(n.is_ascii(), "{n}");
                }
            }
        }
        // an out-of-range tab clamps rather than panicking on a stale UI index
        assert!(!build_panel_state(99, &owned, &[], &rich()).is_empty());
    }
}
