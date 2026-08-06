use crate::biomes::{Biome, biome_passable};
use crate::constants::{START_REGION_MIN, WORLD_SIZE};
use crate::enums::ResourceType;
use crate::math::{Fx, V2, spline};
use crate::noise::fbm;
use crate::plates::{PlateSample, Plates};
use crate::presets::{MapBias, map_preset_by_index};
use crate::rng::{Rng, hash2, mix_seed};

/// Deterministic biome terrain from a single seed. Shared by the sim
/// (authority: where land/resources are) and render. No per-tile rows — both
/// sides recompute from the seed. Fixed-point throughout so every client agrees.
///
/// The MAP PRESET travels inside the seed's top 3 bits (`compose_seed`), so
/// every per-seed cache (`passable_grid`, `region_grid`, `elevation_at`) and
/// the lockstep wire stay plain-u32 and preset-aware for free.
#[derive(Clone, Copy, Debug)]
pub struct TerrainSample {
    pub height: Fx,
    pub moisture: Fx,
    pub biome: Biome,
}

const H_SCALE: Fx = crate::fx!("0.042");
const WARP_SCALE: Fx = crate::fx!("0.02");
const WARP_AMP: Fx = crate::fx!("9");
pub(crate) const SEA: Fx = crate::fx!("0.38");

// archipelago: large-scale blob mask that shatters the continent
const ISLAND_SCALE: Fx = crate::fx!("0.015");
// ridged-multifractal mountain ranges: fold the noise about its midline so
// crests chain into ranges instead of fbm blobs
const RIDGE_SCALE: Fx = crate::fx!("0.022");
const RIDGE_GAIN: Fx = crate::fx!("0.13");
// The massif envelope: a very low-frequency dome (wavelength ~110 tiles) that
// decides WHERE a range has mass. Ridges are multiplied by it, so a crest is
// texture on a mountain instead of a wall standing on open plain, and the
// skirt of the dome is the foothill belt.
const MASSIF_SCALE: Fx = crate::fx!("0.009");
const MASSIF_GAIN: Fx = crate::fx!("0.36");
// where the height field starts saturating instead of clipping
const SOFT_CAP: Fx = crate::fx!("0.88");
// terraced hill flanks (low-poly plateau stylization) — mesa country only:
// terracing is exactly what manufactures flat tops with sheer sides, so it is
// gated OUT of the massifs.
const TERRACE_LEVELS: Fx = crate::fx!("9");
const TERRACE_MIX: Fx = crate::fx!("0.30");

const BASE_MASK: u32 = 0x1FFF_FFFF;

/// Pack a preset index into the top 3 bits of a world seed. Base seeds stay
/// below 2^29 (the menu rolls < 100 000), so old plain seeds decode as
/// preset 0 (Continental) — fully backward compatible.
pub fn compose_seed(base: u32, preset: u8) -> u32 {
    (base & BASE_MASK) | (((preset as u32) & 0x7) << 29)
}

pub fn seed_preset(seed: u32) -> u8 {
    (seed >> 29) as u8
}

pub fn seed_base(seed: u32) -> u32 {
    seed & BASE_MASK
}

/// The preset bias a composed seed carries (render reads `elev_gain` from it).
pub fn seed_bias(seed: u32) -> MapBias {
    map_preset_by_index(seed_preset(seed) as i32).bias
}

/// Continentalness at a point: the tectonic crust elevation (the plate field is
/// the authority on where land is at all) plus a low-frequency noise warp that
/// keeps coastlines organic, faded to ocean at the map border so the camera's
/// backdrop disc always meets open sea.
const CONT_SCALE: Fx = crate::fx!("0.006");

fn smooth01(t: Fx) -> Fx {
    let t = t.clamp(Fx::ZERO, Fx::ONE);
    t * t * (crate::fx!("3") - crate::fx!("2") * t)
}

fn continent(plates: &Plates, base: u32, x: Fx, y: Fx) -> (Fx, PlateSample) {
    let p = plates.sample(x, y);
    let n = fbm(
        x * CONT_SCALE + Fx::from_num(53),
        y * CONT_SCALE + Fx::from_num(71),
        base ^ 0xc047,
        3,
    );
    // crust rides the plate, noise decides the shoreline's fingers and bays
    let c = p.base + (n - crate::fx!("0.5")) * crate::fx!("0.30");
    let cc = Fx::from_num(WORLD_SIZE) / Fx::from_num(2);
    let dx = x - cc;
    let dy = y - cc;
    let d2 = (dx * dx + dy * dy) / (cc * cc);
    // a weak central dome anchors a mainland so no seed rolls all-ocean
    let dome = (crate::fx!("0.9") - d2 * crate::fx!("1.3")).max(Fx::ZERO) * crate::fx!("0.30");
    let m = Fx::from_num(WORLD_SIZE);
    // the border fade is a min() of four ramps, which clips corner land into
    // rectangles; a noise offset and a smoothstep hide the frame
    let jag = (fbm(x * crate::fx!("0.05"), y * crate::fx!("0.05"), base ^ 0x3f19, 2) - crate::fx!("0.5"))
        * crate::fx!("18");
    let e = ((x.min(y).min(m - x).min(m - y) + jag) / crate::fx!("34")).clamp(Fx::ZERO, Fx::ONE);
    let edge = e * e * (crate::fx!("3") - crate::fx!("2") * e);
    let c = (c.max(dome) + crate::fx!("0.335")) * edge;
    (c.clamp(Fx::ZERO, crate::fx!("1.2")), p)
}

// continentalness -> base elevation: ocean floor, shelf, a STEEP coast
// segment (fjords/cliff coasts under the domain warp), then gently rising
// interior. The 0.44..0.52 jump is where the waterline (SEA 0.38) crosses.
const SPL_CONT: &[(Fx, Fx)] = &[
    (crate::fx!("0"), crate::fx!("0.05")),
    (crate::fx!("0.34"), crate::fx!("0.16")),
    (crate::fx!("0.48"), crate::fx!("0.3")),
    (crate::fx!("0.58"), crate::fx!("0.43")),
    (crate::fx!("0.72"), crate::fx!("0.47")),
    (crate::fx!("0.92"), crate::fx!("0.52")),
    (crate::fx!("1.2"), crate::fx!("0.6")),
];
// erosion -> relief amplitude: ancient eroded shields are FLAT, young
// terrain is mountainous (Minecraft 1.18's diversity lever — flat plains
// seeds and alpine seeds from the same algorithm)
const SPL_ERO: &[(Fx, Fx)] = &[
    (crate::fx!("0"), crate::fx!("0.5")),
    (crate::fx!("0.35"), crate::fx!("0.34")),
    (crate::fx!("0.6"), crate::fx!("0.16")),
    (crate::fx!("1"), crate::fx!("0.06")),
];
const ERO_SCALE: Fx = crate::fx!("0.012");

