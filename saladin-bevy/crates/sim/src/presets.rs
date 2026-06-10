use crate::math::Fx;

/// A map preset biases worldgen so the same generator yields recognizably
/// different land. `elev_gain` multiplies render relief (client only); the
/// shift fields nudge classification thresholds; `river_gain` scales river
/// width (0 = no rivers); `cliff_gain` scales how eagerly steep ground turns
/// into impassable cliff walls.
#[derive(Clone, Copy, Debug)]
pub struct MapBias {
    pub sea_shift: Fx,
    pub moist_shift: Fx,
    pub elev_gain: Fx,
    pub river_gain: Fx,
    pub cliff_gain: Fx,
    /// >0 multiplies the height field by a large-scale blob mask, shattering
    /// the single continent into islands (archipelago).
    pub island_gain: Fx,
}

#[derive(Clone, Copy, Debug)]
pub struct MapPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub bias: MapBias,
}

pub const NEUTRAL_BIAS: MapBias = MapBias {
    sea_shift: crate::fx!("0"),
    moist_shift: crate::fx!("0"),
    elev_gain: crate::fx!("1"),
    river_gain: crate::fx!("1"),
    cliff_gain: crate::fx!("1"),
    island_gain: crate::fx!("0"),
};

pub const MAP_PRESETS: [MapPreset; 4] = [
    MapPreset {
        id: "continental",
        label: "Continental",
        description: "Balanced land - every biome, winding rivers, the odd cliff.",
        bias: NEUTRAL_BIAS,
    },
    MapPreset {
        id: "river-valley",
        label: "River Valley",
        description: "Wet lowland split by broad rivers - fight for the fords.",
        bias: MapBias {
            sea_shift: crate::fx!("-0.02"),
            moist_shift: crate::fx!("0.14"),
            elev_gain: crate::fx!("0.9"),
            river_gain: crate::fx!("1.9"),
            cliff_gain: crate::fx!("0.6"),
            island_gain: crate::fx!("0"),
        },
    },
    MapPreset {
        id: "highlands",
        label: "Highlands",
        description: "Rugged uplands - cliff walls with scarce ramps command the field.",
        bias: MapBias {
            sea_shift: crate::fx!("-0.03"),
            moist_shift: crate::fx!("0.02"),
            elev_gain: crate::fx!("1.45"),
            river_gain: crate::fx!("0.7"),
            cliff_gain: crate::fx!("1.7"),
            island_gain: crate::fx!("0"),
        },
    },
    MapPreset {
        id: "archipelago",
        label: "Archipelago",
        description: "A sea of scattered islands - control the straits.",
        bias: MapBias {
            sea_shift: crate::fx!("0"),
            moist_shift: crate::fx!("0.06"),
            elev_gain: crate::fx!("1.0"),
            river_gain: crate::fx!("0.3"),
            cliff_gain: crate::fx!("0.8"),
            island_gain: crate::fx!("1"),
        },
    },
];

pub fn map_preset_by_id(id: &str) -> &'static MapPreset {
    MAP_PRESETS.iter().find(|p| p.id == id).unwrap_or(&MAP_PRESETS[0])
}

pub fn map_preset_by_index(index: i32) -> &'static MapPreset {
    let n = MAP_PRESETS.len() as i32;
    let i = ((index % n) + n) % n;
    &MAP_PRESETS[i as usize]
}

pub fn bias_of(preset_id: &str) -> MapBias {
    map_preset_by_id(preset_id).bias
}
