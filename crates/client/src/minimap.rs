//! Minimap: a second top-down camera rendered into a bottom-right viewport,
//! plus click-to-navigate (clicks inside the viewport refocus the main camera
//! on the corresponding world point — port of the TS minimap's onClickWorld).
//!
//! Raw scene re-render leaves units sub-pixel (288 world tiles into 214 px),
//! so a blip layer (RenderLayers MINIMAP_LAYER, minimap camera only) draws
//! team-colored markers for units/buildings, resource dots, and the main
//! camera's ground footprint as a gizmo rectangle.

use crate::camera::{CameraState, GameCamera, focus_on};
use crate::terrain::HeightField;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ScalingMode, Viewport};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Player, Pos, ResourceNode, Unit};
use saladin_sim::{PLAYER_COLORS, ResourceType, WORLD_SIZE, building_def};

const SIZE: u32 = 214;
const MARGIN: u32 = 8;
/// Render layer only the minimap camera sees (world default layer is 0).
pub const MINIMAP_LAYER: usize = 2;

// blip altitudes: units over buildings over resource dots, all over terrain
const BLIP_Y_NODE: f32 = 16.0;
const BLIP_Y_BUILDING: f32 = 18.0;
const BLIP_Y_UNIT: f32 = 20.0;
const VIEW_RECT_Y: f32 = 24.0;

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
        // up = -Z so the viewport matches the menu preview image exactly:
        // screen-right = +X, screen-down = +Z (and clicks map 1:1)
        Transform::from_xyz(c, 300.0, c).looking_at(Vec3::new(c, 0.0, c), -Vec3::Z),
        MinimapCam,
        // the world plus the blip/view-rect overlay layer
        RenderLayers::from_layers(&[0, MINIMAP_LAYER]),
    ));
}

pub fn despawn_minimap(
    mut commands: Commands,
    q: Query<Entity, Or<(With<MinimapCam>, With<Blip>)>>,
    mut map: ResMut<BlipMap>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
    map.0.clear();
}

// ── blip layer ───────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct Blip;

/// GameId → blip entity (the render map for the marker layer).
#[derive(Resource, Default)]
pub struct BlipMap(pub HashMap<u64, Entity>);

/// Shared quad + per-color unlit materials for the blip layer.
#[derive(Resource)]
pub struct BlipAssets {
    quad: Handle<Mesh>,
    mats: HashMap<u32, Handle<StandardMaterial>>,
}

pub fn init_blip_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut config: ResMut<GizmoConfigStore>,
) {
    commands.insert_resource(BlipAssets {
        quad: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
        mats: HashMap::default(),
    });
    // gizmos draw ONLY into the minimap (the view rectangle)
    let (cfg, _) = config.config_mut::<DefaultGizmoConfigGroup>();
    cfg.render_layers = RenderLayers::layer(MINIMAP_LAYER);
    cfg.line.width = 1.5;
}

fn blip_mat(
    assets: &mut BlipAssets,
    mats: &mut Assets<StandardMaterial>,
    color: u32,
) -> Handle<StandardMaterial> {
    assets
        .mats
        .entry(color)
        .or_insert_with(|| {
            mats.add(StandardMaterial {
                base_color: Color::srgb_u8(
                    ((color >> 16) & 0xff) as u8,
                    ((color >> 8) & 0xff) as u8,
                    (color & 0xff) as u8,
                ),
                unlit: true,
                ..default()
            })
        })
        .clone()
}

/// What a resource node looks like on the map: AoE-style dots. Wood reads as
/// the forest mass, so its dots stay small and dark.
fn node_blip(res: ResourceType) -> (u32, f32) {
    match res {
        ResourceType::Wood => (0x2e5a2a, 1.4),
        ResourceType::Stone => (0xb8b8b8, 2.6),
        ResourceType::Gold => (0xf2c542, 2.6),
        ResourceType::Food => (0xc4533a, 2.2),
    }
}