/// The raw height field. Continentalness (crust + noise) picks the base
/// elevation, the massif envelope decides where a range has MASS, erosion
/// picks how much relief sits on it, and the ridged fold — multiplied by that
/// envelope — supplies crests and saddles ON that mass rather than instead of it.
/// Domain warp distorts everything organically; terraces stylize the mesa
/// country outside the massifs. The worldgrid pipeline then erodes, floods and
/// carves this shape.
pub(crate) fn height_at(plates: &Plates, base: u32, bias: MapBias, x: Fx, y: Fx) -> Fx {
    let island_gain = bias.island_gain;
    let half = crate::fx!("0.5");
    let two = crate::fx!("2");
    let wx = (fbm(x * WARP_SCALE, y * WARP_SCALE, base ^ 0x1b56, 3) - half) * two * WARP_AMP;
    let wy = (fbm(x * WARP_SCALE + Fx::from_num(31), y * WARP_SCALE + Fx::from_num(17), base ^ 0x77c1, 3)
        - half)
        * two
        * WARP_AMP;

    let (c, plate) = continent(plates, base, x + wx * crate::fx!("0.4"), y + wy * crate::fx!("0.4"));
    let ero = fbm((x + wx) * ERO_SCALE + Fx::from_num(211), (y + wy) * ERO_SCALE + Fx::from_num(97), base ^ 0xe705, 3);
    let detail = fbm((x + wx) * H_SCALE, (y + wy) * H_SCALE, base, 5) - half;
    let rn = fbm((x + wx) * RIDGE_SCALE, (y + wy) * RIDGE_SCALE, base ^ 0x71d6, 4);
    let folded = Fx::ONE - (rn * two - Fx::ONE).abs();
    let pv = folded * folded;

    let shelf_h = spline(SPL_CONT, c);
    // A range has to have MASS before it can have a crest: a broad dome along
    // the orogenic seam, faded out over water so the ocean ring survives.
    let mn = fbm((x + wx) * MASSIF_SCALE, (y + wy) * MASSIF_SCALE, base ^ 0x4d55, 3);
    let land_gate = ((shelf_h - crate::fx!("0.34")) * crate::fx!("6")).clamp(Fx::ZERO, Fx::ONE);
    let massif = smooth01((mn - crate::fx!("0.44")) * crate::fx!("3.2"))
        * (crate::fx!("0.35") + smooth01(plate.belt * crate::fx!("2.5")) * crate::fx!("0.65"))
        * land_gate;
    let base_h = shelf_h + massif * MASSIF_GAIN * bias.relief_gain;
    // young crust carries alpine relief, shield interiors are worn flat
    let amp = (spline(SPL_ERO, ero) + plate.belt * crate::fx!("0.55")) * bias.relief_gain;
    // land carries the full relief budget; the sea floor stays calm so the
    // coast contour comes from continentalness, not detail noise
    let landness = ((base_h - crate::fx!("0.3")) * crate::fx!("7")).clamp(crate::fx!("0.25"), Fx::ONE);
    // ridges belong to the orogenic seam; away from it they fade to gentle swell
    let ridge_w = crate::fx!("0.35") + plate.belt * crate::fx!("1.6");
    let mut h = base_h
        + (detail * crate::fx!("0.8") + pv * massif * RIDGE_GAIN * crate::fx!("4") * ridge_w)
            * amp
            * landness;

    // terraced flanks OUTSIDE the massifs: mesa and plateau country only
    let band = ((h - crate::fx!("0.54")) * crate::fx!("14"))
        .clamp(Fx::ZERO, Fx::ONE)
        .min(((crate::fx!("0.74") - h) * crate::fx!("14")).clamp(Fx::ZERO, Fx::ONE))
        * (Fx::ONE - massif);
    if band > Fx::ZERO {
        let t = h * TERRACE_LEVELS;
        let fl = t.floor();
        let fr = t - fl;
        let s = fr * fr * (crate::fx!("3") - crate::fx!("2") * fr);
        let stepped = (fl + s * s) / TERRACE_LEVELS;
        h += (stepped - h) * TERRACE_MIX * band;
    }
    // a hard clamp at the ceiling shears every big massif into a flat mesa top
    // — saturate smoothly so summits stay summits
    h = h.max(Fx::ZERO);
    if h > SOFT_CAP {
        let over = h - SOFT_CAP;
        h = SOFT_CAP + over / (Fx::ONE + over * crate::fx!("8"));
    }
    h = h.min(Fx::ONE);

    if island_gain > Fx::ZERO {
        let mask = fbm(x * ISLAND_SCALE + Fx::from_num(7), y * ISLAND_SCALE + Fx::from_num(13), base ^ 0x15a7, 3);
        // blobs keep their height; the straits between them sink to sea floor
        let blob = ((mask - crate::fx!("0.12")) * crate::fx!("1.9")).clamp(Fx::ZERO, Fx::ONE);
        h *= Fx::ONE - island_gain + island_gain * blob;
    }
    h
}

/// One tile/point sample of the generated world. Heights interpolate
/// bilinearly between the worldgrid's eroded + river-carved corner field;
/// biome and moisture are per-tile lookups. All the heavy lifting
/// (erosion, depression filling, drainage rivers, moisture BFS, Whittaker
/// classification) happened once in `worldgrid::build` — this is O(1).
pub fn sample_terrain(seed: u32, x: Fx, y: Fx) -> TerrainSample {
    let grid = crate::worldgrid::world_grid(seed);
    let height = crate::worldgrid::height_bilinear(grid, x, y);
    let (biome, moisture) = crate::worldgrid::tile_lookup(grid, x, y);
    TerrainSample { height, moisture, biome }
}

pub fn is_land(seed: u32, x: Fx, y: Fx) -> bool {
    biome_passable(sample_terrain(seed, x, y).biome)
}

const ADJ4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Walkable land with the OPEN SEA on at least one orthogonal neighbour — the
/// seacoast, not any waterside (a lakeshore is not a coast).
pub fn is_coastal(seed: u32, x: Fx, y: Fx) -> bool {
    if !is_land(seed, x, y) {
        return false;
    }
    ADJ4.iter().any(|(dx, dy)| {
        crate::biomes::biome_sailable(
            sample_terrain(seed, x + Fx::from_num(*dx), y + Fx::from_num(*dy)).biome,
        )
    })
}

