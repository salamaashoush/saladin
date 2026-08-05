//! Cosmetic vegetation scattered by seeded biome sampling. Render-only — never
//! part of the sim. Recomputed from the same seeded worldgen the module agrees
//! on, so it never desyncs: per-tile hash streams decide whether a burst drops,
//! which species it is drawn from, and how each plant is scaled, leaned and
//! tinted.

use saladin_sim::noise::fbm;
use saladin_sim::rng::mix_seed;
use saladin_sim::{Biome, Fx, WORLD_SIZE, hash2, sample_terrain, seed_base, slope_at};

use crate::render::models::props::*;

/// Number of pre-tinted copies the renderer keeps of every prop mesh.
pub const TINTS: usize = 4;

/// One decoration instance. `scale` is the height multiplier and `width`
/// multiplies it again on X/Z, so girth varies independently of height.
pub struct Placement {
    pub mesh: usize,
    pub x: f32,
    pub z: f32,
    pub rot: f32,
    pub scale: f32,
    pub width: f32,
    /// Tilt off vertical (radians) and the compass direction it leans.
    pub lean: f32,
    pub lean_dir: f32,
    /// Which pre-tinted copy of `mesh` to draw: 0 lush .. TINTS-1 parched.
    pub tint: usize,
}

impl Placement {
    fn plain(mesh: usize, x: f32, z: f32, rot: f32, scale: f32) -> Self {
        Placement { mesh, x, z, rot, scale, width: 1.0, lean: 0.0, lean_dir: 0.0, tint: 0 }
    }
}

/// Species table per biome: `(base density, [(prop mesh, weight)])`. This is
/// client dressing, not sim data — the biome rows carry gameplay only.
type Flora = &'static [(usize, f32)];

const NOTHING: Flora = &[];
const REEDBED: Flora = &[(PROP_REEDS, 1.0)];

fn flora(b: Biome) -> (f32, Flora) {
    match b {
        Biome::DeepWater | Biome::SaltFlat => (0.0, NOTHING),
        Biome::ShallowWater => (0.05, REEDBED),
        Biome::River => (0.12, REEDBED),
        Biome::Ford => (0.08, REEDBED),
        Biome::Lake => (0.06, REEDBED),
        Biome::Marsh => (0.35, &[(PROP_REEDS, 7.0), (PROP_TUSSOCK, 2.0), (PROP_FERN, 1.0)]),
        Biome::Sand => (0.10, &[(PROP_DUNE_GRASS, 6.0), (PROP_PEBBLES, 2.0), (PROP_ROCK, 1.0)]),
        Biome::Desert => {
            (0.05, &[(PROP_DUNE_GRASS, 3.0), (PROP_SHRUB, 3.0), (PROP_ROCK, 2.0), (PROP_PEBBLES, 2.0)])
        }
        Biome::Dunes => (0.12, &[(PROP_DUNE_GRASS, 9.0), (PROP_PEBBLES, 1.0)]),
        Biome::Wadi => {
            (0.08, &[(PROP_DUNE_GRASS, 4.0), (PROP_PEBBLES, 4.0), (PROP_SHRUB, 2.0), (PROP_ROCK, 1.0)])
        }
        Biome::Steppe => {
            (0.10, &[(PROP_TUSSOCK, 6.0), (PROP_SHRUB, 4.0), (PROP_DUNE_GRASS, 2.0), (PROP_ROCK, 1.0)])
        }
        Biome::Grassland => (
            0.07,
            &[(PROP_TUSSOCK, 6.0), (PROP_SHRUB, 5.0), (PROP_FLOWERS, 3.0), (PROP_SAPLING, 1.0), (PROP_BOULDER, 0.4)],
        ),
        Biome::Savanna => {
            (0.09, &[(PROP_TUSSOCK, 5.0), (PROP_ACACIA, 3.0), (PROP_SHRUB, 3.0), (PROP_ROCK, 1.0)])
        }
        Biome::Scrub => (
            0.30,
            &[(PROP_SHRUB, 6.0), (PROP_TUSSOCK, 3.0), (PROP_ROCK, 2.0), (PROP_OLIVE, 1.5), (PROP_FLOWERS, 1.0)],
        ),
        Biome::Forest => (
            0.24,
            &[(PROP_FERN, 5.0), (PROP_SHRUB, 5.0), (PROP_SAPLING, 3.0), (PROP_TUSSOCK, 2.0), (PROP_DEADFALL, 0.8)],
        ),
        Biome::Pine => (
            0.32,
            &[(PROP_PINE, 5.0), (PROP_FERN, 3.0), (PROP_SHRUB, 2.0), (PROP_SAPLING, 2.0), (PROP_DEADFALL, 0.8)],
        ),
        Biome::OliveGrove => (
            0.28,
            &[(PROP_OLIVE, 5.0), (PROP_TUSSOCK, 3.0), (PROP_SHRUB, 2.0), (PROP_ROCK, 1.5), (PROP_FLOWERS, 1.0)],
        ),
        Biome::Oasis => {
            (0.30, &[(PROP_PALM, 5.0), (PROP_REEDS, 2.0), (PROP_TUSSOCK, 2.0), (PROP_FLOWERS, 1.0)])
        }
        Biome::Hills => (
            0.16,
            &[(PROP_ROCK, 5.0), (PROP_PEBBLES, 3.0), (PROP_TUSSOCK, 3.0), (PROP_SHRUB, 3.0), (PROP_BOULDER, 1.0)],
        ),
        Biome::Alpine => {
            (0.20, &[(PROP_ROCK, 5.0), (PROP_PEBBLES, 3.0), (PROP_TUSSOCK, 3.0), (PROP_BOULDER, 3.0)])
        }
        Biome::Hammada => (0.28, &[(PROP_PEBBLES, 6.0), (PROP_ROCK, 5.0), (PROP_BOULDER, 3.0)]),
        Biome::Cliff => (0.20, &[(PROP_PEBBLES, 5.0), (PROP_BOULDER, 4.0), (PROP_ROCK, 3.0)]),
        Biome::Mountain => (0.14, &[(PROP_BOULDER, 5.0), (PROP_ROCK, 4.0), (PROP_PEBBLES, 4.0)]),
        Biome::Snow => (0.08, &[(PROP_BOULDER, 4.0), (PROP_ROCK, 3.0), (PROP_PEBBLES, 3.0)]),
    }
}

