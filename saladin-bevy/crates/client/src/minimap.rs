//! Minimap: a second top-down camera rendered into a bottom-right viewport,
//! plus click-to-navigate (clicks inside the viewport refocus the main camera
//! on the corresponding world point — port of the TS minimap's onClickWorld).

use crate::camera::{CameraState, GameCamera, focus_on};
use crate::terrain::HeightField;
use bevy::camera::{ScalingMode, Viewport};
use bevy::prelude::*;

use saladin_sim::WORLD_SIZE;

const SIZE: u32 = 214;
const MARGIN: u32 = 8;

#[derive(Component)]
pub struct MinimapCam;

/// A second camera looking straight down at the whole map, rendered into a
/// small corner viewport. Reuses the same 3D scene, so it always reflects the
/// real game state.
pub fn spawn_minimap(mut commands: Commands) {
    let c = WORLD_SIZE as f32 / 2.0;
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            viewport: Some(Viewport {
                physical_position: UVec2::new(20, 44),
                physical_size: UVec2::new(SIZE, SIZE),
                ..default()
            }),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: WORLD_SIZE as f32,
            },
            far: 600.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(c, 300.0, c).looking_at(Vec3::new(c, 0.0, c), Vec3::Z),
        MinimapCam,

    ));
}

pub fn despawn_minimap(mut commands: Commands, q: Query<Entity, With<MinimapCam>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Pin the minimap to the bottom-right corner as the window resizes.
pub fn update_minimap_viewport(
    windows: Query<&Window>,
    mut q: Query<&mut Camera, With<MinimapCam>>,
    mut q_frame: Query<&mut Node, With<crate::ui::assets::MinimapFrame>>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok(mut cam) = q.single_mut() else { return };
    let w = window.physical_width();
    let h = window.physical_height();
    if let Some(vp) = cam.viewport.as_mut() {
        vp.physical_position =
            UVec2::new(w.saturating_sub(SIZE + MARGIN), h.saturating_sub(SIZE + MARGIN));
    }
    // bronze frame hugging the viewport (UI works in logical px)
    if let Ok(mut node) = q_frame.single_mut() {
        let s = window.scale_factor();
        let size = SIZE as f32 / s;
        let margin = MARGIN as f32 / s;
        node.position_type = PositionType::Absolute;
        node.right = Val::Px(margin - 2.0);
        node.bottom = Val::Px(margin - 2.0);
        node.width = Val::Px(size + 4.0);
        node.height = Val::Px(size + 4.0);
    }
}

/// Click inside the minimap viewport → refocus the main camera there.
pub fn minimap_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    q_mini: Query<&Camera, With<MinimapCam>>,
    field: Option<Res<HeightField>>,
    mut state: ResMut<CameraState>,
    mut q_main: Query<&mut Transform, With<GameCamera>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(cam) = q_mini.single() else { return };
    let Some(vp) = cam.viewport.as_ref() else { return };
    let scale = window.scale_factor();
    let pos = Vec2::new(vp.physical_position.x as f32, vp.physical_position.y as f32) / scale;
    let size = Vec2::new(vp.physical_size.x as f32, vp.physical_size.y as f32) / scale;
    let rel = (cursor - pos) / size;
    if rel.x < 0.0 || rel.x > 1.0 || rel.y < 0.0 || rel.y > 1.0 {
        return;
    }
    // viewport up is world +Z because the camera looks down with up = +Z
    let wx = rel.x * WORLD_SIZE as f32;
    let wz = (1.0 - rel.y) * WORLD_SIZE as f32;
    if let Ok(mut tf) = q_main.single_mut() {
        focus_on(&mut state, &mut tf, wx, wz, field.as_deref());
    }
}
