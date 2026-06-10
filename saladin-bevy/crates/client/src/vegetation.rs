//! Cosmetic vegetation scattered by seeded biome sampling (port of
//! src/game/Vegetation.ts). Render-only — never part of the sim. Recomputed
//! from the same seeded worldgen the module agrees on, so it never desyncs:
//! a per-tile hash decides whether a prop drops (biome density is the accept
//! probability), a second hash stream jitters position/rotation/scale.

use saladin_sim::rng::mix_seed;
use saladin_sim::{Decoration, Fx, WORLD_SIZE, biome_decoration, hash2, sample_terrain};

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