/// The two movement domains. A hull and a walker read the SAME A* over
/// different passability grids; nothing else about them differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Domain {
    Land,
    Sea,
}

impl Domain {
    /// How tight a smoothed path leg has to be for a mover here. Land is
    /// SAMPLED, exactly as every march in the game has always been measured — a
    /// man shaving a tenth of a tile off a wall corner is invisible. The sea is
    /// EXACT: `movement` walks whatever waypoints it is handed with no terrain
    /// test at all, so the same tenth of a tile is a hull standing on a headland
    /// and nothing downstream ever puts it back.
    pub fn smoothing(self) -> crate::pathfinding::Smoothing {
        match self {
            Domain::Land => crate::pathfinding::Smoothing::Sampled,
            Domain::Sea => crate::pathfinding::Smoothing::Exact,
        }
    }
}

/// Tile-space passability for one domain — the single place a closure-building
/// call site should go to decide what its mover may enter.
pub fn domain_passable(seed: u32, domain: Domain, tx: i32, ty: i32) -> bool {
    match domain {
        Domain::Land => is_passable(seed, tx, ty),
        Domain::Sea => is_sailable(seed, tx, ty),
    }
}

/// Tile-space passability for the pathfinder: in-bounds land at the tile centre.
pub fn is_passable(seed: u32, tx: i32, ty: i32) -> bool {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return false;
    }
    passable_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// Per-seed passability bitmap, computed once and leaked (a process touches a
/// handful of seeds at most). Terrain sampling is fbm noise — pricey enough
/// that the old compute-per-call `is_passable` dominated A*-heavy profiles.
/// A thread-local memo of the last seed keeps the hot path lock-free.
pub fn passable_grid(seed: u32) -> &'static [bool] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[bool] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [bool])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [bool]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [bool] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let mut v = vec![false; (WORLD_SIZE * WORLD_SIZE) as usize];
            for ty in 0..WORLD_SIZE {
                for tx in 0..WORLD_SIZE {
                    v[(ty * WORLD_SIZE + tx) as usize] =
                        is_land(seed, Fx::from_num(tx) + crate::fx!("0.5"), Fx::from_num(ty) + crate::fx!("0.5"));
                }
            }
            let leaked: &'static [bool] = Box::leak(v.into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Per-seed BUILDABLE bitmap (biome_buildable: excludes water, mountains,
/// cliffs AND fords — fords stay walkable chokepoints, never tower platforms),
/// cached+leaked like `passable_grid`.
pub fn buildable_grid(seed: u32) -> &'static [bool] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[bool] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [bool])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [bool]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [bool] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let half = crate::fx!("0.5");
            let mut v = vec![false; (WORLD_SIZE * WORLD_SIZE) as usize];
            for ty in 0..WORLD_SIZE {
                for tx in 0..WORLD_SIZE {
                    let b = sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half).biome;
                    v[(ty * WORLD_SIZE + tx) as usize] = crate::biomes::biome_buildable(b);
                }
            }
            let leaked: &'static [bool] = Box::leak(v.into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Per-seed movement-cost grid: what one tile of this ground costs a walker.
/// Clamped at 1 so the pathfinder's octile heuristic stays admissible — the
/// "fast" biomes (a dry wadi bed, a salt pan) are fast because everything
/// around them drags, not because they cost less than open ground.
pub fn move_cost_grid(seed: u32) -> &'static [Fx] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[Fx] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [Fx])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [Fx]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [Fx] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let world = crate::worldgrid::world_grid(seed);
            let v: Vec<Fx> = world
                .biome
                .iter()
                .zip(world.slope.iter())
                .map(|(&b, &s)| {
                    let c = crate::biomes::move_cost_mul(b);
                    let c = if c >= Fx::MAX { Fx::ONE } else { c };
                    // a climb costs before it is forbidden, so a low saddle is
                    // the cheap route out of a range with no special case
                    (c * (Fx::ONE + s * crate::worldgrid::CLIMB_COST)).max(Fx::ONE)
                })
                .collect();
            let leaked: &'static [Fx] = Box::leak(v.into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// What entering tile (tx, ty) costs a walker, 1 = open ground.
pub fn move_cost_at(seed: u32, tx: i32, ty: i32) -> Fx {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return Fx::ONE;
    }
    move_cost_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// Tile-space buildability (in-bounds + buildable biome).
pub fn is_buildable_tile(seed: u32, tx: i32, ty: i32) -> bool {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return false;
    }
    buildable_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// Per-seed water bitmap: every tile a hull floats on — sea, shelf, river and
/// lake alike. A lake is included on purpose and its own water region traps a
/// skiff inside it, which is the correct answer for free; fords are LAND, so a
/// ford severs a river for a hull exactly as a river severs a road for a walker.
/// Cached and leaked like `passable_grid`, with its OWN last-seed cell — one
/// cell shared between domains would thrash on every A* expansion.
pub fn sailable_grid(seed: u32) -> &'static [bool] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[bool] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [bool])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [bool]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [bool] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let world = crate::worldgrid::world_grid(seed);
            let v: Vec<bool> =
                world.biome.iter().map(|&b| crate::biomes::biome_is_water(b)).collect();
            let leaked: &'static [bool] = Box::leak(v.into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Tile-space passability for a hull: in-bounds water of any kind.
pub fn is_sailable(seed: u32, tx: i32, ty: i32) -> bool {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return false;
    }
    sailable_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// True open water — the Fishing Hut's shoreline test. NOT the same as
/// "impassable" (cliffs and mountains are impassable but dry).
pub fn is_water_tile(seed: u32, tx: i32, ty: i32) -> bool {
    is_sailable(seed, tx, ty)
}

/// Connected-water-body id per tile (flood over `sailable_grid`), the naval twin
/// of `region_grid`. `u16::MAX` = dry. A lake gets its own id, so a lake skiff is
/// trapped in its lake with no special case anywhere.
pub fn water_region_grid(seed: u32) -> &'static [u16] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[u16] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [u16])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [u16]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [u16] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let leaked: &'static [u16] = Box::leak(flood_regions(sailable_grid(seed)).into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Water-body id at a world position (`u16::MAX` = dry tile).
pub fn water_region_at(seed: u32, x: Fx, y: Fx) -> u16 {
    let tx = x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    water_region_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// The map's biggest connected body of water — the ocean every coast shares.
/// Measured to hold 97-100% of all salt water on every preset, so "the sea" is
/// one place and a harbour on it can reach every other harbour on it.
pub fn main_water_body(seed: u32) -> u16 {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static BODIES: OnceLock<Mutex<HashMap<u32, u16>>> = OnceLock::new();
    let m = BODIES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = m.lock().unwrap();
    *m.entry(seed).or_insert_with(|| largest_region(water_region_grid(seed)))
}

/// Can a hull floating at `from` ever work at `to`? The naval twin of
/// `node_reachable`, and MANDATORY rather than nice-to-have: without it an
/// unreachable naval order floods the whole ocean looking for a route.
pub fn sea_reachable(seed: u32, from: V2, to: V2) -> bool {
    let body = water_region_at(seed, from.x, from.y);
    if body == u16::MAX {
        return true; // hull on a weird tile: do not over-filter
    }
    let grid = water_region_grid(seed);
    let tx = to.x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = to.y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= WORLD_SIZE || ny >= WORLD_SIZE {
                continue;
            }
            if grid[(ny * WORLD_SIZE + nx) as usize] == body {
                return true;
            }
        }
    }
    false
}

