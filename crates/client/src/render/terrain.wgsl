// Terrain surface extension over StandardMaterial: the mesh's vertex colors
// carry the biome palette; this shader fixes what vertex colors can't do —
// steep faces smear their interpolated colors across whole quads, so cliffs
// looked like mud. Slope-based rock blending + procedural grain give every
// fragment real surface detail at any zoom.

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
    // rock albedo for steep faces (linear)
    rock_color: vec4<f32>,
    // x: slope where rock starts, y: slope where rock saturates,
    // z: grain amplitude, w: macro tint amplitude
    params: vec4<f32>,
    // x: soil-overlay strength; the mesh carries per-vertex fertility in uv.x
    overlay: vec4<f32>,
    // x: elapsed seconds, yz: cloud drift direction, w: cloud shadow depth
    sky: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> terrain: TerrainExtension;

fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
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

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wp = in.world_position.xyz;
    let n = normalize(in.world_normal);
    let slope = 1.0 - clamp(n.y, 0.0, 1.0);

    // surface grain: two octaves, world-anchored so it never swims —
    // coarse octave dominates so the texture reads at gameplay zoom
    let g = vnoise(wp.xz * 1.3) * 0.6 + vnoise(wp.xz * 4.7) * 0.4;
    // broad patchiness so large fills read as ground, not paint
    let macro_v = vnoise(wp.xz * 0.07) * 0.7 + vnoise(wp.xz * 0.23) * 0.3;

    var base = pbr_input.material.base_color;

    // steep faces shear to bare rock, with strata bands down the face
    let rocky = smoothstep(terrain.params.x, terrain.params.y, slope);
    if rocky > 0.0 {
        let strata = 0.82 + 0.18 * vnoise(vec2<f32>(wp.y * 6.0, (wp.x + wp.z) * 0.35));
        let rock = terrain.rock_color.rgb * strata * (0.8 + 0.4 * g);
        base = vec4<f32>(mix(base.rgb, rock, rocky * 0.9), base.a);
    }

    // Live water: the mesh bakes the swell into vertex colour, which is static
    // and reads as painted. Two crossing wave trains and a sun glint make the
    // same surface move.
    let wet = 1.0 - clamp(in.uv.y, 0.0, 1.0);
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
    let detail = 1.0 + (g - 0.5) * terrain.params.z * (1.0 - rocky);
    let macro_m = 1.0 + (macro_v - 0.5) * terrain.params.w * (1.0 - rocky);
    base = vec4<f32>(base.rgb * detail * macro_m, base.a);

    // soil overlay: while a farm is being sited, wash the ground in how well
    // it would grow — barren red through to deep green on the best alluvium
    if terrain.overlay.x > 0.0 {
#ifdef VERTEX_UVS
        let soil = clamp(in.uv.x, 0.0, 1.0);
        let dry = in.uv.y;
        let barren = vec3<f32>(0.32, 0.06, 0.05);
        let fair = vec3<f32>(0.35, 0.30, 0.05);
        let rich = vec3<f32>(0.06, 0.42, 0.10);
        var tint = mix(barren, fair, smoothstep(0.10, 0.30, soil));
        tint = mix(tint, rich, smoothstep(0.30, 0.62, soil));
        // banded contours so the eye reads discrete quality steps, not a wash
        let band = 0.85 + 0.15 * step(0.5, fract(soil * 8.0));
        base = vec4<f32>(mix(base.rgb, tint * band, terrain.overlay.x * dry * 0.7), base.a);
#endif
    }

    pbr_input.material.base_color = base;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
