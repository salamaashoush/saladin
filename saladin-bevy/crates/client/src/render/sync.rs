//! Sim → render reconciliation (port of SaladinGame's spawn*/onPos/loop body):
//! shared mesh+material handles per (kind, team) so Bevy auto-instances the
//! draw calls; interpolation + idle bob + facing; LOD impostor swap at far
//! zoom; selection rings; rout markers; wall yaw; node scaling; HP bars.

use crate::camera::CameraState;
use crate::selection::Selection;
use crate::terrain::{HeightField, height_at};
use bevy::prelude::*;
use std::collections::HashMap;
use saladin_protocol::{Building, GameId, Owner, Player, Pos, ResourceNode, Unit};
use saladin_sim::{
    BuildingKind, PLAYER_COLORS, ResourceType, UnitKind, WORLD_SIZE, building_def,
    footprint_tiles, unit_def,
};
use std::collections::HashSet;

/// Far-zoom threshold above which unit bodies switch to impostor meshes.
const IMPOSTOR_VIEW_SIZE: f32 = 34.0;
pub const BAR_W: f32 = 0.9;
pub const BAR_H: f32 = 0.12;

#[derive(Resource)]
pub struct RenderAssets {
    pub units: Vec<Handle<Mesh>>,
    pub impostors: Vec<Handle<Mesh>>,
    pub buildings: Vec<Handle<Mesh>>,
    pub nodes: HashMap<ResourceType, Handle<Mesh>>,
    pub ring: Handle<Mesh>,
    pub bar_quad: Handle<Mesh>,
    pub rout_quad: Handle<Mesh>,
    pub flag_pole: Handle<Mesh>,
    pub flag_cloth: Handle<Mesh>,
}

#[derive(Resource)]
pub struct RenderMaterials {
    team_unit: HashMap<(u32, bool), Handle<StandardMaterial>>, // (color, selected)
    team_tint: HashMap<u32, Handle<StandardMaterial>>,
    pub node: HashMap<ResourceType, Handle<StandardMaterial>>,
    pub ring: Handle<StandardMaterial>,
    pub ring_building: Handle<StandardMaterial>,
    pub bar_bg: Handle<StandardMaterial>,
    pub bar_green: Handle<StandardMaterial>,
    pub bar_yellow: Handle<StandardMaterial>,
    pub bar_red: Handle<StandardMaterial>,
    pub rout: Handle<StandardMaterial>,
    pub flag_pole: Handle<StandardMaterial>,
    pub flag_cloth: Handle<StandardMaterial>,
    pub ghost_ok: Handle<StandardMaterial>,
    pub ghost_bad: Handle<StandardMaterial>,
    pub demolish: Handle<StandardMaterial>,
    pub arrow: Handle<StandardMaterial>,
}

fn color_of(hex: u32) -> Color {
    Color::srgb_u8(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
}

fn overlay(mats: &mut Assets<StandardMaterial>, color: Color, alpha: f32) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: color.with_alpha(alpha),
        unlit: true,
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        cull_mode: None,
        double_sided: true,
        depth_bias: 4.0,
        ..default()
    })
}

/// Unlit alpha-blended quad material carrying a baked UI texture (selection
/// ring dashes, rally cloth).
fn textured_overlay(
    mats: &mut Assets<StandardMaterial>,
    tex: Handle<Image>,
    tint: Color,
) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: tint,
        base_color_texture: Some(tex),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        depth_bias: 4.0,
        ..default()
    })
}

