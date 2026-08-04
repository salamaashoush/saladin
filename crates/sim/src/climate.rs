//! Climate model — the second axis every biome needs.
//!
//! Temperature comes from latitude plus an elevation lapse rate; precipitation
//! comes from an advected moisture parcel (orographic rain, real rain shadows)
//! blended with the planetary pressure belts. Biomes then fall out of a
//! Whittaker lookup on (temperature x precipitation) instead of a single
//! moisture ramp, which is why a map can hold desert, steppe, olive groves,
//! cedar forest and alpine tundra at once.
//!
//! Each seed also draws a CLIMATE ARCHETYPE — a latitude window and humidity
//! budget modelled on a real theatre of the Crusades — so seeds differ in kind,
//! not just in noise.

use crate::biomes::Biome;
use crate::constants::WORLD_SIZE;
use crate::math::{Fx, spline};
use crate::noise::fbm;
use crate::rng::hash2_u32;

/// A named climate regime. `lat_center` is the latitude at the middle of the
/// map and `lat_span` how many degrees the map covers north to south, which is
/// what makes one seed a desert frontier and the next a rainy upland.
#[derive(Clone, Copy, Debug)]
pub struct ClimateArchetype {
    pub id: &'static str,
    pub label: &'static str,
    pub blurb: &'static str,
    pub lat_center: Fx,
    pub lat_span: Fx,
    /// Humidity an air parcel carries off the sea, 0..1.
    pub humidity: Fx,
    /// Extra drying applied over land — continental interiors and hot regimes.
    pub aridity: Fx,
    /// Mean precipitation the regime should land on, 0..1. The parcel sweep
    /// supplies the SHAPE (rain shadows, coastal gradients); this pins the
    /// LEVEL, so an archetype reads the same whatever terrain a seed rolls.
    pub target_precip: Fx,
    /// Normalized elevation above which trees give out.
    pub tree_line: Fx,
}

pub const CLIMATES: [ClimateArchetype; 8] = [
    ClimateArchetype {
        id: "levant",
        label: "Levantine Coast",
        blurb: "Wet seaboard, parched interior - the road from Acre to Damascus.",
        lat_center: crate::fx!("33"),
        lat_span: crate::fx!("7"),
        humidity: crate::fx!("0.86"),
        aridity: crate::fx!("0.07"),
        target_precip: crate::fx!("0.46"),
        tree_line: crate::fx!("0.80"),
    },
    ClimateArchetype {
        id: "crescent",
        label: "Fertile Crescent",
        blurb: "Great rivers through dry plain - the floodplain feeds armies.",
        lat_center: crate::fx!("35"),
        lat_span: crate::fx!("6"),
        humidity: crate::fx!("0.74"),
        aridity: crate::fx!("0.15"),
        target_precip: crate::fx!("0.32"),
        tree_line: crate::fx!("0.78"),
    },
    ClimateArchetype {
        id: "arabia",
        label: "Arabian Frontier",
        blurb: "Sand seas and stone desert; oases are worth a war.",
        lat_center: crate::fx!("23"),
        lat_span: crate::fx!("9"),
        humidity: crate::fx!("0.52"),
        aridity: crate::fx!("0.32"),
        target_precip: crate::fx!("0.13"),
        tree_line: crate::fx!("0.86"),
    },
    ClimateArchetype {
        id: "anatolia",
        label: "Anatolian Upland",
        blurb: "Cold high plateau ringed by cedar and snow.",
        lat_center: crate::fx!("39"),
        lat_span: crate::fx!("6"),
        humidity: crate::fx!("0.80"),
        aridity: crate::fx!("0.12"),
        target_precip: crate::fx!("0.47"),
        tree_line: crate::fx!("0.72"),
    },
    ClimateArchetype {
        id: "nile",
        label: "River of Egypt",
        blurb: "One green artery through the waste - farm it or starve.",
        lat_center: crate::fx!("25"),
        lat_span: crate::fx!("11"),
        humidity: crate::fx!("0.58"),
        aridity: crate::fx!("0.28"),
        target_precip: crate::fx!("0.24"),
        tree_line: crate::fx!("0.84"),
    },
    ClimateArchetype {
        id: "maghreb",
        label: "Maghreb Shore",
        blurb: "Green coastal range, then steppe falling away to sand.",
        lat_center: crate::fx!("34"),
        lat_span: crate::fx!("10"),
        humidity: crate::fx!("0.83"),
        aridity: crate::fx!("0.17"),
        target_precip: crate::fx!("0.37"),
        tree_line: crate::fx!("0.78"),
    },
    ClimateArchetype {
        id: "aegean",
        label: "Aegean Reach",
        blurb: "Mild sea air, olive terraces and scrub on every headland.",
        lat_center: crate::fx!("38"),
        lat_span: crate::fx!("5"),
        humidity: crate::fx!("0.90"),
        aridity: crate::fx!("0.06"),
        target_precip: crate::fx!("0.52"),
        tree_line: crate::fx!("0.76"),
    },
    ClimateArchetype {
        id: "caucasus",
        label: "Northern Marches",
        blurb: "Cold rain, deep forest, snowfields over every pass.",
        lat_center: crate::fx!("44"),
        lat_span: crate::fx!("6"),
        humidity: crate::fx!("0.90"),
        aridity: crate::fx!("0.03"),
        target_precip: crate::fx!("0.63"),
        tree_line: crate::fx!("0.68"),
    },
];

