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
// The sun keeps a fixed bearing RELATIVE to the camera (classic RTS "over the
// shoulder" key light): rotating with Q/E re-aims the sun too, so the player
// never stares at the unlit side of every building. These reproduce the
// original fixed sun at (40, 70, 20) when yaw == 0.
const SUN_DIST: f32 = 83.0;
const SUN_ELEV_COS: f32 = 0.5384; // 44.72 / |(40,70,20)|
const SUN_ELEV_SIN: f32 = 0.8427; // 70 / |(40,70,20)|
/// Sun bearing relative to the camera ring angle (radians, clockwise).
const SUN_CAM_OFFSET: f32 = 0.3218;
// The terrain's flat sea surface sits at -0.43*TERRAIN_SCALE; the backdrop
// disc must stay strictly BELOW it everywhere inside the map, or it covers
// the real water and paints a flat second blue with a hard mesh-intersection
// edge (the infamous "two blues" bug).
const OCEAN_Y: f32 = -0.30;
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
    // Safety floor only: the generated ocean apron covers everything the
    // camera can reach, so this stays the flat sea hue and melts into the
    // horizon far beyond the fog.
    let sea = lin(0x4ea4bd);
    let haze = horizon();
    let col = sea.lerp(haze, smoothstep(900.0, 2600.0, d));
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
            // bright enough that faces away from the sun shade, not blacken
            AmbientLight {
                color: Color::srgb_u8(0xc9, 0xbe, 0xa4),
                brightness: 480.0,
                ..default()
            },
        ));
    }
    for mut t in &mut env {
        t.translation.x = cam.translation.x;
        t.translation.z = cam.translation.z;
    }
}

/// Re-aim the sun as the camera yaws (Q/E) so its bearing relative to the view
/// stays the one the scene was lit for — every angle reads like the classic
/// iso shot instead of staring at unlit walls. The sky dome's baked glow
/// turns with it so the bright patch stays over the sun.
pub fn sun_follows_camera(
    cam: Res<crate::camera::CameraState>,
    mut sun: Query<&mut Transform, (With<SunLight>, Without<SkyDome>)>,
    mut sky: Query<&mut Transform, (With<SkyDome>, Without<SunLight>)>,
) {
    let az = std::f32::consts::FRAC_PI_4 + cam.yaw - SUN_CAM_OFFSET;
    let pos = Vec3::new(az.cos() * SUN_ELEV_COS, SUN_ELEV_SIN, az.sin() * SUN_ELEV_COS) * SUN_DIST;
    let target = Transform::from_translation(pos).looking_at(Vec3::ZERO, Vec3::Y);
    for mut t in &mut sun {
        if t.rotation.angle_between(target.rotation) > 1e-4 {
            *t = target;
        }
    }
    let dome_rot = Quat::from_rotation_y(-cam.yaw);
    for mut t in &mut sky {
        if t.rotation.angle_between(dome_rot) > 1e-4 {
            t.rotation = dome_rot;
        }
    }
}

// ── living world: water sparkle, shore ripples, seagulls ────────────────────

/// Shallow/deep-water tiles that border land — anchor points for shore
/// ripples and gull roosts. Computed once per match.
#[derive(Resource, Default)]
pub struct ShoreList(pub Vec<Vec3>);

/// A drifting glint layer over the open water (scrolling baked sparkle dots).
#[derive(Component)]
pub struct SparkleLayer {
    pub speed: Vec2,
}

#[derive(Component)]
pub struct Gull {
    pub center: Vec3,
    pub r: f32,
    pub w: f32,
    pub phase: f32,
}

#[derive(Component)]
pub struct GullWing {
    pub left: bool,
}

