//! Sky dome + endless ocean + lighting rig (port of src/game/Environment.ts).
//!
//! The TS version used custom GLSL shaders for the sky gradient / sun glow and
//! the ocean's distance shading. Here both are baked as per-vertex colors on
//! unlit StandardMaterials, and the ocean shimmer is a base_color pulse driven
//! by Res<Time> instead of a per-fragment sine band.

use bevy::asset::RenderAssetUsages;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use saladin_sim::WORLD_SIZE;
use std::f32::consts::{PI, TAU};

#[derive(Component)]
pub struct SkyDome;

#[derive(Component)]
pub struct OceanPlane;

#[derive(Component)]
pub struct SunLight;

const SKY_RADIUS: f32 = 1200.0;
// Sea sits just below the shoreline render height (render_height(SEA) == 0,
// water tiles dip to about -0.275 at TERRAIN_SCALE 0.5).
const OCEAN_Y: f32 = -0.05;
const FOG_START: f32 = 260.0;
const FOG_END: f32 = 1100.0;

fn lin(hex: u32) -> Vec3 {
    let c = Color::srgb_u8(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
        .to_linear();
    Vec3::new(c.red, c.green, c.blue)
}

/// Horizon haze — also feeds ClearColor + fog so the whole map dissolves into it.
fn horizon() -> Vec3 {
    lin(0xd8d2be)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sky_color(dir: Vec3) -> [f32; 4] {
    let zenith = lin(0x5d8fc4);
    let haze = horizon();
    let sun_glow = lin(0xffe9c2);
    let sun_dir = Vec3::new(40.0, 70.0, 20.0).normalize();

    let t = dir.y.clamp(0.0, 1.0).powf(0.62);
    let mut col = haze.lerp(zenith, t);
    let sun = dir.dot(sun_dir).max(0.0).powf(6.0);
    let low_bias = 1.0 - smoothstep(0.0, 0.5, dir.y);
    col = col.lerp(sun_glow, sun * (0.35 + 0.4 * low_bias));
    let horizon_band = 1.0 - smoothstep(0.0, 0.18, dir.y.abs());
    col = col.lerp(haze, horizon_band * 0.25);
    [col.x, col.y, col.z, 1.0]
}

fn build_sky_mesh() -> Mesh {
    let stacks = 16u32;
    let sectors = 32u32;
    let ring = sectors + 1;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();

    for i in 0..=stacks {
        let phi = PI * i as f32 / stacks as f32;
        let (y, r) = (phi.cos(), phi.sin());
        for j in 0..=sectors {
            let theta = TAU * j as f32 / sectors as f32;
            let dir = Vec3::new(r * theta.cos(), y, r * theta.sin());
            positions.push((dir * SKY_RADIUS).to_array());
            normals.push((-dir).to_array());
            colors.push(sky_color(dir));
        }
    }

    let mut indices: Vec<u32> = Vec::with_capacity((stacks * sectors * 6) as usize);
    for i in 0..stacks {
        for j in 0..sectors {
            let a = i * ring + j;
            let b = a + ring;
            // Inward-facing winding (viewed from inside the dome).
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn ocean_color(d: f32) -> [f32; 4] {
    // match the terrain mesh's water gradient so the map edge has no seam
    let deep = lin(0x2f7494);
    let shallow = lin(0x3a86a8);
    let haze = horizon();

    // Near the centre (under the camera): lighter shallow water. Farther: deep teal.
    let mut col = shallow.lerp(deep, smoothstep(20.0, 320.0, d));
    // Far out, melt into the horizon haze to kill the visible plane edge.
    col = col.lerp(haze, smoothstep(800.0, 2600.0, d));
    [col.x, col.y, col.z, 1.0]
}

/// Concentric-ring disc so the radial shallow→deep→haze gradient bakes into
/// vertex colors (the TS version computed it per-fragment in a shader).
fn build_ocean_mesh() -> Mesh {
    let radii: [f32; 11] = [0.0, 20.0, 80.0, 160.0, 320.0, 600.0, 800.0, 1200.0, 1600.0, 2600.0, 4000.0];
    let segments = 48u32;
    let ring = segments + 1;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();

    for &r in &radii {
        for j in 0..=segments {
            let theta = TAU * j as f32 / segments as f32;
            positions.push([r * theta.cos(), 0.0, r * theta.sin()]);
            normals.push([0.0, 1.0, 0.0]);
            colors.push(ocean_color(r));
        }
    }

    let mut indices: Vec<u32> = Vec::with_capacity((radii.len() - 1) * segments as usize * 6);
    for i in 0..(radii.len() as u32 - 1) {
        for j in 0..segments {
            let a = i * ring + j;
            let c = a + ring;
            indices.extend_from_slice(&[a, a + 1, c, a + 1, c + 1, c]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Spawn the sky/ocean/light rig once at match start. Includes the full sun +
/// ambient + clear-color setup so integration can drop the crude main.rs light.
pub fn spawn_environment(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let h = horizon();
    commands.insert_resource(ClearColor(Color::linear_rgb(h.x, h.y, h.z)));

    let center = Vec3::new(WORLD_SIZE as f32 / 2.0, 0.0, WORLD_SIZE as f32 / 2.0);

    commands.spawn((
        SkyDome,
        Mesh3d(meshes.add(build_sky_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            cull_mode: None,
            double_sided: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::from_translation(center),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // Opaque — a see-through ocean reveals the bright sky beyond the finite
    // terrain, which reads as a hard square edge. Solid sea hides that seam.
    commands.spawn((
        OceanPlane,
        Mesh3d(meshes.add(build_ocean_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(center.with_y(OCEAN_Y)),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // DirectionalLight('#fff3d6', 1.1) at (40, 70, 20) with shadows.
    commands.spawn((
        SunLight,
        DirectionalLight {
            color: Color::srgb_u8(0xff, 0xf3, 0xd6),
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            maximum_distance: FOG_START,
            first_cascade_far_bound: 40.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(40.0, 70.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Keep sky + ocean centered under the camera each frame (geometry stays on its
/// own origin — no baked world offset), and lazily attach the horizon fog to
/// the camera (main.rs owns the camera entity, so fog rides along here).
pub fn follow_camera(
    mut commands: Commands,
    cameras: Query<
        (Entity, &Transform, Has<DistanceFog>),
        (With<Camera3d>, Without<SkyDome>, Without<OceanPlane>),
    >,
    mut env: Query<&mut Transform, Or<(With<SkyDome>, With<OceanPlane>)>>,
) {
    let Ok((cam_entity, cam, has_fog)) = cameras.single() else { return };
    if !has_fog {
        let h = horizon();
        commands.entity(cam_entity).insert((
            DistanceFog {
                color: Color::linear_rgb(h.x, h.y, h.z),
                falloff: FogFalloff::Linear { start: FOG_START, end: FOG_END },
                ..default()
            },
            // HemisphereLight('#ffffff', '#6b5a3a', 0.9) approximated as a warm
            // per-camera ambient.
            AmbientLight {
                color: Color::srgb_u8(0xc9, 0xbe, 0xa4),
                brightness: 300.0,
                ..default()
            },
        ));
    }
    for mut t in &mut env {
        t.translation.x = cam.translation.x;
        t.translation.z = cam.translation.z;
    }
}

/// Faint shimmer so the flat sea isn't a dead colour field — base_color pulse
/// multiplies the baked vertex gradient (the TS version used a sine band in
/// the fragment shader).
pub fn shimmer_ocean(
    time: Res<Time>,
    ocean: Query<&MeshMaterial3d<StandardMaterial>, With<OceanPlane>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let s = 1.0 + 0.04 * (time.elapsed_secs() * 0.8).sin();
    for handle in &ocean {
        if let Some(mut mat) = materials.get_mut(&handle.0) {
            mat.base_color = Color::linear_rgb(s, s, s);
        }
    }
}