/// 4-connected component labelling over a tile mask. `u16::MAX` = not in the
/// mask. Ids are assigned in raster order of each component's first tile, so
/// the labelling is a pure function of the mask.
fn flood_regions(mask: &[bool]) -> Vec<u16> {
    let n = (WORLD_SIZE * WORLD_SIZE) as usize;
    let mut v = vec![u16::MAX; n];
    let mut next_region: u16 = 0;
    let mut stack: Vec<i32> = Vec::new();
    for start in 0..n {
        if !mask[start] || v[start] != u16::MAX {
            continue;
        }
        let region = next_region;
        next_region += 1;
        v[start] = region;
        stack.push(start as i32);
        while let Some(idx) = stack.pop() {
            let (tx, ty) = (idx % WORLD_SIZE, idx / WORLD_SIZE);
            for (dx, dy) in ADJ4 {
                let (nx, ny) = (tx + dx, ty + dy);
                if nx < 0 || ny < 0 || nx >= WORLD_SIZE || ny >= WORLD_SIZE {
                    continue;
                }
                let ni = (ny * WORLD_SIZE + nx) as usize;
                if mask[ni] && v[ni] == u16::MAX {
                    v[ni] = region;
                    stack.push(ni as i32);
                }
            }
        }
    }
    v
}

/// Per-region tile counts, indexed by region id.
fn region_sizes(grid: &[u16]) -> Vec<u32> {
    let mut counts: Vec<u32> = Vec::new();
    for &r in grid {
        if r == u16::MAX {
            continue;
        }
        let i = r as usize;
        if i >= counts.len() {
            counts.resize(i + 1, 0);
        }
        counts[i] += 1;
    }
    counts
}

/// Biggest region in a labelling; ties break to the lowest id.
fn largest_region(grid: &[u16]) -> u16 {
    let counts = region_sizes(grid);
    let mut best = (0u16, 0u32);
    for (r, &c) in counts.iter().enumerate() {
        if c > best.1 {
            best = (r as u16, c);
        }
    }
    best.0
}

/// Connected-region id per tile (flood fill over `passable_grid`), cached per
/// seed like the grids above. `u16::MAX` = impassable. Lets gameplay ask
/// "can this unit ever walk there?" in O(1) — the cure for gatherers
/// ping-ponging between nodes on islands they can never reach.
pub fn region_grid(seed: u32) -> &'static [u16] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[u16] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [u16])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [u16]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [u16] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let leaked: &'static [u16] =
                Box::leak(flood_regions(passable_grid(seed)).into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Region id at a world position (`u16::MAX` = impassable tile).
pub fn region_at(seed: u32, x: Fx, y: Fx) -> u16 {
    let tx = x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    region_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// Can a walker standing at `from` ever harvest a node at `node`? True when the
/// node's tile — or any neighbouring tile (coastal fish sit on water, harvested
/// from the adjacent shore) — shares the walker's connected region.
pub fn node_reachable(seed: u32, from: V2, node: V2) -> bool {
    let region = region_at(seed, from.x, from.y);
    if region == u16::MAX {
        return true; // walker on a weird tile: do not over-filter
    }
    let grid = region_grid(seed);
    let tx = node.x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = node.y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= WORLD_SIZE || ny >= WORLD_SIZE {
                continue;
            }
            if grid[(ny * WORLD_SIZE + nx) as usize] == region {
                return true;
            }
        }
    }
    false
}

/// THE vertical scale of the world, in world units per unit of height field.
/// One monotone function of the height field and nothing else — a biome label
/// can never move a vertex, so a label flip can never raise a wall.
///
/// Water renders as a flat SEA SURFACE (short shoreline shelf, then constant
/// level): the sea is a body of water, not a terrain dent — and the backdrop
/// ocean disc (`environment::OCEAN_Y`) must stay strictly below it.
///
/// Above the waterline the curve is cubic: gentle in the lowlands, severe near
/// a summit, so the same height step reads as a shallow plain low down and a
/// steep face high up.
const SURF_A: Fx = crate::fx!("2.5");
const SURF_B: Fx = crate::fx!("5");
const SURF_C: Fx = crate::fx!("20");

pub fn surface_height(h: Fx, elev_gain: Fx) -> Fx {
    if h < SEA {
        let shelf = ((SEA - h) / crate::fx!("0.05")).min(Fx::ONE);
        return crate::fx!("-0.2") * shelf - crate::fx!("0.015");
    }
    let u = h - SEA;
    let u2 = u * u;
    (u * SURF_A + u2 * SURF_B + u2 * u * SURF_C) * elev_gain
}

#[derive(Clone, Copy, Debug)]
pub struct ScatteredNode {
    pub pos: V2,
    pub res_type: ResourceType,
    pub yield_: i32,
    /// Stock regained per economy tick. Zero is a finite deposit — a felled
    /// wood, a mined-out seam, a hunted herd. A fishery is not: a school swims
    /// back, which is what makes fishing a flow rather than a stock.
    pub regen: i32,
}

/// Soil fertility (0..1) at a world position — what a farm yields there.
pub fn fertility_at(seed: u32, x: Fx, y: Fx) -> Fx {
    let g = crate::worldgrid::world_grid(seed);
    g.fertility[crate::worldgrid::tile_index(x, y)]
}

/// Ore potential (0..1) — how mineralized the rock under a position is.
pub fn ore_at(seed: u32, x: Fx, y: Fx) -> Fx {
    let g = crate::worldgrid::world_grid(seed);
    g.ore[crate::worldgrid::tile_index(x, y)]
}

/// World-space rise per tile of the meshed surface — what the camera shows as
/// steepness IS this number, and it is what decides the Cliff/Mountain label.
pub fn slope_at(seed: u32, x: Fx, y: Fx) -> Fx {
    let g = crate::worldgrid::world_grid(seed);
    g.slope[crate::worldgrid::tile_index(x, y)]
}