/// Upsert a marker per sim row. Units/buildings get their team color (white
/// for unowned), nodes their resource color; markers ride the SIM position, so
/// the minimap is exact even when the 3D layer lerps.
pub fn sync_blips(
    mut commands: Commands,
    mut assets: ResMut<BlipAssets>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut map: ResMut<BlipMap>,
    q_sim: Query<(&GameId, &Pos, Option<&Unit>, Option<&Building>, Option<&ResourceNode>, Option<&Owner>)>,
    q_players: Query<&Player>,
    mut q_blips: Query<&mut Transform, With<Blip>>,
) {
    let owner_color: HashMap<u64, u32> = q_players
        .iter()
        .map(|p| (p.player_id, PLAYER_COLORS[p.color as usize % PLAYER_COLORS.len()]))
        .collect();

    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for (gid, pos, unit, bld, node, owner) in &q_sim {
        let x = pos.pos.x.to_num::<f32>();
        let z = pos.pos.y.to_num::<f32>();
        let team = owner.and_then(|o| owner_color.get(&o.0).copied());
        let (color, size, y, moves) = if let Some(u) = unit {
            if u.garrisoned_in != 0 {
                continue; // sheltered units vanish from the field
            }
            (team.unwrap_or(0xffffff), 3.4, BLIP_Y_UNIT, true)
        } else if let Some(b) = bld {
            (team.unwrap_or(0xdddddd), building_def(b.kind).footprint as f32 + 4.0, BLIP_Y_BUILDING, false)
        } else if let Some(n) = node {
            let (c, s) = node_blip(n.res_type);
            (c, s, BLIP_Y_NODE, false)
        } else {
            continue;
        };
        seen.insert(gid.0);
        let world = Vec3::new(x, y, z);
        match map.0.get(&gid.0) {
            Some(&e) => {
                if moves && let Ok(mut t) = q_blips.get_mut(e) {
                    t.translation = world;
                }
            }
            None => {
                let mat = blip_mat(&mut assets, &mut mats, color);
                let e = commands
                    .spawn((
                        Blip,
                        crate::MatchScoped,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(mat),
                        Transform::from_translation(world).with_scale(Vec3::new(size, 1.0, size)),
                        RenderLayers::layer(MINIMAP_LAYER),
                    ))
                    .id();
                map.0.insert(gid.0, e);
            }
        }
    }
    // dead rows lose their marker (garrisoned units too — they re-blip on exit)
    map.0.retain(|gid, e| {
        if seen.contains(gid) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
}

/// The main camera's ground footprint, drawn as a rectangle on the minimap.
pub fn draw_view_rect(mut gizmos: Gizmos, cam: Res<CameraState>, windows: Query<&Window>) {
    let Ok(window) = windows.single() else { return };
    let aspect = window.width() / window.height().max(1.0);
    // same rig math as CameraState::rig_offset: ring angle + zoom-driven height
    let t = ((cam.view_size - 10.0) / 75.0).clamp(0.0, 1.0);
    let height = 48.0 + t * 14.0;
    let pitch = height.atan2(59.4);
    let w = cam.view_size * 2.0 * aspect;
    let d = cam.view_size * 2.0 / pitch.sin();
    let a = std::f32::consts::FRAC_PI_4 + cam.yaw;
    let right = Vec3::new(a.sin(), 0.0, -a.cos());
    let fwd = Vec3::new(-a.cos(), 0.0, -a.sin());
    let rot = Quat::from_mat3(&Mat3::from_cols(right, fwd, Vec3::Y));
    let center = Vec3::new(cam.center.x, VIEW_RECT_Y, cam.center.z);
    gizmos.rect(Isometry3d::new(center, rot), Vec2::new(w, d), Color::WHITE);
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
    // preview-aligned: screen-right = +X, screen-down = +Z
    let wx = rel.x * WORLD_SIZE as f32;
    let wz = rel.y * WORLD_SIZE as f32;
    if let Ok(mut tf) = q_main.single_mut() {
        focus_on(&mut state, &mut tf, wx, wz, field.as_deref());
    }
}
