use crate::biomes::{Biome, biome_passable};
use crate::constants::WORLD_SIZE;
use crate::enums::ResourceType;
use crate::math::{Fx, V2};
use crate::noise::fbm;
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
const M_SCALE: Fx = crate::fx!("0.03");
const WARP_SCALE: Fx = crate::fx!("0.02");
const WARP_AMP: Fx = crate::fx!("9");
const SEA: Fx = crate::fx!("0.38");

// rivers: a winding fbm channel carved through the lowlands
const RIVER_SCALE: Fx = crate::fx!("0.013");
const RIVER_HALF_W: Fx = crate::fx!("0.017");
// archipelago: large-scale blob mask that shatters the continent
const ISLAND_SCALE: Fx = crate::fx!("0.015");
const FORD_SCALE: Fx = crate::fx!("0.05");
const FORD_T: Fx = crate::fx!("0.58");
// cliffs: where the height field steps too fast for a walker
const CLIFF_STEP: Fx = crate::fx!("0.05");
const RAMP_SCALE: Fx = crate::fx!("0.06");
const RAMP_T: Fx = crate::fx!("0.64");
// mountain passes keep ranges crossable
const PASS_SCALE: Fx = crate::fx!("0.045");
const PASS_T: Fx = crate::fx!("0.66");
// fertile pockets in the dry belt
const OASIS_SCALE: Fx = crate::fx!("0.06");
const OASIS_T: Fx = crate::fx!("0.74");

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

/// Seeded continental mask: low-frequency plate noise instead of the old
/// centered disc, so different seeds get genuinely different geography —
/// off-center continents, inland seas, peninsulas, island chains — with a
/// guaranteed ocean ring at the map border (the camera/ocean disc expect it).
const CONT_SCALE: Fx = crate::fx!("0.006");

fn continent(base: u32, x: Fx, y: Fx) -> Fx {
    let c = fbm(
        x * CONT_SCALE + Fx::from_num(53),
        y * CONT_SCALE + Fx::from_num(71),
        base ^ 0xc047,
        3,
    );
    // remap: below the basin threshold sinks to ocean, above plateaus to land
    let plate = ((c - crate::fx!("0.26")) * crate::fx!("2.4")).clamp(Fx::ZERO, crate::fx!("1.1"));
    let plates = crate::fx!("0.32") + plate * crate::fx!("0.8");
    // a weak central dome anchors a mainland so no seed rolls all-ocean; the
    // plates carve bays, peninsulas and side continents around/through it
    let cc = Fx::from_num(WORLD_SIZE) / Fx::from_num(2);
    let dx = x - cc;
    let dy = y - cc;
    let d2 = (dx * dx + dy * dy) / (cc * cc);
    let dome = (crate::fx!("1.02") - d2 * crate::fx!("1.15")).max(Fx::ZERO) * crate::fx!("0.8");
    let m = Fx::from_num(WORLD_SIZE);
    let edge = (x.min(y).min(m - x).min(m - y) / crate::fx!("16")).clamp(Fx::ZERO, Fx::ONE);
    plates.max(dome) * edge
}

/// The raw height field (warp + fbm + island mask + radial falloff) — shared
/// by the full sample and the cliff gradient probe, which must read the SAME
/// surface.
fn height_at(base: u32, island_gain: Fx, x: Fx, y: Fx) -> Fx {
    let half = crate::fx!("0.5");
    let two = crate::fx!("2");
    let wx = (fbm(x * WARP_SCALE, y * WARP_SCALE, base ^ 0x1b56, 3) - half) * two * WARP_AMP;
    let wy = (fbm(x * WARP_SCALE + Fx::from_num(31), y * WARP_SCALE + Fx::from_num(17), base ^ 0x77c1, 3)
        - half)
        * two
        * WARP_AMP;
    let mut h = fbm((x + wx) * H_SCALE, (y + wy) * H_SCALE, base, 5);
    h = h * crate::fx!("0.78") + crate::fx!("0.18");
    if island_gain > Fx::ZERO {
        let mask = fbm(x * ISLAND_SCALE + Fx::from_num(7), y * ISLAND_SCALE + Fx::from_num(13), base ^ 0x15a7, 3);
        // blobs keep their height; the straits between them sink to sea floor
        let blob = ((mask - crate::fx!("0.12")) * crate::fx!("1.9")).clamp(Fx::ZERO, Fx::ONE);
        h *= Fx::ONE - island_gain + island_gain * blob;
    }
    h * continent(base, x, y)
}