pub fn build_materials(
    mats: &mut Assets<StandardMaterial>,
    ring_tex: Handle<Image>,
    flag_tex: Handle<Image>,
) -> RenderMaterials {
    let mut node = HashMap::new();
    for r in [ResourceType::Wood, ResourceType::Stone, ResourceType::Food, ResourceType::Gold] {
        node.insert(
            r,
            mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() }),
        );
    }
    RenderMaterials {
        team_unit: HashMap::new(),
        team_tint: HashMap::new(),
        node,
        ring: textured_overlay(mats, ring_tex.clone(), Color::WHITE),
        ring_building: textured_overlay(mats, ring_tex, Color::srgb(0.65, 1.0, 0.55)),
        bar_bg: overlay(mats, Color::srgb_u8(0x14, 0x14, 0x14), 1.0),
        bar_green: overlay(mats, Color::srgb_u8(0x33, 0xdd, 0x44), 1.0),
        bar_yellow: overlay(mats, Color::srgb_u8(0xdd, 0xcc, 0x33), 1.0),
        bar_red: overlay(mats, Color::srgb_u8(0xdd, 0x33, 0x33), 1.0),
        rout: overlay(mats, Color::srgb_u8(0xff, 0x55, 0x33), 1.0),
        flag_pole: overlay(mats, Color::srgb_u8(0x3a, 0x2a, 0x18), 1.0),
        flag_cloth: mats.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(flag_tex),
            unlit: true,
            cull_mode: None,
            double_sided: true,
            depth_bias: 4.0,
            ..default()
        }),
        ghost_ok: overlay(mats, Color::srgb_u8(0x44, 0xee, 0x55), 0.5),
        ghost_bad: overlay(mats, Color::srgb_u8(0xee, 0x44, 0x33), 0.5),
        demolish: overlay(mats, Color::srgb_u8(0xff, 0x40, 0x30), 0.4),
        arrow: overlay(mats, Color::srgb_u8(0x2e, 0x21, 0x14), 1.0),
    }
}

impl RenderMaterials {
    pub fn unit_mat(
        &mut self,
        mats: &mut Assets<StandardMaterial>,
        hex: u32,
        selected: bool,
    ) -> Handle<StandardMaterial> {
        self.team_unit
            .entry((hex, selected))
            .or_insert_with(|| {
                mats.add(StandardMaterial {
                    base_color: color_of(hex),
                    emissive: if selected { LinearRgba::rgb(0.45, 0.45, 0.12) } else { LinearRgba::BLACK },
                    perceptual_roughness: 0.85,
                    ..default()
                })
            })
            .clone()
    }

    /// Mostly-true vertex colors with a hint of team — buildings keep their
    /// baked stone/timber palette while still reading ownership.
    pub fn tint_mat(&mut self, mats: &mut Assets<StandardMaterial>, hex: u32) -> Handle<StandardMaterial> {
        self.team_tint
            .entry(hex)
            .or_insert_with(|| {
                let s = color_of(hex).to_srgba();
                let l = |b: f32| 0.86 * 0.74 + b * 0.26;
                mats.add(StandardMaterial {
                    base_color: Color::srgb(l(s.red), l(s.green), l(s.blue)),
                    perceptual_roughness: 0.9,
                    ..default()
                })
            })
            .clone()
    }
}

// ── per-entity render components ─────────────────────────────────────────────

/// Root of one sim entity's render tree (`GameId` value mirrored for cleanup).
#[derive(Component)]
pub struct RenderRoot(#[allow(dead_code)] pub u64);

/// Authoritative target the root eases toward; facing eased separately.
#[derive(Component)]
pub struct Lerp {
    pub target: Vec3,
    pub yaw: f32,
    pub bob_phase: f32,
    pub bob: bool,
}

#[derive(Component)]
pub struct UnitBody {
    pub kind: UnitKind,
    pub impostor: bool,
}

#[derive(Component)]
pub struct SelRing;

#[derive(Component)]
pub struct RoutFlag;

/// Floating HP bar pieces (billboarded each frame).
#[derive(Component)]
pub struct HpBar {
    pub of: u64,
    pub fill: bool,
}

/// Selected-building ring + rally flag markers (one of each at most).
#[derive(Component)]
pub struct BuildingSelRing;
#[derive(Component)]
pub struct RallyFlag;

#[derive(Resource, Default)]
pub struct RenderMap(pub HashMap<u64, Entity>);

/// Building occupancy for wall yaw (client-side mirror of stampOccupancy).
#[derive(Resource, Default)]
pub struct OccupiedTiles(pub HashSet<i32>);

/// Wall run orientation: 8-way neighbour double-angle average (wallAngleAt).
pub fn wall_angle_at(occ: &HashSet<i32>, x: f32, z: f32) -> f32 {
    let tx = x.floor() as i32;
    let ty = z.floor() as i32;
    let mut ax = 0.0_f32;
    let mut ay = 0.0_f32;
    let mut n = 0;
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
        if !occ.contains(&((ty + dy) * WORLD_SIZE + (tx + dx))) {
            continue;
        }
        let ang = (dy as f32).atan2(dx as f32);
        ax += (2.0 * ang).cos();
        ay += (2.0 * ang).sin();
        n += 1;
    }
    if n == 0 { 0.0 } else { -ay.atan2(ax) / 2.0 }
}