/// The climate regime a seed rolls. Independent of the map preset: the preset
/// shapes the GEOGRAPHY (islands, rivers, cliffs), the archetype the WEATHER.
pub fn climate_archetype(seed: u32) -> &'static ClimateArchetype {
    let base = crate::terrain::seed_base(seed);
    &CLIMATES[(hash2_u32(0x51ed, 0x0c1, base) % CLIMATES.len() as u32) as usize]
}

// Annual mean temperature by latitude, normalized 0 (polar) .. 1 (equatorial).
const SPL_LAT_TEMP: &[(Fx, Fx)] = &[
    (crate::fx!("0"), crate::fx!("1.0")),
    (crate::fx!("15"), crate::fx!("0.93")),
    (crate::fx!("25"), crate::fx!("0.82")),
    (crate::fx!("32"), crate::fx!("0.70")),
    (crate::fx!("38"), crate::fx!("0.58")),
    (crate::fx!("45"), crate::fx!("0.44")),
    (crate::fx!("52"), crate::fx!("0.30")),
    (crate::fx!("65"), crate::fx!("0.10")),
];

// Planetary pressure belts: ITCZ rain at the equator, the subtropical ridge
// (the world's desert belt) around 25-30 degrees, westerly rain past 40.
const SPL_LAT_RAIN: &[(Fx, Fx)] = &[
    (crate::fx!("0"), crate::fx!("0.92")),
    (crate::fx!("10"), crate::fx!("0.70")),
    (crate::fx!("18"), crate::fx!("0.34")),
    (crate::fx!("26"), crate::fx!("0.12")),
    (crate::fx!("31"), crate::fx!("0.22")),
    (crate::fx!("37"), crate::fx!("0.48")),
    (crate::fx!("45"), crate::fx!("0.70")),
    (crate::fx!("58"), crate::fx!("0.76")),
];

/// Temperature falls this much (normalized) from sea level to the map's peak.
const LAPSE: Fx = crate::fx!("1.05");

/// Latitude at tile row `ty` under `c` (north is row 0, so latitude falls south).
pub fn latitude_at(c: &ClimateArchetype, ty: usize) -> Fx {
    let t = Fx::from_num(ty as i32) / Fx::from_num(WORLD_SIZE);
    c.lat_center + (crate::fx!("0.5") - t) * c.lat_span
}

/// Sea-level temperature for a row, 0..1.
pub fn sea_level_temp(c: &ClimateArchetype, ty: usize) -> Fx {
    spline(SPL_LAT_TEMP, latitude_at(c, ty))
}