pub fn sample_terrain(seed: u32, x: Fx, y: Fx) -> TerrainSample {
    let bias = seed_bias(seed);
    let base = seed_base(seed);
    let half = crate::fx!("0.5");
    let two = crate::fx!("2");
    let sea = SEA + bias.sea_shift;

    let wx = (fbm(x * WARP_SCALE, y * WARP_SCALE, base ^ 0x1b56, 3) - half) * two * WARP_AMP;
    let wy = (fbm(x * WARP_SCALE + Fx::from_num(31), y * WARP_SCALE + Fx::from_num(17), base ^ 0x77c1, 3)
        - half)
        * two
        * WARP_AMP;

    let h = height_at(base, bias.island_gain, x, y);

    let moisture = fbm(
        (x + wx) * M_SCALE + Fx::from_num(100),
        (y + wy) * M_SCALE + Fx::from_num(50),
        base ^ 0x9e37,
        4,
    ) + bias.moist_shift;

    let mut biome = classify(h, moisture, sea, base, x, y);

    // rivers carve the lowlands between the coast and the high country; a
    // low-frequency ford channel periodically interrupts them with crossings
    if bias.river_gain > Fx::ZERO
        && biome_passable(biome)
        && h >= sea
        && h < crate::fx!("0.66")
    {
        let rv = fbm(x * RIVER_SCALE, y * RIVER_SCALE, base ^ 0x52e5, 4);
        // taper: rivers narrow as the land climbs (they "start" in the hills)
        let altitude = ((crate::fx!("0.66") - h) / (crate::fx!("0.66") - sea)).clamp(Fx::ZERO, Fx::ONE);
        let w = RIVER_HALF_W * bias.river_gain * (crate::fx!("0.35") + altitude * crate::fx!("0.65"));
        if (rv - half).abs() < w {
            let ford = fbm(x * FORD_SCALE + Fx::from_num(7), y * FORD_SCALE + Fx::from_num(3), base ^ 0xf00d, 3);
            biome = if ford > FORD_T { Biome::Ford } else { Biome::River };
        }
    }

    // cliffs: a too-steep height step becomes a wall, except where the ramp
    // channel cuts an opening
    if bias.cliff_gain > Fx::ZERO
        && biome_passable(biome)
        && biome != Biome::Ford
        && h > crate::fx!("0.5")
    {
        let step = CLIFF_STEP / bias.cliff_gain;
        let one = Fx::ONE;
        let ig = bias.island_gain;
        let grad = (height_at(base, ig, x + one, y) - h)
            .abs()
            .max((height_at(base, ig, x - one, y) - h).abs())
            .max((height_at(base, ig, x, y + one) - h).abs())
            .max((height_at(base, ig, x, y - one) - h).abs());
        if grad > step {
            let ramp = fbm(x * RAMP_SCALE + Fx::from_num(13), y * RAMP_SCALE + Fx::from_num(29), base ^ 0xc11f, 3);
            if ramp <= RAMP_T {
                biome = Biome::Cliff;
            }
        }
    }

    TerrainSample { height: h, moisture, biome }
}

fn classify(h: Fx, m: Fx, sea: Fx, base: u32, x: Fx, y: Fx) -> Biome {
    if h < sea - crate::fx!("0.06") {
        return Biome::DeepWater;
    }
    if h < sea {
        return Biome::ShallowWater;
    }
    if h < sea + crate::fx!("0.04") {
        return Biome::Sand;
    }
    if h > crate::fx!("0.82") {
        return Biome::Snow;
    }
    if h > crate::fx!("0.72") {
        // mountain passes: a noise channel cuts walkable saddles through the
        // ranges so high country never splits the mainland in two
        let pv = fbm(x * PASS_SCALE + Fx::from_num(17), y * PASS_SCALE + Fx::from_num(23), base ^ 0x9a55, 3);
        return if pv > PASS_T { Biome::Hills } else { Biome::Mountain };
    }
    if h > crate::fx!("0.6") {
        return Biome::Hills;
    }
    if m < crate::fx!("0.26") {
        if h < sea + crate::fx!("0.12") {
            return Biome::Oasis;
        }
        // fertile pockets deep in the dry belt — palms around hidden water
        let ov = fbm(x * OASIS_SCALE + Fx::from_num(41), y * OASIS_SCALE + Fx::from_num(59), base ^ 0x0a51, 3);
        return if ov > OASIS_T { Biome::Oasis } else { Biome::Desert };
    }
    if m < crate::fx!("0.4") {
        return Biome::Dunes;
    }
    if m < crate::fx!("0.52") {
        return Biome::Steppe;
    }
    if m < crate::fx!("0.72") {
        return Biome::Grassland;
    }
    Biome::Forest
}

