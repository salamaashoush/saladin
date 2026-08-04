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
    pub rock_color: LinearRgba,
    /// x: slope where rock starts, y: slope where it saturates,
    /// z: grain amplitude, w: macro tint amplitude
    pub params: Vec4,
    /// x: soil-overlay strength (0 = off). While the player is siting a farm
    /// the ground itself has to answer "where will anything grow", so the
    /// fertility the worldgen computed is painted straight onto the terrain.
    pub overlay: Vec4,
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
                rock_color: Color::srgb_u8(0x84, 0x7a, 0x68).to_linear(),
                params: Vec4::new(0.22, 0.5, 0.3, 0.2),
                overlay: Vec4::ZERO,
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
        app.add_systems(Update, drive_soil_overlay);
    }
}