pub fn rebuild_occupancy(
    mut occ: ResMut<OccupiedTiles>,
    q: Query<(&Pos, &Building)>,
) {
    occ.0.clear();
    for (p, b) in &q {
        let f = building_def(b.kind).footprint;
        for t in footprint_tiles(f, p.pos.x, p.pos.y) {
            occ.0.insert(t.ty * WORLD_SIZE + t.tx);
        }
    }
}

fn node_scale(remaining: i32) -> f32 {
    0.5 + 0.5 * (remaining as f32 / 120.0).min(1.0)
}

/// Reconcile every sim row into a render tree. Shared handles per (mesh,
/// material) mean Bevy batches each kind×team into one instanced draw.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_render(
    mut commands: Commands,
    assets: Res<RenderAssets>,
    mut rmats: ResMut<RenderMaterials>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut map: ResMut<RenderMap>,
    occ: Res<OccupiedTiles>,
    field: Res<HeightField>,
    selection: Res<Selection>,
    cam_state: Res<CameraState>,
    q_sim: Query<(&GameId, &Pos, Option<&Unit>, Option<&Building>, Option<&ResourceNode>, Option<&Owner>)>,
    q_players: Query<&Player>,
    mut q_roots: Query<(&mut Lerp, &mut Visibility, &mut Transform), With<RenderRoot>>,
    mut q_bodies: Query<(&ChildOf, &mut UnitBody, &mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>)>,
    mut q_rings: Query<(&ChildOf, &mut Visibility), (With<SelRing>, Without<RenderRoot>)>,
    mut q_routs: Query<(&ChildOf, &mut Visibility), (With<RoutFlag>, Without<RenderRoot>, Without<SelRing>)>,
) {
    let owner_color: HashMap<u64, u32> = q_players
        .iter()
        .map(|p| (p.player_id, PLAYER_COLORS[p.color as usize % PLAYER_COLORS.len()]))
        .collect();
    let impostor = cam_state.view_size >= IMPOSTOR_VIEW_SIZE;

    let mut seen: HashSet<u64> = HashSet::new();
    // per-root info gathered for the child passes
    let mut unit_state: HashMap<Entity, (u32, bool, bool, UnitKind)> = HashMap::new(); // (color, selected, routing, kind)

    for (gid, pos, unit, bld, node, owner) in &q_sim {
        let x = pos.pos.x.to_num::<f32>();
        let z = pos.pos.y.to_num::<f32>();
        let ground = height_at(&field, x, z);
        let team = owner.and_then(|o| owner_color.get(&o.0).copied());

        if let Some(u) = unit {
            seen.insert(gid.0);
            let selected = selection.units.contains(&gid.0);
            let color = team.unwrap_or(0xdddddd);
            let yaw = if u.has_target {
                let dx = u.target.x.to_num::<f32>() - x;
                let dz = u.target.y.to_num::<f32>() - z;
                if dx.abs() + dz.abs() > 1e-4 { dx.atan2(dz) } else { f32::NAN }
            } else {
                f32::NAN
            };
            let world = Vec3::new(x, ground, z);
            let root = *map.0.entry(gid.0).or_insert_with(|| {
                spawn_unit_tree(&mut commands, &assets, &mut rmats, &mut mats, gid.0, u.kind, color, world)
            });
            if let Ok((mut lerp, mut vis, _)) = q_roots.get_mut(root) {
                lerp.target = world;
                if !yaw.is_nan() {
                    lerp.yaw = yaw;
                }
                *vis = if u.garrisoned_in != 0 { Visibility::Hidden } else { Visibility::Inherited };
            }
            unit_state.insert(root, (color, selected, u.routing, u.kind));
        } else if let Some(b) = bld {
            seen.insert(gid.0);
            let world = Vec3::new(x, ground, z);
            let root = *map.0.entry(gid.0).or_insert_with(|| {
                let mat = rmats.tint_mat(&mut mats, team.unwrap_or(0x9c958a));
                commands
                    .spawn((
                        RenderRoot(gid.0),
                        Mesh3d(assets.buildings[b.kind as usize].clone()),
                        MeshMaterial3d(mat),
                        Transform::from_translation(world),
                        Lerp { target: world, yaw: 0.0, bob_phase: 0.0, bob: false },
                    ))
                    .id()
            });
            if let Ok((mut lerp, _, mut tf)) = q_roots.get_mut(root) {
                lerp.target = world;
                tf.translation = world; // buildings snap
                if b.kind == BuildingKind::Wall {
                    let yaw = wall_angle_at(&occ.0, x, z);
                    lerp.yaw = yaw;
                    tf.rotation = Quat::from_rotation_y(yaw);
                } else {
                    // player-chosen quarter-turn facing (rides the Build command)
                    let yaw = pos.facing.to_num::<f32>();
                    if yaw != 0.0 {
                        lerp.yaw = yaw;
                        tf.rotation = Quat::from_rotation_y(yaw);
                    }
                }
            }
        } else if let Some(n) = node {
            seen.insert(gid.0);
            let world = Vec3::new(x, ground, z);
            let root = *map.0.entry(gid.0).or_insert_with(|| {
                commands
                    .spawn((
                        RenderRoot(gid.0),
                        Mesh3d(assets.nodes[&n.res_type].clone()),
                        MeshMaterial3d(rmats.node[&n.res_type].clone()),
                        Transform::from_translation(world),
                        Lerp { target: world, yaw: 0.0, bob_phase: 0.0, bob: false },
                    ))
                    .id()
            });
            if let Ok((_, _, mut tf)) = q_roots.get_mut(root) {
                tf.translation = world;
                tf.scale = Vec3::splat(node_scale(n.remaining));
            }
        }
    }

    // child passes: body material/LOD, ring + rout visibility
    for (child_of, mut body, mut mesh, mut mat) in &mut q_bodies {
        let Some(&(color, selected, _routing, _)) = unit_state.get(&child_of.parent()) else { continue };
        let kind = body.kind;
        let want = rmats.unit_mat(&mut mats, color, selected);
        if mat.0 != want {
            mat.0 = want;
        }
        if body.impostor != impostor {
            body.impostor = impostor;
            mesh.0 = if impostor { assets.impostors[kind as usize].clone() } else { assets.units[kind as usize].clone() };
        }
    }
    for (child_of, mut vis) in &mut q_rings {
        let on = unit_state.get(&child_of.parent()).map(|s| s.1).unwrap_or(false);
        *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (child_of, mut vis) in &mut q_routs {
        let on = unit_state.get(&child_of.parent()).map(|s| s.2).unwrap_or(false);
        *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
    }

    // cleanup
    let gone: Vec<u64> = map.0.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some(e) = map.0.remove(&id) {
            commands.entity(e).despawn();
        }
    }
}

