//! Sim → render reconciliation (port of SaladinGame's spawn*/onPos/loop body):
//! shared mesh+material handles per (kind, team) so Bevy auto-instances the
//! draw calls; interpolation + idle bob + facing; LOD impostor swap at far
//! zoom; selection rings; rout markers; wall yaw; node scaling; HP bars.

use crate::camera::CameraState;
use crate::selection::Selection;
use crate::terrain::{HeightField, height_at};
use bevy::mesh::{MeshBuilder, Meshable};
use bevy::prelude::*;
use std::collections::HashMap;
use saladin_protocol::{Building, GameId, Owner, Player, Pos, ResourceNode, Unit, WorldConfig};
use saladin_sim::{
    BuildingKind, PLAYER_COLORS, ResourceType, UnitKind, WORLD_SIZE, building_def,
    footprint_tiles, unit_def,
};
use std::collections::HashSet;

/// Far-zoom threshold above which unit bodies switch to impostor meshes.
const IMPOSTOR_VIEW_SIZE: f32 = 34.0;
pub const BAR_W: f32 = 0.9;
pub const BAR_H: f32 = 0.12;

/// One animatable rig part as stored handles: child entity sits at `pivot`,
/// mesh vertices are pre-translated relative to it.
#[derive(Clone)]
pub struct RigHandle {
    pub group: crate::render::models::RigGroup,
    pub pivot: Vec3,
    pub mesh: Handle<Mesh>,
}

#[derive(Resource)]
pub struct RenderAssets {
    /// Base unit rigs (team parts white); per-team copies bake lazily into
    /// `team_rigs`/`team_impostors` so detail colors render true.
    pub unit_rigs: Vec<Vec<RigHandle>>,
    pub impostors: Vec<Handle<Mesh>>,
    pub team_rigs: HashMap<(usize, u32), Vec<RigHandle>>,
    pub team_impostors: HashMap<(usize, u32), Handle<Mesh>>,
    pub buildings: Vec<Handle<Mesh>>,
    pub nodes: HashMap<ResourceType, Vec<Handle<Mesh>>>,
    pub fish_node: Handle<Mesh>,
    pub carry_sack: Handle<Mesh>,
    pub puff: Handle<Mesh>,
    pub flame: Handle<Mesh>,
    pub ripple: Handle<Mesh>,
    pub scorch: Handle<Mesh>,
    pub rubble_chunk: Handle<Mesh>,
    pub rubble_pile: Handle<Mesh>,
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
    pub foam: Handle<StandardMaterial>,
    pub smoke_light: Handle<StandardMaterial>,
    pub smoke_dark: Handle<StandardMaterial>,
    pub flame: Handle<StandardMaterial>,
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
        foam: overlay(mats, Color::srgb_u8(0xe8, 0xf6, 0xf8), 0.4),
        smoke_light: overlay(mats, Color::srgb_u8(0xb8, 0xb4, 0xac), 0.5),
        smoke_dark: overlay(mats, Color::srgb_u8(0x45, 0x41, 0x3c), 0.55),
        flame: overlay(mats, Color::srgb_u8(0xff, 0x9a, 0x2e), 0.85),
    }
}

