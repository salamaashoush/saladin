use serde::{Deserialize, Serialize};

macro_rules! u8_enum {
    ($name:ident { $($(#[$vm:meta])* $variant:ident = $val:literal),+ $(,)? } default $def:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(u8)]
        pub enum $name {
            $($(#[$vm])* $variant = $val),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn from_u8(v: u8) -> Option<$name> {
                match v {
                    $($val => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl Default for $name {
            fn default() -> Self { $name::$def }
        }
    };
}

// Discriminants are load-bearing: `UNIT_DEFS[kind as usize]`, bincode's variant
// index, `Building.queue: [u8; QUEUE_CAP]` and `Census` all index by them.
// APPEND only — removing a kind renumbers every kind after it and turns every
// saved Knight into a Horse Archer.
u8_enum!(UnitKind {
    Peasant = 0,
    Spearman = 1,
    Archer = 2,
    Knight = 3,
    HorseArcher = 4,
    Mamluk = 5,
    Crossbowman = 6,
    Ram = 7,
    Mangonel = 8,
    Imam = 9,
    Sergeant = 10,
    Chaplain = 11,
    Naffatun = 12,
    FishingSkiff = 13,
    Barge = 14,
} default Peasant);

// What a unit is FOR. Every capability the upgrade table, the supply ledger and
// the tactical layer gate on reads this instead of re-deriving a role from the
// stat SHAPE — `attack > 0 && !ranged && range <= 2` also describes a Battering
// Ram, which is how a siege engine came to be issued plate barding.
u8_enum!(UnitRole {
    Worker = 0,
    Foot = 1,
    Archer = 2,
    Cavalry = 3,
    HorseArcher = 4,
    Siege = 5,
    Support = 6,
    /// A hull. The one role that moves in `Domain::Sea`, and the reason a hull
    /// is not simply a Worker with a boat mesh: it never fights, never eats,
    /// never garrisons, and cannot stand where the other six stand.
    Boat = 7,
} default Worker);

u8_enum!(DamageType {
    Slash = 0,
    Pierce = 1,
    Blunt = 2,
    Siege = 3,
} default Slash);

u8_enum!(ArmorClass {
    Unarmored = 0,
    Leather = 1,
    Mail = 2,
    Stone = 3,
} default Unarmored);

u8_enum!(BuildingKind {
    Keep = 0,
    Barracks = 1,
    Tower = 2,
    Wall = 3,
    Gatehouse = 4,
    House = 5,
    Stable = 6,
    Blacksmith = 7,
    Market = 8,
    Granary = 9,
    FishingHut = 10,
    SiegeWorkshop = 11,
    Watchtower = 12,
    Farm = 13,
    Storehouse = 14,
    Mosque = 15,
    Harbour = 16,
} default Keep);

u8_enum!(ResourceType {
    Wood = 0,
    Stone = 1,
    Food = 2,
    Gold = 3,
} default Wood);

u8_enum!(GatherState {
    Idle = 0,
    ToResource = 1,
    Harvesting = 2,
    ToStockpile = 3,
    /// Walking to, or working on, a construction/repair job site.
    Constructing = 4,
} default Idle);

u8_enum!(BuildState {
    /// Founded and paid for, not yet finished: a real, frail target that does
    /// nothing until a builder tops it out.
    Site = 0,
    Complete = 1,
    /// Standing and fully operational while it works toward `target_kind`.
    Upgrading = 2,
} default Complete);

u8_enum!(Faction {
    Ayyubid = 0,
    Crusader = 1,
} default Ayyubid);

u8_enum!(Stance {
    Aggressive = 0,
    Defensive = 1,
    HoldGround = 2,
} default Aggressive);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_u8_roundtrips() {
        for &k in UnitKind::ALL {
            assert_eq!(UnitKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(UnitKind::from_u8(250), None);
        assert_eq!(BuildingKind::from_u8(12), Some(BuildingKind::Watchtower));
    }

    #[test]
    fn defaults_match_zero() {
        assert_eq!(UnitKind::default() as u8, 0);
        assert_eq!(ResourceType::default() as u8, 0);
    }
}
