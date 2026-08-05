// Terrain surface extension over StandardMaterial. The mesh ships continuous
// FIELDS, not biome labels: vertex colour is the blurred palette, colour.a the
// blurred rock exposure, uv.xy soil/land and uv_b.xy aridity/rock-hue. This
// stage does the three things vertex data structurally cannot — perturb the
// normal for sub-tile relief that survives tonemapping, project rock
// triplanar so faces stop smearing, and re-sharpen every blurred field along a
// noise-warped contour at PIXEL resolution instead of at the vertex lattice.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

struct TerrainExtension {
    // warm ochre scarp and cool alpine stone; uv_b.y picks between them
    rock_warm: vec4<f32>,
    rock_cool: vec4<f32>,
    // debris apron at the foot of a face
    scree_color: vec4<f32>,
    // x: slope where rock starts, y: slope where rock saturates,
    // z: grain albedo amplitude, w: macro tint amplitude
    params: vec4<f32>,
    // x: bump strength, yz: world-units-per-pixel the bump fades out over,
    // w: bump base frequency
    params2: vec4<f32>,
    // x: strata depth, y: scree strength, z: aridity drift, w: contour warp
    params3: vec4<f32>,
    // x: soil-overlay strength; the mesh carries per-vertex fertility in uv.x
    overlay: vec4<f32>,
    // x: elapsed seconds, yz: cloud drift direction, w: cloud shadow depth
    sky: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> terrain: TerrainExtension;

fn hash2(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// one octave of bilinear value noise
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// value + analytic gradient, so a bump normal costs no extra taps
fn vnoise_d(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    let k1 = b - a;
    let k2 = c - a;
    let k3 = a - b - c + d;
    return vec3<f32>(
        a + k1 * u.x + k2 * u.y + k3 * u.x * u.y,
        du.x * (k1 + k3 * u.y),
        du.y * (k2 + k3 * u.x),
    );
}

// One grain octave, rotated: value noise on an axis-aligned lattice streaks
// along x and z, and on a 45-degree iso camera those streaks are the most
// visible thing in the frame. Returns (value, world-space gradient), the
// gradient rotated back out of the octave's own frame.
fn octave(p: vec2<f32>, fr: f32, c: f32, s: f32, off: vec2<f32>) -> vec3<f32> {
    let m = mat2x2<f32>(c, s, -s, c);
    let nd = vnoise_d(m * p * fr + off);
    return vec3<f32>(
        nd.x,
        (c * nd.y + s * nd.z) * fr,
        (c * nd.z - s * nd.y) * fr,
    );
}

// Three rotated octaves in one projection plane, each faded out at its own
// Nyquist limit. Returns (value, gradU, gradV) in that plane's coordinates.
fn grain(p: vec2<f32>, px: f32, f0: f32) -> vec3<f32> {
    // Amplitude falls slightly faster than frequency rises, so amp * fr — the
    // surface slope each octave actually contributes — stays bounded.
    let f1 = f0 * 2.2;
    let f2 = f1 * 2.2;
    let a0 = 1.000 * (1.0 - smoothstep(0.16 / f0, 0.40 / f0, px));
    let a1 = 0.420 * (1.0 - smoothstep(0.16 / f1, 0.40 / f1, px));
    let a2 = 0.176 * (1.0 - smoothstep(0.16 / f2, 0.40 / f2, px));
    var acc = vec3<f32>(0.0);
    var w = 0.0;
    if a0 > 0.004 {
        let o = octave(p, f0, 1.0, 0.0, vec2<f32>(0.0, 0.0));
        acc += o * a0;
        w += a0;
    }
    if a1 > 0.004 {
        let o = octave(p, f1, 0.4267, 0.9044, vec2<f32>(11.3, 5.7));
        // fold this octave about its midline: a plain sum of 0..1 value noise
        // clusters hard around 0.5 and reads as no texture at all
        acc += vec3<f32>(abs(o.x * 2.0 - 1.0), o.y, o.z) * a1;
        w += a1;
    }
    if a2 > 0.004 {
        let o = octave(p, f2, -0.6749, 0.7379, vec2<f32>(3.1, 19.4));
        acc += o * a2;
        w += a2;
    }
    return select(vec3<f32>(0.5, 0.0, 0.0), vec3<f32>(acc.x / w, acc.y, acc.z), w > 0.0);
}

// Every mask goes through this: a soft field re-crisped along a contour that
// wanders at pixel resolution, so a blend region stays a blend region without
// following the vertex lattice.
fn sharp(field: f32, t: f32, w: f32, warp: f32) -> f32 {
    return smoothstep(t - w, t + w, field + warp);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wp = in.world_position.xyz;
    let n = normalize(in.world_normal);
    let slope = 1.0 - clamp(n.y, 0.0, 1.0);
    // world units a pixel covers: the honest Nyquist limit for every octave
    let px = max(max(fwidth(wp.x), fwidth(wp.z)), fwidth(wp.y));
    // triplanar weights, needed before the grain: an xz-only projection turns
    // every steep face into vertical streaks, both in the bump and the albedo
    var tw = pow(abs(n), vec3<f32>(4.0));
    tw /= (tw.x + tw.y + tw.z);

    var arid = 0.0;
    var rock_hue = 0.5;
#ifdef VERTEX_UVS_B
    arid = clamp(in.uv_b.x, 0.0, 1.0);
    rock_hue = clamp(in.uv_b.y, 0.0, 1.0);
#endif
    var exposure = 0.0;
#ifdef VERTEX_COLORS
    exposure = clamp(in.color.a, 0.0, 1.0);
#endif
    let wet = 1.0 - clamp(in.uv.y, 0.0, 1.0);

    // ── surface grain: three octaves of value noise carried as a NORMAL ──────
    // A normal perturbation is worth roughly ten times an albedo multiply of
    // the same amplitude, and unlike albedo it survives tonemapping + bloom.
    let f0 = terrain.params2.w;
    let gy = grain(wp.xz, px, f0);
    var g = gy.x;
    var grad = vec3<f32>(gy.y, 0.0, gy.z);
    if tw.y < 0.94 {
        let gx = grain(wp.zy, px, f0);
        let gz = grain(wp.xy, px, f0);
        g = gy.x * tw.y + gx.x * tw.x + gz.x * tw.z;
        grad = vec3<f32>(gy.y, 0.0, gy.z) * tw.y
            + vec3<f32>(0.0, gx.z, gx.y) * tw.x
            + vec3<f32>(gz.y, gz.z, 0.0) * tw.z;
    }
    let bump_fade = 1.0 - smoothstep(terrain.params2.y, terrain.params2.z, px);

    // broad patchiness so large fills read as ground, not paint
    let macro_v = vnoise(wp.xz * 0.07) * 0.7 + vnoise(wp.xz * 0.23) * 0.3;
    // triplanar too: on a vertical face an xz-only warp is constant down the
    // face, and every re-sharpened mask edge prints a vertical stripe
    let warp = (vnoise(wp.zy * 2.0) * tw.x
        + vnoise(wp.xz * 2.0) * tw.y
        + vnoise(wp.xy * 2.0) * tw.z
        - 0.5) * terrain.params3.w;

    var base = pbr_input.material.base_color;

    // ── rock, scree and the ground between them ─────────────────────────────
    let steep = smoothstep(terrain.params.x, terrain.params.y, slope);
    let rock_field = clamp(exposure * 0.62 + steep * 0.62 + exposure * steep * 0.4, 0.0, 1.0);
    let rocky = sharp(rock_field, 0.56, 0.16, warp) * (1.0 - wet);
    // debris settles where the neighbourhood is rocky but THIS ground is not:
    // the apron at the foot of a face, never the face itself
    let scree = sharp(rock_field, 0.26, 0.15, warp * 0.8)
        * (1.0 - rocky)
        * smoothstep(0.55, 0.90, n.y)
        * (1.0 - wet)
        * terrain.params3.y;

    if scree > 0.002 {
        let peb = vnoise(wp.xz * 4.5);
        let speck = smoothstep(0.54, 0.80, peb);
        let talus = terrain.scree_color.rgb * (0.86 + 0.34 * g);
        base = vec4<f32>(
            mix(base.rgb, mix(talus, talus * 1.26, speck), scree * 0.70),
            base.a,
        );
    }

    if rocky > 0.002 {
        let det = vnoise(wp.zy * 3.1) * tw.x
            + vnoise(wp.xz * 3.1) * tw.y
            + vnoise(wp.xy * 3.1) * tw.z;
        let coarse = vnoise(wp.zy * 0.7) * tw.x
            + vnoise(wp.xz * 0.7) * tw.y
            + vnoise(wp.xy * 0.7) * tw.z;
        // strata band on HEIGHT alone, warped sideways so the layers undulate
        // instead of running dead straight around the hill
        let band = wp.y * 4.2
            + (vnoise(wp.xz * 0.10) - 0.5) * 3.0
            + (vnoise(wp.xz * 0.42) - 0.5) * 0.9;
        let strata = vnoise(vec2<f32>(band, 17.3)) * 0.62
            + vnoise(vec2<f32>(band * 2.6, 41.1)) * 0.38;
        let hue = clamp(rock_hue + (strata - 0.5) * 0.5, 0.0, 1.0);
        var rock = mix(terrain.rock_cool.rgb, terrain.rock_warm.rgb, hue);
        rock *= 1.0 - terrain.params3.x * (0.5 - strata);
        rock *= 0.84 + 0.32 * det + 0.16 * coarse;
        base = vec4<f32>(mix(base.rgb, rock, rocky), base.a);
    }

    // dry ground bleaches and warms continuously — no label anywhere in it
    let dry = smoothstep(0.42, 0.86, arid) * terrain.params3.z * (1.0 - wet);
    if dry > 0.002 {
        let lum = dot(base.rgb, vec3<f32>(0.3, 0.59, 0.11));
        base = vec4<f32>(
            mix(base.rgb, mix(base.rgb, vec3<f32>(lum) * vec3<f32>(1.14, 1.03, 0.74), 0.65), dry),
            base.a,
        );
    }

    // Live water: the mesh bakes the swell into vertex colour, which is static
    // and reads as painted. Two crossing wave trains and a sun glint make the
    // same surface move.
    if wet > 0.0 {
        let t = terrain.sky.x;
        let w1 = vnoise(wp.xz * 0.11 + vec2<f32>(t * 0.35, t * 0.12));
        let w2 = vnoise(wp.xz * 0.31 - vec2<f32>(t * 0.19, t * 0.44));
        let wave = (w1 * 0.6 + w2 * 0.4 - 0.5);
        // narrow crests catch the sun; troughs deepen toward the sea colour
        let glint = smoothstep(0.24, 0.42, wave);
        base = vec4<f32>(base.rgb * (1.0 + wave * 0.16 * wet) + vec3<f32>(0.10, 0.13, 0.14) * glint * wet, base.a);
    }

    // Cloud shadows: slow soft-edged patches drifting across the map. Nothing
    // sells scale on a static heightfield like weather crossing it.
    if terrain.sky.w > 0.0 {
        let drift = terrain.sky.yz * terrain.sky.x;
        let c1 = vnoise(wp.xz * 0.013 + drift * 0.013);
        let c2 = vnoise(wp.xz * 0.031 + drift * 0.031);
        let cover = smoothstep(0.46, 0.78, c1 * 0.68 + c2 * 0.32);
        base = vec4<f32>(base.rgb * (1.0 - cover * terrain.sky.w), base.a);
    }

    // grain + macro tint on everything else
    let detail = 1.0 + (g - 0.5) * terrain.params.z;
    let macro_m = 1.0 + (macro_v - 0.5) * terrain.params.w * (1.0 - rocky);
    base = vec4<f32>(base.rgb * detail * macro_m, base.a);

    // soil overlay: while a farm is being sited, wash the ground in how well
    // it would grow — barren red through to deep green on the best alluvium
    if terrain.overlay.x > 0.0 {
#ifdef VERTEX_UVS
        let soil = clamp(in.uv.x, 0.0, 1.0);
        let land = in.uv.y;
        let barren = vec3<f32>(0.32, 0.06, 0.05);
        let fair = vec3<f32>(0.35, 0.30, 0.05);
        let rich = vec3<f32>(0.06, 0.42, 0.10);
        var tint = mix(barren, fair, smoothstep(0.10, 0.30, soil));
        tint = mix(tint, rich, smoothstep(0.30, 0.62, soil));
        // banded contours so the eye reads discrete quality steps, not a wash
        let band = 0.85 + 0.15 * step(0.5, fract(soil * 8.0));
        base = vec4<f32>(mix(base.rgb, tint * band, terrain.overlay.x * land * 0.7), base.a);
#endif
    }

    pbr_input.material.base_color = base;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    // The bump goes in LAST: it must not disturb any mask above, and the
    // tangential projection keeps it face-aligned on a cliff and flat-aligned
    // on a plain without a seam where the two meet.
    if bump_fade > 0.004 {
        let s = terrain.params2.x * bump_fade * (1.0 - wet * 0.7) * (1.0 + rocky * 0.8);
        let tangential = grad - n * dot(grad, n);
        pbr_input.N = normalize(pbr_input.N - tangential * s);
    }

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
