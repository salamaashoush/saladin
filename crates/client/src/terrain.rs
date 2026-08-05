use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use saladin_sim::noise::fbm;
use saladin_sim::{
    Biome, Fx, WORLD_SIZE, biome_def, fertility_at, hash2, sample_terrain, seed_bias,
    surface_height,
};

/// Precomputed per-tile render heights — sampled once at match start so the hot
/// render path is an O(1) array lookup instead of fbm per unit per frame.
#[derive(Resource)]
pub struct HeightField {
    h: Vec<f32>,
    n: i32,
}

/// `surface_height` already carries the world's whole vertical scale.
const TERRAIN_SCALE: f32 = 1.0;

fn sample_height(seed: u32, x: Fx, y: Fx) -> f32 {
    let s = sample_terrain(seed, x, y);
    surface_height(s.height, seed_bias(seed).elev_gain).to_num::<f32>() * TERRAIN_SCALE
}

/// Sampled on the SAME half-tile lattice the detail mesh is built from, not on
/// tile centres — otherwise props, units and picking sit up to half a tile of
/// relief off the surface the camera actually draws.
pub fn build_height_field(seed: u32) -> HeightField {
    let n = WORLD_SIZE * 2 + 1;
    let mut h = Vec::with_capacity((n * n) as usize);
    for iy in 0..n {
        for ix in 0..n {
            h.push(sample_height(
                seed,
                Fx::from_num(ix as f32 * 0.5),
                Fx::from_num(iy as f32 * 0.5),
            ));
        }
    }
    HeightField { h, n }
}

/// O(1) render height at world (x, z) — bilinear between the four nearest
/// lattice samples, so props/units track the sloped surface instead of
/// stepping per tile (nearest-tile left rocks hovering on every hillside).
pub fn height_at(field: &HeightField, x: f32, z: f32) -> f32 {
    let n = field.n;
    let fx = (x * 2.0).clamp(0.0, (n - 1) as f32);
    let fz = (z * 2.0).clamp(0.0, (n - 1) as f32);
    let (x0, z0) = (fx.floor() as i32, fz.floor() as i32);
    let (x1, z1) = ((x0 + 1).min(n - 1), (z0 + 1).min(n - 1));
    let (tx, tz) = (fx - x0 as f32, fz - z0 as f32);
    let s = |gx: i32, gz: i32| field.h[(gz * n + gx) as usize];
    let a = s(x0, z0) * (1.0 - tx) + s(x1, z0) * tx;
    let b = s(x0, z1) * (1.0 - tx) + s(x1, z1) * tx;
    a * (1.0 - tz) + b * tz
}