/// Orogenic belt weight (0..1) — the rock's identity under a position.
pub fn belt_at(seed: u32, x: Fx, y: Fx) -> Fx {
    let g = crate::worldgrid::world_grid(seed);
    g.belt[crate::worldgrid::tile_index(x, y)]
}

/// Mean annual temperature (0 cold .. 1 hot) at a world position.
pub fn temp_at(seed: u32, x: Fx, y: Fx) -> Fx {
    let g = crate::worldgrid::world_grid(seed);
    g.temp[crate::worldgrid::tile_index(x, y)]
}

/// The climate regime this seed's world belongs to.
pub fn world_climate(seed: u32) -> &'static crate::climate::ClimateArchetype {
    crate::worldgrid::world_grid(seed).climate
}

/// Everything a scatter rule is allowed to know about a candidate tile. Node
/// placement reads GEOLOGY and CLIMATE, not just the biome label: gold follows
/// the mineralized belts, herds follow the grazing, timber follows the rain.
#[derive(Clone, Copy, Debug)]
pub struct NodeSite {
    pub biome: Biome,
    pub ore: Fx,
    pub fertility: Fx,
    pub precip: Fx,
    pub temp: Fx,
    pub height: Fx,
    /// World-space rise per tile — the same number the camera renders as
    /// steepness and the pathfinder charges for, so a deposit that reads as
    /// clinging to a scarp really is on one.
    pub slope: Fx,
    /// The water body this tile borders, if any — a shore's fishery depends on
    /// whether it faces the open sea, a lake or a river.
    pub adjacent_water: Option<Biome>,
}

pub fn node_site(seed: u32, x: Fx, y: Fx) -> NodeSite {
    let s = sample_terrain(seed, x, y);
    let mut adjacent_water = None;
    for (dx, dy) in ADJ4 {
        let b = sample_terrain(seed, x + Fx::from_num(dx), y + Fx::from_num(dy)).biome;
        if crate::biomes::biome_is_water(b) {
            // a lake or river beside the tile beats the open sea behind it
            adjacent_water = match adjacent_water {
                Some(Biome::Lake) => Some(Biome::Lake),
                Some(prev) if crate::biomes::biome_is_fresh_water(prev) && !crate::biomes::biome_is_fresh_water(b) => Some(prev),
                _ => Some(b),
            };
        }
    }
    NodeSite {
        biome: s.biome,
        ore: ore_at(seed, x, y),
        fertility: fertility_at(seed, x, y),
        precip: s.moisture,
        temp: temp_at(seed, x, y),
        height: s.height,
        slope: slope_at(seed, x, y),
        adjacent_water,
    }
}

/// Which surface a rule places on. `Water` was `coastal_only`, which resolved
/// through `is_coastal` to the LAND beside the sea: every "fishery" the
/// generator has ever placed stood on a beach, so the fishing hut's aura, the
/// water-node harvest branch and the fish-school mesh were all unreachable code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScatterDomain {
    Land,
    Water,
}

/// One scatter rule: count, yield, per-biome accept-probability, which surface.
/// `clustered` modulates acceptance with a grove-mask noise so the kind lands
/// in clumps (forests as woods, not uniform confetti). `patch` places each
/// accepted site as a tight cluster of min..=max nodes (AoE-style mines —
/// gold/stone read as one deposit, not strewn singles).
#[derive(Clone, Copy)]
pub struct ScatterRule {
    pub res_type: ResourceType,
    pub count: i32,
    pub yield_: i32,
    pub density: fn(&NodeSite) -> Fx,
    pub domain: ScatterDomain,
    /// Per economy tick, for a stock that grows back. 0 on every land rule.
    pub regen: i32,
    pub clustered: bool,
    pub patch: (i32, i32),
}

const GROVE_SCALE: Fx = crate::fx!("0.07");
const GROVE_T: Fx = crate::fx!("0.55");
const GROVE_BOOST: Fx = crate::fx!("2.2");
const GROVE_CUT: Fx = crate::fx!("0.12");

/// Deterministically place all resource nodes for a seed. Each rule draws from
/// its own RNG stream (via `mix_seed`) so adding/removing a kind never shifts
/// the others.
pub fn scatter_nodes(seed: u32, rules: &[ScatterRule]) -> Vec<ScatteredNode> {
    let mut out = Vec::new();
    let span = Fx::from_num(WORLD_SIZE - 6);
    let three = crate::fx!("3");
    let base = seed_base(seed);
    for (ri, rule) in rules.iter().enumerate() {
        let ri = ri as u32;
        let mut rand = Rng::new(mix_seed(seed, 1013u32.wrapping_mul(ri + 1)));
        let mut placed = 0;
        let mut attempts = 0;
        let budget = rule.count.max(60) * 80;
        let roll_seed = mix_seed(seed, ri + 1);
        let on_surface = |x: Fx, y: Fx| match rule.domain {
            ScatterDomain::Land => is_land(seed, x, y),
            ScatterDomain::Water => {
                is_sailable(seed, x.floor().to_num::<i32>(), y.floor().to_num::<i32>())
            }
        };
        while placed < rule.count && attempts < budget {
            attempts += 1;
            let x = three + rand.next_fx() * span;
            let y = three + rand.next_fx() * span;
            if !on_surface(x, y) {
                continue;
            }
            let roll = hash2(x.floor().to_num::<i32>(), y.floor().to_num::<i32>(), roll_seed);
            let mut density = (rule.density)(&node_site(seed, x, y));
            if rule.clustered {
                let gv = fbm(x * GROVE_SCALE, y * GROVE_SCALE, base ^ 0x6701, 3);
                density *= if gv > GROVE_T { GROVE_BOOST } else { GROVE_CUT };
            }
            if roll < density {
                let (pmin, pmax) = rule.patch;
                let want = if pmax > pmin {
                    pmin + (rand.next_fx() * Fx::from_num(pmax - pmin + 1))
                        .floor()
                        .to_num::<i32>()
                        .min(pmax - pmin)
                } else {
                    pmin
                };
                // first node on the accepted tile, the rest packed around it
                out.push(ScatteredNode {
                    pos: V2::new(x, y),
                    res_type: rule.res_type,
                    yield_: rule.yield_,
                    regen: rule.regen,
                });
                placed += 1;
                let mut added = 1;
                let mut tries = 0;
                while added < want && placed < rule.count && tries < want * 6 {
                    tries += 1;
                    let ox = (rand.next_fx() - crate::fx!("0.5")) * crate::fx!("2.4");
                    let oy = (rand.next_fx() - crate::fx!("0.5")) * crate::fx!("2.4");
                    let (px, py) = (x + ox, y + oy);
                    if !on_surface(px, py) {
                        continue;
                    }
                    // one node per tile inside the patch
                    let (ptx, pty) = (px.floor().to_num::<i32>(), py.floor().to_num::<i32>());
                    let dup = out.iter().rev().take(want as usize).any(|n| {
                        n.pos.x.floor().to_num::<i32>() == ptx && n.pos.y.floor().to_num::<i32>() == pty
                    });
                    if dup {
                        continue;
                    }
                    out.push(ScatteredNode {
                        pos: V2::new(px, py),
                        res_type: rule.res_type,
                        yield_: rule.yield_,
                        regen: rule.regen,
                    });
                    placed += 1;
                    added += 1;
                }
            }
        }
    }
    out
}