pub fn is_land(seed: u32, x: Fx, y: Fx) -> bool {
    biome_passable(sample_terrain(seed, x, y).biome)
}

const ADJ4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Buildable land with open water on at least one orthogonal neighbour.
pub fn is_coastal(seed: u32, x: Fx, y: Fx) -> bool {
    if !is_land(seed, x, y) {
        return false;
    }
    for (dx, dy) in ADJ4 {
        let b = sample_terrain(seed, x + Fx::from_num(dx), y + Fx::from_num(dy)).biome;
        if b == Biome::DeepWater || b == Biome::ShallowWater {
            return true;
        }
    }
    false
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

/// Tile-space buildability (in-bounds + buildable biome).
pub fn is_buildable_tile(seed: u32, tx: i32, ty: i32) -> bool {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return false;
    }
    buildable_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// True open water (sea or river) — the Fishing Hut's shoreline test. NOT the
/// same as "impassable" (cliffs/mountains are impassable but dry).
pub fn is_water_tile(seed: u32, tx: i32, ty: i32) -> bool {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return false;
    }
    let half = crate::fx!("0.5");
    matches!(
        sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half).biome,
        Biome::DeepWater | Biome::ShallowWater | Biome::River
    )
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
            let pass = passable_grid(seed);
            let n = (WORLD_SIZE * WORLD_SIZE) as usize;
            let mut v = vec![u16::MAX; n];
            let mut next_region: u16 = 0;
            let mut stack: Vec<i32> = Vec::new();
            for start in 0..n {
                if !pass[start] || v[start] != u16::MAX {
                    continue;
                }
                let region = next_region;
                next_region += 1;
                v[start] = region;
                stack.push(start as i32);
                while let Some(idx) = stack.pop() {
                    let (tx, ty) = (idx % WORLD_SIZE, idx / WORLD_SIZE);
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (tx + dx, ty + dy);
                        if nx < 0 || ny < 0 || nx >= WORLD_SIZE || ny >= WORLD_SIZE {
                            continue;
                        }
                        let ni = (ny * WORLD_SIZE + nx) as usize;
                        if pass[ni] && v[ni] == u16::MAX {
                            v[ni] = region;
                            stack.push(ni as i32);
                        }
                    }
                }
            }
            let leaked: &'static [u16] = Box::leak(v.into_boxed_slice());
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

/// Render elevation in world units (client mesh only — never feeds the sim).
/// Water renders as a flat SEA SURFACE (short shoreline shelf, then constant
/// level): the sea is a body of water, not a terrain dent — and the backdrop
/// ocean disc must never poke through inside the map.
pub fn render_height(h: Fx, emphasis: Fx, elev_gain: Fx) -> Fx {
    if h < SEA {
        let shelf = ((SEA - h) / crate::fx!("0.05")).min(Fx::ONE);
        return crate::fx!("-0.4") * shelf - crate::fx!("0.03");
    }
    let base = (h - SEA) * Fx::from_num(9);
    let relief = base + base * base * crate::fx!("0.18");
    relief * emphasis * elev_gain
}

#[derive(Clone, Copy, Debug)]
pub struct ScatteredNode {
    pub pos: V2,
    pub res_type: ResourceType,
    pub yield_: i32,
}