/// Surface normal at (x, z) from finite differences of the height field —
/// rocks and landmarks tilt onto the slope they sit on.
pub fn normal_at(field: &HeightField, x: f32, z: f32) -> Vec3 {
    let e = 0.6;
    let hx = height_at(field, x + e, z) - height_at(field, x - e, z);
    let hz = height_at(field, x, z + e) - height_at(field, x, z - e);
    Vec3::new(-hx, 2.0 * e, -hz).normalize()
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
    surface_height(s.height, seed_bias(seed).elev_gain).to_num::<f32>()
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
fn water_color(riverness: f32, shore_dist: f32, swell: f32) -> [f32; 3] {
    let foam = hex_linear(0xd6f0f4);
    let shore = hex_linear(0x8fd6e2);
    let base = lerp3(hex_linear(0x4ea4bd), hex_linear(0x5cacc6), riverness);
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

/// Sun-bleached straw every vegetated biome drifts toward as it dries out.
const DRY_HUE: u32 = 0xab9c62;
/// Cold desaturated green-grey the cool end drifts toward.
const COOL_HUE: u32 = 0x8b9790;

/// Biome base blended toward its shade by elevation, plus snow-cap whitening
/// and a foam strip on the first sliver of beach above the waterline.
/// `h_norm` is the raw 0..1 field height; `patch` a slow 0..1 noise field that
/// mottles vegetation/sand so big fills read as meadows and drifts instead of
/// flat paint. `moist`/`temp` are the worldgen's own climate at this tile: the
/// same Grassland browns out as it dries and greys off as it cools, so one
/// label spans a range of ground.
#[allow(clippy::too_many_arguments)]
fn biome_color(
    biome: Biome,
    h_norm: f32,
    sea: f32,
    patch: f32,
    moist: f32,
    temp: f32,
) -> [f32; 3] {
    let def = biome_def(biome);
    // Normalised height above sea, not raw render height: on the old curve
    // ~90% of land sat below t = 0.09, so every grassland vertex was one hue.
    let elev = ((h_norm - sea) / (1.0 - sea)).clamp(0.0, 1.0);
    let mut c = lerp3(hex_linear(def.color), hex_linear(def.shade), elev * 0.70);
    match biome {
        Biome::Grassland
        | Biome::Steppe
        | Biome::Forest
        | Biome::Oasis
        | Biome::Savanna
        | Biome::Scrub
        | Biome::Pine
        | Biome::OliveGrove => {
            // dry-bleached patches on the high side, lush dips on the low side
            let dry = hex_linear(0xa8a05c);
            c = lerp3(c, hex_linear(def.shade), (patch - 0.5).max(0.0) * 0.9);
            c = lerp3(c, dry, (0.5 - patch).max(0.0) * 0.5);
        }
        Biome::Desert | Biome::Dunes | Biome::Sand | Biome::Hammada | Biome::SaltFlat => {
            c = lerp3(c, hex_linear(def.shade), (patch - 0.5).max(0.0) * 0.6);
        }
        Biome::Hills | Biome::Mountain | Biome::Cliff | Biome::Alpine => {
            c = lerp3(c, hex_linear(def.shade), (patch - 0.5).max(0.0) * 0.7);
        }
        _ => {}
    }
    if biome == Biome::Snow {
        c = lerp3(c, hex_linear(0xf4f8fb), (h_norm - 0.82) * 4.0);
    }
    if biome == Biome::Sand {
        let beach = 1.0 - ((h_norm - sea).abs() * 18.0).min(1.0);
        if beach > 0.0 {
            c = lerp3(c, hex_linear(0xefe4bf), beach * 0.5);
        }
    }
    let vegetated = matches!(
        biome,
        Biome::Grassland
            | Biome::Steppe
            | Biome::Forest
            | Biome::Oasis
            | Biome::Savanna
            | Biome::Scrub
            | Biome::Pine
            | Biome::OliveGrove
            | Biome::Marsh
            | Biome::Alpine
            | Biome::Hills
    );
    if vegetated {
        c = lerp3(c, hex_linear(DRY_HUE), smooth01((0.44 - moist) / 0.28) * 0.42);
    }
    c = lerp3(c, hex_linear(COOL_HUE), smooth01((0.38 - temp) / 0.28) * 0.24);
    let warm = smooth01((temp - 0.62) / 0.28) * 0.10;
    [c[0] * (1.0 + warm), c[1] * (1.0 + warm * 0.35), c[2] * (1.0 - warm * 0.5)]
}

/// Slope (world rise per tile) between which topsoil thins into bare rock.
/// Land slope is p90 ~0.17 / p99 ~0.54, so this exposes rock on the genuinely
/// steep few percent and nothing else.
const ROCK_LO: f32 = 0.26;
const ROCK_HI: f32 = 0.75;
/// Relief AO: radius the local mean height is taken over, and how hard the
/// difference darkens. Large-scale by design — gullies read deep and shoulders
/// catch light, which is the cheapest cure there is for a flat fill.
const AO_R: usize = 5;
const AO_K: f32 = 0.9;
/// Land coverage at which a vertex stops being sea, and the width of the wash.
/// `land` is NOT blurred where the palette gets two passes: the smoothstepped
/// tap already spreads a 0/1 field over one tile, and blurring on top of that
/// washes foam a tile inland — which prints as pale columns down any bank
/// steep enough to compress that tile into a few pixels.
const LAND_T: f32 = 0.50;
const LAND_W: f32 = 0.55;

const FN: usize = WORLD_SIZE as usize;

/// One separable [1, 2, 1] pass over an N x N field, edge-clamped.
fn blur3(src: &[f32]) -> Vec<f32> {
    let n = FN;
    let mut tmp = vec![0f32; n * n];
    for y in 0..n {
        let r = y * n;
        for x in 0..n {
            tmp[r + x] = (src[r + x.saturating_sub(1)]
                + src[r + x] * 2.0
                + src[r + (x + 1).min(n - 1)])
                * 0.25;
        }
    }
    let mut out = vec![0f32; n * n];
    for y in 0..n {
        let (ym, yp, r) = (y.saturating_sub(1) * n, (y + 1).min(n - 1) * n, y * n);
        for x in 0..n {
            out[r + x] = (tmp[ym + x] + tmp[r + x] * 2.0 + tmp[yp + x]) * 0.25;
        }
    }
    out
}

/// Separable edge-clamped box mean over a (2r+1) square, by running sum.
fn box_blur(src: &[f32], r: usize) -> Vec<f32> {
    let n = FN;
    let w = (2 * r + 1) as f32;
    let mut tmp = vec![0f32; n * n];
    for y in 0..n {
        let row = y * n;
        let mut acc = src[row] * (r + 1) as f32;
        for k in 1..=r {
            acc += src[row + k.min(n - 1)];
        }
        for x in 0..n {
            tmp[row + x] = acc / w;
            acc -= src[row + x.saturating_sub(r)];
            acc += src[row + (x + r + 1).min(n - 1)];
        }
    }
    let mut out = vec![0f32; n * n];
    for x in 0..n {
        let mut acc = tmp[x] * (r + 1) as f32;
        for k in 1..=r {
            acc += tmp[k.min(n - 1) * n + x];
        }
        for y in 0..n {
            out[y * n + x] = acc / w;
            acc -= tmp[y.saturating_sub(r) * n + x];
            acc += tmp[(y + r + 1).min(n - 1) * n + x];
        }
    }
    out
}

/// The four taps + weights of a bilinear read of an N x N per-tile field at a
/// fractional world position (tile centres sit at i + 0.5), edge-clamped.
/// Resolved once per vertex, then reused for every field.
struct Tap {
    i: [usize; 4],
    w: [f32; 4],
}

fn tap(x: f32, y: f32) -> Tap {
    let n = FN as i32;
    let fx = (x - 0.5).clamp(0.0, (n - 1) as f32);
    let fy = (y - 0.5).clamp(0.0, (n - 1) as f32);
    let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
    let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
    // Smoothstepped weights, not raw bilinear: plain bilinear is only C0, and
    // its kinks at every tile boundary print a one-tile lattice of creases
    // across any steep face.
    let (tx, ty) = (smooth01(fx - x0 as f32), smooth01(fy - y0 as f32));
    Tap {
        i: [
            (y0 * n + x0) as usize,
            (y0 * n + x1) as usize,
            (y1 * n + x0) as usize,
            (y1 * n + x1) as usize,
        ],
        w: [(1.0 - tx) * (1.0 - ty), tx * (1.0 - ty), (1.0 - tx) * ty, tx * ty],
    }
}

fn bl(f: &[f32], t: &Tap) -> f32 {
    f[t.i[0]] * t.w[0] + f[t.i[1]] * t.w[1] + f[t.i[2]] * t.w[2] + f[t.i[3]] * t.w[3]
}

/// Continuous per-tile colour fields, blurred over ~2.5 tiles and sampled
/// BILINEARLY per vertex. The biome LABEL never reaches the colour path —
/// that is what removes the per-triangle staircase at every boundary and
/// buys real transitions between neighbours.
struct Fields {
    pal: [Vec<f32>; 3],
    /// 0..1 land coverage: the continuous coastline.
    land: Vec<f32>,
    /// 0..1 bare-rock exposure; the fragment stage re-sharpens it per pixel.
    rock: Vec<f32>,
    /// 1 - moisture.
    arid: Vec<f32>,
    /// 0 = cool grey alpine stone, 1 = warm ochre desert scarp.
    hue: Vec<f32>,
    /// 0..1 channel-ness, so river water shades into sea water continuously.
    river: Vec<f32>,
}

fn build_fields(seed: u32) -> Fields {
    let n = FN;
    let g = saladin_sim::worldgrid::world_grid(seed);
    let gain = seed_bias(seed).elev_gain;
    let sea = SEA_LEVEL + seed_bias(seed).sea_shift.to_num::<f32>();
    let hs: Vec<f32> = g.tile_h.iter().map(|h| surface_height(*h, gain).to_num::<f32>()).collect();
    let mean = box_blur(&hs, AO_R);

    let mut pal = [vec![0f32; n * n], vec![0f32; n * n], vec![0f32; n * n]];
    let mut land = vec![0f32; n * n];
    let mut rock = vec![0f32; n * n];
    let mut arid = vec![0f32; n * n];
    let mut hue = vec![0f32; n * n];
    let mut river = vec![0f32; n * n];
    for ty in 0..n {
        for tx in 0..n {
            let i = ty * n + tx;
            let b = g.biome[i];
            let moist = g.moisture[i].to_num::<f32>();
            let temp = g.temp[i].to_num::<f32>();
            arid[i] = 1.0 - moist;
            hue[i] = smooth01(temp * 1.6 - 0.34) * (1.0 - g.belt[i].to_num::<f32>() * 0.35);
            river[i] = f32::from(matches!(b, Biome::River | Biome::Ford));
            if saladin_sim::biome_is_water(b) {
                continue;
            }
            land[i] = 1.0;
            rock[i] = smooth01((g.slope[i].to_num::<f32>() - ROCK_LO) / (ROCK_HI - ROCK_LO));
            // two scales of mottling: 90-tile drifts that shape a whole
            // region, and a 5-tile octave that is what actually varies inside
            // one screenful at gameplay zoom
            let patch = fbm(
                Fx::from_num(tx as i32) * saladin_sim::fx!("0.07"),
                Fx::from_num(ty as i32) * saladin_sim::fx!("0.07"),
                seed ^ 0x9a55,
                2,
            )
            .to_num::<f32>()
                * 0.55
                + fbm(
                    Fx::from_num(tx as i32) * saladin_sim::fx!("0.21"),
                    Fx::from_num(ty as i32) * saladin_sim::fx!("0.21"),
                    seed ^ 0x31c7,
                    2,
                )
                .to_num::<f32>()
                    * 0.45;
            let c = biome_color(b, g.tile_h[i].to_num::<f32>(), sea, patch, moist, temp);
            let ao = (1.0 - (mean[i] - hs[i]) * AO_K).clamp(0.72, 1.18);
            pal[0][i] = c[0] * ao;
            pal[1][i] = c[1] * ao;
            pal[2][i] = c[2] * ao;
        }
    }

    // The palette blur carries WATER at weight zero, so a coast never bleeds
    // blue into the beach; `land` does the land/sea blending on its own.
    let land2 = blur3(&blur3(&land));
    for ch in &mut pal {
        let mut b = blur3(&blur3(ch));
        for i in 0..n * n {
            b[i] /= land2[i].max(1e-3);
        }
        *ch = b;
    }
    Fields {
        pal,
        land,
        rock: blur3(&blur3(&rock)),
        arid,
        hue: blur3(&hue),
        river: blur3(&river),
    }
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

/// Split a quad, alternating the diagonal by parity. A fixed diagonal turns
/// every silhouette into a run of identical, identically-handed teeth and
/// biases `compute_smooth_normals` into a faint directional lattice.
/// Corners are (a, b, c, d) = (x, y), (x+1, y), (x+1, y+1), (x, y+1); both
/// splits keep the +Y winding.
fn quad(a: u32, b: u32, c: u32, d: u32, parity: i32) -> [u32; 6] {
    if parity & 1 == 0 { [a, c, b, a, d, c] } else { [a, d, b, b, d, c] }
}

/// Half a tile of lateral jitter on interior detail vertices, so a boundary
/// contour breaks up instead of following the lattice. Kept under a quarter
/// tile: a vertex must not cross into the next tile.
const JITTER: f32 = 0.30;

pub fn build_terrain_mesh(seed: u32) -> Mesh {
    let t0 = std::time::Instant::now();
    let n = WORLD_SIZE;
    let lo = -APRON;
    let hi = n + APRON;
    let stride = (hi - lo + 1) as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(stride * stride);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(stride * stride);
    // Soil fertility rides in UV.x so the terrain shader can paint the farm
    // overlay without a second mesh or a texture upload; UV.y flags dry land,
    // so the overlay never paints the sea (fertility is zero out there, which
    // would otherwise read as "barren ground") and the water shader knows
    // which fragments are wet.
    let mut uv0: Vec<[f32; 2]> = Vec::with_capacity(stride * stride);
    // UV_1 carries what the fragment stage needs and vertex colour has no room
    // for: aridity (grassland drying continuously into steppe) and rock hue
    // (cool alpine grey through warm desert ochre).
    let mut uv1: Vec<[f32; 2]> = Vec::with_capacity(stride * stride);
    let sun = SUN.normalize();

    let f = build_fields(seed);
    let shore_dist = land_distance_grid(seed);
    // two octaves of very low-frequency swell, render-only
    let swell = |vx: i32, vy: i32| -> f32 {
        fbm(Fx::from_num(vx) * saladin_sim::fx!("0.035"), Fx::from_num(vy) * saladin_sim::fx!("0.035"), seed ^ 0x0cea, 2)
            .to_num::<f32>()
            + fbm(Fx::from_num(vx) * saladin_sim::fx!("0.011"), Fx::from_num(vy) * saladin_sim::fx!("0.011"), seed ^ 0x5ea1, 2)
                .to_num::<f32>()
            - 1.0 // centre the sum of two 0..1 fields
    };
    let water_y = surface_height(Fx::ZERO, Fx::ONE).to_num::<f32>() * TERRAIN_SCALE;
    for vy in lo..=hi {
        for vx in lo..=hi {
            // Far outside the playable map the continent mask guarantees open
            // ocean — skip the full terrain stack, it IS sea.
            if vx < -2 || vy < -2 || vx > n + 2 || vy > n + 2 {
                positions.push([vx as f32, water_y, vy as f32]);
                let c = water_color(0.0, 999.0, swell(vx, vy));
                let dither = (hash2(vx, vy, seed ^ 0x5eed).to_num::<f32>() - 0.5) * 0.07;
                let m = 0.95 + dither;
                colors.push([c[0] * m, c[1] * m, c[2] * m, 0.0]);
                uv0.push([0.0, 0.0]);
                uv1.push([0.0, 0.0]);
                continue;
            }
            let s = sample_terrain(seed, Fx::from_num(vx), Fx::from_num(vy));
            let h_raw = surface_height(s.height, seed_bias(seed).elev_gain).to_num::<f32>();
            positions.push([vx as f32, h_raw * TERRAIN_SCALE, vy as f32]);

            let t = tap(vx as f32, vy as f32);
            let land = smooth01((bl(&f.land, &t) - LAND_T) / LAND_W);
            let c = lerp3(
                water_color(bl(&f.river, &t), bl(&shore_dist, &t), swell(vx, vy)),
                [bl(&f.pal[0], &t), bl(&f.pal[1], &t), bl(&f.pal[2], &t)],
                land,
            );

            // Directional slope tint: finite-difference normal from neighbour
            // render heights — sun-facing slopes lighten, far sides darken.
            let hx = raw_height(seed, vx + 1, vy);
            let hz = raw_height(seed, vx, vy + 1);
            let lit = Vec3::new(h_raw - hx, 1.0, h_raw - hz).normalize().dot(sun);
            let shade_mul = 0.78 + lit.clamp(-1.0, 1.0) * 0.26;

            // Deterministic per-vertex dither so flat facets get a touch of
            // grain; water gets more — wave glints keep the open sea alive.
            let grain_amp = 0.05 + (1.0 - land) * 0.02;
            let dither = (hash2(vx, vy, seed ^ 0x5eed).to_num::<f32>() - 0.5) * grain_amp;

            let m = (shade_mul + dither).max(0.55);
            colors.push([c[0] * m, c[1] * m, c[2] * m, bl(&f.rock, &t)]);
            uv0.push([fertility_at(seed, Fx::from_num(vx), Fx::from_num(vy)).to_num::<f32>(), land]);
            uv1.push([bl(&f.arid, &t), bl(&f.hue, &t)]);
        }
    }

    let idx = |x: i32, y: i32| ((y - lo) as usize * stride + (x - lo) as usize) as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(((hi - lo) * (hi - lo) * 6) as usize);
    for ty in lo..hi {
        for tx in lo..hi {
            // the playable interior is covered by the half-tile detail grid below
            if tx >= 0 && ty >= 0 && tx < n && ty < n {
                continue;
            }
            let (a, b, c, d) = (idx(tx, ty), idx(tx + 1, ty), idx(tx + 1, ty + 1), idx(tx, ty + 1));
            indices.extend_from_slice(&quad(a, b, c, d, tx + ty));
        }
    }

    // ── half-tile detail grid over the playable map ──────────────────────────
    // Twice the vertex density inside [0, n] so biome edges, slope shading and
    // patch mottling stop smearing across whole tiles at gameplay zoom. The
    // apron stays coarse; boundary half-verts are forced onto the apron edge's
    // straight line so the seam can't crack.
    let m = (n * 2 + 1) as usize;
    let base = positions.len() as u32;
    // The edge ring stays exactly on the lattice: the forced-collinear apron
    // weld below is what keeps the seam from cracking.
    let jitter = |ix: usize, iy: usize| -> (f32, f32) {
        if ix == 0 || iy == 0 || ix == m - 1 || iy == m - 1 {
            return (0.0, 0.0);
        }
        let (a, b) = (ix as i32, iy as i32);
        (
            (hash2(a, b, seed ^ 0x9e11).to_num::<f32>() - 0.5) * JITTER,
            (hash2(b, a, seed ^ 0x51f3).to_num::<f32>() - 0.5) * JITTER,
        )
    };
    let mut hgrid = vec![0f32; m * m];
    for iy in 0..m {
        for ix in 0..m {
            let (jx, jy) = jitter(ix, iy);
            let fx = Fx::from_num(ix as f32 * 0.5 + jx);
            let fy = Fx::from_num(iy as f32 * 0.5 + jy);
            let s = sample_terrain(seed, fx, fy);
            hgrid[iy * m + ix] =
                surface_height(s.height, seed_bias(seed).elev_gain).to_num::<f32>();
        }
    }
    // seam weld: edge half-verts sit exactly on the apron edge's interpolation
    for iy in 0..m {
        for ix in 0..m {
            let on_edge = ix == 0 || iy == 0 || ix == m - 1 || iy == m - 1;
            if !on_edge {
                continue;
            }
            let i = iy * m + ix;
            if ix % 2 == 1 {
                hgrid[i] = (hgrid[i - 1] + hgrid[i + 1]) * 0.5;
            } else if iy % 2 == 1 {
                hgrid[i] = (hgrid[i - m] + hgrid[i + m]) * 0.5;
            }
        }
    }
    for iy in 0..m {
        for ix in 0..m {
            let i = iy * m + ix;
            let (jx, jy) = jitter(ix, iy);
            let (x, y) = (ix as f32 * 0.5 + jx, iy as f32 * 0.5 + jy);
            let h_raw = hgrid[i];
            positions.push([x, h_raw * TERRAIN_SCALE, y]);

            let t = tap(x, y);
            let land = smooth01((bl(&f.land, &t) - LAND_T) / LAND_W);
            let c = lerp3(
                water_color(
                    bl(&f.river, &t),
                    bl(&shore_dist, &t),
                    swell(x.floor() as i32, y.floor() as i32),
                ),
                [bl(&f.pal[0], &t), bl(&f.pal[1], &t), bl(&f.pal[2], &t)],
                land,
            );

            // slope tint from grid neighbours (0.5-step gradient, scaled to
            // match the apron's 1-tile contrast)
            let hx = hgrid[i + if ix + 1 < m { 1 } else { 0 }];
            let hz = hgrid[i + if iy + 1 < m { m } else { 0 }];
            let lit = Vec3::new((h_raw - hx) * 2.0, 1.0, (h_raw - hz) * 2.0).normalize().dot(sun);
            let shade_mul = 0.78 + lit.clamp(-1.0, 1.0) * 0.26;
            let grain_amp = 0.05 + (1.0 - land) * 0.02;
            let dither =
                (hash2(ix as i32, iy as i32, seed ^ 0x5eed).to_num::<f32>() - 0.5) * grain_amp;
            let mul = (shade_mul + dither).max(0.55);
            colors.push([c[0] * mul, c[1] * mul, c[2] * mul, bl(&f.rock, &t)]);
            uv0.push([fertility_at(seed, Fx::from_num(x), Fx::from_num(y)).to_num::<f32>(), land]);
            uv1.push([bl(&f.arid, &t), bl(&f.hue, &t)]);
        }
    }
    for ty in 0..(m - 1) {
        for tx in 0..(m - 1) {
            let a = base + (ty * m + tx) as u32;
            let b = a + 1;
            let d = a + m as u32;
            let c = d + 1;
            indices.extend_from_slice(&quad(a, b, c, d, (tx + ty) as i32));
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    info!(
        "terrain mesh: {} verts, {} tris, {:.0} ms",
        mesh.count_vertices(),
        mesh.indices().map(|i| i.len() / 3).unwrap_or(0),
        t0.elapsed().as_secs_f32() * 1000.0
    );
    mesh
}
