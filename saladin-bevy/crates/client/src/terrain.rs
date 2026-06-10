use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use saladin_sim::noise::fbm;
use saladin_sim::{
    Biome, Fx, WORLD_SIZE, biome_def, biome_height_emphasis, hash2, render_height,
    sample_terrain, seed_bias,
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

fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Continuous water shading by DEPTH below the waterline — one gradient from
/// a bright shoreline sliver through shallow turquoise into deep sea, instead
/// of the old hard Shallow/Deep biome swap that drew a sharp two-tone edge.
/// Water shading by DISTANCE TO LAND, not seabed depth — the seabed height
/// noise made depth-tinting blotch pale patches mid-ocean. A narrow bright
/// band hugs every coastline; the open sea is one even blue.
/// Open water: foam crest at the waterline, a wide turquoise shelf easing
/// into the sea hue, then large slow SWELL bands so the open ocean reads as
/// living water instead of a flat fill.
fn water_color(biome: Biome, shore_dist: f32, swell: f32) -> [f32; 3] {
    let foam = hex_linear(0xd6f0f4);
    let shore = hex_linear(0x8fd6e2);
    let sea_blue = hex_linear(0x4ea4bd);
    let base = if biome == Biome::River { hex_linear(0x5cacc6) } else { sea_blue };
    // a WIDE easing — the band must read as a gradient at gameplay zoom,
    // never as a contour line
    let mut c = lerp3(shore, base, smooth01(shore_dist / 9.0));
    // foam crest hugging the land edge
    let f = 1.0 - smooth01(shore_dist / 1.2);
    c = lerp3(c, foam, f * 0.6);
    // swell: +-7% brightness in long bands, fading out near the beach
    let open = smooth01((shore_dist - 1.5) / 4.0);
    let m = 1.0 + swell * 0.14 * open;
    [c[0] * m, c[1] * m, c[2] * m]
}

/// Biome base blended toward its shade by elevation, plus snow-cap whitening
/// and a foam strip on the first sliver of beach above the waterline.
/// `h_norm` is the raw 0..1 field height; `rel_y` the unscaled render height.
fn biome_color(biome: Biome, h_norm: f32, rel_y: f32, sea: f32) -> [f32; 3] {
    let def = biome_def(biome);
    let mut c = lerp3(hex_linear(def.color), hex_linear(def.shade), rel_y * 0.045);
    if biome == Biome::Snow {
        c = lerp3(c, hex_linear(0xf4f8fb), (h_norm - 0.82) * 4.0);
    }
    if biome == Biome::Sand {
        let beach = 1.0 - ((h_norm - sea).abs() * 18.0).min(1.0);
        if beach > 0.0 {
            c = lerp3(c, hex_linear(0xefe4bf), beach * 0.5);
        }
    }
    c
}

/// Tile distance to the nearest passable land, multi-source BFS over the
/// whole grid (water tiles only; land = 0). Render-side, computed per mesh
/// build — cheap (one pass over the map).
fn land_distance_grid(seed: u32) -> Vec<f32> {
    // two-pass chamfer transform (3-4 metric): near-Euclidean iso-lines, so
    // the coast glow is round instead of a Manhattan diamond staircase
    let n = WORLD_SIZE as usize;
    let pass = saladin_sim::passable_grid(seed);
    let big = 1.0e9f32;
    let mut dist: Vec<f32> =
        (0..n * n).map(|i| if pass[i] { 0.0 } else { big }).collect();
    let (ortho, diag) = (1.0f32, 1.4f32);
    for y in 0..n {
        for x in 0..n {
            let i = y * n + x;
            let mut d = dist[i];
            if x > 0 { d = d.min(dist[i - 1] + ortho); }
            if y > 0 { d = d.min(dist[i - n] + ortho); }
            if x > 0 && y > 0 { d = d.min(dist[i - n - 1] + diag); }
            if x + 1 < n && y > 0 { d = d.min(dist[i - n + 1] + diag); }
            dist[i] = d;
        }
    }
    for y in (0..n).rev() {
        for x in (0..n).rev() {
            let i = y * n + x;
            let mut d = dist[i];
            if x + 1 < n { d = d.min(dist[i + 1] + ortho); }
            if y + 1 < n { d = d.min(dist[i + n] + ortho); }
            if x + 1 < n && y + 1 < n { d = d.min(dist[i + n + 1] + diag); }
            if x > 0 && y + 1 < n { d = d.min(dist[i + n - 1] + diag); }
            dist[i] = d;
        }
    }
    dist
}

/// Continuous vertex-colored terrain heightmap: one shared vertex per tile corner
/// (so there are NO gaps between height steps), colored by biome. Built once from
/// the seed — the same worldgen the sim uses for passability/resources.
/// Vertices extend APRON tiles past the playable map: the worldgen samples
/// fine out there and the continent mask guarantees open ocean, so the whole
/// visible frame is GENERATED sea — the backdrop disc only survives in the
/// far haze, never as a flat "second blue" next to real water.
const APRON: i32 = 224;

pub fn build_terrain_mesh(seed: u32) -> Mesh {
    let n = WORLD_SIZE;
    let lo = -APRON;
    let hi = n + APRON;
    let stride = (hi - lo + 1) as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(stride * stride);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(stride * stride);
    let sun = SUN.normalize();

    let sea = SEA_LEVEL + seed_bias(seed).sea_shift.to_num::<f32>();
    let shore_dist = land_distance_grid(seed);
    // two octaves of very low-frequency swell, render-only
    let swell = |vx: i32, vy: i32| -> f32 {
        fbm(Fx::from_num(vx) * Fx::lit("0.035"), Fx::from_num(vy) * Fx::lit("0.035"), seed ^ 0x0cea, 2)
            .to_num::<f32>()
            + fbm(Fx::from_num(vx) * Fx::lit("0.011"), Fx::from_num(vy) * Fx::lit("0.011"), seed ^ 0x5ea1, 2)
                .to_num::<f32>()
            - 1.0 // centre the sum of two 0..1 fields
    };
    let water_y = render_height(Fx::ZERO, Fx::ONE, Fx::ONE).to_num::<f32>() * TERRAIN_SCALE;
    for vy in lo..=hi {
        for vx in lo..=hi {
            // Far outside the playable map the continent mask guarantees open
            // ocean — skip the full terrain stack, it IS sea.
            if vx < -2 || vy < -2 || vx > n + 2 || vy > n + 2 {
                positions.push([vx as f32, water_y, vy as f32]);
                let c = water_color(Biome::DeepWater, 999.0, swell(vx, vy));
                let dither = (hash2(vx, vy, seed ^ 0x5eed).to_num::<f32>() - 0.5) * 0.07;
                let m = 0.95 + dither;
                colors.push([c[0] * m, c[1] * m, c[2] * m, 1.0]);
                continue;
            }
            let s = sample_terrain(seed, Fx::from_num(vx), Fx::from_num(vy));
            let h_raw =
                render_height(s.height, biome_height_emphasis(s.biome), seed_bias(seed).elev_gain).to_num::<f32>();
            positions.push([vx as f32, h_raw * TERRAIN_SCALE, vy as f32]);

            let water = matches!(s.biome, Biome::DeepWater | Biome::ShallowWater | Biome::River);
            let c = if water {
                // vertex = corner of up to 4 tiles; average their shore
                // distances for a smooth field (open sea outside the grid)
                let (mut acc, mut cnt) = (0.0f32, 0.0f32);
                for (ox, oy) in [(-1i32, -1i32), (0, -1), (-1, 0), (0, 0)] {
                    let (gx, gy) = (vx + ox, vy + oy);
                    if gx >= 0 && gy >= 0 && gx < n && gy < n {
                        acc += shore_dist[(gy * n + gx) as usize];
                        cnt += 1.0;
                    } else {
                        acc += 64.0;
                        cnt += 1.0;
                    }
                }
                water_color(s.biome, acc / cnt.max(1.0), swell(vx, vy))
            } else {
                biome_color(s.biome, s.height.to_num::<f32>(), h_raw, sea)
            };

            // Directional slope tint: finite-difference normal from neighbour
            // render heights — sun-facing slopes lighten, far sides darken.
            let hx = raw_height(seed, vx + 1, vy);
            let hz = raw_height(seed, vx, vy + 1);
            let lit = Vec3::new(h_raw - hx, 1.0, h_raw - hz).normalize().dot(sun);
            let shade_mul = 0.86 + lit.clamp(-1.0, 1.0) * 0.16;

            // Deterministic per-vertex dither so flat facets get a touch of
            // grain; water gets more — wave glints keep the open sea alive.
            let grain_amp = if water { 0.07 } else { 0.05 };
            let dither = (hash2(vx, vy, seed ^ 0x5eed).to_num::<f32>() - 0.5) * grain_amp;

            let m = (shade_mul + dither).max(0.55);
            colors.push([c[0] * m, c[1] * m, c[2] * m, 1.0]);
        }
    }

    let idx = |x: i32, y: i32| ((y - lo) as usize * stride + (x - lo) as usize) as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(((hi - lo) * (hi - lo) * 6) as usize);
    for ty in lo..hi {
        for tx in lo..hi {
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
