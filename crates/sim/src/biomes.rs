use crate::math::Fx;
use serde::{Deserialize, Serialize};

/// One row per biome: render colors, gameplay flags, movement cost, decoration,
/// and per-resource spawn densities. Single source of truth — `terrain.rs`
/// classifies a tile into a `Biome` and reads everything else from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Biome {
    DeepWater = 0,
    ShallowWater = 1,
    Sand = 2,
    Desert = 3,
    Dunes = 4,
    Steppe = 5,
    Grassland = 6,
    Forest = 7,
    Hills = 8,
    Mountain = 9,
    Snow = 10,
    Oasis = 11,
    River = 12,
    Ford = 13,
    Cliff = 14,
}

/// Cosmetic prop kinds the client scatters per biome (client-only dressing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Decoration {
    None = 0,
    Shrub = 1,
    Palm = 2,
    Rock = 3,
    DuneGrass = 4,
    PineCluster = 5,
    Boulder = 6,
    Reeds = 7,
}

#[derive(Clone, Copy, Debug)]
pub struct Density {
    pub tree: Fx,
    pub rock: Fx,
    pub game: Fx,
    pub fish: Fx,
    pub gold: Fx,
}

const Z: Fx = crate::fx!("0");

#[derive(Clone, Copy, Debug)]
pub struct DecoSpec {
    pub kind: Decoration,
    pub density: Fx,
}

#[derive(Clone, Copy, Debug)]
pub struct BiomeDef {
    pub label: &'static str,
    pub color: u32,
    pub shade: u32,
    pub passable: bool,
    pub buildable: bool,
    /// Movement/pathing multiplier; `Fx::MAX` stands in for the impassable ∞.
    pub move_cost_mul: Fx,
    pub height_emphasis: Fx,
    pub decoration: DecoSpec,
    pub density: Density,
}

const fn d(tree: Fx, rock: Fx, game: Fx, fish: Fx, gold: Fx) -> Density {
    Density { tree, rock, game, fish, gold }
}
const NONE_DENS: Density = d(Z, Z, Z, Z, Z);

