use crate::buildings_defs::BuildingDef;
use crate::units::UnitDef;

/// Garrisoning posts a unit INSIDE a defensive structure: it leaves the field
/// (safe from melee/fire) and, if a ranged shooter, lends its fire to the host's
/// auto-volley. Data drives who may garrison and how many a structure holds.

pub fn can_garrison(def: &UnitDef) -> bool {
    def.garrisonable
}

pub fn can_host_garrison(host: &BuildingDef) -> bool {
    host.garrison_cap > 0
}

pub fn garrison_free_slots(host: &BuildingDef, occupants: i32) -> i32 {
    (host.garrison_cap - occupants.max(0)).max(0)
}

/// An occupant's contribution to its host's fire.
#[derive(Clone, Copy, Debug)]
pub struct GarrisonOccupant {
    pub attack: i32,
    pub ranged: bool,
}

/// Extra fire damage garrisoned shooters add to one volley. Only the first
/// `garrison_cap` ranged occupants man the firing slits, so a packed keep can't
/// fire infinitely; non-shooters occupy space but add nothing.
pub fn garrison_fire_power(occupants: &[GarrisonOccupant], host: &BuildingDef) -> i32 {
    let cap = host.garrison_cap;
    if cap <= 0 {
        return 0;
    }
    let mut total = 0;
    let mut firing = 0;
    for o in occupants {
        if !o.ranged || o.attack <= 0 {
            continue;
        }
        if firing >= cap {
            break;
        }
        total += o.attack;
        firing += 1;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings_defs::building_def;
    use crate::enums::{BuildingKind, UnitKind};
    use crate::units::unit_def;

    #[test]
    fn who_can_garrison() {
        assert!(can_garrison(unit_def(UnitKind::Archer)));
        assert!(!can_garrison(unit_def(UnitKind::Knight))); // cavalry can't
    }

    #[test]
    fn tower_fire_capped() {
        let tower = building_def(BuildingKind::Tower); // cap 5
        let archers: Vec<GarrisonOccupant> =
            (0..8).map(|_| GarrisonOccupant { attack: 9, ranged: true }).collect();
        assert_eq!(garrison_fire_power(&archers, tower), 45); // 5 * 9, capped
        // melee occupants add nothing
        let foot = [GarrisonOccupant { attack: 12, ranged: false }];
        assert_eq!(garrison_fire_power(&foot, tower), 0);
    }
}