/// One scatter rule: count, yield, per-biome accept-probability, coastal-only.
/// `clustered` modulates acceptance with a grove-mask noise so the kind lands
/// in clumps (forests as woods, not uniform confetti).
#[derive(Clone, Copy)]
pub struct ScatterRule {
    pub res_type: ResourceType,
    pub count: i32,
    pub yield_: i32,
    pub density: fn(Biome) -> Fx,
    pub coastal_only: bool,
    pub clustered: bool,
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
        while placed < rule.count && attempts < budget {
            attempts += 1;
            let x = three + rand.next_fx() * span;
            let y = three + rand.next_fx() * span;
            let reachable =
                if rule.coastal_only { is_coastal(seed, x, y) } else { is_land(seed, x, y) };
            if !reachable {
                continue;
            }
            let roll = hash2(x.floor().to_num::<i32>(), y.floor().to_num::<i32>(), roll_seed);
            let biome = sample_terrain(seed, x, y).biome;
            let mut density = (rule.density)(biome);
            if rule.clustered {
                let gv = fbm(x * GROVE_SCALE, y * GROVE_SCALE, base ^ 0x6701, 3);
                density *= if gv > GROVE_T { GROVE_BOOST } else { GROVE_CUT };
            }
            if roll < density {
                out.push(ScatteredNode { pos: V2::new(x, y), res_type: rule.res_type, yield_: rule.yield_ });
                placed += 1;
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

/// The map's biggest connected region — the "mainland" every player starts on.
pub fn dominant_region(seed: u32) -> u16 {
    let grid = region_grid(seed);
    let mut counts: [u32; 256] = [0; 256];
    let mut overflow: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();
    for &r in grid {
        if r == u16::MAX {
            continue;
        }
        if (r as usize) < counts.len() {
            counts[r as usize] += 1;
        } else {
            *overflow.entry(r).or_insert(0) += 1;
        }
    }
    let mut best = (0u16, 0u32);
    for (r, &c) in counts.iter().enumerate() {
        if c > best.1 {
            best = (r as u16, c);
        }
    }
    for (r, c) in overflow {
        if c > best.1 {
            best = (r, c);
        }
    }
    best.0
}

/// Where slot `i` actually starts on this map: the spawn anchor snapped to the
/// nearest tile of the DOMINANT region, so every player shares one landmass
/// (rivers stay crossable via fords; nobody founds on a sliver island).
pub fn start_point(seed: u32, slot: usize) -> V2 {
    let c = crate::content::spawn_corner(slot);
    let main = dominant_region(seed);
    let grid = region_grid(seed);
    let sx = c.x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let sy = c.y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
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
                if grid[(ty * WORLD_SIZE + tx) as usize] == main {
                    return V2::new(Fx::from_num(tx) + half, Fx::from_num(ty) + half);
                }
            }
        }
    }
    find_land_near(seed, c.x, c.y)
}

/// A safe keep site near the slot's start: every footprint tile passable,
/// buildable, on the DOMINANT region, with open ground around it (peasants
/// must reach the deposit edge from all sides — a keep wedged against
/// cliffs/water strands its economy).
pub fn find_keep_site(seed: u32, slot: usize, footprint: i32) -> V2 {
    let start = start_point(seed, slot);
    let main = dominant_region(seed);
    let grid = region_grid(seed);
    let half = crate::fx!("0.5");
    let fp_lo = -(footprint / 2);
    let fp_hi = footprint / 2 + footprint % 2;
    let sx = start.x.to_num::<i32>();
    let sy = start.y.to_num::<i32>();
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
    for r in 0..WORLD_SIZE {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let (cx, cy) = (sx + dx, sy + dy);
                if ok(cx, cy) {
                    return V2::new(Fx::from_num(cx) + half, Fx::from_num(cy) + half);
                }
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
            // deterministic ring scan outward from the start; hash picks the
            // tile order apart so different kinds spread to different sides
            let sx = start.x.to_num::<i32>();
            let sy = start.y.to_num::<i32>();
            'ring: for r in 4..(FAIR_RADIUS.to_num::<i32>()) {
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dy.abs()) != r {
                            continue;
                        }
                        let (tx, ty) = (sx + dx, sy + dy);
                        if tx < 3 || ty < 3 || tx >= WORLD_SIZE - 3 || ty >= WORLD_SIZE - 3 {
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
                        extra.push(ScatteredNode { pos: p, res_type, yield_ });
                        left -= 1;
                        if left == 0 {
                            break 'ring;
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
            density: crate::biomes::tree_density,
            coastal_only: false,
            clustered: true,
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
