//! Grid worldgen pipeline — the authority for every tile's height, biome,
//! climate, soil and ore. Built once per seed, cached + leaked like
//! `passable_grid`.
//!
//! Stages (all deterministic fixed-point; ties broken by cell index):
//!   1. tectonics + corner height field: plate crust decides where land is and
//!      where ranges rise, warped fbm supplies the detail (`terrain::height_at`)
//!   2. thermal erosion sweeps: talus transport softens noise spikes into
//!      coherent slopes (double-buffered, order-independent)
//!   3. hydraulic incision: two stream-power passes cut the drainage network
//!      into the land, so valleys are carved BY the rivers that later run in
//!      them and ridges sharpen between them
//!   4. depression filling (Barnes priority-flood) + basins: wet basins hold
//!      lakes, arid basins evaporate into salt pans
//!   5. D8 flow routing + accumulation: rain follows steepest descent;
//!      accumulation picks out real drainage trunks
//!   6. rivers: high-accumulation land becomes channel (Ford where a hashed
//!      crossing allows it), with deltas and marsh where a trunk meets the sea
//!   7. climate: latitude temperature with an elevation lapse rate, and
//!      precipitation from an advected moisture parcel (see `climate.rs`)
//!   8. soil + ore: alluvium along the drainage, mineralization along the
//!      orogenic seams
//!   9. classify: Whittaker(temperature x precipitation) for the lowlands,
//!      climate-aware highlands, cliffs where the corner gradient steps too
//!      fast, and the arid/basin refinements (dunes, hammada, oasis, wadi)

use crate::biomes::Biome;
use crate::climate::{ClimateArchetype, ClimateField, climate_archetype, snow_line};
use crate::constants::WORLD_SIZE;
use crate::math::{Fx, fx_sqrt};
use crate::noise::fbm;
use crate::plates::Plates;
use crate::rng::hash2;
use crate::terrain::{SEA, height_at, seed_base, seed_bias};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub struct WorldGrid {
    /// (N+1)^2 corner heights, post-erosion + river carving — bilinear
    /// interpolation of these is THE height surface everyone sees.
    pub corner_h: Vec<Fx>,
    /// N^2 per-tile biome.
    pub biome: Vec<Biome>,
    /// N^2 per-tile moisture (0..1) — the precipitation field, kept under the
    /// old name because gameplay and render both read it.
    pub moisture: Vec<Fx>,
    /// N^2 per-tile center height (average of the 4 carved corners).
    pub tile_h: Vec<Fx>,
    /// N^2 per-tile temperature 0..1 (1 = hottest).
    pub temp: Vec<Fx>,
    /// N^2 soil fertility 0..1 — what a farm yields on this tile.
    pub fertility: Vec<Fx>,
    /// N^2 ore potential 0..1 — where gold and rich stone can be found.
    pub ore: Vec<Fx>,
    /// The climate regime this seed rolled.
    pub climate: &'static ClimateArchetype,
    /// Prevailing wind, for render dressing and dune orientation.
    pub wind: (i32, i32),
}

const N: usize = WORLD_SIZE as usize;
const C: usize = N + 1;

// thermal erosion: material moves where the slope exceeds the talus angle
const EROSION_SWEEPS: usize = 6;
const TALUS: Fx = crate::fx!("0.045");
const EROSION_K: Fx = crate::fx!("0.24");

// hydraulic incision: h -= K * sqrt(accumulation) * slope, twice
const INCISION_PASSES: usize = 2;
const INCISION_K: Fx = crate::fx!("0.0016");
const INCISION_MAX: Fx = crate::fx!("0.035");

// rivers: accumulation threshold (scaled down by the preset's river_gain)
const RIVER_ACC_BASE: Fx = crate::fx!("260");
const RIVER_WIDE_MUL: Fx = crate::fx!("5");
const FORD_HASH_T: Fx = crate::fx!("0.74");
const CARVE_MAX: Fx = crate::fx!("0.05");

// classification bands
const BEACH_BAND: Fx = crate::fx!("0.035");
const HILL_T: Fx = crate::fx!("0.62");
const MOUNTAIN_T: Fx = crate::fx!("0.78");
const CLIFF_STEP: Fx = crate::fx!("0.055");
const PASS_SCALE: Fx = crate::fx!("0.045");
const PASS_T: Fx = crate::fx!("0.6");
const RAMP_SCALE: Fx = crate::fx!("0.06");
const RAMP_T: Fx = crate::fx!("0.64");

