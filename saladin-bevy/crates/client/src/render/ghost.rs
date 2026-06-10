//! Build-placement ghost + demolish overlay (port of updateGhost /
//! updateDemolishGhost): the actual building model, tinted translucent green or
//! red by validity, following the cursor; wall drags show the whole line.

use crate::camera::{GameCamera, pick_ground};
use crate::input::{InputMode, WallDrag, build_cells, place_valid};
use crate::render::sync::{RenderAssets, RenderMaterials, wall_angle_at};
use crate::terrain::{HeightField, height_at};
use crate::LocalPlayer;
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Pos, ResourceNode, WorldConfig};
use saladin_sim::{BuildingKind, Occupant, building_def, occupancy_set};
use std::collections::HashSet;

/// One ghost cell (the root holds nothing; each cell is its own mesh entity).
#[derive(Component)]
pub struct GhostCell;

#[derive(Component)]
pub struct DemolishOverlay;

/// Rebuild the ghost cells each frame in Build mode. Cheap: a handful of
/// entities, despawned + respawned (matches the TS clearGhost/updateGhost).
#[allow(clippy::too_many_arguments)]
pub fn update_ghost(
    mut commands: Commands,
    mode: Res<InputMode>,
    wall_drag: Res<WallDrag>,
    windows: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    field: Option<Res<HeightField>>,
    cfg: Res<WorldConfig>,
    assets: Res<RenderAssets>,
    rmats: Res<RenderMaterials>,
    q_buildings: Query<(&Pos, &Building, &Owner)>,
    q_nodes: Query<&Pos, With<ResourceNode>>,
    local: Res<LocalPlayer>,
    q_cells: Query<Entity, With<GhostCell>>,
) {
    for e in &q_cells {
        commands.entity(e).despawn();
    }
    let InputMode::Build(kind) = *mode else { return };
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let field_ref = field.as_deref();
    let Some(g) = pick_ground(camera, cam_tf, cursor, field_ref) else { return };

    let occ_list: Vec<Occupant> =
        q_buildings.iter().map(|(p, b, _)| Occupant { kind: b.kind, pos: p.pos }).collect();
    let mut occ = occupancy_set(&occ_list, true);
    for p in &q_nodes {
        occ.insert(saladin_sim::tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()));
    }
    let own: Vec<saladin_sim::V2> = q_buildings
        .iter()
        .filter(|(_, _, o)| o.0 == local.0)
        .map(|(p, _, _)| p.pos)
        .collect();
    let occ_render: HashSet<i32> = occ.iter().copied().collect();

    let is_wall = kind == BuildingKind::Wall;
    let yaw = if is_wall { ghost_wall_angle(&occ_render, wall_drag.0, g.x, g.z) } else { 0.0 };
    for (cx, cy) in build_cells(kind, g.x, g.z, wall_drag.0) {
        let valid = place_valid(kind, cx, cy, cfg.seed, &occ, &own);
        let y = field_ref.map(|f| height_at(f, cx, cy)).unwrap_or(0.0);
        commands.spawn((
            GhostCell,
            Mesh3d(assets.buildings[kind as usize].clone()),
            MeshMaterial3d(if valid { rmats.ghost_ok.clone() } else { rmats.ghost_bad.clone() }),
            Transform::from_xyz(cx, y, cy).with_rotation(Quat::from_rotation_y(yaw)),
        ));
    }
}

/// Wall ghost orientation: along the drag axis, or its connection angle.
fn ghost_wall_angle(occ: &HashSet<i32>, drag: Option<(i32, i32)>, hx: f32, hz: f32) -> f32 {
    if let Some((sx, sy)) = drag {
        let dx = hx.floor() as i32 - sx;
        let dy = hz.floor() as i32 - sy;
        if dx == 0 && dy == 0 {
            return wall_angle_at(occ, hx, hz);
        }
        return if dx.abs() >= dy.abs() { 0.0 } else { -std::f32::consts::FRAC_PI_2 };
    }
    wall_angle_at(occ, hx, hz)
}

/// Red translucent box over the own building under the cursor in demolish mode.
#[allow(clippy::too_many_arguments)]
pub fn update_demolish_overlay(
    mut commands: Commands,
    mode: Res<InputMode>,
    windows: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    field: Option<Res<HeightField>>,
    local: Res<LocalPlayer>,
    rmats: Res<RenderMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    q_buildings: Query<(&GameId, &Owner, &Pos, &Building)>,
    q_overlay: Query<Entity, With<DemolishOverlay>>,
) {
    for e in &q_overlay {
        commands.entity(e).despawn();
    }
    if *mode != InputMode::Demolish {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let field_ref = field.as_deref();
    let Some(g) = pick_ground(camera, cam_tf, cursor, field_ref) else { return };

    for (_, o, p, b) in &q_buildings {
        if o.0 != local.0 {
            continue;
        }
        let def = building_def(b.kind);
        let half = def.footprint as f32 / 2.0;
        let bx = p.pos.x.to_num::<f32>();
        let bz = p.pos.y.to_num::<f32>();
        if (g.x - bx).abs() <= half && (g.z - bz).abs() <= half {
            let h = def.height.to_num::<f32>() + 0.4;
            let y = field_ref.map(|f| height_at(f, bx, bz)).unwrap_or(0.0);
            commands.spawn((
                DemolishOverlay,
                Mesh3d(meshes.add(Mesh::from(Cuboid::new(
                    def.footprint as f32 * 1.05,
                    h,
                    def.footprint as f32 * 1.05,
                )))),
                MeshMaterial3d(rmats.demolish.clone()),
                Transform::from_xyz(bx, y + h / 2.0, bz),
            ));
            return;
        }
    }
}