fn pick(table: &[(usize, f32)], roll: f32) -> usize {
    let target = roll * table.iter().map(|&(_, w)| w).sum::<f32>();
    let mut acc = 0.0;
    for &(m, w) in table {
        acc += w;
        if target < acc {
            return m;
        }
    }
    table[table.len() - 1].0
}

/// Rare ruin landmarks — one roll per 48x48-tile cell, jittered onto open
/// passable ground. Render-only "secrets" rewarding exploration; the same
/// seed always hides the same monuments in the same places.
pub fn landmark_placements(seed: u32, variants: usize) -> Vec<Placement> {
    let mut out = Vec::new();
    if seed == 0 || variants == 0 {
        return out;
    }
    const CELL: i32 = 48;
    let cells = WORLD_SIZE / CELL;
    let half = saladin_sim::fx!("0.5");
    for cy in 0..cells {
        for cx in 0..cells {
            let roll = hash2(cx, cy, mix_seed(seed, 0x4a15)).to_num::<f32>();
            if roll > 0.45 {
                continue;
            }
            // jittered candidate scan inside the cell: first open spot wins
            'cell: for probe in 0..12 {
                let hx = hash2(cx * 31 + probe, cy * 17, mix_seed(seed, 0xa11c));
                let hy = hash2(cx * 13, cy * 41 + probe, mix_seed(seed, 0x5eec));
                let tx = cx * CELL + 4 + (hx * Fx::from_num(CELL - 8)).to_num::<i32>();
                let ty = cy * CELL + 4 + (hy * Fx::from_num(CELL - 8)).to_num::<i32>();
                // need a clear 3x3 of buildable land so the monument sits flat
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let p = sample_terrain(
                            seed,
                            Fx::from_num(tx + dx) + half,
                            Fx::from_num(ty + dy) + half,
                        );
                        if !saladin_sim::biome_buildable(p.biome) {
                            continue 'cell;
                        }
                    }
                }
                let which = (hash2(tx, ty, mix_seed(seed, 0x1dc)).to_num::<f32>()
                    * variants as f32) as usize;
                out.push(Placement::plain(
                    which.min(variants - 1),
                    tx as f32 + 0.5,
                    ty as f32 + 0.5,
                    hash2(ty, tx, mix_seed(seed, 0x707)).to_num::<f32>() * std::f32::consts::TAU,
                    0.95 + hash2(tx ^ 5, ty ^ 9, seed).to_num::<f32>() * 0.25,
                ));
                break;
            }
        }
    }
    out
}

/// Thicket/clearing mask, mirroring the grove field the sim uses to clump
/// resource nodes: statistically even scatter reads as confetti.
const CLUMP_SCALE: Fx = saladin_sim::fx!("0.07");
const CLUMP_T: f32 = 0.44;
const CLUMP_GAIN: f32 = 3.4;
const CLUMP_FLOOR: f32 = 0.30;
const CLUMP_BOOST: f32 = 1.85;
/// Holds the total placement count at or under the pre-species-table budget.
const DENSITY: f32 = 0.62;

fn idx(tx: i32, ty: i32) -> usize {
    (ty * WORLD_SIZE + tx) as usize
}