/// Elevation where snow lies year round, in normalized height. Hot latitudes
/// push it above the peaks; cold ones drop it onto the shoulders.
pub fn snow_line(c: &ClimateArchetype, ty: usize) -> Fx {
    crate::fx!("0.78") + sea_level_temp(c, ty) * crate::fx!("0.17")
}

/// Dryness of a row before the terrain-aware pass exists — enough to decide
/// whether a closed basin fills with water or evaporates into a salt pan
/// (that decision has to be made while the surface is still being carved).
pub fn coarse_dryness(c: &ClimateArchetype, ty: usize) -> Fx {
    let belt = spline(SPL_LAT_RAIN, latitude_at(c, ty));
    ((Fx::ONE - belt) * crate::fx!("0.6") + c.aridity - c.humidity * crate::fx!("0.25"))
        .clamp(Fx::ZERO, Fx::ONE)
}

pub struct ClimateField {
    /// Per-tile temperature 0..1 after the lapse rate.
    pub temp: Vec<Fx>,
    /// Per-tile precipitation 0..1.
    pub precip: Vec<Fx>,
    /// Prevailing wind, from the sea inland.
    pub wind: (i32, i32),
}

/// Build the climate over an existing height field.
///
/// `is_water` marks sea/lake/river tiles (the moisture sources). The parcel
/// sweep runs along the prevailing wind: a parcel recharges over water, rains
/// out where the ground climbs, and dries slowly over flat land — so leeward
/// slopes get a real rain shadow and windward ranges get a forest belt.
pub fn build(
    seed: u32,
    c: &ClimateArchetype,
    tile_h: &[Fx],
    is_water: &dyn Fn(usize) -> bool,
    moist_shift: Fx,
    sea: Fx,
) -> ClimateField {
    let n = WORLD_SIZE as usize;
    let base = crate::terrain::seed_base(seed);
    let wind = match hash2_u32(7, 13, base ^ 0x1d0f) % 4 {
        0 => (1i32, 0i32),
        1 => (-1, 0),
        2 => (0, 1),
        _ => (0, -1),
    };

    // ── orographic parcel sweep ──────────────────────────────────────────────
    let mut parcel: Vec<Fx> = vec![c.humidity; n * n];
    let mut orographic: Vec<Fx> = vec![Fx::ZERO; n * n];
    let xs: Vec<usize> = if wind.0 >= 0 { (0..n).collect() } else { (0..n).rev().collect() };
    let ys: Vec<usize> = if wind.1 >= 0 { (0..n).collect() } else { (0..n).rev().collect() };
    // Advance the OUTER loop along the wind so an entire upwind line — including
    // its cross-wind neighbours — is finished before the next line reads it.
    let along_x = wind.0 != 0;
    let outer: &[usize] = if along_x { &xs } else { &ys };
    let inner: &[usize] = if along_x { &ys } else { &xs };
    for &o in outer {
        for &j in inner {
            let (tx, ty) = if along_x { (o, j) } else { (j, o) };
            let i = ty * n + tx;
            // Air mixes across the flow. Sampling one cell straight upwind
            // propagates every rain shadow as a perfectly straight stripe, so
            // draw from three upwind cells: the shadow spreads and bends the
            // way it does behind a real range.
            let (px, py) = (-wind.1, wind.0); // cross-wind axis
            let mut up_h = Fx::ZERO;
            let mut up_p = Fx::ZERO;
            for (dx, dy, w) in [
                (-wind.0, -wind.1, crate::fx!("0.5")),
                (-wind.0 + px, -wind.1 + py, crate::fx!("0.25")),
                (-wind.0 - px, -wind.1 - py, crate::fx!("0.25")),
            ] {
                let (ux, uy) = (tx as i32 + dx, ty as i32 + dy);
                let (h, p) = if ux >= 0 && uy >= 0 && ux < n as i32 && uy < n as i32 {
                    let u = uy as usize * n + ux as usize;
                    (tile_h[u], parcel[u])
                } else {
                    (tile_h[i], c.humidity)
                };
                up_h += h * w;
                up_p += p * w;
            }
            if is_water(i) {
                // over water the parcel recharges toward saturation
                parcel[i] = up_p + (c.humidity - up_p) * crate::fx!("0.5");
                orographic[i] = parcel[i];
                continue;
            }
            let climb = (tile_h[i] - up_h).max(Fx::ZERO);
            let lift = (climb * crate::fx!("9")).min(crate::fx!("0.55"));
            // what falls here is what the parcel still carries, times how hard
            // the ground is pushing it up: flat land gets its baseline rain,
            // a windward wall wrings the parcel out
            orographic[i] = (up_p * (crate::fx!("0.66") + lift * crate::fx!("2.4"))).min(Fx::ONE);
            // the parcel only loses what actually rained out; per-tile relief
            // noise must not drain it before it ever reaches the interior
            let mut p = up_p - up_p * (crate::fx!("0.0035") + lift * crate::fx!("0.22"));
            // descending air re-absorbs a little (foehn), never back to full
            let fall = (up_h - tile_h[i]).max(Fx::ZERO);
            p += fall * crate::fx!("0.6") * (c.humidity - p).max(Fx::ZERO);
            parcel[i] = p.clamp(Fx::ZERO, Fx::ONE);
        }
    }

    // ── combine with the pressure belts, elevation and noise ────────────────
    let mut temp = vec![Fx::ZERO; n * n];
    let mut precip = vec![Fx::ZERO; n * n];
    let (mut t_sum, mut p_sum, mut land) = (Fx::ZERO, Fx::ZERO, 0i32);
    for ty in 0..n {
        let t_sea = sea_level_temp(c, ty);
        let belt = spline(SPL_LAT_RAIN, latitude_at(c, ty));
        for tx in 0..n {
            let i = ty * n + tx;
            let h = tile_h[i];
            let above = (h - sea).max(Fx::ZERO);
            temp[i] = (t_sea - above * LAPSE).clamp(Fx::ZERO, Fx::ONE);
            let x = Fx::from_num(tx as i32);
            let y = Fx::from_num(ty as i32);
            let n_p = fbm(
                x * crate::fx!("0.013") + Fx::from_num(100),
                y * crate::fx!("0.013") + Fx::from_num(50),
                base ^ 0x9e37,
                3,
            );
            let p = orographic[i] * crate::fx!("0.58")
                + belt * crate::fx!("0.26")
                + n_p * crate::fx!("0.14")
                + crate::fx!("0.08")
                - c.aridity
                + moist_shift;
            precip[i] = p.clamp(Fx::ZERO, Fx::ONE);
            if h >= sea {
                t_sum += temp[i];
                p_sum += precip[i];
                land += 1;
            }
        }
    }

    // ── bias correction + contrast stretch about the land mean ──────────────
    // The sweep gives precipitation its SHAPE; how wet the map is overall is
    // the archetype's business, so shift the land mean onto its target (an
    // additive shift leaves every gradient intact). Then stretch about that
    // mean: without it a map can sit inside one Whittaker cell and read as a
    // single biome from coast to coast.
    if land > 0 {
        let ln = Fx::from_num(land);
        let t_mean = t_sum / ln;
        let shift = c.target_precip + moist_shift - p_sum / ln;
        let p_mean = c.target_precip + moist_shift;
        const T_GAIN: Fx = crate::fx!("1.35");
        const P_GAIN: Fx = crate::fx!("1.70");
        for i in 0..n * n {
            temp[i] = (t_mean + (temp[i] - t_mean) * T_GAIN).clamp(Fx::ZERO, Fx::ONE);
            let p = precip[i] + shift;
            precip[i] = (p_mean + (p - p_mean) * P_GAIN).clamp(Fx::ZERO, Fx::ONE);
        }
    }

    ClimateField { temp, precip, wind }
}