// arid refinements
/// Precipitation under which a channel only runs after a storm.
const WADI_T: Fx = crate::fx!("0.17");
/// Dryness over which a closed basin evaporates into a salt pan.
const SALT_DRYNESS: Fx = crate::fx!("0.50");
/// Sand seas need flat ground, no rain and a long dry fetch.
const DUNE_SCALE: Fx = crate::fx!("0.035");
const DUNE_T: Fx = crate::fx!("0.52");
/// Desert within reach of fresh water greens up.
const OASIS_REACH: i32 = 3;

const D4: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
const D8: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

#[inline]
fn tidx(tx: usize, ty: usize) -> usize {
    ty * N + tx
}

#[inline]
fn cidx(cx: usize, cy: usize) -> usize {
    cy * C + cx
}

fn tile_height(corner: &[Fx], tx: usize, ty: usize) -> Fx {
    (corner[cidx(tx, ty)] + corner[cidx(tx + 1, ty)] + corner[cidx(tx, ty + 1)] + corner[cidx(tx + 1, ty + 1)])
        / crate::fx!("4")
}

fn retile(corner: &[Fx], tile_h: &mut [Fx]) {
    for (i, h) in tile_h.iter_mut().enumerate() {
        *h = tile_height(corner, i % N, i / N);
    }
}

/// Sink the four corners under a tile to at most `bed`.
fn carve(corner: &mut [Fx], tx: usize, ty: usize, bed: Fx) {
    for (cx, cy) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
        let ci = cidx(cx, cy);
        if corner[ci] > bed {
            corner[ci] = bed;
        }
    }
}

/// Raise the four corners under a tile to at least `floor_h`.
fn raise(corner: &mut [Fx], tx: usize, ty: usize, floor_h: Fx) {
    for (cx, cy) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
        let ci = cidx(cx, cy);
        if corner[ci] < floor_h {
            corner[ci] = floor_h;
        }
    }
}

/// Barnes priority-flood: every land cell ends with a monotone path to the map
/// border, and the amount each cell had to be raised is how deep a closed
/// basin was.
fn flood_fill_depressions(tile_h: &[Fx]) -> Vec<Fx> {
    let eps = Fx::from_bits(1 << 8);
    let mut filled: Vec<Fx> = tile_h.to_vec();
    let mut visited = vec![false; N * N];
    let mut heap: BinaryHeap<Reverse<(Fx, u32)>> = BinaryHeap::new();
    for ty in 0..N {
        for tx in 0..N {
            if tx == 0 || ty == 0 || tx == N - 1 || ty == N - 1 {
                let i = tidx(tx, ty);
                visited[i] = true;
                heap.push(Reverse((filled[i], i as u32)));
            }
        }
    }
    while let Some(Reverse((level, i))) = heap.pop() {
        let (tx, ty) = ((i as usize) % N, (i as usize) / N);
        for (dx, dy) in D4 {
            let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                continue;
            }
            let j = tidx(nx as usize, ny as usize);
            if visited[j] {
                continue;
            }
            visited[j] = true;
            if filled[j] < level + eps {
                filled[j] = level + eps;
            }
            heap.push(Reverse((filled[j], j as u32)));
        }
    }
    filled
}

/// D8 steepest-descent routing over a depression-filled surface, plus the
/// upslope area draining through every cell.
fn route_flow(tile_h: &[Fx], filled: &[Fx], sea: Fx) -> (Vec<i32>, Vec<Fx>) {
    let mut flow: Vec<i32> = vec![-1; N * N];
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            if tile_h[i] < sea {
                continue; // ocean cells absorb
            }
            let mut best = filled[i];
            for (dx, dy) in D8 {
                let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
                if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                    continue;
                }
                let j = tidx(nx as usize, ny as usize);
                if filled[j] < best {
                    best = filled[j];
                    flow[i] = j as i32;
                }
            }
        }
    }
    let mut order: Vec<u32> = (0..(N * N) as u32).collect();
    order.sort_unstable_by_key(|&i| (Reverse(filled[i as usize]), i));
    let mut acc: Vec<Fx> = vec![Fx::ONE; N * N];
    for &i in &order {
        let i = i as usize;
        if flow[i] >= 0 {
            let a = acc[i];
            acc[flow[i] as usize] += a;
        }
    }
    (flow, acc)
}

