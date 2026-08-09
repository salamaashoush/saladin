//! Build-placement ghost + demolish overlay (port of updateGhost /
//! updateDemolishGhost): the actual building model, tinted translucent green or
//! red by validity, following the cursor; wall drags show the whole line.

use crate::camera::{GameCamera, pick_ground};
use crate::input::{GhostRot, InputMode, PlaceHint, WallDrag, build_cells, build_probe};
use crate::render::sync::{RenderAssets, RenderMaterials};
use crate::terrain::{HeightField, height_at};
use crate::LocalPlayer;
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Player, Pos, ResourceNode, WorldConfig};
use saladin_sim::{BuildingKind, Stockpile, building_def, operational};

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
    q_units: Query<(&Owner, &Pos, &saladin_protocol::Unit)>,
    // grouped: bevy caps a system at 16 parameters
    (local, q_players): (Res<LocalPlayer>, Query<&Player>),
    q_cells: Query<Entity, With<GhostCell>>,
    ghost_rot: Res<GhostRot>,
    mut hint: ResMut<PlaceHint>,
) {
    for e in &q_cells {
        commands.entity(e).despawn();
    }
    if hint.0.is_some() {
        hint.0 = None;
    }
    let InputMode::Build(kind) = *mode else { return };
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let field_ref = field.as_deref();
    let Some(g) = pick_ground(camera, cam_tf, cursor, field_ref) else { return };

    let me = q_players.iter().find(|p| p.player_id == local.0);
    let stock: Stockpile = me.map(|p| p.stock).unwrap_or_default();
    // ONE gathering, shared with the command that ships: a preview can never
    // turn green on a placement `build` will refuse
    let mut probe = build_probe(
        kind,
        local.0,
        cfg.seed,
        stock,
        q_buildings.iter().map(|(p, b, o)| (p.pos, b.kind, o.0, operational(b.state))),
        q_nodes.iter().map(|p| p.pos),
        crate::input::builder_positions(
            local.0,
            q_units.iter().map(|(o, p, u)| (o.0, p.pos, u.kind, u.garrisoned_in)),
        ),
    );

    // wall pillars are rotationally symmetric; everything else uses R-rotation
    let yaw = if kind == BuildingKind::Wall {
        0.0
    } else {
        ghost_rot.0 as f32 * std::f32::consts::FRAC_PI_2
    };
    let faction = me.map(|p| p.faction).unwrap_or(saladin_sim::Faction::Ayyubid);
    let def = building_def(kind);
    for (cx, cy) in build_cells(kind, g.x, g.z, wall_drag.0) {
        let verdict = probe.check(kind, cx, cy);
        if let Err(e) = verdict
            && hint.0.is_none()
        {
            hint.0 = Some(e);
        }
        // a dragged wall is paid for tile by tile: the ghost has to show where
        // the money runs out, not colour the whole run by the first segment
        if verdict.is_ok() && kind == BuildingKind::Wall {
            probe.stock.pay(&def.cost);
        }
        let y = field_ref.map(|f| height_at(f, cx, cy)).unwrap_or(0.0);
        commands.spawn((
            GhostCell,
            Mesh3d(assets.buildings[kind as usize * 2 + faction as usize].clone()),
            MeshMaterial3d(if verdict.is_ok() { rmats.ghost_ok.clone() } else { rmats.ghost_bad.clone() }),
            Transform::from_xyz(cx, y, cy).with_rotation(Quat::from_rotation_y(yaw)),
        ));
    }
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