// ── Whittaker classification ────────────────────────────────────────────────

const T_BINS: usize = 8;
const P_BINS: usize = 8;

/// Whittaker's biome diagram, discretized: rows are temperature (cold to hot),
/// columns precipitation (arid to wet). Elevation, slope and standing water are
/// applied as overrides afterwards.
const WHITTAKER: [[Biome; P_BINS]; T_BINS] = [
    // cold: tundra/alpine meadow giving way to boreal conifer
    [Biome::Alpine, Biome::Alpine, Biome::Alpine, Biome::Pine, Biome::Pine, Biome::Pine, Biome::Pine, Biome::Pine],
    [Biome::Steppe, Biome::Steppe, Biome::Alpine, Biome::Pine, Biome::Pine, Biome::Pine, Biome::Forest, Biome::Forest],
    [Biome::Desert, Biome::Steppe, Biome::Steppe, Biome::Grassland, Biome::Pine, Biome::Forest, Biome::Forest, Biome::Forest],
    [Biome::Desert, Biome::Steppe, Biome::Steppe, Biome::Grassland, Biome::Grassland, Biome::Forest, Biome::Forest, Biome::Forest],
    // temperate mediterranean: the maquis and olive-terrace band
    [Biome::Desert, Biome::Steppe, Biome::Scrub, Biome::Grassland, Biome::OliveGrove, Biome::OliveGrove, Biome::Forest, Biome::Forest],
    [Biome::Desert, Biome::Desert, Biome::Scrub, Biome::Savanna, Biome::OliveGrove, Biome::OliveGrove, Biome::Forest, Biome::Forest],
    [Biome::Desert, Biome::Desert, Biome::Savanna, Biome::Savanna, Biome::Savanna, Biome::Scrub, Biome::Forest, Biome::Forest],
    [Biome::Desert, Biome::Desert, Biome::Desert, Biome::Savanna, Biome::Savanna, Biome::Scrub, Biome::Forest, Biome::Forest],
];
fn bin(v: Fx, bins: usize) -> usize {
    let i = (v * Fx::from_num(bins as i32)).to_num::<i32>();
    (i.max(0) as usize).min(bins - 1)
}