fn build(seed: u32) -> WorldGrid {
    let base = seed_base(seed);
    let bias = seed_bias(seed);
    let sea = SEA + bias.sea_shift;
    let clim = climate_archetype(seed);
    let plates = Plates::new(base);

    // ── 1. corner heights over the plate field ──────────────────────────────
    let mut corner: Vec<Fx> = vec![Fx::ZERO; C * C];
    for cy in 0..C {
        for cx in 0..C {
            corner[cidx(cx, cy)] =
                height_at(&plates, base, bias, Fx::from_num(cx as i32), Fx::from_num(cy as i32));
        }
    }

    // ── 2. thermal erosion (double-buffered, order-independent) ─────────────
    let mut next = corner.clone();
    for _ in 0..EROSION_SWEEPS {
        for cy in 1..C - 1 {
            for cx in 1..C - 1 {
                let i = cidx(cx, cy);
                let h = corner[i];
                if h < sea {
                    continue; // seabed keeps its noise — only land erodes
                }
                let mut low = h;
                let mut low_i = i;
                for (dx, dy) in D4 {
                    let j = cidx((cx as i32 + dx) as usize, (cy as i32 + dy) as usize);
                    if corner[j] < low {
                        low = corner[j];
                        low_i = j;
                    }
                }
                let drop = h - low;
                if drop > TALUS {
                    let moved = (drop - TALUS) * EROSION_K;
                    next[i] -= moved;
                    next[low_i] += moved;
                }
            }
        }
        corner.copy_from_slice(&next);
    }

    let mut tile_h: Vec<Fx> = vec![Fx::ZERO; N * N];
    retile(&corner, &mut tile_h);

    // ── 3. hydraulic incision ───────────────────────────────────────────────
    // Rivers cut their own valleys: stream power (upslope area x local slope)
    // lowers the bed, which sharpens the ridges between drainages and gives
    // the later river pass a trench to run in instead of a plateau lip.
    for _ in 0..INCISION_PASSES {
        let filled = flood_fill_depressions(&tile_h);
        let (flow, acc) = route_flow(&tile_h, &filled, sea);
        for ty in 0..N {
            for tx in 0..N {
                let i = tidx(tx, ty);
                if tile_h[i] < sea || flow[i] < 0 {
                    continue;
                }
                let down = flow[i] as usize;
                let slope = (tile_h[i] - tile_h[down]).max(Fx::ZERO);
                let cut = (INCISION_K * fx_sqrt(acc[i]) * slope).min(INCISION_MAX);
                if cut > Fx::ZERO {
                    carve(&mut corner, tx, ty, tile_h[i] - cut);
                }
            }
        }
        retile(&corner, &mut tile_h);
    }

    // ── 4. depressions: lakes where it rains, salt pans where it does not ───
    let filled = flood_fill_depressions(&tile_h);
    const BASIN_DEPTH: Fx = crate::fx!("0.022");
    let mut lake = vec![false; N * N];
    let mut salt = vec![false; N * N];
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            if tile_h[i] < sea || filled[i] - tile_h[i] <= BASIN_DEPTH {
                continue;
            }
            if crate::climate::coarse_dryness(clim, ty) > SALT_DRYNESS {
                salt[i] = true;
            } else {
                lake[i] = true;
            }
        }
    }
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            if lake[i] {
                // the bed sits below its banks so the lake reads as real water
                carve(&mut corner, tx, ty, (tile_h[i] - crate::fx!("0.01")).min(sea - crate::fx!("0.012")));
            } else if salt[i] {
                // a sabkha is a flat pan at the basin floor, not a hole
                let pan = filled[i].max(sea + crate::fx!("0.006"));
                carve(&mut corner, tx, ty, pan);
                raise(&mut corner, tx, ty, pan);
            }
        }
    }
    retile(&corner, &mut tile_h);

    // ── 5. flow over the final surface ──────────────────────────────────────
    let filled = flood_fill_depressions(&tile_h);
    let (_flow, acc) = route_flow(&tile_h, &filled, sea);

    // ── 6. rivers, deltas and marsh ─────────────────────────────────────────
    let mut river = vec![false; N * N];
    let mut wide = vec![false; N * N];
    let mut marsh = vec![false; N * N];
    if bias.river_gain > Fx::ZERO {
        let th = RIVER_ACC_BASE / bias.river_gain;
        let wide_th = th * RIVER_WIDE_MUL;
        for i in 0..N * N {
            if tile_h[i] >= sea && !salt[i] && !lake[i] && acc[i] >= th {
                river[i] = true;
                wide[i] = acc[i] >= wide_th;
            }
        }
        for ty in 1..N - 1 {
            for tx in 1..N - 1 {
                let i = tidx(tx, ty);
                if !wide[i] {
                    continue;
                }
                let mut low_j = i;
                let mut low = tile_h[i];
                for (dx, dy) in D4 {
                    let j = tidx((tx as i32 + dx) as usize, (ty as i32 + dy) as usize);
                    if !river[j] && tile_h[j] >= sea && tile_h[j] < low {
                        low = tile_h[j];
                        low_j = j;
                    }
                }
                if low_j != i {
                    river[low_j] = true;
                }
            }
        }
        for ty in 0..N {
            for tx in 0..N {
                let i = tidx(tx, ty);
                if !river[i] {
                    continue;
                }
                let depth = (fx_sqrt(acc[i]) * crate::fx!("0.004")).min(CARVE_MAX);
                carve(&mut corner, tx, ty, (tile_h[i] - depth).min(sea - crate::fx!("0.012")));
            }
        }
        retile(&corner, &mut tile_h);

        // deltas: where a big trunk reaches the sea over flat ground it spreads
        // into reed marsh — the richest soil and the worst place to build
        for ty in 1..N - 1 {
            for tx in 1..N - 1 {
                let i = tidx(tx, ty);
                if tile_h[i] < sea || river[i] || lake[i] || salt[i] {
                    continue;
                }
                if tile_h[i] > sea + crate::fx!("0.05") {
                    continue;
                }
                let mut near_trunk = false;
                let mut near_sea = false;
                for (dx, dy) in D8 {
                    let j = tidx((tx as i32 + dx) as usize, (ty as i32 + dy) as usize);
                    near_trunk |= wide[j];
                    near_sea |= tile_h[j] < sea;
                }
                if near_trunk && near_sea {
                    marsh[i] = true;
                }
            }
        }
    }

    // ── 7. climate ──────────────────────────────────────────────────────────
    let water_mask: Vec<bool> =
        (0..N * N).map(|i| tile_h[i] < sea || river[i] || lake[i]).collect();
    let ClimateField { temp, precip, wind } = crate::climate::build(
        seed,
        clim,
        &tile_h,
        &|i| water_mask[i],
        bias.moist_shift,
        sea,
    );

    // ── 8. soil and ore ─────────────────────────────────────────────────────
    // Fertility follows the water: floodplains and deltas carry the alluvium,
    // volcanic arcs weather into good soil, steep or frozen ground carries
    // none. Ore follows the plate seams that actually mineralize.
    // How far the floodplain reaches from fresh water, in tiles.
    const FLOODPLAIN_REACH: i32 = 7;
    let mut to_water: Vec<i32> = vec![i32::MAX; N * N];
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    for i in 0..N * N {
        if river[i] || lake[i] || marsh[i] {
            to_water[i] = 0;
            queue.push_back(i as u32);
        }
    }
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        if to_water[i] >= FLOODPLAIN_REACH {
            continue;
        }
        let (tx, ty) = (i % N, i / N);
        for (dx, dy) in D4 {
            let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                continue;
            }
            let j = tidx(nx as usize, ny as usize);
            if to_water[j] > to_water[i] + 1 {
                to_water[j] = to_water[i] + 1;
                queue.push_back(j as u32);
            }
        }
    }

    let mut fertility = vec![Fx::ZERO; N * N];
    let mut ore = vec![Fx::ZERO; N * N];
    let reach = Fx::from_num(FLOODPLAIN_REACH);
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            let p = plates.sample(Fx::from_num(tx as i32), Fx::from_num(ty as i32));
            ore[i] = (p.ore * crate::fx!("0.75") + p.belt * crate::fx!("0.25")).min(Fx::ONE);
            if tile_h[i] < sea {
                continue;
            }
            // Relief over a 3-tile window, not the corner step inside one tile:
            // an incised channel leaves steep corners on its own banks, which is
            // exactly where the alluvium is.
            // The carved channel itself is not "relief" — a bank beside a river
            // is the flattest, richest ground there is.
            let mut relief = Fx::ZERO;
            for (dx, dy) in D4 {
                let (nx, ny) = ((tx as i32 + dx).clamp(0, N as i32 - 1), (ty as i32 + dy).clamp(0, N as i32 - 1));
                let j = tidx(nx as usize, ny as usize);
                if river[j] || lake[j] || marsh[j] || tile_h[j] < sea {
                    continue;
                }
                relief = relief.max((tile_h[j] - tile_h[i]).abs());
            }
            let flat = (Fx::ONE - (relief * crate::fx!("10")).min(crate::fx!("0.88"))).max(Fx::ZERO);
            let flood = if to_water[i] <= FLOODPLAIN_REACH {
                Fx::ONE - Fx::from_num(to_water[i]) / reach
            } else {
                Fx::ZERO
            };
            let alluvial = (fx_sqrt(acc[i]) * crate::fx!("0.012")).min(Fx::ONE);
            let warm = if temp[i] < crate::fx!("0.22") { crate::fx!("0.35") } else { Fx::ONE };
            let f = (crate::fx!("0.10")
                + flood * crate::fx!("0.42")
                + alluvial * crate::fx!("0.14")
                + precip[i] * crate::fx!("0.30")
                + p.belt * crate::fx!("0.08"))
                * flat
                * warm;
            fertility[i] = if marsh[i] { (f + crate::fx!("0.25")).min(Fx::ONE) } else { f.min(Fx::ONE) };
        }
    }

    // ── 9. classify ─────────────────────────────────────────────────────────
    let mut biome: Vec<Biome> = vec![Biome::DeepWater; N * N];
    let deep_margin = crate::fx!("0.06");
    for ty in 0..N {
        let snow_h = snow_line(clim, ty);
        for tx in 0..N {
            let i = tidx(tx, ty);
            let h = tile_h[i];
            let t = temp[i];
            let p = precip[i];
            let x = Fx::from_num(tx as i32);
            let y = Fx::from_num(ty as i32);

            if salt[i] {
                biome[i] = Biome::SaltFlat;
                continue;
            }
            if lake[i] {
                biome[i] = Biome::Lake;
                continue;
            }
            if river[i] {
                biome[i] = if p < WADI_T {
                    Biome::Wadi
                } else if hash2(tx as i32, ty as i32, base ^ 0xf00d) > FORD_HASH_T && !wide[i] {
                    Biome::Ford
                } else {
                    Biome::River
                };
                continue;
            }
            if marsh[i] {
                biome[i] = if p < WADI_T { Biome::SaltFlat } else { Biome::Marsh };
                continue;
            }
            if h < sea - deep_margin {
                biome[i] = Biome::DeepWater;
                continue;
            }
            if h < sea {
                biome[i] = Biome::ShallowWater;
                continue;
            }
            if h < sea + BEACH_BAND {
                biome[i] = Biome::Sand;
                continue;
            }
            if h > snow_h {
                biome[i] = Biome::Snow;
                continue;
            }
            if h > MOUNTAIN_T {
                let pv = fbm(x * PASS_SCALE + Fx::from_num(17), y * PASS_SCALE + Fx::from_num(23), base ^ 0x9a55, 3);
                biome[i] = if pv > PASS_T {
                    crate::climate::highland(t, p, clim.tree_line, h)
                } else {
                    Biome::Mountain
                };
                continue;
            }
            // cliffs: too-steep corner step inside the tile (ramp channel cuts
            // openings); checked before the climate LUT so plateau edges wall up
            if bias.cliff_gain > Fx::ZERO && h > crate::fx!("0.5") {
                let c00 = corner[cidx(tx, ty)];
                let c10 = corner[cidx(tx + 1, ty)];
                let c01 = corner[cidx(tx, ty + 1)];
                let c11 = corner[cidx(tx + 1, ty + 1)];
                let step = c00.max(c10).max(c01).max(c11) - c00.min(c10).min(c01).min(c11);
                if step > CLIFF_STEP / bias.cliff_gain {
                    let ramp = fbm(x * RAMP_SCALE + Fx::from_num(13), y * RAMP_SCALE + Fx::from_num(29), base ^ 0xc11f, 3);
                    if ramp <= RAMP_T {
                        biome[i] = Biome::Cliff;
                        continue;
                    }
                }
            }
            if h > HILL_T {
                biome[i] = crate::climate::highland(t, p, clim.tree_line, h);
                continue;
            }
            biome[i] = crate::climate::whittaker(t, p);
        }
    }

    // ── 9b. arid refinement: one desert is not another ──────────────────────
    // Sand seas need flat ground and a dry fetch, stone pavement takes the
    // slopes, and anywhere the water table surfaces the desert greens up.
    let mut refined = biome.clone();
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            if biome[i] != Biome::Desert {
                continue;
            }
            let mut fresh_near = false;
            'scan: for dy in -OASIS_REACH..=OASIS_REACH {
                for dx in -OASIS_REACH..=OASIS_REACH {
                    let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                        continue;
                    }
                    if crate::biomes::biome_is_fresh_water(biome[tidx(nx as usize, ny as usize)]) {
                        fresh_near = true;
                        break 'scan;
                    }
                }
            }
            if fresh_near || fertility[i] > crate::fx!("0.46") {
                refined[i] = Biome::Oasis;
                continue;
            }
            let c00 = corner[cidx(tx, ty)];
            let c10 = corner[cidx(tx + 1, ty)];
            let c01 = corner[cidx(tx, ty + 1)];
            let c11 = corner[cidx(tx + 1, ty + 1)];
            let step = c00.max(c10).max(c01).max(c11) - c00.min(c10).min(c01).min(c11);
            if step > crate::fx!("0.016") || tile_h[i] > crate::fx!("0.58") {
                refined[i] = Biome::Hammada;
                continue;
            }
            // dunes march downwind of the driest ground: a stretched noise
            // field aligned with the prevailing wind reads as barchan trains
            let (sx, sy) = if wind.0 != 0 { (crate::fx!("0.4"), Fx::ONE) } else { (Fx::ONE, crate::fx!("0.4")) };
            let dv = fbm(
                Fx::from_num(tx as i32) * DUNE_SCALE * sx,
                Fx::from_num(ty as i32) * DUNE_SCALE * sy,
                base ^ 0xd07e,
                3,
            );
            if dv > DUNE_T {
                refined[i] = Biome::Dunes;
            }
        }
    }

    WorldGrid {
        corner_h: corner,
        biome: refined,
        moisture: precip,
        tile_h,
        temp,
        fertility,
        ore,
        climate: clim,
        wind,
    }
}

