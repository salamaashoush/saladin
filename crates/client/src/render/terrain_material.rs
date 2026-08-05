//! Terrain surface material: StandardMaterial (vertex-colored biome palette)
//! extended with a slope-aware WGSL fragment — steep faces blend to banded
//! rock instead of smearing interpolated vertex colors, and every fragment
//! gets world-anchored procedural grain. See assets/shaders/terrain.wgsl.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, ShaderType, Debug)]
pub struct TerrainUniform {
    /// Warm ochre scarp and cool alpine stone. One global `rock_color` painted
    /// a Hammada scarp, an Alpine face and a grassland cliff the same beige;
    /// the mesh's uv_b.y picks between these two per tile.
    pub rock_warm: LinearRgba,
    pub rock_cool: LinearRgba,
    /// Debris apron at the foot of a face.
    pub scree_color: LinearRgba,
    /// x: slope where rock starts, y: slope where it saturates,
    /// z: grain amplitude, w: macro tint amplitude
    pub params: Vec4,
    /// x: bump strength, yz: world-units-per-pixel the bump normal fades out
    /// over (so the strategic camera stays clean), w: bump base frequency.
    pub params2: Vec4,
    /// x: strata depth, y: scree strength, z: aridity drift, w: how far the
    /// contour warp displaces every re-sharpened mask.
    pub params3: Vec4,
    /// x: soil-overlay strength (0 = off). While the player is siting a farm
    /// the ground itself has to answer "where will anything grow", so the
    /// fertility the worldgen computed is painted straight onto the terrain.
    pub overlay: Vec4,
    /// x: elapsed seconds, yz: cloud drift direction, w: cloud shadow depth.
    pub sky: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[reflect(opaque)]
pub struct TerrainExtension {
    #[uniform(100)]
    pub settings: TerrainUniform,
}

impl Default for TerrainExtension {
    fn default() -> Self {
        TerrainExtension {
            settings: TerrainUniform {
                rock_warm: Color::srgb_u8(0xb2, 0x91, 0x60).to_linear(),
                rock_cool: Color::srgb_u8(0x77, 0x76, 0x72).to_linear(),
                scree_color: Color::srgb_u8(0x9b, 0x92, 0x82).to_linear(),
                params: Vec4::new(0.20, 0.52, 0.17, 0.34),
                params2: Vec4::new(0.28, 0.085, 0.17, 1.6),
                params3: Vec4::new(0.38, 0.85, 0.30, 0.26),
                overlay: Vec4::ZERO,
                sky: Vec4::new(0.0, 0.62, 0.78, 0.16),
            },
        }
    }
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        // embedded: release binaries ship without an assets directory, and a
        // missing material shader doesn't degrade — the terrain vanishes
        "embedded://saladin_client/render/terrain.wgsl".into()
    }
}

/// Fade the soil overlay in while the player is siting a field and out again
/// when they are not — an instant switch reads as a glitch, a quarter-second
/// wash reads as the ground answering the question.
/// Advance the shader clock the water and cloud shadows run on.
pub fn drive_sky_clock(time: Res<Time>, mut materials: ResMut<Assets<TerrainMaterial>>) {
    let t = time.elapsed_secs_wrapped();
    for (_, mat) in materials.iter_mut() {
        mat.extension.settings.sky.x = t;
    }
}

pub fn drive_soil_overlay(
    mode: Res<crate::input::InputMode>,
    time: Res<Time>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let want = match *mode {
        crate::input::InputMode::Build(k) => {
            f32::from(saladin_sim::building_def(k).min_fertility > saladin_sim::ZERO)
        }
        _ => 0.0,
    };
    for (_, mat) in materials.iter_mut() {
        let cur = mat.extension.settings.overlay.x;
        if (cur - want).abs() < 0.002 {
            continue;
        }
        let step = time.delta_secs() * 4.0;
        mat.extension.settings.overlay.x = if want > cur {
            (cur + step).min(want)
        } else {
            (cur - step).max(want)
        };
    }
}

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "terrain.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        app.add_systems(Update, (drive_soil_overlay, drive_sky_clock));
    }
}