fn spawn_unit_tree(
    commands: &mut Commands,
    assets: &RenderAssets,
    rmats: &mut RenderMaterials,
    mats: &mut Assets<StandardMaterial>,
    id: u64,
    kind: UnitKind,
    color: u32,
    world: Vec3,
) -> Entity {
    let def = unit_def(kind);
    let h = def.height.to_num::<f32>();
    let r = def.radius.to_num::<f32>();
    let mat = rmats.unit_mat(mats, color, false);
    commands
        .spawn((
            RenderRoot(id),
            Transform::from_translation(world),
            Visibility::Inherited,
            Lerp {
                target: world,
                yaw: 0.0,
                bob_phase: (id % 1000) as f32 / 1000.0 * std::f32::consts::TAU,
                bob: true,
            },
        ))
        .with_children(|p| {
            p.spawn((
                UnitBody { kind, impostor: false },
                Mesh3d(assets.units[kind as usize].clone()),
                MeshMaterial3d(mat),
            ));
            p.spawn((
                SelRing,
                Mesh3d(assets.ring.clone()),
                MeshMaterial3d(rmats.ring.clone()),
                Transform::from_xyz(0.0, 0.05, 0.0).with_scale(Vec3::splat(r.max(0.2) * 3.2)),
                Visibility::Hidden,
            ));
            p.spawn((
                RoutFlag,
                Mesh3d(assets.rout_quad.clone()),
                MeshMaterial3d(rmats.rout.clone()),
                Transform::from_xyz(0.0, h + r * 2.4 + 0.72, 0.0),
                Visibility::Hidden,
            ));
        })
        .id()
}