// ── fair starts ──────────────────────────────────────────────────────────────

pub const FAIR_RADIUS: Fx = crate::fx!("20");
pub const FAIR_MIN_WOOD: usize = 4;
pub const FAIR_MIN_STONE: usize = 2;
pub const FAIR_MIN_FOOD: usize = 2;

/// The map's biggest connected region — the "mainland".
pub fn dominant_region(seed: u32) -> u16 {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static DOM: OnceLock<Mutex<HashMap<u32, u16>>> = OnceLock::new();
    let m = DOM.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = m.lock().unwrap();
    *m.entry(seed).or_insert_with(|| largest_region(region_grid(seed)))
}

/// Every landmass a player may be seated on, biggest first (ties by region id).
///
/// On a mainland preset this is exactly `[dominant_region(seed)]` — the rule
/// that has always applied, expressed through the new one, which is why those
/// maps do not move a tile. Where the generator makes islands on purpose
/// (`MapBias::sea_starts`) it is every land region of at least
/// `START_REGION_MIN` tiles that touches the main water body: big enough to hold
/// a start's guaranteed resources, and connected to every other start by sea, so
/// a match on it can still be won.
pub fn start_regions(seed: u32) -> &'static [u16] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static SETS: OnceLock<Mutex<HashMap<u32, &'static [u16]>>> = OnceLock::new();
    let sets = SETS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut s = sets.lock().unwrap();
    if let Some(&v) = s.get(&seed) {
        return v;
    }
    let main = dominant_region(seed);
    let list: Vec<u16> = if !seed_bias(seed).sea_starts {
        vec![main]
    } else {
        let regions = region_grid(seed);
        let sizes = region_sizes(regions);
        let bodies = water_region_grid(seed);
        let ocean = main_water_body(seed);
        let mut on_the_sea = vec![false; sizes.len()];
        for ty in 0..WORLD_SIZE {
            for tx in 0..WORLD_SIZE {
                let r = regions[(ty * WORLD_SIZE + tx) as usize];
                if r == u16::MAX || on_the_sea[r as usize] {
                    continue;
                }
                on_the_sea[r as usize] = ADJ4.iter().any(|(dx, dy)| {
                    let (nx, ny) = (tx + dx, ty + dy);
                    nx >= 0
                        && ny >= 0
                        && nx < WORLD_SIZE
                        && ny < WORLD_SIZE
                        && bodies[(ny * WORLD_SIZE + nx) as usize] == ocean
                });
            }
        }
        let mut v: Vec<u16> = (0..sizes.len() as u16)
            .filter(|&r| {
                r == main || (sizes[r as usize] >= START_REGION_MIN && on_the_sea[r as usize])
            })
            .collect();
        v.sort_by_key(|&r| (std::cmp::Reverse(sizes[r as usize]), r));
        v
    };
    let leaked: &'static [u16] = Box::leak(list.into_boxed_slice());
    s.insert(seed, leaked);
    leaked
}

/// How many of the eight slots each qualifying island may take, proportional to
/// its area. Rounded UP, so the caps always cover all eight and a two-island map
/// never leaves a slot homeless.
fn start_quota(seed: u32) -> Vec<u32> {
    let regions = start_regions(seed);
    if regions.len() < 2 {
        return vec![crate::constants::MAX_PLAYERS as u32];
    }
    let sizes = region_sizes(region_grid(seed));
    let total: u64 = regions.iter().map(|&r| sizes[r as usize] as u64).sum();
    let slots = crate::constants::MAX_PLAYERS as u64;
    regions
        .iter()
        .map(|&r| {
            let t = sizes[r as usize] as u64;
            (slots * t).div_ceil(total.max(1)).max(1) as u32
        })
        .collect()
}

/// Nearest tile to `from` belonging to any region in `want`, by the same
/// deterministic ring scan the dominant-region snap has always used.
fn nearest_region_tile(seed: u32, from: V2, want: &dyn Fn(u16) -> bool) -> Option<V2> {
    let grid = region_grid(seed);
    let sx = from.x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let sy = from.y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let half = crate::fx!("0.5");
    for r in 0..WORLD_SIZE {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let (tx, ty) = (sx + dx, sy + dy);
                if tx < 3 || ty < 3 || tx >= WORLD_SIZE - 3 || ty >= WORLD_SIZE - 3 {
                    continue;
                }
                if want(grid[(ty * WORLD_SIZE + tx) as usize]) {
                    return Some(V2::new(Fx::from_num(tx) + half, Fx::from_num(ty) + half));
                }
            }
        }
    }
    None
}

/// All eight seats at once. Slots are filled in order, each taking the nearest
/// tile of any island that still has room, so the seating is a pure function of
/// the seed and reads the same on every client. Cached because `start_point` is
/// called per slot by the fair-start top-up and the keep siting.
fn start_seats(seed: u32) -> &'static [V2] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static SEATS: OnceLock<Mutex<HashMap<u32, &'static [V2]>>> = OnceLock::new();
    let seats = SEATS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut s = seats.lock().unwrap();
    if let Some(&v) = s.get(&seed) {
        return v;
    }
    let regions = start_regions(seed);
    let quota = start_quota(seed);
    let main = dominant_region(seed);
    let mut used = vec![0u32; regions.len()];
    let mut out: Vec<V2> = Vec::with_capacity(crate::constants::MAX_PLAYERS);
    for slot in 0..crate::constants::MAX_PLAYERS {
        let c = crate::content::spawn_corner(slot);
        let open = |r: u16| {
            regions
                .iter()
                .position(|&q| q == r)
                .is_some_and(|i| used[i] < quota[i])
        };
        let p = nearest_region_tile(seed, c, &open)
            // every island full: fall back to the mainland, i.e. exactly the
            // rule this replaced, so the change adds no way to fail
            .or_else(|| nearest_region_tile(seed, c, &|r| r == main))
            .unwrap_or_else(|| find_land_near(seed, c.x, c.y));
        if let Some(i) = regions.iter().position(|&q| q == region_at(seed, p.x, p.y)) {
            used[i] += 1;
        }
        out.push(p);
    }
    let leaked: &'static [V2] = Box::leak(out.into_boxed_slice());
    s.insert(seed, leaked);
    leaked
}

