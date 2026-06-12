//! Grid worldgen pipeline — the authority for every tile's height, biome and
//! moisture. Built once per seed, cached + leaked like `passable_grid`.
//!
//! Stages (all deterministic fixed-point; ties broken by cell index):
//!   1. corner height field: warped fbm + ridged ranges + terraced flanks
//!      (`terrain::height_at`), sampled at every tile corner
//!   2. thermal erosion sweeps: talus transport softens noise spikes into
//!      coherent slopes (double-buffered, order-independent)
//!   3. depression filling: Barnes priority-flood from the ocean border so
//!      every land cell has a monotone path to the sea
//!   4. D8 flow routing + accumulation: rain follows steepest descent;
//!      accumulation picks out real drainage trunks
//!   5. rivers: high-accumulation land becomes River (Ford where a hashed
//!      channel or gentle slope allows a crossing); stream-power carving
//!      sinks the corner field under them so valleys hold their rivers
//!   6. moisture: multi-source BFS distance to ocean/river water blended
//!      with noise — lush valleys and coasts, dry interior plateaus
//!   7. classify: water bands / beach / Whittaker(temp x moist) lowlands /
//!      hills / mountain ranges with pass saddles / snow caps; cliffs where
//!      the corner gradient steps too fast (ramp channel keeps openings)

use crate::biomes::Biome;
use crate::constants::WORLD_SIZE;
use crate::math::{Fx, fx_sqrt};
use crate::noise::fbm;
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
    /// N^2 per-tile moisture (0..1-ish).
    pub moisture: Vec<Fx>,
    /// N^2 per-tile center height (average of the 4 carved corners).
    pub tile_h: Vec<Fx>,
}

const N: usize = WORLD_SIZE as usize;
const C: usize = N + 1;

// thermal erosion: material moves where the slope exceeds the talus angle
const EROSION_SWEEPS: usize = 6;
const TALUS: Fx = crate::fx!("0.045");
const EROSION_K: Fx = crate::fx!("0.24");

// rivers: accumulation threshold (scaled down by the preset's river_gain)
const RIVER_ACC_BASE: Fx = crate::fx!("210");
const RIVER_WIDE_MUL: Fx = crate::fx!("5");
const FORD_HASH_T: Fx = crate::fx!("0.74");
const CARVE_MAX: Fx = crate::fx!("0.05");

// moisture: how far water humidity reaches inland (tiles)
const MOIST_REACH: Fx = crate::fx!("26");

// classification bands
const BEACH_BAND: Fx = crate::fx!("0.035");
const HILL_T: Fx = crate::fx!("0.62");
const MOUNTAIN_T: Fx = crate::fx!("0.76");
const SNOW_T: Fx = crate::fx!("0.86");
const CLIFF_STEP: Fx = crate::fx!("0.055");
const PASS_SCALE: Fx = crate::fx!("0.045");
const PASS_T: Fx = crate::fx!("0.6");
const RAMP_SCALE: Fx = crate::fx!("0.06");
const RAMP_T: Fx = crate::fx!("0.64");
const OASIS_SCALE: Fx = crate::fx!("0.06");
const OASIS_T: Fx = crate::fx!("0.72");

#[inline]
fn tidx(tx: usize, ty: usize) -> usize {
    ty * N + tx
}

#[inline]
fn cidx(cx: usize, cy: usize) -> usize {
    cy * C + cx
}

