//! Cosmetic vegetation scattered by seeded biome sampling (port of
//! src/game/Vegetation.ts). Render-only — never part of the sim. Recomputed
//! from the same seeded worldgen the module agrees on, so it never desyncs:
//! a per-tile hash decides whether a prop drops (biome density is the accept
//! probability), a second hash stream jitters position/rotation/scale.

use saladin_sim::rng::mix_seed;
use saladin_sim::{Biome, Decoration, Fx, WORLD_SIZE, biome_decoration, hash2, sample_terrain};

/// One decoration instance: (mesh index into `prop_meshes()`, world x, world z,
/// yaw radians, uniform scale).
pub struct Placement {
    pub mesh: usize,
    pub x: f32,
    pub z: f32,
    pub rot: f32,
    pub scale: f32,
}

/// Decoration kind → index into `render::models::props::prop_meshes()`.
fn mesh_index(kind: Decoration) -> Option<usize> {
    match kind {
        Decoration::Shrub => Some(crate::render::models::props::PROP_SHRUB),
        Decoration::DuneGrass => Some(crate::render::models::props::PROP_DUNE_GRASS),
        Decoration::Rock => Some(crate::render::models::props::PROP_ROCK),
        Decoration::Boulder => Some(crate::render::models::props::PROP_BOULDER),
        Decoration::Reeds => Some(crate::render::models::props::PROP_REEDS),
        Decoration::Palm => Some(crate::render::models::props::PROP_PALM),
        Decoration::PineCluster => Some(crate::render::models::props::PROP_PINE),
        Decoration::None => None,
    }
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
    let half = Fx::lit("0.5");
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
                out.push(Placement {
                    mesh: which.min(variants - 1),
                    x: tx as f32 + 0.5,
                    z: ty as f32 + 0.5,
                    rot: hash2(ty, tx, mix_seed(seed, 0x707)).to_num::<f32>() * std::f32::consts::TAU,
                    scale: 0.95 + hash2(tx ^ 5, ty ^ 9, seed).to_num::<f32>() * 0.25,
                });
                break;
            }
        }
    }
    out
}

/// Deterministic decoration placements for the seeded map. A tile hosts at
/// most one prop; the biome table decides which kind and at what density
/// (water/mountain rows carry their own intentional kinds: reeds, boulders).
pub fn vegetation_placements(seed: u32) -> Vec<Placement> {
    let mut out = Vec::new();
    if seed == 0 {
        return out;
    }
    let half = Fx::lit("0.5");
    for ty in 1..WORLD_SIZE - 1 {
        for tx in 1..WORLD_SIZE - 1 {
            let s = sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            // wildflower patches sprinkle the grass independently of the
            // biome's main decoration channel
            if matches!(s.biome, Biome::Grassland | Biome::Oasis) {
                let roll = hash2(tx, ty, mix_seed(seed, 9001));
                if roll < Fx::lit("0.05") {
                    let jx = hash2(tx, ty, mix_seed(seed, 9103)).to_num::<f32>();
                    let jy = hash2(tx, ty, mix_seed(seed, 9207)).to_num::<f32>();
                    out.push(Placement {
                        mesh: crate::render::models::props::PROP_FLOWERS,
                        x: tx as f32 + 0.2 + jx * 0.6,
                        z: ty as f32 + 0.2 + jy * 0.6,
                        rot: jx * std::f32::consts::TAU,
                        scale: 0.8 + jy * 0.5,
                    });
                }
            }
            let dec = biome_decoration(s.biome);
            if dec.density <= Fx::ZERO {
                continue;
            }
            let Some(mesh) = mesh_index(dec.kind) else { continue };
            let kind = dec.kind as u32;
            // Independent hash stream per decoration so kinds don't correlate.
            let roll = hash2(tx, ty, mix_seed(seed, 7000 + kind));
            if roll >= dec.density {
                continue;
            }
            let jx = hash2(tx, ty, mix_seed(seed, 8101 + kind)).to_num::<f32>();
            let jy = hash2(tx, ty, mix_seed(seed, 8203 + kind)).to_num::<f32>();
            let jr = hash2(tx, ty, mix_seed(seed, 8307 + kind)).to_num::<f32>();
            let js = hash2(tx, ty, mix_seed(seed, 8419 + kind)).to_num::<f32>();
            out.push(Placement {
                mesh,
                x: tx as f32 + 0.2 + jx * 0.6,
                z: ty as f32 + 0.2 + jy * 0.6,
                rot: jr * std::f32::consts::TAU,
                scale: 0.75 + js * 0.6,
            });
        }
    }
    out
}