/// Where slot `i` actually starts on this map: the spawn anchor snapped to the
/// nearest tile of a legal start island (`start_regions`). On a mainland preset
/// that is the dominant region and nothing else, so every player shares one
/// landmass; on an archipelago the eight seats spread over the islands the sea
/// connects, in proportion to how much land each one has.
pub fn start_point(seed: u32, slot: usize) -> V2 {
    start_seats(seed)[slot % crate::constants::MAX_PLAYERS]
}

/// How far the keep scan looks before it loosens the flatness cap.
const KEEP_SCAN_R: i32 = 70;

/// A safe keep site near the slot's start: every footprint tile passable,
/// buildable, FLAT, on the START'S OWN island, with open ground around it
/// (peasants must reach the deposit edge from all sides — a keep wedged
/// against cliffs/water strands its economy). Of the candidates on the
/// nearest ring that qualifies, the FLATTEST wins, so a keep never ends up
/// perched on a crest just because the scan reached it first.
pub fn find_keep_site(seed: u32, slot: usize, footprint: i32) -> V2 {
    let start = start_point(seed, slot);
    // the start's OWN landmass, which on a mainland preset IS the dominant one
    let main = region_at(seed, start.x, start.y);
    let grid = region_grid(seed);
    let half = crate::fx!("0.5");
    let fp_lo = -(footprint / 2);
    let fp_hi = footprint / 2 + footprint % 2;
    let sx = start.x.to_num::<i32>();
    let sy = start.y.to_num::<i32>();
    let flatness = |cx: i32, cy: i32| -> (Fx, Fx) {
        let c = V2::new(Fx::from_num(cx) + half, Fx::from_num(cy) + half);
        let mut worst = Fx::ZERO;
        for dy in fp_lo..fp_hi {
            for dx in fp_lo..fp_hi {
                worst = worst.max(slope_at(seed, c.x + Fx::from_num(dx), c.y + Fx::from_num(dy)));
            }
        }
        (worst, crate::buildings::footprint_relief(seed, footprint, c.x, c.y))
    };
    let ok = |cx: i32, cy: i32| -> bool {
        // footprint entirely on the mainland
        for dy in fp_lo..fp_hi {
            for dx in fp_lo..fp_hi {
                let (tx, ty) = (cx + dx, cy + dy);
                if tx < 4 || ty < 4 || tx >= WORLD_SIZE - 4 || ty >= WORLD_SIZE - 4 {
                    return false;
                }
                if grid[(ty * WORLD_SIZE + tx) as usize] != main {
                    return false;
                }
                let b = sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half).biome;
                if !crate::biomes::biome_buildable(b) {
                    return false;
                }
            }
        }
        // open ground: most tiles within radius 4 walkable on the mainland
        let mut open = 0;
        for dy in -4..=4i32 {
            for dx in -4..=4i32 {
                let (tx, ty) = (cx + dx, cy + dy);
                if tx >= 0
                    && ty >= 0
                    && tx < WORLD_SIZE
                    && ty < WORLD_SIZE
                    && grid[(ty * WORLD_SIZE + tx) as usize] == main
                {
                    open += 1;
                }
            }
        }
        open >= 58 // ~72% of the 9x9 block
    };
    // two relaxation steps of the flatness cap, then the terrain-only rules
    // over the whole map — the fall-through must never be an unchecked tile
    let caps = [
        (crate::buildings::BUILD_SLOPE_MAX, crate::buildings::FOUNDATION_RELIEF, KEEP_SCAN_R),
        (
            crate::buildings::BUILD_SLOPE_MAX * crate::fx!("1.7"),
            crate::buildings::FOUNDATION_RELIEF * crate::fx!("1.7"),
            KEEP_SCAN_R,
        ),
        (Fx::MAX, Fx::MAX, WORLD_SIZE),
    ];
    for (slope_cap, relief_cap, max_r) in caps {
        for r in 0..max_r {
            let mut best: Option<(Fx, i32, i32)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let (cx, cy) = (sx + dx, sy + dy);
                    if !ok(cx, cy) {
                        continue;
                    }
                    let (worst, relief) = flatness(cx, cy);
                    if worst > slope_cap || relief > relief_cap {
                        continue;
                    }
                    let score = worst + relief;
                    if best.is_none_or(|(b, _, _)| score < b) {
                        best = Some((score, cx, cy));
                    }
                }
            }
            if let Some((_, cx, cy)) = best {
                return V2::new(Fx::from_num(cx) + half, Fx::from_num(cy) + half);
            }
        }
    }
    start
}