const BIOME_DEFS: [BiomeDef; 15] = [
    // DeepWater
    BiomeDef {
        label: "Sea",
        color: 0x4093ad,
        shade: 0x357f9a,
        passable: false,
        buildable: false,
        move_cost_mul: Fx::MAX,
        height_emphasis: crate::fx!("1"),
        decoration: DecoSpec { kind: Decoration::None, density: Z },
        density: NONE_DENS,
    },
    // ShallowWater
    BiomeDef {
        label: "Shallows",
        color: 0x5fb0c6,
        shade: 0x4a9cb4,
        passable: false,
        buildable: false,
        move_cost_mul: Fx::MAX,
        height_emphasis: crate::fx!("1"),
        decoration: DecoSpec { kind: Decoration::Reeds, density: crate::fx!("0.05") },
        density: NONE_DENS,
    },
    // Sand
    BiomeDef {
        label: "Coast",
        color: 0xe2cf96,
        shade: 0xc9b377,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.1"),
        height_emphasis: crate::fx!("0.6"),
        decoration: DecoSpec { kind: Decoration::DuneGrass, density: crate::fx!("0.1") },
        density: d(Z, Z, Z, crate::fx!("0.6"), Z),
    },
    // Desert
    BiomeDef {
        label: "Desert",
        color: 0xdcb866,
        shade: 0xc09a48,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.2"),
        height_emphasis: crate::fx!("0.7"),
        decoration: DecoSpec { kind: Decoration::Shrub, density: crate::fx!("0.05") },
        density: NONE_DENS,
    },
    // Dunes
    BiomeDef {
        label: "Dunes",
        color: 0xcaa257,
        shade: 0xa9853f,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.5"),
        height_emphasis: crate::fx!("1.2"),
        decoration: DecoSpec { kind: Decoration::DuneGrass, density: crate::fx!("0.12") },
        density: NONE_DENS,
    },
    // Steppe
    BiomeDef {
        label: "Steppe",
        color: 0xb3ad6b,
        shade: 0x938c4f,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.0"),
        height_emphasis: crate::fx!("0.9"),
        decoration: DecoSpec { kind: Decoration::Shrub, density: crate::fx!("0.1") },
        density: d(crate::fx!("0.06"), crate::fx!("0.12"), crate::fx!("0.28"), crate::fx!("0.1"), Z),
    },
    // Grassland
    BiomeDef {
        label: "Grassland",
        color: 0x77a64a,
        shade: 0x577f33,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.0"),
        height_emphasis: crate::fx!("0.9"),
        decoration: DecoSpec { kind: Decoration::Shrub, density: crate::fx!("0.07") },
        density: d(crate::fx!("0.32"), crate::fx!("0.05"), crate::fx!("0.4"), crate::fx!("0.15"), Z),
    },
    // Forest
    BiomeDef {
        label: "Forest",
        color: 0x3f7d38,
        shade: 0x285626,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.3"),
        height_emphasis: crate::fx!("1.0"),
        decoration: DecoSpec { kind: Decoration::PineCluster, density: crate::fx!("0.22") },
        density: d(crate::fx!("0.85"), Z, crate::fx!("0.12"), Z, Z),
    },
    // Hills
    BiomeDef {
        label: "Hills",
        color: 0x8f7d54,
        shade: 0x6b5d3c,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.4"),
        height_emphasis: crate::fx!("1.6"),
        decoration: DecoSpec { kind: Decoration::Rock, density: crate::fx!("0.16") },
        density: d(Z, crate::fx!("0.55"), Z, Z, crate::fx!("0.18")),
    },
    // Mountain
    BiomeDef {
        label: "Mountain",
        color: 0x7c7167,
        shade: 0x564e47,
        passable: false,
        buildable: false,
        move_cost_mul: Fx::MAX,
        height_emphasis: crate::fx!("2.4"),
        decoration: DecoSpec { kind: Decoration::Boulder, density: crate::fx!("0.14") },
        density: d(Z, crate::fx!("0.4"), Z, Z, crate::fx!("0.35")),
    },
    // Snow
    BiomeDef {
        label: "Snow",
        color: 0xeef2f5,
        shade: 0xc7d3dc,
        passable: false,
        buildable: false,
        move_cost_mul: Fx::MAX,
        height_emphasis: crate::fx!("2.6"),
        decoration: DecoSpec { kind: Decoration::Boulder, density: crate::fx!("0.08") },
        density: NONE_DENS,
    },
    // Oasis
    BiomeDef {
        label: "Oasis",
        color: 0x4f9d6a,
        shade: 0x357a4c,
        passable: true,
        buildable: true,
        move_cost_mul: crate::fx!("1.0"),
        height_emphasis: crate::fx!("0.7"),
        decoration: DecoSpec { kind: Decoration::Palm, density: crate::fx!("0.3") },
        density: d(crate::fx!("0.45"), Z, crate::fx!("0.35"), crate::fx!("0.2"), Z),
    },
    // River — carved freshwater channel; impassable except at fords
    BiomeDef {
        label: "River",
        color: 0x55a8c4,
        shade: 0x4093ad,
        passable: false,
        buildable: false,
        move_cost_mul: Fx::MAX,
        height_emphasis: crate::fx!("1"),
        decoration: DecoSpec { kind: Decoration::Reeds, density: crate::fx!("0.12") },
        density: d(Z, Z, Z, crate::fx!("0.5"), Z),
    },
    // Ford — shallow river crossing: walkable, slow, never buildable (keeps the
    // chokepoint a chokepoint instead of a tower platform)
    BiomeDef {
        label: "Ford",
        color: 0x9ec1a8,
        shade: 0x7ba287,
        passable: true,
        buildable: false,
        move_cost_mul: crate::fx!("1.6"),
        height_emphasis: crate::fx!("0.5"),
        decoration: DecoSpec { kind: Decoration::Reeds, density: crate::fx!("0.08") },
        density: NONE_DENS,
    },
    // Cliff — an elevation step too steep to walk; ramps interrupt it
    BiomeDef {
        label: "Cliff",
        color: 0x6e6258,
        shade: 0x4a423b,
        passable: false,
        buildable: false,
        move_cost_mul: Fx::MAX,
        height_emphasis: crate::fx!("2.2"),
        decoration: DecoSpec { kind: Decoration::Boulder, density: crate::fx!("0.2") },
        density: d(Z, crate::fx!("0.3"), Z, Z, crate::fx!("0.2")),
    },
];

pub fn biome_def(b: Biome) -> &'static BiomeDef {
    &BIOME_DEFS[b as usize]
}

pub fn biome_passable(b: Biome) -> bool {
    biome_def(b).passable
}
pub fn biome_buildable(b: Biome) -> bool {
    biome_def(b).buildable
}
pub fn move_cost_mul(b: Biome) -> Fx {
    biome_def(b).move_cost_mul
}
pub fn biome_height_emphasis(b: Biome) -> Fx {
    biome_def(b).height_emphasis
}
pub fn biome_decoration(b: Biome) -> DecoSpec {
    biome_def(b).decoration
}

pub fn tree_density(b: Biome) -> Fx {
    biome_def(b).density.tree
}
pub fn rock_density(b: Biome) -> Fx {
    biome_def(b).density.rock
}
pub fn game_density(b: Biome) -> Fx {
    biome_def(b).density.game
}
pub fn fish_density(b: Biome) -> Fx {
    biome_def(b).density.fish
}
pub fn gold_density(b: Biome) -> Fx {
    biome_def(b).density.gold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_is_impassable() {
        assert!(!biome_passable(Biome::DeepWater));
        assert!(!biome_passable(Biome::Mountain));
        assert!(biome_passable(Biome::Grassland));
    }

    #[test]
    fn forest_is_treeful() {
        assert_eq!(tree_density(Biome::Forest), crate::fx!("0.85"));
        assert_eq!(tree_density(Biome::Desert), crate::fx!("0"));
    }
}