impl RenderMaterials {
    /// White-based unit material — team color is baked into the mesh's vertex
    /// colors (`bake_team`), so the material only carries the selection glow.
    pub fn unit_mat(
        &mut self,
        mats: &mut Assets<StandardMaterial>,
        _hex: u32,
        selected: bool,
    ) -> Handle<StandardMaterial> {
        self.team_unit
            .entry((0, selected))
            .or_insert_with(|| {
                mats.add(StandardMaterial {
                    base_color: Color::WHITE,
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
    /// Smoothly turn toward `yaw` (units + animals; static props never).
    pub turn: bool,
    /// Step-bounce on Y — ONLY while actually moving; a standing army that
    /// bobs in place reads as pulsing.
    pub hop: bool,
}

#[derive(Component)]
pub struct UnitBody {
    pub group: crate::render::models::RigGroup,
    pub pivot: Vec3,
    /// true = the far-zoom merged impostor child (hidden at gameplay zoom).
    pub impostor_part: bool,
    /// Peasant hauling bundle — visibility owned by the animator (shown only
    /// while `AnimState.carrying`).
    pub sack: bool,
}

/// Per-unit animation inputs mirrored from the sim each sync — the animator
/// is pure render math driven by these flags + wall time.
#[derive(Component)]
pub struct AnimState {
    pub kind: UnitKind,
    pub moving: bool,
    pub combat: bool,
    pub harvest: bool,
    pub carrying: bool,
    pub phase: f32,
    /// sim walk speed — leg swing cadence scales with it so cavalry gallops
    /// faster than a trundling ram
    pub stride: f32,
}

/// Fish-school food node: the school slowly circles its ripple rings and
/// bobs with the water.
#[derive(Component)]
pub struct FishNode {
    pub base_y: f32,
    pub phase: f32,
}

/// A live game animal (deer/boar food node): wanders around its sim anchor
/// (render-only — gatherers still walk to the anchor), grazes at waypoints,
/// and flops into a carcass the moment the first harvest tick lands.
#[derive(Component)]
pub struct AnimalNode {
    pub anchor: Vec3,
    pub remaining: i32,
    pub full: i32,
    pub carcass: bool,
    pub stand_mesh: Handle<Mesh>,
    pub graze_mesh: Handle<Mesh>,
    pub carcass_mesh: Handle<Mesh>,
    pub waypoint: Vec3,
    pub pause: f32,
    pub rng: u32,
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

impl RenderAssets {
    /// Lazily bake the (kind, team color) rig — white team parts recolored,
    /// every other vertex color kept true. One mesh per (kind, color, group),
    /// so Bevy still instances each batch.
    pub fn team_rig(
        &mut self,
        meshes: &mut Assets<Mesh>,
        kind: UnitKind,
        color: u32,
    ) -> Vec<RigHandle> {
        use crate::render::models::bake_team;
        self.team_rigs
            .entry((kind as usize, color))
            .or_insert_with(|| {
                self.unit_rigs[kind as usize]
                    .iter()
                    .map(|p| RigHandle {
                        group: p.group,
                        pivot: p.pivot,
                        mesh: match meshes.get(&p.mesh).map(|m| bake_team(m, color)) {
                            Some(m) => meshes.add(m),
                            None => p.mesh.clone(),
                        },
                    })
                    .collect()
            })
            .clone()
    }

    pub fn team_impostor(
        &mut self,
        meshes: &mut Assets<Mesh>,
        kind: UnitKind,
        color: u32,
    ) -> Handle<Mesh> {
        use crate::render::models::bake_team;
        let base = &self.impostors[kind as usize];
        self.team_impostors
            .entry((kind as usize, color))
            .or_insert_with(|| match meshes.get(base).map(|m| bake_team(m, color)) {
                Some(m) => meshes.add(m),
                None => base.clone(),
            })
            .clone()
    }
}

/// Biome-aware node variant pick: palms at oases, conifers in forest, olives
/// on the dry steppe; boars root in the woods, deer graze the open grass.
fn node_variant(res: ResourceType, seed: u32, x: f32, z: f32, roll: usize, len: usize) -> usize {
    use crate::render::models::props::*;
    use saladin_sim::{Biome, Fx, sample_terrain};
    let biome = sample_terrain(seed, Fx::from_num(x), Fx::from_num(z)).biome;
    let idx = match res {
        ResourceType::Wood => match biome {
            Biome::Oasis => TREE_PALM,
            Biome::Forest => [TREE_CONIFER, TREE_BROADLEAF_TALL, TREE_CONIFER, TREE_BROADLEAF][roll % 4],
            Biome::Steppe | Biome::Desert | Biome::Dunes | Biome::Sand | Biome::Hills => TREE_OLIVE,
            _ => [TREE_BROADLEAF, TREE_BROADLEAF_TALL, TREE_BROADLEAF, TREE_CONIFER][roll % 4],
        },
        ResourceType::Food => match biome {
            Biome::Forest => [FOOD_BOAR, FOOD_BERRY, FOOD_BOAR, FOOD_DEER][roll % 4],
            Biome::Oasis => [FOOD_BERRY, FOOD_DEER_GRAZING][roll % 2],
            _ => [FOOD_DEER, FOOD_DEER_GRAZING, FOOD_BOAR, FOOD_BERRY][roll % 4],
        },
        _ => roll % len,
    };
    idx.min(len - 1)
}

/// Reconcile every sim row into a render tree. Shared handles per (mesh,
/// material) mean Bevy batches each kind×team into one instanced draw.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_render(
    mut commands: Commands,
    (mut assets, mut meshes): (ResMut<RenderAssets>, ResMut<Assets<Mesh>>),
    mut rmats: ResMut<RenderMaterials>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut map: ResMut<RenderMap>,
    occ: Res<OccupiedTiles>,
    field: Res<HeightField>,
    selection: Res<Selection>,
    cam_state: Res<CameraState>,
    world_cfg: Res<WorldConfig>,
    q_sim: Query<(&GameId, &Pos, Option<&Unit>, Option<&Building>, Option<&ResourceNode>, Option<&Owner>)>,
    q_players: Query<&Player>,
    mut q_roots: Query<
        (&mut Lerp, &mut Visibility, &mut Transform, Option<&mut AnimState>, Option<&mut DamageState>),
        With<RenderRoot>,
    >,
    mut q_bodies: Query<
        (&ChildOf, &UnitBody, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>),
        (Without<RenderRoot>, Without<SelRing>, Without<RoutFlag>),
    >,
    (mut q_rings, mut q_routs, mut q_animals): (
        Query<(&ChildOf, &mut Visibility), (With<SelRing>, Without<RenderRoot>)>,
        Query<(&ChildOf, &mut Visibility), (With<RoutFlag>, Without<RenderRoot>, Without<SelRing>)>,
        Query<&mut AnimalNode>,
    ),
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
                spawn_unit_tree(
                    &mut commands,
                    &mut assets,
                    &mut meshes,
                    &mut rmats,
                    &mut mats,
                    gid.0,
                    u.kind,
                    color,
                    world,
                )
            });
            if let Ok((mut lerp, mut vis, _, anim, _)) = q_roots.get_mut(root) {
                lerp.target = world;
                lerp.hop = u.has_target;
                if !yaw.is_nan() {
                    lerp.yaw = yaw;
                }
                *vis = if u.garrisoned_in != 0 { Visibility::Hidden } else { Visibility::Inherited };
                if let Some(mut anim) = anim {
                    anim.moving = u.has_target;
                    anim.combat = u.attack_target != 0;
                    anim.harvest = u.gather_state == saladin_sim::GatherState::Harvesting;
                    anim.carrying = u.carrying > 0;
                }
            }
            unit_state.insert(root, (color, selected, u.routing, u.kind));
        } else if let Some(b) = bld {
            seen.insert(gid.0);
            let world = Vec3::new(x, ground, z);
            let root = *map.0.entry(gid.0).or_insert_with(|| {
                let mat = rmats.tint_mat(&mut mats, team.unwrap_or(0x9c958a));
                let def = building_def(b.kind);
                commands
                    .spawn((
                        RenderRoot(gid.0),
                        Mesh3d(assets.buildings[b.kind as usize].clone()),
                        MeshMaterial3d(mat),
                        Transform::from_translation(world),
                        Lerp { target: world, yaw: 0.0, bob_phase: 0.0, turn: false, hop: false },
                        DamageState {
                            ratio: 1.0,
                            span: def.footprint as f32 * 0.55,
                            roof: def.height.to_num::<f32>(),
                            acc: [0.0; 2],
                            applied: 0,
                        },
                    ))
                    .id()
            });
            if let Ok((mut lerp, _, mut tf, _, dmg)) = q_roots.get_mut(root) {
                lerp.target = world;
                tf.translation = world; // buildings snap
                if let Some(mut dmg) = dmg {
                    let max = building_def(b.kind).max_hp.max(1);
                    dmg.ratio = b.hp as f32 / max as f32;
                }
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
                // Coastal food sits on water tiles — draw a fish school there,
                // never a deer standing on the sea.
                use crate::render::models::props::*;
                let variants = &assets.nodes[&n.res_type];
                let roll = (gid.0 ^ (gid.0 >> 17)) as usize;
                let idx = node_variant(n.res_type, world_cfg.seed, x, z, roll, variants.len());
                let fishy = n.res_type == ResourceType::Food && ground < -0.005;
                let mesh = if fishy { assets.fish_node.clone() } else { variants[idx].clone() };
                // Deterministic per-node yaw so herds/groves don't all face north.
                let yaw = ((gid.0 >> 5) % 628) as f32 * 0.01;
                let mut e = commands.spawn((
                    RenderRoot(gid.0),
                    Mesh3d(mesh),
                    MeshMaterial3d(rmats.node[&n.res_type].clone()),
                    Transform::from_translation(world).with_rotation(Quat::from_rotation_y(yaw)),
                    Lerp { target: world, yaw, bob_phase: 0.0, turn: false, hop: false },
                ));
                if fishy {
                    e.insert(FishNode { base_y: ground, phase: (gid.0 % 628) as f32 * 0.01 });
                }
                // Land game animals get a wander/graze/carcass brain.
                if n.res_type == ResourceType::Food
                    && ground >= -0.005
                    && matches!(idx, FOOD_DEER | FOOD_BOAR | FOOD_DEER_GRAZING)
                {
                    let deerish = idx != FOOD_BOAR;
                    e.insert(AnimalNode {
                        anchor: world,
                        remaining: n.remaining,
                        full: n.remaining,
                        carcass: false,
                        stand_mesh: variants[if deerish { FOOD_DEER } else { FOOD_BOAR }].clone(),
                        graze_mesh: variants[if deerish { FOOD_DEER_GRAZING } else { FOOD_BOAR }].clone(),
                        carcass_mesh: variants[if deerish { FOOD_DEER_CARCASS } else { FOOD_BOAR_CARCASS }]
                            .clone(),
                        waypoint: world,
                        pause: (gid.0 % 50) as f32 * 0.1,
                        rng: (gid.0 as u32) | 1,
                    });
                }
                e.id()
            });
            if let Ok((_, _, mut tf, _, _)) = q_roots.get_mut(root) {
                tf.scale = Vec3::splat(node_scale(n.remaining));
            }
            if let Ok(mut animal) = q_animals.get_mut(root) {
                animal.remaining = n.remaining;
            } else if let Ok((_, _, mut tf, _, _)) = q_roots.get_mut(root) {
                // static nodes snap to the sim position; animals own their pose
                tf.translation = world;
            }
        }
    }

