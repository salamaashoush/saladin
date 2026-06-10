use crate::math::Fx;

/// A map preset biases worldgen so the same generator yields recognizably
/// different land. `elev_gain` multiplies render relief (client only); the shift
/// fields nudge classification thresholds.
#[derive(Clone, Copy, Debug)]
pub struct MapBias {
    pub sea_shift: Fx,
    pub moist_shift: Fx,
    pub elev_gain: Fx,
}

#[derive(Clone, Copy, Debug)]
pub struct MapPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub bias: MapBias,
}

pub const NEUTRAL_BIAS: MapBias =
    MapBias { sea_shift: crate::fx!("0"), moist_shift: crate::fx!("0"), elev_gain: crate::fx!("1") };

pub const MAP_PRESETS: [MapPreset; 5] = [
    MapPreset {
        id: "continental",
        label: "Continental",
        description: "Balanced land with a fair mix of every biome.",
        bias: NEUTRAL_BIAS,
    },
    MapPreset {
        id: "verdant",
        label: "Verdant",
        description: "Wet, fertile country — broad grassland and deep forest.",
        bias: MapBias { sea_shift: crate::fx!("-0.02"), moist_shift: crate::fx!("0.16"), elev_gain: crate::fx!("0.9") },
    },
    MapPreset {
        id: "desert",
        label: "Arabian Desert",
        description: "Parched dunes and sand, sparse oases by the water.",
        bias: MapBias { sea_shift: crate::fx!("0"), moist_shift: crate::fx!("-0.2"), elev_gain: crate::fx!("0.85") },
    },
    MapPreset {
        id: "highlands",
        label: "Highlands",
        description: "Rugged uplands — towering hills, mountains, and snow.",
        bias: MapBias { sea_shift: crate::fx!("-0.03"), moist_shift: crate::fx!("0.02"), elev_gain: crate::fx!("1.45") },
    },
    MapPreset {
        id: "archipelago",
        label: "Archipelago",
        description: "A sea of scattered islands — control the straits.",
        bias: MapBias { sea_shift: crate::fx!("0.1"), moist_shift: crate::fx!("0.06"), elev_gain: crate::fx!("1.0") },
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
