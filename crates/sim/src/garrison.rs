use crate::buildings_defs::BuildingDef;
use crate::math::Fx;
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

// ── the volley ───────────────────────────────────────────────────────────────
// `garrison_fire_power` SUMS every occupant's attack into ONE shot fired with
// the HOST's damage type and no specialist bonus at all. Five Archers in a Tower
// therefore became a single 62-damage host-typed blow that one-volleyed a 70 hp
// Spearman and routed whatever survived, and a garrisoned Crossbowman lost BOTH
// its Pierce and its 2.2x anti-mail — the reason a Crusader wants to stand still
// behind a wall was the exact thing garrisoning threw away.

/// One occupant, as it actually shoots.
#[derive(Clone, Copy, Debug)]
pub struct GarrisonShooter {
    pub attack: i32,
    pub ranged: bool,
    pub damage_type: crate::enums::DamageType,
    pub bonus_vs_armor: [Fx; 4],
}

impl GarrisonShooter {
    pub fn of(def: &UnitDef) -> Self {
        GarrisonShooter {
            attack: def.attack,
            ranged: def.ranged,
            damage_type: def.damage_type,
            bonus_vs_armor: def.bonus_vs_armor,
        }
    }
}

/// One thing being shot at, already ordered by the caller (nearest first,
/// `GameId` breaking ties, so peers agree).
#[derive(Clone, Copy, Debug)]
pub struct GarrisonTarget {
    pub armor: crate::enums::ArmorClass,
    pub damage_reduction: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonShot {
    /// Index into the caller's target slice.
    pub target: usize,
    pub damage: i32,
}

/// Resolve one volley: ONE shot per manning occupant, each with its OWN damage
/// type and specialist bonus, spread round-robin across the targets. Writes into
/// a caller-retained buffer so a tower firing every combat tick allocates
/// nothing.
pub fn garrison_volley(
    occupants: &[GarrisonShooter],
    host: &BuildingDef,
    targets: &[GarrisonTarget],
    out: &mut Vec<GarrisonShot>,
) {
    out.clear();
    let cap = host.garrison_cap;
    if cap <= 0 || targets.is_empty() {
        return;
    }
    let mut firing = 0;
    for o in occupants {
        if firing >= cap {
            break;
        }
        if !o.ranged || o.attack <= 0 {
            continue;
        }
        let idx = firing as usize % targets.len();
        let t = targets[idx];
        let atk = crate::combat::Attacker {
            attack: Fx::from_num(o.attack),
            damage_type: o.damage_type,
            bonus_vs_armor: o.bonus_vs_armor,
        };
        let dmg = (crate::combat::effective_damage(&atk, t.armor) - t.damage_reduction).max(1);
        out.push(GarrisonShot { target: idx, damage: dmg });
        firing += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings_defs::building_def;
    use crate::enums::{ArmorClass, BuildingKind, UnitKind};
    use crate::units::unit_def;

    fn shooters(kind: UnitKind, n: usize) -> Vec<GarrisonShooter> {
        (0..n).map(|_| GarrisonShooter::of(unit_def(kind))).collect()
    }

    /// The shipped bug, stated as a test: a garrisoned Crossbowman must still be
    /// a Crossbowman.
    #[test]
    fn a_garrisoned_crossbowman_is_still_a_crossbowman() {
        let tower = building_def(BuildingKind::Tower); // cap 5
        let occ = shooters(UnitKind::Crossbowman, 6);
        let knight = unit_def(UnitKind::Knight);
        let targets = [GarrisonTarget { armor: knight.armor_class, damage_reduction: 0 }];
        let mut out = Vec::new();
        garrison_volley(&occ, tower, &targets, &mut out);
        assert_eq!(out.len(), 5, "the sixth man has no firing slit");

        let x = unit_def(UnitKind::Crossbowman);
        let solo = crate::combat::Attacker {
            attack: Fx::from_num(x.attack),
            damage_type: x.damage_type,
            bonus_vs_armor: x.bonus_vs_armor,
        };
        let expected = crate::combat::effective_damage(&solo, knight.armor_class);
        for s in &out {
            assert_eq!(s.damage, expected, "the bolt lost its type or its bonus");
        }
        // and the anti-mail bonus is genuinely doing work
        let plain = crate::combat::Attacker::new(Fx::from_num(x.attack), x.damage_type);
        assert!(expected > crate::combat::effective_damage(&plain, ArmorClass::Mail));
    }

    /// One 62-damage host-typed blow one-volleyed a spearman. Five separate
    /// arrows kill the same man over several volleys, and can be spread.
    #[test]
    fn five_archers_fire_five_arrows_not_one_boulder() {
        let tower = building_def(BuildingKind::Tower);
        let occ = shooters(UnitKind::Archer, 5);
        let spear = unit_def(UnitKind::Spearman);
        let one = [GarrisonTarget { armor: spear.armor_class, damage_reduction: 0 }];
        let mut out = Vec::new();
        garrison_volley(&occ, tower, &one, &mut out);
        assert_eq!(out.len(), 5);
        let single: i32 = out[0].damage;
        assert!(single < spear.max_hp, "one arrow must not one-shot a spearman");
        assert!(out.iter().map(|s| s.damage).sum::<i32>() < spear.max_hp * 2);

        // three men in the open take the volley between them
        let three: Vec<GarrisonTarget> =
            (0..3).map(|_| GarrisonTarget { armor: spear.armor_class, damage_reduction: 0 }).collect();
        garrison_volley(&occ, tower, &three, &mut out);
        let hit_counts = [0, 1, 2].map(|i| out.iter().filter(|s| s.target == i).count());
        assert_eq!(hit_counts, [2, 2, 1], "the volley did not spread");
    }

    #[test]
    fn armour_soaks_a_garrison_arrow_and_nothing_fires_at_nothing() {
        let tower = building_def(BuildingKind::Tower);
        let occ = shooters(UnitKind::Archer, 2);
        let mut out = Vec::new();
        let soft = [GarrisonTarget { armor: ArmorClass::Leather, damage_reduction: 0 }];
        let mailed = [GarrisonTarget { armor: ArmorClass::Leather, damage_reduction: 3 }];
        garrison_volley(&occ, tower, &soft, &mut out);
        let bare = out[0].damage;
        garrison_volley(&occ, tower, &mailed, &mut out);
        assert_eq!(out[0].damage, bare - 3);
        garrison_volley(&occ, tower, &[], &mut out);
        assert!(out.is_empty());
        // a hall with no firing slits never fires, however full it is
        garrison_volley(&occ, building_def(BuildingKind::Barracks), &soft, &mut out);
        assert!(out.is_empty());
        // melee occupants take up room and shoot nothing
        garrison_volley(&shooters(UnitKind::Spearman, 3), tower, &soft, &mut out);
        assert!(out.is_empty());
    }

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