/// Per-seed world grid, computed once and leaked (same memo pattern as
/// `passable_grid` — a process touches a handful of seeds at most).
pub fn world_grid(seed: u32) -> &'static WorldGrid {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    thread_local! {
        static LAST: Cell<(u32, Option<&'static WorldGrid>)> = const { Cell::new((u32::MAX, None)) };
    }
    let (last_seed, last) = LAST.with(|c| c.get());
    if last_seed == seed && let Some(g) = last {
        return g;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static WorldGrid>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static WorldGrid = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let leaked: &'static WorldGrid = Box::leak(Box::new(build(seed)));
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, Some(grid))));
    grid
}

/// Bilinear height at fractional tile coordinates, clamped to the grid edge
/// (everything outside is the guaranteed ocean ring anyway).
pub fn height_bilinear(grid: &WorldGrid, x: Fx, y: Fx) -> Fx {
    let max = Fx::from_num((N - 1) as i32);
    let x = x.clamp(Fx::ZERO, max);
    let y = y.clamp(Fx::ZERO, max);
    let x0 = x.floor();
    let y0 = y.floor();
    let fx_ = x - x0;
    let fy = y - y0;
    let cx = x0.to_num::<i32>() as usize;
    let cy = y0.to_num::<i32>() as usize;
    let h00 = grid.corner_h[cidx(cx, cy)];
    let h10 = grid.corner_h[cidx(cx + 1, cy)];
    let h01 = grid.corner_h[cidx(cx, cy + 1)];
    let h11 = grid.corner_h[cidx(cx + 1, cy + 1)];
    let top = h00 + (h10 - h00) * fx_;
    let bot = h01 + (h11 - h01) * fx_;
    top + (bot - top) * fy
}

/// Per-tile lookups for `sample_terrain` (clamped like the height read).
pub fn tile_lookup(grid: &WorldGrid, x: Fx, y: Fx) -> (Biome, Fx) {
    let i = tile_index(x, y);
    (grid.biome[i], grid.moisture[i])
}

/// Flat index of the tile containing a world position, clamped in bounds.
pub fn tile_index(x: Fx, y: Fx) -> usize {
    let max = Fx::from_num((N - 1) as i32);
    let tx = x.clamp(Fx::ZERO, max).floor().to_num::<i32>() as usize;
    let ty = y.clamp(Fx::ZERO, max).floor().to_num::<i32>() as usize;
    tidx(tx, ty)
}
