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

    // grain + macro tint on everything else
    let detail = 1.0 + (g - 0.5) * terrain.params.z * (1.0 - rocky);
    let macro_m = 1.0 + (macro_v - 0.5) * terrain.params.w * (1.0 - rocky);
    base = vec4<f32>(base.rgb * detail * macro_m, base.a);

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