fn build(seed: u32) -> WorldGrid {
    let base = seed_base(seed);
    let bias = seed_bias(seed);
    let sea = SEA + bias.sea_shift;

    // ── 1. corner heights from the shaped point field ───────────────────────
    let mut corner: Vec<Fx> = vec![Fx::ZERO; C * C];
    for cy in 0..C {
        for cx in 0..C {
            corner[cidx(cx, cy)] =
                height_at(base, bias.island_gain, Fx::from_num(cx as i32), Fx::from_num(cy as i32));
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
                // move material toward the lowest 4-neighbor past the talus angle
                let mut low = h;
                let mut low_i = i;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
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
        // keep `next` in sync for the following sweep's -=/+= accumulation
    }

    let tile_height =
        |corner: &[Fx], tx: usize, ty: usize| -> Fx {
            (corner[cidx(tx, ty)]
                + corner[cidx(tx + 1, ty)]
                + corner[cidx(tx, ty + 1)]
                + corner[cidx(tx + 1, ty + 1)])
                / crate::fx!("4")
        };
    let mut tile_h: Vec<Fx> = (0..N * N).map(|i| tile_height(&corner, i % N, i / N)).collect();

    // ── 3. Barnes priority-flood depression filling ──────────────────────────
    // Seed the heap with the border (guaranteed ocean by the edge fade); every
    // popped cell raises unvisited neighbors to at least its own filled level,
    // so all drainage paths run monotonically to the map edge.
    let eps = Fx::from_bits(1 << 8);
    let mut filled: Vec<Fx> = tile_h.clone();
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
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
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

    // ── 3b. lakes: depressions the flood had to raise become standing water
    // (their surface = the fill level, so rivers drain in and out naturally;
    // Valheim/DF-style tarns and inland seas instead of filled-flat plains)
    const LAKE_DEPTH: Fx = crate::fx!("0.022");
    let mut lake = vec![false; N * N];
    for i in 0..N * N {
        if tile_h[i] >= sea && filled[i] - tile_h[i] > LAKE_DEPTH {
            lake[i] = true;
        }
    }
    // carve lake beds into the corner field so they render as real water
    // sitting below their banks
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            if !lake[i] {
                continue;
            }
            let bed = (tile_h[i] - crate::fx!("0.01")).min(sea - crate::fx!("0.012"));
            for (cx, cy) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
                let ci = cidx(cx, cy);
                if corner[ci] > bed {
                    corner[ci] = bed;
                }
            }
        }
    }
    if lake.iter().any(|&l| l) {
        for i in 0..N * N {
            if lake[i] {
                tile_h[i] = tile_height(&corner, i % N, i / N);
            }
        }
    }

    // ── 4. D8 flow + accumulation ────────────────────────────────────────────
    const D8: [(i32, i32); 8] =
        [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];
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
            let down = flow[i] as usize;
            let a = acc[i];
            acc[down] += a;
        }
    }

    // ── 5. rivers + carving ──────────────────────────────────────────────────
    let mut river = vec![false; N * N];
    let mut wide = vec![false; N * N];
    if bias.river_gain > Fx::ZERO {
        let th = RIVER_ACC_BASE / bias.river_gain;
        let wide_th = th * RIVER_WIDE_MUL;
        for i in 0..N * N {
            if tile_h[i] >= sea && acc[i] >= th {
                river[i] = true;
                wide[i] = acc[i] >= wide_th;
            }
        }
        // widen the trunks one tile to the lower side
        for ty in 1..N - 1 {
            for tx in 1..N - 1 {
                let i = tidx(tx, ty);
                if !wide[i] {
                    continue;
                }
                let mut low_j = i;
                let mut low = tile_h[i];
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
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
        // stream-power carving: sink the corners under every river tile so
        // the channel sits in a valley instead of on a plateau lip
        for ty in 0..N {
            for tx in 0..N {
                let i = tidx(tx, ty);
                if !river[i] {
                    continue;
                }
                let depth = (fx_sqrt(acc[i]) * crate::fx!("0.004")).min(CARVE_MAX);
                let bed = (tile_h[i] - depth).min(sea - crate::fx!("0.012"));
                for (cx, cy) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
                    let ci = cidx(cx, cy);
                    if corner[ci] > bed {
                        corner[ci] = bed;
                    }
                }
            }
        }
        for i in 0..N * N {
            tile_h[i] = tile_height(&corner, i % N, i / N);
        }
    }

    // ── 6. moisture: BFS distance to water + noise ───────────────────────────
    let mut dist: Vec<i32> = vec![i32::MAX; N * N];
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    for i in 0..N * N {
        if tile_h[i] < sea || river[i] {
            dist[i] = 0;
            queue.push_back(i as u32);
        }
    }
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        let (tx, ty) = (i % N, i / N);
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                continue;
            }
            let j = tidx(nx as usize, ny as usize);
            if dist[j] > dist[i] + 1 {
                dist[j] = dist[i] + 1;
                queue.push_back(j as u32);
            }
        }
    }
    // rain shadow: a seed-picked prevailing wind carries ocean humidity
    // inland; high ground squeezes it out, so the far side of every range
    // dries into desert (DF-style geographic deserts, not noise deserts)
    let wind = match base % 4 {
        0 => (1i32, 0i32),
        1 => (-1, 0),
        2 => (0, 1),
        _ => (0, -1),
    };
    let mut humid: Vec<Fx> = vec![Fx::ONE; N * N];
    let xs: Vec<usize> = if wind.0 > 0 { (0..N).collect() } else { (0..N).rev().collect() };
    let ys: Vec<usize> = if wind.1 > 0 { (0..N).collect() } else { (0..N).rev().collect() };
    for &ty in &ys {
        for &tx in &xs {
            let i = tidx(tx, ty);
            if tile_h[i] < sea {
                humid[i] = Fx::ONE;
                continue;
            }
            let (ux, uy) = (tx as i32 - wind.0, ty as i32 - wind.1);
            let upwind = if ux >= 0 && uy >= 0 && ux < N as i32 && uy < N as i32 {
                humid[tidx(ux as usize, uy as usize)]
            } else {
                Fx::ONE
            };
            // slow decay over open land, hard squeeze over high ground
            let squeeze = ((tile_h[i] - crate::fx!("0.58")) * crate::fx!("0.16")).max(Fx::ZERO);
            humid[i] = (upwind * crate::fx!("0.994") - squeeze).clamp(Fx::ZERO, Fx::ONE);
        }
    }

    let cc = Fx::from_num((N / 2) as i32);
    let mut moisture: Vec<Fx> = vec![Fx::ZERO; N * N];
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            let x = Fx::from_num(tx as i32);
            let y = Fx::from_num(ty as i32);
            let d = Fx::from_num(dist[i].min(10_000));
            let near_water = (Fx::ONE - (d / MOIST_REACH).min(Fx::ONE)).max(Fx::ZERO);
            // Valheim-scale belts: low-frequency zone noise so biomes arrive
            // as big contiguous regions, not confetti
            let n_m = fbm(
                x * crate::fx!("0.016") + Fx::from_num(100),
                y * crate::fx!("0.016") + Fx::from_num(50),
                base ^ 0x9e37,
                3,
            );
            // hospitable heartland, harsher fringes (weak radial gradient)
            let rad = ((x - cc) * (x - cc) + (y - cc) * (y - cc)) / (cc * cc);
            let radial = (crate::fx!("0.5") - rad * crate::fx!("0.55")).clamp(crate::fx!("-0.1"), crate::fx!("0.12"));
            let dry_high = ((tile_h[i] - crate::fx!("0.55")) * crate::fx!("0.9")).max(Fx::ZERO);
            moisture[i] = (near_water * crate::fx!("0.34")
                + n_m * crate::fx!("0.38")
                + humid[i] * crate::fx!("0.24")
                + radial
                - dry_high
                + bias.moist_shift)
                .clamp(Fx::ZERO, Fx::ONE);
        }
    }

    // ── 7. classify ──────────────────────────────────────────────────────────
    let mut biome: Vec<Biome> = vec![Biome::DeepWater; N * N];
    let deep_margin = crate::fx!("0.06");
    for ty in 0..N {
        for tx in 0..N {
            let i = tidx(tx, ty);
            let h = tile_h[i];
            let m = moisture[i];
            let x = Fx::from_num(tx as i32);
            let y = Fx::from_num(ty as i32);

            if river[i] {
                // hashed crossings + gentle headwaters stay walkable
                let ford = hash2(tx as i32, ty as i32, base ^ 0xf00d) > FORD_HASH_T;
                biome[i] = if ford && !wide[i] { Biome::Ford } else { Biome::River };
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
            if h > SNOW_T {
                biome[i] = Biome::Snow;
                continue;
            }
            if h > MOUNTAIN_T {
                let pv = fbm(x * PASS_SCALE + Fx::from_num(17), y * PASS_SCALE + Fx::from_num(23), base ^ 0x9a55, 3);
                biome[i] = if pv > PASS_T { Biome::Hills } else { Biome::Mountain };
                continue;
            }
            // cliffs: too-steep corner step inside the tile (ramp channel cuts
            // openings); checked before the lowland LUT so plateau edges wall up
            if bias.cliff_gain > Fx::ZERO && h > crate::fx!("0.5") {
                let c00 = corner[cidx(tx, ty)];
                let c10 = corner[cidx(tx + 1, ty)];
                let c01 = corner[cidx(tx, ty + 1)];
                let c11 = corner[cidx(tx + 1, ty + 1)];
                let mx = c00.max(c10).max(c01).max(c11);
                let mn = c00.min(c10).min(c01).min(c11);
                if mx - mn > CLIFF_STEP / bias.cliff_gain {
                    let ramp = fbm(x * RAMP_SCALE + Fx::from_num(13), y * RAMP_SCALE + Fx::from_num(29), base ^ 0xc11f, 3);
                    if ramp <= RAMP_T {
                        biome[i] = Biome::Cliff;
                        continue;
                    }
                }
            }
            if h > HILL_T {
                biome[i] = Biome::Hills;
                continue;
            }
            // Whittaker-ish moisture belts for the lowlands
            biome[i] = if m < crate::fx!("0.24") {
                let ov = fbm(x * OASIS_SCALE + Fx::from_num(41), y * OASIS_SCALE + Fx::from_num(59), base ^ 0x0a51, 3);
                if ov > OASIS_T { Biome::Oasis } else { Biome::Desert }
            } else if m < crate::fx!("0.38") {
                Biome::Dunes
            } else if m < crate::fx!("0.52") {
                Biome::Steppe
            } else if m < crate::fx!("0.72") {
                Biome::Grassland
            } else {
                Biome::Forest
            };
        }
    }

    WorldGrid { corner_h: corner, biome, moisture, tile_h }
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
    let max = Fx::from_num((N - 1) as i32);
    let tx = x.clamp(Fx::ZERO, max).floor().to_num::<i32>() as usize;
    let ty = y.clamp(Fx::ZERO, max).floor().to_num::<i32>() as usize;
    let i = tidx(tx, ty);
    (grid.biome[i], grid.moisture[i])
}