/// The lowland biome for a (temperature, precipitation) pair.
pub fn whittaker(temp: Fx, precip: Fx) -> Biome {
    WHITTAKER[bin(temp, T_BINS)][bin(precip, P_BINS)]
}

/// The highland biome for ground above the hill line: climate still decides
/// whether an upland is cedar forest, bare rock or alpine meadow.
pub fn highland(temp: Fx, precip: Fx, tree_line: Fx, h: Fx) -> Biome {
    if h > tree_line {
        return Biome::Alpine;
    }
    if temp < crate::fx!("0.30") {
        if precip > crate::fx!("0.45") { Biome::Pine } else { Biome::Alpine }
    } else if precip > crate::fx!("0.62") {
        Biome::Pine
    } else if precip > crate::fx!("0.30") {
        Biome::Hills
    } else if precip > crate::fx!("0.16") {
        Biome::Scrub
    } else {
        Biome::Hammada
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_and_dry_is_desert_cold_and_wet_is_conifer() {
        assert_eq!(whittaker(crate::fx!("0.95"), crate::fx!("0.05")), Biome::Desert);
        assert_eq!(whittaker(crate::fx!("0.05"), crate::fx!("0.95")), Biome::Pine);
        assert_eq!(whittaker(crate::fx!("0.5"), crate::fx!("0.95")), Biome::Forest);
    }

    #[test]
    fn latitude_drives_temperature_the_right_way() {
        let c = &CLIMATES[0];
        let north = sea_level_temp(c, 0);
        let south = sea_level_temp(c, (WORLD_SIZE - 1) as usize);
        assert!(south > north, "the south of the map must be warmer ({south} vs {north})");
    }

    #[test]
    fn every_archetype_is_reachable() {
        let mut seen = [false; CLIMATES.len()];
        for s in 0..400u32 {
            let a = climate_archetype(crate::terrain::compose_seed(s, 0));
            let i = CLIMATES.iter().position(|c| c.id == a.id).unwrap();
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s), "some climate archetype never rolls");
    }
}
