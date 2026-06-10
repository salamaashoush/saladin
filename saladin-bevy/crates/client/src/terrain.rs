use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use saladin_sim::{
    Biome, Fx, WORLD_SIZE, biome_def, biome_height_emphasis, hash2, render_height, sample_terrain,
    seed_bias,
};

/// Precomputed per-tile render heights — sampled once at match start so the hot
/// render path is an O(1) array lookup instead of fbm per unit per frame.
#[derive(Resource)]
pub struct HeightField {
    h: Vec<f32>,
    n: i32,
}

/// Vertical exaggeration of the terrain relief — tamed so mountains read without
/// turning the map into spikes.
const TERRAIN_SCALE: f32 = 0.5;

fn sample_height(seed: u32, x: Fx, y: Fx) -> f32 {
    let s = sample_terrain(seed, x, y);
    render_height(s.height, biome_height_emphasis(s.biome), seed_bias(seed).elev_gain).to_num::<f32>() * TERRAIN_SCALE
}

pub fn build_height_field(seed: u32) -> HeightField {
    let n = WORLD_SIZE;
    let half = Fx::lit("0.5");
    let mut h = Vec::with_capacity((n * n) as usize);
    for ty in 0..n {
        for tx in 0..n {
            h.push(sample_height(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half));
        }
    }
    HeightField { h, n }
}

/// O(1) render height at world (x, z) — nearest tile.
pub fn height_at(field: &HeightField, x: f32, z: f32) -> f32 {
    let tx = (x as i32).clamp(0, field.n - 1);
    let tz = (z as i32).clamp(0, field.n - 1);
    field.h[(tz * field.n + tx) as usize]
}

fn hex_linear(hex: u32) -> [f32; 3] {
    let c = Color::srgb_u8(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
        .to_linear();
    [c.red, c.green, c.blue]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Sun azimuth for the cheap per-vertex slope tint — matches the scene's
/// directional light so lit faces agree with the real shading.
const SUN: Vec3 = Vec3::new(40.0, 70.0, 20.0);

/// Raw waterline of the height field (`SEA` in sim terrain) — the foam strip
/// hugs this, not the TS-era 0.4.
const SEA_LEVEL: f32 = 0.38;

/// Unscaled render height at integer vertex coords — slope/elevation tints use
/// this so contrast matches the source look regardless of TERRAIN_SCALE.
fn raw_height(seed: u32, vx: i32, vy: i32) -> f32 {
    let s = sample_terrain(seed, Fx::from_num(vx), Fx::from_num(vy));
    render_height(s.height, biome_height_emphasis(s.biome), seed_bias(seed).elev_gain).to_num::<f32>()
}

/// Biome base blended toward its shade by elevation, plus snow-cap whitening
/// and a foam strip on the first sliver of beach above the waterline.
/// `h_norm` is the raw 0..1 field height; `rel_y` the unscaled render height.
fn biome_color(biome: Biome, h_norm: f32, rel_y: f32) -> [f32; 3] {
    let def = biome_def(biome);
    let mut c = lerp3(hex_linear(def.color), hex_linear(def.shade), rel_y * 0.045);
    if biome == Biome::Snow {
        c = lerp3(c, hex_linear(0xf4f8fb), (h_norm - 0.82) * 4.0);
    }
    if biome == Biome::Sand {
        let beach = 1.0 - ((h_norm - SEA_LEVEL).abs() * 18.0).min(1.0);
        if beach > 0.0 {
            c = lerp3(c, hex_linear(0xefe4bf), beach * 0.5);
        }
    }
    c
}

/// Continuous vertex-colored terrain heightmap: one shared vertex per tile corner
/// (so there are NO gaps between height steps), colored by biome. Built once from
/// the seed — the same worldgen the sim uses for passability/resources.
pub fn build_terrain_mesh(seed: u32) -> Mesh {
    let n = WORLD_SIZE;
    let stride = (n + 1) as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(stride * stride);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(stride * stride);
    let sun = SUN.normalize();

    for vy in 0..=n {
        for vx in 0..=n {
            let s = sample_terrain(seed, Fx::from_num(vx), Fx::from_num(vy));
            let h_raw =
                render_height(s.height, biome_height_emphasis(s.biome), seed_bias(seed).elev_gain).to_num::<f32>();
            positions.push([vx as f32, h_raw * TERRAIN_SCALE, vy as f32]);

            let c = biome_color(s.biome, s.height.to_num::<f32>(), h_raw);

            // Directional slope tint: finite-difference normal from neighbour
            // render heights — sun-facing slopes lighten, far sides darken.
            let hx = raw_height(seed, vx + 1, vy);
            let hz = raw_height(seed, vx, vy + 1);
            let lit = Vec3::new(h_raw - hx, 1.0, h_raw - hz).normalize().dot(sun);
            let shade_mul = 0.86 + lit.clamp(-1.0, 1.0) * 0.16;

            // Deterministic per-vertex dither so flat facets get a touch of grain.
            let dither = (hash2(vx, vy, seed ^ 0x5eed).to_num::<f32>() - 0.5) * 0.05;

            let m = (shade_mul + dither).max(0.55);
            colors.push([c[0] * m, c[1] * m, c[2] * m, 1.0]);
        }
    }

    let idx = |x: i32, y: i32| (y as usize * stride + x as usize) as u32;
    let mut indices: Vec<u32> = Vec::with_capacity((n * n * 6) as usize);
    for ty in 0..n {
        for tx in 0..n {
            let (a, b, c, d) = (idx(tx, ty), idx(tx + 1, ty), idx(tx + 1, ty + 1), idx(tx, ty + 1));
            indices.extend_from_slice(&[a, c, b, a, d, c]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}