/// Deterministic decoration placements for the seeded map: thickets and
/// clearings of mixed species, plus transition props read off the neighbouring
/// biomes and the slope field.
pub fn vegetation_placements(seed: u32) -> Vec<Placement> {
    let mut out = Vec::new();
    if seed == 0 {
        return out;
    }
    let half = saladin_sim::fx!("0.5");
    let base = seed_base(seed);
    let n = (WORLD_SIZE * WORLD_SIZE) as usize;
    let mut biome = vec![Biome::DeepWater; n];
    let mut arid = vec![0f32; n];
    for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            let s = sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            biome[idx(tx, ty)] = s.biome;
            arid[idx(tx, ty)] = 1.0 - s.moisture.to_num::<f32>();
        }
    }

    for ty in 1..WORLD_SIZE - 1 {
        for tx in 1..WORLD_SIZE - 1 {
            let here = biome[idx(tx, ty)];
            let dry = arid[idx(tx, ty)];
            let (density, table) = flora(here);

            let gv = fbm(
                Fx::from_num(tx) * CLUMP_SCALE,
                Fx::from_num(ty) * CLUMP_SCALE,
                base ^ 0x5c0b,
                3,
            )
            .to_num::<f32>();
            let thicket = ((gv - CLUMP_T) * CLUMP_GAIN).clamp(0.0, 1.0);
            let clump = CLUMP_FLOOR + CLUMP_BOOST * thicket * thicket;

            if density > 0.0 && !table.is_empty() {
                // Reeds root in the shallows they can reach the bottom of. A
                // wide continental shelf is still shallow water, and left
                // ungated the whole bay grows a reed bed a mile from shore.
                let drowned = !saladin_sim::biome_passable(here)
                    && !(-2..=2).any(|dx| {
                        (-2..=2).any(|dy| saladin_sim::is_passable(seed, tx + dx, ty + dy))
                    });
                if !drowned {
                    let roll = hash2(tx, ty, mix_seed(seed, 7001)).to_num::<f32>();
                    if roll < density * clump * DENSITY {
                        let burst = 1 + (hash2(tx, ty, mix_seed(seed, 7103)).to_num::<f32>()
                            * (1.0 + 2.6 * thicket)) as usize;
                        for k in 0..burst.min(4) as u32 {
                            out.push(plant(seed, tx, ty, k, table, dry));
                        }
                    }
                }
            }

            // ── transitions: the ground does not change species on a line ──
            if !saladin_sim::biome_passable(here) {
                continue;
            }
            let near = |pred: fn(Biome) -> bool| {
                [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| pred(biome[idx(tx + dx, ty + dy)]))
            };
            let steep = slope_at(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half)
                .to_num::<f32>();
            if steep > 0.30 || near(|b| matches!(b, Biome::Cliff | Biome::Mountain)) {
                edge(seed, tx, ty, 0x9a01, 0.42, PROP_PEBBLES, dry, &mut out);
            }
            if matches!(here, Biome::Grassland | Biome::Steppe | Biome::Savanna | Biome::Scrub)
                && near(|b| matches!(b, Biome::Sand | Biome::Desert | Biome::Dunes))
            {
                edge(seed, tx, ty, 0x9b17, 0.34, PROP_DUNE_GRASS, dry, &mut out);
            }
            if !matches!(here, Biome::Forest | Biome::Pine)
                && near(|b| matches!(b, Biome::Forest | Biome::Pine))
            {
                let deadwood = hash2(tx, ty, mix_seed(seed, 0x9c2d)).to_num::<f32>() < 0.22;
                let kind = if deadwood { PROP_DEADFALL } else { PROP_SAPLING };
                edge(seed, tx, ty, 0x9c31, 0.24, kind, dry, &mut out);
            }
        }
    }
    out
}

fn edge(seed: u32, tx: i32, ty: i32, salt: u32, p: f32, mesh: usize, dry: f32, out: &mut Vec<Placement>) {
    if hash2(tx, ty, mix_seed(seed, salt)).to_num::<f32>() >= p {
        return;
    }
    out.push(plant_from(seed, tx, ty, salt, &[(mesh, 1.0)], dry));
}

fn plant(seed: u32, tx: i32, ty: i32, k: u32, table: Flora, dry: f32) -> Placement {
    plant_from(seed, tx, ty, 8100 + k * 37, table, dry)
}

fn plant_from(seed: u32, tx: i32, ty: i32, salt: u32, table: &[(usize, f32)], dry: f32) -> Placement {
    let h = |o: u32| hash2(tx, ty, mix_seed(seed, salt.wrapping_add(o))).to_num::<f32>();
    let mesh = pick(table, h(1));
    let (jx, jz) = (h(2), h(3));
    let (hs, ws) = (h(4), h(5));
    let (lean, dir) = (h(6), h(7));
    // dry ground picks the parched end of the tint ladder; the hash keeps
    // neighbours from banding
    let t = dry * 0.72 + h(8) * 0.5 - 0.11;
    Placement {
        mesh,
        x: tx as f32 + 0.12 + jx * 0.76,
        z: ty as f32 + 0.12 + jz * 0.76,
        rot: h(9) * std::f32::consts::TAU,
        scale: 0.68 + hs * 0.72,
        width: 0.8 + ws * 0.5,
        lean: lean * 0.12,
        lean_dir: dir * std::f32::consts::TAU,
        tint: ((t * TINTS as f32) as isize).clamp(0, TINTS as isize - 1) as usize,
    }
}