/// Top up the scatter so EVERY spawn slot has the guaranteed minimum of wood /
/// stone / food within `FAIR_RADIUS` — placed deterministically on passable
/// tiles ringing the start, in the start's own connected region. Mirrored
/// fairness by construction: all slots get the same minima.
pub fn fair_start_nodes(
    seed: u32,
    existing: &[ScatteredNode],
    slots: usize,
    tree_yield: i32,
    stone_yield: i32,
    food_yield: i32,
) -> Vec<ScatteredNode> {
    let mut extra: Vec<ScatteredNode> = Vec::new();
    let r2 = FAIR_RADIUS * FAIR_RADIUS;
    for slot in 0..slots {
        let start = start_point(seed, slot);
        let region = region_at(seed, start.x, start.y);
        let mut have = [0usize; 3]; // wood, stone, food
        let count = |nodes: &[ScatteredNode], have: &mut [usize; 3]| {
            for n in nodes {
                let dx = n.pos.x - start.x;
                let dy = n.pos.y - start.y;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                // A grove on the far side of a cliff is not a grove this start
                // HAS. Counting it satisfied the guarantee without topping up,
                // and the top-up is the only thing that places in-region: seed
                // 48514 slot 1 had seven trees inside the radius and NOT ONE it
                // could walk to, so that base never gathered a stick of wood
                // for the whole match.
                if region_at(seed, n.pos.x, n.pos.y) != region {
                    continue;
                }
                match n.res_type {
                    ResourceType::Wood => have[0] += 1,
                    ResourceType::Stone => have[1] += 1,
                    ResourceType::Food => have[2] += 1,
                    ResourceType::Gold => {}
                }
            }
        };
        count(existing, &mut have);
        count(&extra, &mut have);

        let wants = [
            (FAIR_MIN_WOOD.saturating_sub(have[0]), ResourceType::Wood, tree_yield),
            (FAIR_MIN_STONE.saturating_sub(have[1]), ResourceType::Stone, stone_yield),
            (FAIR_MIN_FOOD.saturating_sub(have[2]), ResourceType::Food, food_yield),
        ];
        for (missing, res_type, yield_) in wants {
            let mut left = missing;
            if left == 0 {
                continue;
            }
            // AoE-style ring bands: each resource class starts its scan at
            // its own distance (food close, wood mid, stone farther), so a
            // start never spawns with everything piled at one distance; the
            // scan widens toward FAIR_RADIUS when the near rings come up dry
            let band_lo = match res_type {
                ResourceType::Food => 3,
                ResourceType::Wood => 4,
                ResourceType::Stone => 6,
                ResourceType::Gold => 8,
            };
            let sx = start.x.to_num::<i32>();
            let sy = start.y.to_num::<i32>();
            // Guaranteed stone goes on ground stone could have come from while
            // any is in reach — otherwise the one deposit every start is owed
            // is also the one deposit sitting in the middle of a meadow. The
            // unfiltered second pass is what keeps the guarantee absolute.
            for pass in 0..2 {
                if left == 0 {
                    break;
                }
                let rocky_only = pass == 0 && res_type == ResourceType::Stone;
                'ring: for r in band_lo..(FAIR_RADIUS.to_num::<i32>()) {
                    // walk the ring perimeter starting at a per-(slot, resource)
                    // hashed corner so different kinds land on different sides
                    let perimeter: Vec<(i32, i32)> = {
                        let mut cells = Vec::with_capacity((8 * r) as usize);
                        for dx in -r..=r {
                            cells.push((dx, -r));
                        }
                        for dy in (-r + 1)..=r {
                            cells.push((r, dy));
                        }
                        for dx in (-r..r).rev() {
                            cells.push((dx, r));
                        }
                        for dy in ((-r + 1)..r).rev() {
                            cells.push((-r, dy));
                        }
                        let spin = (hash2(slot as i32 * 31 + 7, res_type as i32 * 17 + 3, seed)
                            * Fx::from_num(cells.len() as i32))
                        .to_num::<usize>()
                            % cells.len().max(1);
                        cells.rotate_left(spin);
                        cells
                    };
                    for (dx, dy) in perimeter {
                        {
                            let (tx, ty) = (sx + dx, sy + dy);
                            if tx < 3 || ty < 3 || tx >= WORLD_SIZE - 3 || ty >= WORLD_SIZE - 3 {
                                continue;
                            }
                            // the rings are square, the guarantee is a circle:
                            // a corner tile of ring 18 is 25 tiles out and does
                            // not count towards the minimum it was placed for
                            let (fdx, fdy) = (Fx::from_num(dx), Fx::from_num(dy));
                            if fdx * fdx + fdy * fdy > r2 {
                                continue;
                            }
                            if !is_passable(seed, tx, ty) {
                                continue;
                            }
                            let p = V2::new(
                                Fx::from_num(tx) + crate::fx!("0.5"),
                                Fx::from_num(ty) + crate::fx!("0.5"),
                            );
                            if region_at(seed, p.x, p.y) != region {
                                continue;
                            }
                            if rocky_only
                                && slope_at(seed, p.x, p.y) < crate::content::SCREE_T
                                && crate::biomes::rock_density(
                                    sample_terrain(seed, p.x, p.y).biome,
                                ) == Fx::ZERO
                            {
                                continue;
                            }
                            // thin out: accept ~1 tile in 3, hashed per kind
                            if hash2(tx, ty, mix_seed(seed, res_type as u32 + 77)) > crate::fx!("0.34") {
                                continue;
                            }
                            // keep clear of already-placed nodes on the same tile
                            let occupied = existing
                                .iter()
                                .chain(extra.iter())
                                .any(|n| n.pos.x.to_num::<i32>() == tx && n.pos.y.to_num::<i32>() == ty);
                            if occupied {
                                continue;
                            }
                            extra.push(ScatteredNode { pos: p, res_type, yield_, regen: 0 });
                            left -= 1;
                            if left == 0 {
                                break 'ring;
                            }
                        }
                    }
                }
            }
        }
    }
    extra
}

/// Nearest buildable land near (x, y), via deterministic integer ring scan.
pub fn find_land_near(seed: u32, x: Fx, y: Fx) -> V2 {
    if is_land(seed, x, y) {
        return V2::new(x, y);
    }
    let lo = Fx::from_num(3);
    let hi = Fx::from_num(WORLD_SIZE - 3);
    for r in 1..WORLD_SIZE {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let nx = (x + Fx::from_num(dx)).clamp(lo, hi);
                let ny = (y + Fx::from_num(dy)).clamp(lo, hi);
                if is_land(seed, nx, ny) {
                    return V2::new(nx, ny);
                }
            }
        }
    }
    let c = Fx::from_num(WORLD_SIZE) / Fx::from_num(2);
    V2::new(c, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_is_reproducible() {
        let a = sample_terrain(7, crate::fx!("40.5"), crate::fx!("60.25"));
        let b = sample_terrain(7, crate::fx!("40.5"), crate::fx!("60.25"));
        assert_eq!(a.height, b.height);
        assert_eq!(a.biome, b.biome);
    }

    #[test]
    fn map_has_land_and_water() {
        let mut land = 0;
        let mut water = 0;
        let mut y = 4;
        while y < WORLD_SIZE - 4 {
            let mut x = 4;
            while x < WORLD_SIZE - 4 {
                if is_passable(11, x, y) { land += 1 } else { water += 1 }
                x += 4;
            }
            y += 4;
        }
        assert!(land > 0 && water > 0, "expected mixed land/water, got {land}/{water}");
    }

    #[test]
    fn scatter_is_deterministic_and_reachable() {
        let rules = [ScatterRule {
            res_type: ResourceType::Wood,
            count: 50,
            yield_: 120,
            density: |s| crate::biomes::tree_density(s.biome),
            domain: ScatterDomain::Land,
            regen: 0,
            clustered: true,
            patch: (1, 1),
        }];
        let a = scatter_nodes(3, &rules);
        let b = scatter_nodes(3, &rules);
        assert_eq!(a.len(), b.len());
        for (na, nb) in a.iter().zip(b.iter()) {
            assert_eq!(na.pos, nb.pos);
        }
        // every placed node sits on land
        for n in &a {
            assert!(is_land(3, n.pos.x, n.pos.y));
        }
    }
}