    // child passes: body material + impostor LOD visibility, ring + rout
    for (child_of, body, mut vis, mut mat) in &mut q_bodies {
        let Some(&(color, selected, _routing, _)) = unit_state.get(&child_of.parent()) else { continue };
        let want = rmats.unit_mat(&mut mats, color, selected);
        if mat.0 != want {
            mat.0 = want;
        }
        if body.sack {
            // near-zoom visibility owned by the animator (carrying flag)
            if impostor && *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        let show = body.impostor_part == impostor;
        let want_vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want_vis {
            *vis = want_vis;
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

    // cleanup: dead rows play a render-only death (units tip over, buildings
    // and nodes sink) instead of popping out of existence
    let gone: Vec<u64> = map.0.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some(e) = map.0.remove(&id) {
            let (fall, rubble) = q_roots
                .get(e)
                .map(|(_, _, _, anim, dmg)| {
                    (anim.is_some(), dmg.map(|d| (d.span * 1.4).max(0.9)).unwrap_or(0.0))
                })
                .unwrap_or((false, 0.0));
            commands
                .entity(e)
                .remove::<(Lerp, AnimState, AnimalNode)>()
                .insert(Dying { t: 0.0, fall, rubble });
        }
    }
}

/// Render-only death throes: tip forward (units), then sink under the
/// terrain and despawn. Sim rows are already gone; this is pure cosmetics.
#[derive(Component)]
pub struct Dying {
    pub t: f32,
    pub fall: bool,
    /// >0 = a destroyed building: swap to the rubble pile at this scale and
    /// linger before sinking.
    pub rubble: f32,
}

/// Building health mirrored for staged damage FX: light smoke under 75%,
/// dark smoke under 50%, flames join under 25%.
#[derive(Component)]
pub struct DamageState {
    pub ratio: f32,
    pub span: f32,
    pub roof: f32,
    /// spawn-accumulator phases (smoke, flame)
    pub acc: [f32; 2],
    /// highest damage-dressing stage applied (1 = scorch, 2 = rubble+beams)
    pub applied: u8,
}

/// Short-lived cosmetic particle (smoke puff / flame tongue).
#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub age: f32,
    pub life: f32,
    pub base: f32,
}

/// Emit smoke/flame from damaged buildings at a rate scaled by missing HP.
pub fn building_damage_fx(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<RenderAssets>,
    rmats: Res<RenderMaterials>,
    mut q: Query<(Entity, &mut DamageState, &Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut d, tf) in &mut q {
        if d.ratio >= 0.75 {
            continue;
        }
        // Damage dressing: stamp scorch marks at 50%, strew rubble + snapped
        // beams at 25%. Children of the root, so they collapse with it.
        let want_stage = if d.ratio < 0.25 { 2 } else if d.ratio < 0.5 { 1 } else { 0 };
        if want_stage > d.applied {
            let from = d.applied;
            d.applied = want_stage;
            let span = d.span;
            let salt0 = tf.translation.x * 3.3 + tf.translation.z * 9.1;
            let h01 = |k: f32| ((k * 43758.5453).sin() * 0.5 + 0.5).abs();
            let mat = rmats.node[&ResourceType::Stone].clone();
            commands.entity(entity).with_children(|p| {
                if from < 1 && want_stage >= 1 {
                    for i in 0..3 {
                        let k = salt0 + i as f32 * 2.7;
                        let ang = h01(k) * std::f32::consts::TAU;
                        p.spawn((
                            Mesh3d(assets.scorch.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::from_xyz(ang.cos() * span * 0.8, 0.06 + h01(k + 1.0) * 0.2, ang.sin() * span * 0.8)
                                .with_rotation(Quat::from_rotation_y(ang))
                                .with_scale(Vec3::splat(0.8 + span * 0.4)),
                        ));
                    }
                }
                if want_stage >= 2 {
                    for i in 0..3 {
                        let k = salt0 + 31.7 + i as f32 * 3.9;
                        let ang = h01(k) * std::f32::consts::TAU;
                        p.spawn((
                            Mesh3d(assets.rubble_chunk.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::from_xyz(ang.cos() * span * 1.05, 0.02, ang.sin() * span * 1.05)
                                .with_rotation(Quat::from_rotation_y(h01(k + 5.0) * 6.28))
                                .with_scale(Vec3::splat(0.9 + span * 0.3)),
                        ));
                    }
                }
            });
        }
        let heavy = d.ratio < 0.25;
        let smoke_rate = if heavy { 3.0 } else if d.ratio < 0.5 { 1.6 } else { 0.7 };
        let flame_rate = if heavy { 1.6 } else if d.ratio < 0.5 { 0.5 } else { 0.0 };
        // deterministic-ish jitter from the spawn position
        let salt = tf.translation.x * 12.9898 + tf.translation.z * 78.233;
        let h01 = |k: f32| ((k * 43758.5453).sin() * 0.5 + 0.5).abs();
        for (slot, rate) in [(0usize, smoke_rate), (1usize, flame_rate)] {
            if rate <= 0.0 {
                continue;
            }
            d.acc[slot] += rate * dt;
            while d.acc[slot] >= 1.0 {
                d.acc[slot] -= 1.0;
                let k = salt + time.elapsed_secs() + slot as f32 * 17.7 + d.acc[slot];
                let off = Vec3::new(
                    (h01(k) - 0.5) * d.span,
                    d.roof * (0.8 + 0.4 * h01(k + 1.3)),
                    (h01(k + 2.6) - 0.5) * d.span,
                );
                if slot == 0 {
                    let mat = if d.ratio < 0.5 { rmats.smoke_dark.clone() } else { rmats.smoke_light.clone() };
                    commands.spawn((
                        Particle {
                            vel: Vec3::new((h01(k + 3.1) - 0.5) * 0.3, 1.4 + h01(k + 4.7) * 0.5, (h01(k + 5.9) - 0.5) * 0.3),
                            age: 0.0,
                            life: 2.0 + h01(k + 6.2) * 0.8,
                            base: 0.24 + h01(k + 7.9) * 0.18,
                        },
                        Mesh3d(assets.puff.clone()),
                        MeshMaterial3d(mat),
                        Transform::from_translation(tf.translation + off).with_scale(Vec3::splat(0.01)),
                    ));
                } else {
                    commands.spawn((
                        Particle {
                            vel: Vec3::new(0.0, 0.25, 0.0),
                            age: 0.0,
                            life: 0.5 + h01(k + 8.3) * 0.3,
                            base: 0.26 + h01(k + 9.1) * 0.18,
                        },
                        Mesh3d(assets.flame.clone()),
                        MeshMaterial3d(rmats.flame.clone()),
                        Transform::from_translation(tf.translation + off * Vec3::new(1.0, 0.5, 1.0))
                            .with_scale(Vec3::splat(0.01)),
                    ));
                }
            }
        }
    }
}

/// Rise, swell, shrink out, die.
pub fn tick_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut p, mut tf) in &mut q {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation += p.vel * dt;
        let k = (p.age / p.life) * std::f32::consts::PI;
        tf.scale = Vec3::splat((p.base * k.sin()).max(0.01));
    }
}

fn ease_out(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    1.0 - (1.0 - k) * (1.0 - k)
}

pub fn animate_dying(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<RenderAssets>,
    mut q: Query<(Entity, &mut Dying, &mut Transform, Option<&mut Mesh3d>)>,
) {
    let dt = time.delta_secs();
    for (e, mut d, mut tf, mesh) in &mut q {
        let prev = d.t;
        // destroyed buildings collapse into a rubble pile that lingers
        if d.rubble > 0.0 && prev == 0.0 {
            if let Some(mut mesh) = mesh {
                mesh.0 = assets.rubble_pile.clone();
                tf.scale = Vec3::splat(d.rubble);
                commands.entity(e).despawn_related::<Children>();
            }
        }
        d.t += dt;
        if d.fall {
            // incremental local pitch so the unit falls along its facing
            let pitch = |t: f32| -1.5 * ease_out(t / 0.45);
            tf.rotate_local_x(pitch(d.t) - pitch(prev));
        }
        let (sink_at, end) = if d.rubble > 0.0 { (4.0, 7.0) } else { (0.7, 2.0) };
        if d.t > sink_at {
            tf.translation.y -= 0.55 * dt;
        }
        if d.t > end {
            commands.entity(e).despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_unit_tree(
    commands: &mut Commands,
    assets: &mut RenderAssets,
    meshes: &mut Assets<Mesh>,
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
    let rig = assets.team_rig(meshes, kind, color);
    let impostor_mesh = assets.team_impostor(meshes, kind, color);
    let phase = (id % 1000) as f32 / 1000.0 * std::f32::consts::TAU;
    commands
        .spawn((
            RenderRoot(id),
            Transform::from_translation(world),
            Visibility::Inherited,
            Lerp { target: world, yaw: 0.0, bob_phase: phase, turn: true, hop: false },
            AnimState {
                kind,
                moving: false,
                combat: false,
                harvest: false,
                carrying: false,
                phase,
                stride: unit_def(kind).speed.to_num::<f32>(),
            },
        ))
        .with_children(|p| {
            for part in rig {
                p.spawn((
                    UnitBody { group: part.group, pivot: part.pivot, impostor_part: false, sack: false },
                    Mesh3d(part.mesh),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(part.pivot),
                ));
            }
            if kind == UnitKind::Peasant {
                let def = unit_def(kind);
                let h = def.height.to_num::<f32>();
                let r = def.radius.to_num::<f32>();
                p.spawn((
                    UnitBody {
                        group: crate::render::models::RigGroup::Body,
                        pivot: Vec3::ZERO,
                        impostor_part: false,
                        sack: true,
                    },
                    Mesh3d(assets.carry_sack.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(0.0, h * 0.72, -r * 0.85),
                    Visibility::Hidden,
                ));
            }
            p.spawn((
                UnitBody {
                    group: crate::render::models::RigGroup::Body,
                    pivot: Vec3::ZERO,
                    impostor_part: true,
                    sack: false,
                },
                Mesh3d(impostor_mesh),
                MeshMaterial3d(mat),
                Visibility::Hidden,
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

/// Procedural unit animation: walk leg-swing, melee/gather chop, ranged aim,
/// wheel spin, idle sway — all from `AnimState` flags + wall time, zero sim
/// involvement. Skipped entirely at impostor zoom.
pub fn animate_units(
    time: Res<Time>,
    cam_state: Res<CameraState>,
    q_roots: Query<(&AnimState, &Children)>,
    mut q_parts: Query<(&UnitBody, &mut Transform, &mut Visibility)>,
) {
    use crate::render::models::RigGroup as G;
    if cam_state.view_size >= IMPOSTOR_VIEW_SIZE {
        return;
    }
    let t = time.elapsed_secs();
    for (anim, children) in &q_roots {
        let tp = t + anim.phase;
        let mounted = matches!(anim.kind, UnitKind::Knight | UnitKind::HorseArcher | UnitKind::Mamluk);
        let ranged = matches!(
            anim.kind,
            UnitKind::Archer | UnitKind::Crossbowman | UnitKind::HorseArcher
        );
        let gait = 3.5 + anim.stride * 2.4;
        let walk = if anim.moving { (tp * gait).sin() } else { 0.0 };
        let swing_amp = if mounted { 0.38 } else { 0.55 };
        // chop / strike cycle: slow raise, sharp fall
        let strike = {
            let s = (tp * 4.0).sin();
            if s > 0.0 { s * s } else { 0.0 }
        };
        for child in children.iter() {
            let Ok((body, mut tf, mut vis)) = q_parts.get_mut(child) else { continue };
            if body.impostor_part {
                continue;
            }
            if body.sack {
                let want = if anim.carrying { Visibility::Inherited } else { Visibility::Hidden };
                if *vis != want {
                    *vis = want;
                }
                continue;
            }
            let rot = match body.group {
                G::Body => Quat::IDENTITY,
                G::LegL => Quat::from_rotation_x(walk * swing_amp),
                G::LegR => Quat::from_rotation_x(-walk * swing_amp),
                G::ArmR => match anim.kind {
                    UnitKind::Ram => Quat::IDENTITY, // handled via translation below
                    UnitKind::Mangonel => {
                        if anim.combat {
                            Quat::from_rotation_x(strike * 1.25)
                        } else {
                            Quat::IDENTITY
                        }
                    }
                    _ if anim.combat && !ranged => {
                        Quat::from_rotation_x(0.35 - strike * 1.15)
                    }
                    _ if anim.harvest => Quat::from_rotation_x(0.3 - strike * 0.9),
                    _ if anim.moving => Quat::from_rotation_x(-walk * 0.25),
                    _ => Quat::from_rotation_x((tp * 1.6).sin() * 0.06),
                },
                G::ArmL => {
                    if anim.combat && ranged {
                        // raise the bow/crossbow to aim
                        Quat::from_rotation_x(-0.45 - (tp * 3.0).sin().max(0.0) * 0.15)
                    } else if anim.moving {
                        Quat::from_rotation_x(walk * 0.25)
                    } else {
                        Quat::from_rotation_x((tp * 1.6 + 1.7).sin() * 0.06)
                    }
                }
                g if g.is_wheel() => {
                    if mounted {
                        // four horse legs in diagonal trot pairs at their own hips
                        if anim.moving {
                            let pair = if matches!(g, G::WheelFL | G::WheelBR) { 1.0 } else { -1.0 };
                            Quat::from_rotation_x((tp * (gait * 1.35)).sin() * 0.45 * pair)
                        } else {
                            Quat::IDENTITY
                        }
                    } else if anim.moving {
                        Quat::from_rotation_x(t * 5.0)
                    } else {
                        tf.rotation // freeze at current spoke angle
                    }
                }
                _ => Quat::IDENTITY,
            };
            tf.rotation = rot;
            // Ram: the slung beam jabs forward (+Z after the rig yaw) on attack.
            if anim.kind == UnitKind::Ram && body.group == G::ArmR {
                let jab = if anim.combat { strike * 0.45 } else { 0.0 };
                tf.translation = body.pivot + Vec3::new(0.0, 0.0, jab);
            }
        }
    }
}

/// Animal life: live game wanders between waypoints around its sim anchor,
/// grazing at each stop; the first harvest tick flops it into a carcass at
/// the anchor and it never moves again. Pure render — the sim only sees the
/// static node.
pub fn animate_animals(
    time: Res<Time>,
    field: Res<HeightField>,
    mut q: Query<(&mut AnimalNode, &mut Lerp, &mut Mesh3d, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut a, mut lerp, mut mesh, mut tf) in &mut q {
        if a.carcass {
            continue;
        }
        if a.remaining < a.full {
            a.carcass = true;
            lerp.hop = false;
            lerp.turn = false;
            lerp.target = a.anchor;
            tf.translation = a.anchor;
            mesh.0 = a.carcass_mesh.clone();
            continue;
        }
        let here = tf.translation;
        let d = a.waypoint - here;
        let dist = (d.x * d.x + d.z * d.z).sqrt();
        if dist > 0.08 {
            // amble toward the waypoint; interpolate() eases the transform,
            // so motion + turning stay smooth instead of stepping
            let step = (0.9 * dt).min(dist);
            let next = here + Vec3::new(d.x / dist, 0.0, d.z / dist) * step;
            let y = height_at(&field, next.x, next.z);
            lerp.target = Vec3::new(next.x, y, next.z);
            lerp.yaw = d.x.atan2(d.z);
            lerp.hop = true;
            lerp.turn = true;
            if mesh.0 != a.stand_mesh {
                mesh.0 = a.stand_mesh.clone();
            }
        } else {
            // grazing pause, then pick the next waypoint near the anchor
            lerp.hop = false;
            if mesh.0 != a.graze_mesh {
                mesh.0 = a.graze_mesh.clone();
            }
            a.pause -= dt;
            if a.pause <= 0.0 {
                // xorshift32 — render-only randomness, never sim state
                let mut s = a.rng;
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                a.rng = s;
                let ang = (s & 0xffff) as f32 / 65535.0 * std::f32::consts::TAU;
                let rad = 0.5 + ((s >> 16) & 0xff) as f32 / 255.0 * 0.9;
                a.waypoint = a.anchor + Vec3::new(ang.cos() * rad, 0.0, ang.sin() * rad);
                a.pause = 1.0 + ((s >> 24) as f32 / 255.0) * 2.5;
            }
        }
    }
}

/// Fish schools idle: slow circling spin + gentle bob on the water.
pub fn animate_fish(time: Res<Time>, mut q: Query<(&FishNode, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (f, mut tf) in &mut q {
        tf.rotation = Quat::from_rotation_y((t * 0.35 + f.phase) % std::f32::consts::TAU);
        tf.translation.y = f.base_y + ((t * 1.7 + f.phase).sin()) * 0.04;
    }
}

/// Ease roots toward their sim targets, apply yaw + idle bob (TS loop body).
pub fn interpolate(time: Res<Time>, mut q: Query<(&mut Transform, &Lerp), With<RenderRoot>>) {
    let k = (14.0 * time.delta_secs()).min(1.0);
    let bob_t = time.elapsed_secs() * 5.0;
    for (mut tf, l) in &mut q {
        let mut target = l.target;
        if l.hop {
            target.y += (bob_t + l.bob_phase).sin().abs() * 0.07;
        }
        tf.translation = tf.translation.lerp(target, k);
        if l.turn {
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
            // the fishing hut's ring shows its work aura, not its footprint
            let scale = if b.kind == BuildingKind::FishingHut {
                Vec3::splat(saladin_sim::FISHING_HUT_RANGE.to_num::<f32>() * 2.0)
            } else {
                Vec3::splat(def.footprint as f32 * 1.5)
            };
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
    use crate::render::models::props::{fish_node_mesh, resource_node_meshes};
    let mut nodes = HashMap::new();
    for r in [ResourceType::Wood, ResourceType::Stone, ResourceType::Food, ResourceType::Gold] {
        nodes.insert(r, resource_node_meshes(r).into_iter().map(|m| meshes.add(m)).collect());
    }
    let fish_node = meshes.add(fish_node_mesh());
    RenderAssets {
        unit_rigs: UnitKind::ALL
            .iter()
            .map(|k| {
                crate::render::models::unit_rig(*k)
                    .into_iter()
                    .map(|p| RigHandle { group: p.group, pivot: p.pivot, mesh: meshes.add(p.mesh) })
                    .collect()
            })
            .collect(),
        impostors: UnitKind::ALL
            .iter()
            .map(|k| meshes.add(crate::render::models::unit_impostor_mesh(*k)))
            .collect(),
        buildings: BuildingKind::ALL
            .iter()
            .map(|k| meshes.add(crate::render::models::building_mesh(*k)))
            .collect(),
        team_rigs: HashMap::new(),
        team_impostors: HashMap::new(),
        nodes,
        fish_node,
        carry_sack: meshes.add(crate::render::models::units::carry_sack_mesh()),
        puff: meshes.add(Sphere::new(1.0).mesh().uv(6, 5)),
        flame: meshes.add(Cone { radius: 0.5, height: 1.0 }.mesh().resolution(5).build()),
        ripple: meshes.add(
            Torus { minor_radius: 0.03, major_radius: 1.0 }
                .mesh()
                .minor_resolution(4)
                .major_resolution(24)
                .build(),
        ),
        scorch: meshes.add(crate::render::models::props::scorch_mesh()),
        rubble_chunk: meshes.add(crate::render::models::props::rubble_chunk_mesh()),
        rubble_pile: meshes.add(crate::render::models::props::rubble_pile_mesh()),
        // flat ground quad; the dashed-ring texture does the shaping
        ring: meshes.add(Plane3d::default().mesh().size(1.0, 1.0).build()),
        bar_quad: meshes.add(Mesh::from(Rectangle::new(BAR_W, BAR_H))),
        rout_quad: meshes.add(Mesh::from(Rectangle::new(0.34, 0.34))),
        flag_pole: meshes.add(Mesh::from(Cylinder::new(0.04, 1.0))),
        flag_cloth: meshes.add(Mesh::from(Rectangle::new(0.5, 0.3))),
    }
}