/// Ease roots toward their sim targets, apply yaw + idle bob (TS loop body).
pub fn interpolate(time: Res<Time>, mut q: Query<(&mut Transform, &Lerp), With<RenderRoot>>) {
    let k = (14.0 * time.delta_secs()).min(1.0);
    let bob_t = time.elapsed_secs() * 5.0;
    for (mut tf, l) in &mut q {
        let mut target = l.target;
        if l.bob {
            target.y += (bob_t + l.bob_phase).sin().abs() * 0.07;
        }
        tf.translation = tf.translation.lerp(target, k);
        if l.bob {
            let want = Quat::from_rotation_y(l.yaw);
            tf.rotation = tf.rotation.slerp(want, k);
        }
    }
}

/// Float damaged units'/buildings' HP bars above them, camera-billboarded.
/// Bars exist only while damaged — full-HP entities cost nothing.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_hp_bars(
    mut commands: Commands,
    assets: Res<RenderAssets>,
    rmats: Res<RenderMaterials>,
    field: Res<HeightField>,
    cam: Query<&Transform, (With<crate::camera::GameCamera>, Without<HpBar>)>,
    q_units: Query<(&GameId, &Pos, &Unit)>,
    q_buildings: Query<(&GameId, &Pos, &Building)>,
    q_players: Query<&Player>,
    mut q_bars: Query<(Entity, &HpBar, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Ok(cam_tf) = cam.single() else { return };
    let bill = cam_tf.rotation;
    let mask: HashMap<u64, u64> = HashMap::new();
    let _ = (&q_players, &mask);

    // desired bars: id → (world pos above head, ratio)
    let mut want: HashMap<u64, (Vec3, f32)> = HashMap::new();
    for (g, p, u) in &q_units {
        if u.garrisoned_in != 0 {
            continue;
        }
        let def = unit_def(u.kind);
        if def.max_hp <= 0 {
            continue;
        }
        let ratio = u.hp as f32 / def.max_hp as f32;
        if ratio >= 0.999 {
            continue;
        }
        let x = p.pos.x.to_num::<f32>();
        let z = p.pos.y.to_num::<f32>();
        let y = height_at(&field, x, z)
            + def.height.to_num::<f32>()
            + def.radius.to_num::<f32>() * 2.4
            + 0.35;
        want.insert(g.0, (Vec3::new(x, y, z), ratio.clamp(0.0, 1.0)));
    }
    for (g, p, b) in &q_buildings {
        let def = building_def(b.kind);
        if def.max_hp <= 0 {
            continue;
        }
        let ratio = b.hp as f32 / def.max_hp as f32;
        if ratio >= 0.999 {
            continue;
        }
        let x = p.pos.x.to_num::<f32>();
        let z = p.pos.y.to_num::<f32>();
        let y = height_at(&field, x, z) + def.height.to_num::<f32>() + 0.6;
        want.insert(g.0, (Vec3::new(x, y, z), ratio.clamp(0.0, 1.0)));
    }

    let mut have: HashSet<u64> = HashSet::new();
    for (e, bar, mut tf, mut mat) in &mut q_bars {
        match want.get(&bar.of) {
            Some(&(pos, ratio)) => {
                have.insert(bar.of);
                tf.rotation = bill;
                if bar.fill {
                    tf.translation = pos + bill * Vec3::new(-(BAR_W * (1.0 - ratio)) / 2.0, 0.0, 0.001);
                    tf.scale = Vec3::new(ratio.max(0.001), 1.0, 1.0);
                    let want_mat = if ratio > 0.5 {
                        rmats.bar_green.clone()
                    } else if ratio > 0.25 {
                        rmats.bar_yellow.clone()
                    } else {
                        rmats.bar_red.clone()
                    };
                    if mat.0 != want_mat {
                        mat.0 = want_mat;
                    }
                } else {
                    tf.translation = pos;
                }
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (&id, &(pos, _)) in &want {
        if have.contains(&id) {
            continue;
        }
        commands.spawn((
            HpBar { of: id, fill: false },
            Mesh3d(assets.bar_quad.clone()),
            MeshMaterial3d(rmats.bar_bg.clone()),
            Transform::from_translation(pos),
        ));
        commands.spawn((
            HpBar { of: id, fill: true },
            Mesh3d(assets.bar_quad.clone()),
            MeshMaterial3d(rmats.bar_green.clone()),
            Transform::from_translation(pos),
        ));
    }
}

/// Ring + rally flag on the selected building (updateBuildingHighlight port).
#[allow(clippy::too_many_arguments)]
pub fn update_building_highlight(
    mut commands: Commands,
    assets: Res<RenderAssets>,
    rmats: Res<RenderMaterials>,
    field: Res<HeightField>,
    selection: Res<Selection>,
    q_buildings: Query<(&GameId, &Pos, &Building)>,
    mut q_ring: Query<(Entity, &mut Transform), (With<BuildingSelRing>, Without<RallyFlag>)>,
    mut q_flag: Query<(Entity, &mut Transform), (With<RallyFlag>, Without<BuildingSelRing>)>,
) {
    let sel = selection
        .building
        .and_then(|id| q_buildings.iter().find(|(g, ..)| g.0 == id));

    match sel {
        Some((_, p, b)) => {
            let def = building_def(b.kind);
            let x = p.pos.x.to_num::<f32>();
            let z = p.pos.y.to_num::<f32>();
            let pos = Vec3::new(x, height_at(&field, x, z) + 0.06, z);
            let scale = Vec3::splat(def.footprint as f32 * 1.5);
            match q_ring.single_mut() {
                Ok((_, mut tf)) => {
                    tf.translation = pos;
                    tf.scale = scale;
                }
                Err(_) => {
                    commands.spawn((
                        BuildingSelRing,
                        Mesh3d(assets.ring.clone()),
                        MeshMaterial3d(rmats.ring_building.clone()),
                        Transform::from_translation(pos).with_scale(scale),
                    ));
                }
            }
            // rally flag when moved off the building
            let rx = b.rally.x.to_num::<f32>();
            let rz = b.rally.y.to_num::<f32>();
            let show_flag = ((rx - x).powi(2) + (rz - z).powi(2)).sqrt() > 1.0;
            if show_flag {
                let fpos = Vec3::new(rx, height_at(&field, rx, rz), rz);
                match q_flag.single_mut() {
                    Ok((_, mut tf)) => tf.translation = fpos,
                    Err(_) => {
                        commands
                            .spawn((RallyFlag, Transform::from_translation(fpos), Visibility::Inherited))
                            .with_children(|p| {
                                p.spawn((
                                    Mesh3d(assets.flag_pole.clone()),
                                    MeshMaterial3d(rmats.flag_pole.clone()),
                                    Transform::from_xyz(0.0, 0.5, 0.0),
                                ));
                                p.spawn((
                                    Mesh3d(assets.flag_cloth.clone()),
                                    MeshMaterial3d(rmats.flag_cloth.clone()),
                                    Transform::from_xyz(0.27, 0.85, 0.0),
                                ));
                            });
                    }
                }
            } else if let Ok((e, _)) = q_flag.single_mut() {
                commands.entity(e).despawn();
            }
        }
        None => {
            if let Ok((e, _)) = q_ring.single_mut() {
                commands.entity(e).despawn();
            }
            if let Ok((e, _)) = q_flag.single_mut() {
                commands.entity(e).despawn();
            }
        }
    }
}

/// Build the shared mesh handles at match start.
pub fn build_assets(meshes: &mut Assets<Mesh>) -> RenderAssets {
    use crate::render::models::props::resource_node_mesh;
    let mut nodes = HashMap::new();
    for r in [ResourceType::Wood, ResourceType::Stone, ResourceType::Food, ResourceType::Gold] {
        nodes.insert(r, meshes.add(resource_node_mesh(r)));
    }
    RenderAssets {
        units: UnitKind::ALL.iter().map(|k| meshes.add(crate::render::models::unit_mesh(*k))).collect(),
        impostors: UnitKind::ALL
            .iter()
            .map(|k| meshes.add(crate::render::models::unit_impostor_mesh(*k)))
            .collect(),
        buildings: BuildingKind::ALL
            .iter()
            .map(|k| meshes.add(crate::render::models::building_mesh(*k)))
            .collect(),
        nodes,
        // flat ground quad; the dashed-ring texture does the shaping
        ring: meshes.add(Plane3d::default().mesh().size(1.0, 1.0).build()),
        bar_quad: meshes.add(Mesh::from(Rectangle::new(BAR_W, BAR_H))),
        rout_quad: meshes.add(Mesh::from(Rectangle::new(0.34, 0.34))),
        flag_pole: meshes.add(Mesh::from(Cylinder::new(0.04, 1.0))),
        flag_cloth: meshes.add(Mesh::from(Rectangle::new(0.5, 0.3))),
    }
}
