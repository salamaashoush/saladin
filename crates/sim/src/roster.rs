use crate::buildings_defs::building_def;
use crate::enums::{BuildingKind, Faction, UnitKind};
use crate::units::unit_def;

// Faction exclusivity is a FILTER over one shared roster, never a deletion:
// `UnitDef.factions` is a bitmask, `BuildingDef.trains` stays index-stable, and
// a Crusader Mamluk saved before this rule still decodes — it simply cannot be
// trained again.

pub const FACTION_AYYUBID: u8 = 1 << (Faction::Ayyubid as u8);
pub const FACTION_CRUSADER: u8 = 1 << (Faction::Crusader as u8);
pub const FACTION_BOTH: u8 = FACTION_AYYUBID | FACTION_CRUSADER;

pub const fn faction_bit(f: Faction) -> u8 {
    1 << (f as u8)
}

pub fn fields_unit(kind: UnitKind, faction: Faction) -> bool {
    unit_def(kind).factions & faction_bit(faction) != 0
}

/// What `building` actually offers `faction`. Allocation-free for the common
/// path is not worth it here: this feeds a command card and the planner's
/// once-a-second scan, never a per-tick loop.
pub fn roster_for(building: BuildingKind, faction: Faction) -> Vec<UnitKind> {
    building_def(building)
        .trains
        .iter()
        .copied()
        .filter(|k| fields_unit(*k, faction))
        .collect()
}

/// Every kind a faction can ever put in the field.
pub fn faction_roster(faction: Faction) -> Vec<UnitKind> {
    UnitKind::ALL.iter().copied().filter(|k| fields_unit(*k, faction)).collect()
}

/// The training hall a kind comes out of.
pub fn trainer_of(kind: UnitKind) -> Option<BuildingKind> {
    BuildingKind::ALL.iter().copied().find(|b| building_def(*b).trains.contains(&kind))
}

/// One faith hall, two liturgies. The structure is index-stable (renaming a
/// `BuildingKind` per faction would renumber saves); only what it is CALLED
/// changes with who raised it.
pub fn hall_label(kind: BuildingKind, faction: Faction) -> &'static str {
    match (kind, faction) {
        (BuildingKind::Mosque, Faction::Crusader) => "Chapel",
        _ => building_def(kind).label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ayyubid_cannot_train_a_knight_and_a_crusader_cannot_train_a_mamluk() {
        let stable_ayy = roster_for(BuildingKind::Stable, Faction::Ayyubid);
        let stable_cru = roster_for(BuildingKind::Stable, Faction::Crusader);
        assert!(!stable_ayy.contains(&UnitKind::Knight));
        assert!(stable_ayy.contains(&UnitKind::Mamluk));
        assert!(stable_cru.contains(&UnitKind::Knight));
        assert!(!stable_cru.contains(&UnitKind::Mamluk));
        assert!(!stable_cru.contains(&UnitKind::HorseArcher));
    }

    /// The shared spine is deliberate: peasants, levy spears and the siege
    /// train keep the early ladder identical, so no economy or build test needs
    /// a per-faction branch.
    #[test]
    fn both_sides_share_the_spine_and_split_everywhere_else() {
        for k in [UnitKind::Peasant, UnitKind::Spearman, UnitKind::Ram, UnitKind::Mangonel] {
            assert!(fields_unit(k, Faction::Ayyubid) && fields_unit(k, Faction::Crusader), "{k:?}");
        }
        let ayy = faction_roster(Faction::Ayyubid);
        let cru = faction_roster(Faction::Crusader);
        assert!(ayy.len() >= 8 && cru.len() >= 7, "{} / {}", ayy.len(), cru.len());
        for &k in UnitKind::ALL {
            assert!(
                fields_unit(k, Faction::Ayyubid) || fields_unit(k, Faction::Crusader),
                "{k:?} belongs to nobody"
            );
        }
        // asymmetry, not a mirror
        assert_ne!(ayy, cru);
    }

    /// Every kind has to be buyable somewhere, or it is dead data.
    #[test]
    fn every_kind_is_sold_by_a_hall_that_its_faction_can_raise() {
        for &k in UnitKind::ALL {
            let hall = trainer_of(k).unwrap_or_else(|| panic!("{k:?} has no trainer"));
            for f in [Faction::Ayyubid, Faction::Crusader] {
                if fields_unit(k, f) {
                    assert!(roster_for(hall, f).contains(&k), "{k:?} missing from {hall:?}");
                }
            }
        }
    }

    #[test]
    fn no_hall_is_empty_for_either_side() {
        for &b in BuildingKind::ALL {
            if building_def(b).trains.is_empty() {
                continue;
            }
            for f in [Faction::Ayyubid, Faction::Crusader] {
                assert!(!roster_for(b, f).is_empty(), "{b:?} sells nothing to {f:?}");
            }
        }
    }

    #[test]
    fn the_faith_hall_is_named_for_who_raised_it() {
        assert_eq!(hall_label(BuildingKind::Mosque, Faction::Ayyubid), "Mosque");
        assert_eq!(hall_label(BuildingKind::Mosque, Faction::Crusader), "Chapel");
        assert_eq!(hall_label(BuildingKind::Keep, Faction::Crusader), "Keep");
    }
}