/// Soft white sparkle dots on transparency, tiled across the sea.
pub fn sparkle_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let n = 128u32;
    let mut data = vec![0u8; (n * n * 4) as usize];
    let mut h = 0x1234_5678u32;
    let mut rnd = || {
        h ^= h << 13;
        h ^= h >> 17;
        h ^= h << 5;
        h
    };
    for _ in 0..70 {
        let cx = (rnd() % n) as i32;
        let cy = (rnd() % n) as i32;
        let bright = 190 + (rnd() % 66) as i32;
        // a 2-3px soft glint
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (x, y) = ((cx + dx).rem_euclid(n as i32), (cy + dy).rem_euclid(n as i32));
                let fall = if dx == 0 && dy == 0 { 1.0 } else { 0.35 };
                let i = ((y as u32 * n + x as u32) * 4) as usize;
                let a = (bright as f32 * fall) as u8;
                data[i] = 255;
                data[i + 1] = 255;
                data[i + 2] = 255;
                data[i + 3] = data[i + 3].max(a);
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// Scroll the two glint layers in opposite drifts — the sea glitters.
pub fn animate_sparkle(
    time: Res<Time>,
    q: Query<(&SparkleLayer, &MeshMaterial3d<StandardMaterial>)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    for (layer, handle) in &q {
        if let Some(mut mat) = mats.get_mut(&handle.0) {
            mat.uv_transform.translation = layer.speed * t;
        }
    }
}

/// Gulls wheel over the coast: circular soar, gentle height swell, wings
/// flapping in bursts.
pub fn animate_gulls(
    time: Res<Time>,
    mut roots: Query<(&Gull, &mut Transform)>,
    mut wings: Query<(&GullWing, &mut Transform), Without<Gull>>,
) {
    let t = time.elapsed_secs();
    for (g, mut tf) in &mut roots {
        let a = t * g.w + g.phase;
        let pos = g.center + Vec3::new(a.cos() * g.r, 1.6 + (t * 0.7 + g.phase).sin() * 0.5, a.sin() * g.r);
        tf.translation = pos;
        // face the tangent of the circle
        let tangent = Vec3::new(-a.sin(), 0.0, a.cos()) * g.w.signum();
        tf.rotation = Quat::from_rotation_y(tangent.x.atan2(tangent.z));
    }
    for (w, mut tf) in &mut wings {
        // flap bursts: quick beats, then a glide
        let beat = ((t * 7.0).sin() * 0.55).max(-0.15) * if w.left { 1.0 } else { -1.0 };
        tf.rotation = Quat::from_rotation_z(beat);
    }
}

/// Occasionally bloom a foam ring on a shore tile (expand-and-fade pulse via
/// the shared particle curve).
pub fn spawn_shore_ripples(
    time: Res<Time>,
    mut commands: Commands,
    shore: Option<Res<ShoreList>>,
    assets: Option<Res<crate::render::sync::RenderAssets>>,
    rmats: Option<Res<crate::render::sync::RenderMaterials>>,
    cam: Res<crate::camera::CameraState>,
    mut acc: Local<f32>,
) {
    let (Some(shore), Some(assets), Some(rmats)) = (shore, assets, rmats) else { return };
    if shore.0.is_empty() {
        return;
    }
    // only bloom ripples the camera can actually see
    let reach = cam.view_size * 1.6;
    let c = cam.center;
    let visible: Vec<&Vec3> = shore
        .0
        .iter()
        .filter(|p| (p.x - c.x).abs() < reach && (p.z - c.z).abs() < reach)
        .collect();
    if visible.is_empty() {
        return;
    }
    *acc += time.delta_secs() * 6.0;
    let t = time.elapsed_secs();
    while *acc >= 1.0 {
        *acc -= 1.0;
        let k = ((t * 977.0 + *acc * 131.0).sin() * 0.5 + 0.5).abs();
        let at = *visible[(k * (visible.len() - 1) as f32) as usize];
        commands.spawn((
            crate::render::sync::Particle { vel: Vec3::ZERO, age: 0.0, life: 1.6, base: 0.55 },
            Mesh3d(assets.ripple.clone()),
            MeshMaterial3d(rmats.foam.clone()),
            Transform::from_translation(at + Vec3::Y * 0.04).with_scale(Vec3::splat(0.01)),
        ));
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
