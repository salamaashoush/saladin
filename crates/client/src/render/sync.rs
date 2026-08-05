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
use saladin_protocol::{
    BuildState, Building, GameId, Owner, Player, Pos, ResourceNode, Unit, WorldConfig,
};
use saladin_sim::rng::mix_seed;
use saladin_sim::{
    BuildingKind, PLAYER_COLORS, ResourceType, UnitKind, WORLD_SIZE, building_def,
    effective_building_def, footprint_tiles, hash2, tile_key, unit_def,
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
    /// Base unit rigs (team parts white), indexed `kind * 2 + faction`;
    /// per-team copies bake lazily into `team_rigs`/`team_impostors` so
    /// detail colors render true.
    pub unit_rigs: Vec<Vec<RigHandle>>,
    pub impostors: Vec<Handle<Mesh>>,
    pub team_rigs: HashMap<(usize, u32), Vec<RigHandle>>,
    pub team_impostors: HashMap<(usize, u32), Handle<Mesh>>,
    /// Indexed `kind as usize * 2 + faction as usize` — Ayyubid and Crusader
    /// settlements use distinct baked architecture.
    pub buildings: Vec<Handle<Mesh>>,
    /// Half-tile wall run (+X) per faction, yawed per connected neighbor by
    /// `update_wall_arms`.
    pub wall_arm: [Handle<Mesh>; 2],
    pub nodes: HashMap<ResourceType, Vec<Handle<Mesh>>>,
    pub fish_node: Handle<Mesh>,
    pub carry_sack: Handle<Mesh>,
    /// [axe, pick, sickle] — peasant hand tools (empty if not baked)
    pub tools: Vec<Handle<Mesh>>,
    pub puff: Handle<Mesh>,
    pub flame: Handle<Mesh>,
    pub ripple: Handle<Mesh>,
    /// Work-radius ring per `BuildingKind`, baked at that kind's own aura
    /// radius — scaling one shared torus would scale its tube with it.
    pub aura_rings: Vec<Option<Handle<Mesh>>>,
    /// Timber frame hung around a construction site (unit footprint).
    pub scaffold: Handle<Mesh>,
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
    pub bar_build: Handle<StandardMaterial>,
    pub rout: Handle<StandardMaterial>,
    pub flag_pole: Handle<StandardMaterial>,
    pub flag_cloth: Handle<StandardMaterial>,
    pub ghost_ok: Handle<StandardMaterial>,
    pub ghost_bad: Handle<StandardMaterial>,
    pub demolish: Handle<StandardMaterial>,
    pub arrow: Handle<StandardMaterial>,
    pub foam: Handle<StandardMaterial>,
    pub aura: Handle<StandardMaterial>,
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
        bar_build: overlay(mats, Color::srgb_u8(0x6c, 0xa8, 0xe8), 1.0),
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
        aura: overlay(mats, Color::srgb_u8(0x5f, 0xd6, 0xe8), 0.55),
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

/// What a harvesting peasant is actually doing — picks the tool in hand and
/// the swing cycle (chop high, mine low, forage bent over).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    None,
    Chop,
    Mine,
    Forage,
}

/// One swappable hand tool on a peasant's right hand; visible only while the
/// matching activity runs (same ownership pattern as the carry sack).
#[derive(Component)]
pub struct ToolSlot(pub Activity);

/// Per-unit animation inputs mirrored from the sim each sync — the animator
/// is pure render math driven by these flags + wall time.
#[derive(Component)]
pub struct AnimState {
    pub kind: UnitKind,
    pub moving: bool,
    pub combat: bool,
    /// Toe to toe: has an enemy and has stopped walking to deal with it. Feet
    /// plant, the swing owns the pose.
    pub engaged: bool,
    /// Set against cavalry — the sim's own test (`brace && !has_target`), so
    /// the levelled spear on screen is the one the damage model is using.
    pub braced: bool,
    /// A loaded charge going in. Reads as a gallop and a couched lance.
    pub charging: bool,
    /// Broken and running. Overrides every other pose.
    pub routing: bool,
    pub harvest: bool,
    /// Debounce: the sim flaps Harvesting<->ToResource every few ticks at
    /// node edges (separation shoves workers out of range); the work pose
    /// holds until this wall-clock time so the swing never stutters.
    pub work_until: f32,
    pub activity: Activity,
    pub carrying: bool,
    pub phase: f32,
    /// sim walk speed — leg swing cadence scales with it so cavalry gallops
    /// faster than a trundling ram
    pub stride: f32,
}

/// Per-node girth and height jitter fixed at spawn. `sync_render` rewrites
/// `Transform::scale` from the depletion curve every frame, so variety has to
/// survive as a multiplier or it is erased on frame two.
#[derive(Component)]
pub struct NodeBaseScale(pub Vec3);

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
    /// 0 = health, 1 = construction progress. Two bars because they answer two
    /// questions: a site under fire is both 12% built and 40% burnt.
    pub row: u8,
}

/// Selected-building ring + rally flag markers (one of each at most).
#[derive(Component)]
pub struct BuildingSelRing;
/// Thin world-space circle showing a selected building's work radius
/// (fishing hut aura) — a dedicated mesh, NOT the dashed ring texture
/// scaled up (that blows the dashes into giant blobs).
#[derive(Component)]
pub struct AuraRing;
#[derive(Component)]
pub struct RallyFlag;

#[derive(Resource, Default)]
pub struct RenderMap(pub HashMap<u64, Entity>);

/// Building occupancy for wall yaw (client-side mirror of stampOccupancy).
#[derive(Resource, Default)]
pub struct OccupiedTiles(pub HashSet<i32>);

/// Wall run orientation: 8-way neighbour double-angle average (wallAngleAt).
/// Per-wall connectivity mask, bit per +X/-X/+Z/-Z neighbor; arms respawn when
/// it changes (new segments, absorbed segments, razed neighbors).
#[derive(Component)]
pub struct WallArms(pub u8);

#[derive(Component)]
pub struct WallArm;

/// What the render root is currently DRESSED as. The sim row's kind and state
/// both change under a standing entity (a Tower becomes a Watchtower in place,
/// a Site becomes a hall) and the mesh handle, the scaffold and the scorch have
/// to follow without the entity — and its batch — ever being rebuilt.
#[derive(Component)]
pub struct BuiltAs {
    pub kind: BuildingKind,
}

/// Timber frame around an unfinished building. Child of the root, so it
/// inherits the site's position and facing.
#[derive(Component)]
pub struct Scaffold;

/// Scorch marks and strewn rubble stamped by `building_damage_fx`. Marked so
/// repair and upgrade can strip them: the dressing is monotonic by design.
#[derive(Component)]
pub struct DamageDressing;

const ARM_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
// yaw rotating the +X-authored arm toward each ARM_DIRS entry
const ARM_YAWS: [f32; 4] =
    [0.0, std::f32::consts::PI, -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2];

/// Hang a half-tile wall arm toward every adjacent fortification. Corners,
/// T-junctions and crosses all emerge from the mask — no rotation guessing.
pub fn update_wall_arms(
    mut commands: Commands,
    map: Res<RenderMap>,
    assets: Res<RenderAssets>,
    q_sim: Query<(&GameId, &Pos, &Building, Option<&Owner>)>,
    q_players: Query<&Player>,
    q_mat: Query<(Option<&WallArms>, &MeshMaterial3d<StandardMaterial>), With<RenderRoot>>,
    q_arms: Query<(Entity, &ChildOf), With<WallArm>>,
) {
    let owner_faction: HashMap<u64, saladin_sim::Faction> =
        q_players.iter().map(|p| (p.player_id, p.faction)).collect();
    // a wall connects to walls, gates, towers and the keep
    let linkable: HashSet<i32> = q_sim
        .iter()
        .filter(|(_, _, b, _)| {
            matches!(
                b.kind,
                BuildingKind::Wall
                    | BuildingKind::Gatehouse
                    | BuildingKind::Tower
                    | BuildingKind::Watchtower
                    | BuildingKind::Keep
            )
        })
        .flat_map(|(_, p, b, _)| {
            footprint_tiles(building_def(b.kind).footprint, p.pos.x, p.pos.y)
                .into_iter()
                .map(|t| tile_key(t.tx, t.ty))
        })
        .collect();

    for (gid, pos, b, owner) in &q_sim {
        if b.kind != BuildingKind::Wall {
            continue;
        }
        let faction = owner
            .and_then(|o| owner_faction.get(&o.0).copied())
            .unwrap_or(saladin_sim::Faction::Ayyubid);
        let Some(&root) = map.0.get(&gid.0) else { continue };
        let tx = pos.pos.x.to_num::<f32>().floor() as i32;
        let ty = pos.pos.y.to_num::<f32>().floor() as i32;
        let mut mask = 0u8;
        for (i, (dx, dy)) in ARM_DIRS.iter().enumerate() {
            if linkable.contains(&tile_key(tx + dx, ty + dy)) {
                mask |= 1 << i;
            }
        }
        let Ok((arms, mat)) = q_mat.get(root) else { continue };
        if arms.map(|a| a.0) == Some(mask) {
            continue;
        }
        for (e, child_of) in &q_arms {
            if child_of.parent() == root {
                commands.entity(e).despawn();
            }
        }
        let mat_handle = mat.0.clone();
        commands.entity(root).insert(WallArms(mask)).with_children(|p| {
            for i in 0..4 {
                if mask & (1 << i) != 0 {
                    p.spawn((
                        WallArm,
                        Mesh3d(assets.wall_arm[faction as usize].clone()),
                        MeshMaterial3d(mat_handle.clone()),
                        Transform::from_rotation(Quat::from_rotation_y(ARM_YAWS[i])),
                    ));
                }
            }
        });
    }
}

/// Y-scale a site starts at: a poured foundation, not an invisible building.
const SITE_FLOOR: f32 = 0.2;
/// Authored span of `scaffold_mesh` in x/z and y (it is scaled to the building).
const SCAFFOLD_SPAN: f32 = 1.2;
const SCAFFOLD_HEIGHT: f32 = 1.06;

/// Y-scale of a site's shared mesh at `work` progress. Never 0: a founded site
/// is a real target from the tick it is placed, so it has to be visible.
fn site_rise(work: f32) -> f32 {
    SITE_FLOOR + (1.0 - SITE_FLOOR) * work.clamp(0.0, 1.0)
}

fn dressing_stage(ratio: f32) -> u8 {
    if ratio < 0.25 {
        2
    } else if ratio < 0.5 {
        1
    } else {
        0
    }
}

/// The three moments a STANDING building changes its model: a site rising, an
/// upgrade swapping kind in place, and damage being mended. All three keep the
/// entity and the SHARED per-kind handle, so the instanced batch survives —
/// a per-entity mesh or material here would cost one draw call per building.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_building_lifecycle(
    mut commands: Commands,
    assets: Res<RenderAssets>,
    map: Res<RenderMap>,
    q_sim: Query<(&GameId, &Building, Option<&Owner>)>,
    q_players: Query<&Player>,
    mut q_root: Query<
        (
            &mut Mesh3d,
            &mut Transform,
            &mut DamageState,
            &mut BuiltAs,
            &MeshMaterial3d<StandardMaterial>,
        ),
        With<RenderRoot>,
    >,
    q_dress: Query<(Entity, &ChildOf), With<DamageDressing>>,
    mut q_scaffold: Query<(Entity, &ChildOf, &mut Transform), (With<Scaffold>, Without<RenderRoot>)>,
) {
    let owner_faction: HashMap<u64, saladin_sim::Faction> =
        q_players.iter().map(|p| (p.player_id, p.faction)).collect();
    let owner_mask: HashMap<u64, u64> =
        q_players.iter().map(|p| (p.player_id, p.tech_mask)).collect();

    let mut want_scaffold: HashMap<Entity, (Vec3, Handle<StandardMaterial>)> = HashMap::new();
    for (gid, b, owner) in &q_sim {
        let Some(&root) = map.0.get(&gid.0) else { continue };
        let Ok((mut mesh, mut tf, mut dmg, mut built, mat)) = q_root.get_mut(root) else { continue };
        let faction = owner
            .and_then(|o| owner_faction.get(&o.0).copied())
            .unwrap_or(saladin_sim::Faction::Ayyubid);
        let mask = owner.and_then(|o| owner_mask.get(&o.0).copied()).unwrap_or(0);
        let def = effective_building_def(b.kind, mask);

        let mut strip_dressing = false;
        if built.kind != b.kind {
            mesh.0 = assets.buildings[b.kind as usize * 2 + faction as usize].clone();
            built.kind = b.kind;
            dmg.span = def.footprint as f32 * 0.55;
            dmg.roof = def.height.to_num::<f32>();
            strip_dressing = true;
        }
        if dressing_stage(dmg.ratio) < dmg.applied {
            strip_dressing = true;
        }
        if strip_dressing {
            dmg.applied = 0;
            for (e, child_of) in &q_dress {
                if child_of.parent() == root {
                    commands.entity(e).despawn();
                }
            }
        }

        let raising = b.state == BuildState::Site;
        let rise = if raising { site_rise(b.work.to_num::<f32>()) } else { 1.0 };
        if tf.scale.y != rise {
            tf.scale.y = rise;
        }
        if raising {
            let fp = (def.footprint as f32 + 0.25) / SCAFFOLD_SPAN;
            let h = (def.height.to_num::<f32>() * 0.85).max(0.7) / SCAFFOLD_HEIGHT;
            want_scaffold.insert(root, (Vec3::new(fp, h / rise, fp), mat.0.clone()));
        }
    }

    let mut have: HashSet<Entity> = HashSet::new();
    for (e, child_of, mut tf) in &mut q_scaffold {
        match want_scaffold.get(&child_of.parent()) {
            Some((s, _)) => {
                have.insert(child_of.parent());
                tf.scale = *s;
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (root, (s, mat)) in want_scaffold {
        if have.contains(&root) {
            continue;
        }
        commands.entity(root).with_children(|p| {
            p.spawn((
                Scaffold,
                Mesh3d(assets.scaffold.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_scale(s),
            ));
        });
    }
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
        faction: saladin_sim::Faction,
        color: u32,
    ) -> Vec<RigHandle> {
        use crate::render::models::bake_team;
        let slot = kind as usize * 2 + faction as usize;
        self.team_rigs
            .entry((slot, color))
            .or_insert_with(|| {
                self.unit_rigs[slot]
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
            // cedar country: conifer with the odd tall broadleaf in the gullies
            Biome::Pine | Biome::Alpine => [TREE_CONIFER, TREE_CONIFER, TREE_BROADLEAF_TALL, TREE_CONIFER][roll % 4],
            // broadleaf woodland is BROADLEAF; it used to render as pine, which
            // is why every temperate map looked like the same conifer plantation
            Biome::Forest => [TREE_BROADLEAF, TREE_BROADLEAF_TALL, TREE_BROADLEAF, TREE_CONIFER][roll % 4],
            Biome::OliveGrove | Biome::Scrub => [TREE_OLIVE, TREE_OLIVE, TREE_BROADLEAF][roll % 3],
            Biome::Savanna => [TREE_OLIVE, TREE_BROADLEAF, TREE_OLIVE][roll % 3],
            Biome::Marsh => [TREE_BROADLEAF, TREE_BROADLEAF_TALL][roll % 2],
            Biome::Steppe | Biome::Desert | Biome::Dunes | Biome::Sand | Biome::Hills
            | Biome::Hammada | Biome::Wadi => TREE_OLIVE,
            _ => [TREE_BROADLEAF, TREE_BROADLEAF_TALL, TREE_BROADLEAF, TREE_CONIFER][roll % 4],
        },
        ResourceType::Food => match biome {
            Biome::Forest | Biome::Pine => [FOOD_BOAR, FOOD_BERRY, FOOD_BOAR, FOOD_DEER][roll % 4],
            Biome::Oasis => [FOOD_BERRY, FOOD_DEER_GRAZING][roll % 2],
            Biome::Scrub => [FOOD_BERRY, FOOD_BERRY, FOOD_DEER][roll % 3],
            Biome::Marsh => [FOOD_BOAR, FOOD_BERRY][roll % 2],
            // the great herds of the dry grass
            Biome::Savanna => [FOOD_DEER, FOOD_DEER_GRAZING, FOOD_DEER, FOOD_BOAR][roll % 4],
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
    field: Res<HeightField>,
    selection: Res<Selection>,
    cam_state: Res<CameraState>,
    time: Res<Time>,
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
    (mut q_rings, mut q_routs, mut q_animals, q_node_scale, mut prop_grid): (
        Query<(&ChildOf, &mut Visibility), (With<SelRing>, Without<RenderRoot>)>,
        Query<(&ChildOf, &mut Visibility), (With<RoutFlag>, Without<RenderRoot>, Without<SelRing>)>,
        Query<&mut AnimalNode>,
        Query<&NodeBaseScale>,
        ResMut<crate::PropGrid>,
    ),
) {
    let owner_color: HashMap<u64, u32> = q_players
        .iter()
        .map(|p| (p.player_id, PLAYER_COLORS[p.color as usize % PLAYER_COLORS.len()]))
        .collect();
    let owner_faction: HashMap<u64, saladin_sim::Faction> =
        q_players.iter().map(|p| (p.player_id, p.faction)).collect();
    let owner_mask: HashMap<u64, u64> =
        q_players.iter().map(|p| (p.player_id, p.tech_mask)).collect();
    let impostor = cam_state.view_size >= IMPOSTOR_VIEW_SIZE;
    let now = time.elapsed_secs();

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
            let selected = selection.contains(&gid.0);
            let color = team.unwrap_or(0xdddddd);
            let faction = owner
                .and_then(|o| owner_faction.get(&o.0).copied())
                .unwrap_or(saladin_sim::Faction::Ayyubid);
            let yaw = heading_yaw(u.heading);
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
                    faction,
                    color,
                    world,
                )
            });
            if let Ok((mut lerp, mut vis, _, anim, _)) = q_roots.get_mut(root) {
                lerp.target = world;
                lerp.hop = u.has_target;
                lerp.yaw = yaw;
                *vis = if u.garrisoned_in != 0 { Visibility::Hidden } else { Visibility::Inherited };
                if let Some(mut anim) = anim {
                    let def = unit_def(u.kind);
                    anim.moving = u.has_target;
                    anim.combat = u.attack_target != 0;
                    anim.engaged = u.attack_target != 0 && !u.has_target;
                    anim.braced = def.brace && !u.has_target;
                    anim.charging = def.charge_mult > saladin_sim::Fx::ONE
                        && u.charge_cd == 0
                        && u.has_target
                        && (u.attack_target != 0
                            || u.order == saladin_protocol::ORDER_ATTACK
                            || u.order == saladin_protocol::ORDER_ATTACK_MOVE);
                    anim.routing = u.routing;
                    anim.harvest = u.gather_state == saladin_sim::GatherState::Harvesting;
                    // carry_type identifies the node being worked while
                    // harvesting — picks the tool + swing cycle. STICKY: the
                    // sim flaps Harvesting<->ToResource at node edges, so the
                    // last activity is kept between bursts (the tool stays in
                    // hand on the dropoff walk instead of blinking)
                    if anim.harvest {
                        anim.activity = match u.carry_type {
                            ResourceType::Wood => Activity::Chop,
                            ResourceType::Stone | ResourceType::Gold => Activity::Mine,
                            ResourceType::Food => Activity::Forage,
                        };
                        anim.work_until = now + 0.35;
                    } else if u.gather_state == saladin_sim::GatherState::Idle {
                        anim.activity = Activity::None;
                        anim.work_until = 0.0;
                    }
                    // sack rule is the sim's own: loaded or not. The sim
                    // holds carrying at 0 for the whole harvest and sets it
                    // once for the dropoff walk — no strobing possible.
                    anim.carrying = u.carrying > 0;
                }
            }
            unit_state.insert(root, (color, selected, u.routing, u.kind));
        } else if let Some(b) = bld {
            seen.insert(gid.0);
            let world = Vec3::new(x, ground, z);
            let faction = owner
                .and_then(|o| owner_faction.get(&o.0).copied())
                .unwrap_or(saladin_sim::Faction::Ayyubid);
            if !map.0.contains_key(&gid.0) && !prop_grid.0.is_empty() {
                // scatter dressing does not survive a foundation being poured.
                // Models overhang their footprint, so anything bigger than a
                // hut clears the ring around it too.
                let fp = building_def(b.kind).footprint;
                let pad = if fp > 1 { 1 } else { 0 };
                let (cx, cz) = (x as i32, z as i32);
                let r = fp / 2 + pad;
                for tz in cz - r..=cz + r {
                    for tx in cx - r..=cx + r {
                        for e in prop_grid.0.remove(&tile_key(tx, tz)).unwrap_or_default() {
                            commands.entity(e).despawn();
                        }
                    }
                }
            }
            let mask = owner.and_then(|o| owner_mask.get(&o.0).copied()).unwrap_or(0);
            let root = *map.0.entry(gid.0).or_insert_with(|| {
                let mat = rmats.tint_mat(&mut mats, team.unwrap_or(0x9c958a));
                let def = effective_building_def(b.kind, mask);
                commands
                    .spawn((
                        RenderRoot(gid.0),
                        Mesh3d(assets.buildings[b.kind as usize * 2 + faction as usize].clone()),
                        MeshMaterial3d(mat),
                        Transform::from_translation(world),
                        Lerp { target: world, yaw: 0.0, bob_phase: 0.0, turn: false, hop: false },
                        BuiltAs { kind: b.kind },
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
                    // masonry retro-applies to standing buildings, so a teched
                    // hall reads above 1.0 against the base def
                    let max = effective_building_def(b.kind, mask).max_hp.max(1);
                    dmg.ratio = b.hp as f32 / max as f32;
                }
                // player-chosen quarter-turn facing (rides the Build command);
                // walls stay unrotated — connectivity arms carry direction
                let yaw = pos.facing.to_num::<f32>();
                if b.kind != BuildingKind::Wall && yaw != 0.0 {
                    lerp.yaw = yaw;
                    tf.rotation = Quat::from_rotation_y(yaw);
                }
            }
        } else if let Some(n) = node {
            seen.insert(gid.0);
            let world = Vec3::new(x, ground, z);
            let root = *map.0.entry(gid.0).or_insert_with(|| {
                // Coastal food sits on water tiles — draw a fish school there,
                // never a deer standing on the sea.
                use crate::render::models::props::*;
                // GameIds are handed out sequentially, so anything derived from
                // one makes a whole patch share a variant and a facing. Species,
                // yaw and girth all read the TILE instead.
                let stream = |salt: u32| {
                    hash2(x as i32, z as i32, mix_seed(world_cfg.seed, salt)).to_num::<f32>()
                };
                let variants = &assets.nodes[&n.res_type];
                let roll = (stream(0x3b17) * 997.0) as usize;
                let idx = node_variant(n.res_type, world_cfg.seed, x, z, roll, variants.len());
                let fishy = n.res_type == ResourceType::Food && ground < -0.005;
                let mesh = if fishy { assets.fish_node.clone() } else { variants[idx].clone() };
                let yaw = stream(0x7071) * std::f32::consts::TAU;
                // mineral outcrops settle INTO the slope: slightly embedded,
                // tilted most of the way onto the surface normal — never
                // perched flat on a hillside
                let mineral = matches!(n.res_type, ResourceType::Stone | ResourceType::Gold);
                let (pos, rot) = if mineral {
                    let nrm = crate::terrain::normal_at(&field, x, z);
                    let lean = Quat::from_rotation_arc(Vec3::Y, Vec3::Y.lerp(nrm, 0.75).normalize());
                    (world - Vec3::Y * 0.04, lean * Quat::from_rotation_y(yaw))
                } else {
                    (world - Vec3::Y * 0.01, Quat::from_rotation_y(yaw))
                };
                // Every tree the same height reads as a plantation: a wood wants
                // saplings and veterans, and girth has to vary apart from height
                // or they are one mesh at N sizes.
                let spread = match n.res_type {
                    ResourceType::Wood => 0.45,
                    ResourceType::Food => 0.12,
                    _ => 0.18,
                };
                let base = Vec3::new(
                    1.0 + (stream(0x5231) - 0.5) * spread * 1.2,
                    1.0 + (stream(0x5117) - 0.5) * spread * 2.0,
                    1.0 + (stream(0x5231) - 0.5) * spread * 1.2,
                );
                let mut e = commands.spawn((
                    RenderRoot(gid.0),
                    NodeBaseScale(base),
                    Mesh3d(mesh),
                    MeshMaterial3d(rmats.node[&n.res_type].clone()),
                    Transform::from_translation(pos)
                        .with_rotation(rot)
                        .with_scale(base * node_scale(n.remaining)),
                    Lerp { target: pos, yaw, bob_phase: 0.0, turn: false, hop: false },
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
            let base = q_node_scale.get(root).map(|b| b.0).unwrap_or(Vec3::ONE);
            if let Ok((_, _, mut tf, _, _)) = q_roots.get_mut(root) {
                tf.scale = base * node_scale(n.remaining);
            }
            if let Ok(mut animal) = q_animals.get_mut(root) {
                animal.remaining = n.remaining;
            } else if let Ok((_, _, mut tf, _, _)) = q_roots.get_mut(root) {
                // static nodes snap to the sim position (same embed the spawn
                // used — snapping to raw `world` would pop minerals back out
                // of the slope); animals own their pose
                let embed = if matches!(n.res_type, ResourceType::Stone | ResourceType::Gold) {
                    0.04
                } else {
                    0.01
                };
                tf.translation = world - Vec3::Y * embed;
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
        let want_stage = dressing_stage(d.ratio);
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
                            DamageDressing,
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
                            DamageDressing,
                            Mesh3d(assets.rubble_chunk.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::from_xyz(ang.cos() * span * 1.05, 0.02, ang.sin() * span * 1.05)
                                .with_rotation(Quat::from_rotation_y(h01(k + 5.0) * std::f32::consts::TAU))
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
    faction: saladin_sim::Faction,
    color: u32,
    world: Vec3,
) -> Entity {
    let def = unit_def(kind);
    let h = def.height.to_num::<f32>();
    let r = def.radius.to_num::<f32>();
    let mat = rmats.unit_mat(mats, color, false);
    let rig = assets.team_rig(meshes, kind, faction, color);
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
                engaged: false,
                braced: false,
                charging: false,
                routing: false,
                harvest: false,
                work_until: 0.0,
                activity: Activity::None,
                carrying: false,
                phase,
                stride: unit_def(kind).speed.to_num::<f32>(),
            },
        ))
        .with_children(|p| {
            for part in rig {
                let mut e = p.spawn((
                    UnitBody { group: part.group, pivot: part.pivot, impostor_part: false, sack: false },
                    Mesh3d(part.mesh),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(part.pivot),
                ));
                // peasant right hand carries the activity tools — children of
                // the arm so they swing with it; visibility owned by
                // update_tool_visibility (matching activity only)
                if kind == UnitKind::Peasant
                    && part.group == crate::render::models::RigGroup::ArmR
                    && assets.tools.len() == 3
                {
                    let hand = Vec3::new(0.0, -unit_def(kind).height.to_num::<f32>() * 0.38, 0.0);
                    e.with_children(|arm| {
                        for (i, act) in
                            [Activity::Chop, Activity::Mine, Activity::Forage].into_iter().enumerate()
                        {
                            arm.spawn((
                                ToolSlot(act),
                                Mesh3d(assets.tools[i].clone()),
                                MeshMaterial3d(mat.clone()),
                                Transform::from_translation(hand)
                                    .with_rotation(Quat::from_rotation_x(0.5)),
                                Visibility::Hidden,
                            ));
                        }
                    });
                }
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

/// Render yaw for a sim `heading`. The sim keeps facing as one of sixteen
/// compass points counter-clockwise from +X (no trig anywhere in lockstep);
/// models are authored forward = +Z, so the render yaw is a quarter turn minus
/// the compass angle.
///
/// This replaces deriving yaw from `has_target` and the move target, which was
/// wrong the moment a unit stopped to fight: combat clears the walk every
/// strike, so a man kept whatever heading he last WALKED in and swung at
/// enemies standing behind him.
pub fn heading_yaw(heading: u8) -> f32 {
    use std::f32::consts::{FRAC_PI_2, TAU};
    FRAC_PI_2 - (heading % saladin_sim::HEADINGS as u8) as f32 * TAU / saladin_sim::HEADINGS as f32
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
    // pose targets ease in — sim state flips (walk<->chop<->idle) otherwise
    // snap limbs to a new pose in one frame and read as flicker
    let ease = (16.0 * time.delta_secs()).min(1.0);
    for (anim, children) in &q_roots {
        let tp = t + anim.phase;
        let mounted = matches!(anim.kind, UnitKind::Knight | UnitKind::HorseArcher | UnitKind::Mamluk);
        // read the def, not a hand-kept list: the Mangonel became `ranged` and
        // three kinds were appended, and a stale list poses them wrong
        let ranged = unit_def(anim.kind).ranged;
        let stance = if anim.engaged && !mounted { 0.2 } else { 0.0 };
        // a charge is a gallop and a rout is a sprint — same legs, faster clock
        let urgency = if anim.routing {
            1.6
        } else if anim.charging {
            1.35
        } else {
            1.0
        };
        let gait = (3.5 + anim.stride * 2.4) * urgency;
        let walk = if anim.moving { (tp * gait).sin() } else { 0.0 };
        let swing_amp = (if mounted { 0.38 } else { 0.55 }) * if anim.routing { 1.4 } else { 1.0 };
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
                // visible exactly while loaded (sim: the whole dropoff walk)
                let want = if anim.carrying { Visibility::Inherited } else { Visibility::Hidden };
                if *vis != want {
                    *vis = want;
                }
                continue;
            }
            // debounced "working" — survives the sim's Harvesting<->ToResource
            // flapping at node edges
            let working = anim.harvest || t < anim.work_until;
            let foraging = anim.kind == UnitKind::Peasant
                && working
                && anim.activity == Activity::Forage;
            // foragers bow at the hips; arms must FOLLOW the bow (they're rig
            // siblings of the body, not children) or shoulders detach
            let bow = if foraging {
                Quat::from_rotation_x(0.3)
            } else if anim.routing || anim.charging {
                // broken men run bent, a lancer leans into the charge
                Quat::from_rotation_x(if anim.routing { 0.24 } else { 0.16 })
            } else {
                Quat::IDENTITY
            };
            let rot = match body.group {
                G::Body => bow,
                // toe to toe: one foot forward, one back — a fighting stance
                // instead of a man standing to attention while he swings
                G::LegL => Quat::from_rotation_x(walk * swing_amp + stance),
                G::LegR => Quat::from_rotation_x(-walk * swing_amp - stance),
                // a rout is arms-down flight: no weapon pose survives it
                _ if anim.routing && matches!(body.group, G::ArmL | G::ArmR) => {
                    let side = if body.group == G::ArmL { 1.0 } else { -1.0 };
                    Quat::from_rotation_x(-walk * side * 0.9 - 0.2)
                }
                // set spears: the arm levels the shaft forward and STAYS there
                // while the sim counts this unit as braced
                G::ArmR if anim.braced && !anim.combat => Quat::from_rotation_x(1.2),
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
                    // distinct work cycles per activity: woodcutting is a
                    // high overhead chop, mining a heavy low pick swing,
                    // foraging a slow reach toward the ground
                    _ if working && anim.activity != Activity::None => match anim.activity {
                        Activity::Mine => Quat::from_rotation_x(0.8 - strike * 1.5),
                        Activity::Forage => {
                            Quat::from_rotation_x(0.65 + (tp * 2.1).sin() * 0.3)
                        }
                        _ => Quat::from_rotation_x(0.3 - strike * 0.9),
                    },
                    // a levelled lance going in beats a walk swing
                    _ if anim.charging => Quat::from_rotation_x(1.05),
                    _ if anim.moving => Quat::from_rotation_x(-walk * 0.25),
                    _ => Quat::from_rotation_x((tp * 1.6).sin() * 0.06),
                },
                G::ArmL if anim.braced && !anim.combat => Quat::from_rotation_x(0.5),
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
            if body.group == G::ArmR
                && anim.kind == UnitKind::Peasant
                && std::env::var("SALADIN_ANIM_DEBUG").is_ok()
            {
                let (axis, angle) = rot.to_axis_angle();
                eprintln!(
                    "t={t:.3} target={:.3} cur={:.3} axis_x={:.2} harvest={} act={:?} moving={} combat={}",
                    angle,
                    tf.rotation.to_axis_angle().1,
                    axis.x,
                    anim.harvest,
                    anim.activity,
                    anim.moving,
                    anim.combat
                );
            }
            // arms ride the forage bow: their pivots rotate with the torso
            let rot = if foraging && matches!(body.group, G::ArmL | G::ArmR) { bow * rot } else { rot };
            tf.rotation = tf.rotation.slerp(rot, ease);
            if matches!(body.group, G::ArmL | G::ArmR) && anim.kind == UnitKind::Peasant {
                let target = if foraging { bow * body.pivot } else { body.pivot };
                tf.translation = tf.translation.lerp(target, ease);
            }
            // Ram: the slung beam jabs forward (+Z after the rig yaw) on attack.
            if anim.kind == UnitKind::Ram && body.group == G::ArmR {
                let jab = if anim.combat { strike * 0.45 } else { 0.0 };
                tf.translation = body.pivot + Vec3::new(0.0, 0.0, jab);
            }
        }
    }
}

/// Show the tool matching the peasant's current activity; hide the rest.
/// Tools are grandchildren of the rig root (children of the right arm), so
/// climb two parent hops to the AnimState.
pub fn update_tool_visibility(
    time: Res<Time>,
    q_roots: Query<&AnimState>,
    q_arms: Query<&ChildOf, With<UnitBody>>,
    mut q_tools: Query<(&ToolSlot, &ChildOf, &mut Visibility), Without<UnitBody>>,
) {
    let t = time.elapsed_secs();
    for (slot, child_of, mut vis) in &mut q_tools {
        let Ok(arm_parent) = q_arms.get(child_of.parent()) else { continue };
        let Ok(anim) = q_roots.get(arm_parent.parent()) else { continue };
        // shown while working AND on the carry walk, on the same debounced
        // clocks the animator uses — never blinks between swing bursts
        let working = anim.harvest || t < anim.work_until || anim.carrying;
        let want = if working && anim.activity == slot.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
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
    map: Res<RenderMap>,
    cam: Query<&Transform, (With<crate::camera::GameCamera>, Without<HpBar>)>,
    q_roots: Query<&Transform, (With<RenderRoot>, Without<HpBar>, Without<crate::camera::GameCamera>)>,
    q_units: Query<(&GameId, &Pos, &Unit)>,
    q_buildings: Query<(&GameId, &Pos, &Building, Option<&Owner>)>,
    q_players: Query<&Player>,
    mut q_bars: Query<(Entity, &HpBar, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Ok(cam_tf) = cam.single() else { return };
    let bill = cam_tf.rotation;
    let owner_mask: HashMap<u64, u64> =
        q_players.iter().map(|p| (p.player_id, p.tech_mask)).collect();

    // anchor bars on the INTERPOLATED render root, not the 20 Hz sim row —
    // a bar stepping at tick rate over a smoothly-gliding body reads as
    // flicker/jitter
    let anchor = |id: u64, sim_x: f32, sim_z: f32| -> Vec3 {
        map.0
            .get(&id)
            .and_then(|e| q_roots.get(*e).ok())
            .map(|tf| tf.translation)
            .unwrap_or_else(|| Vec3::new(sim_x, height_at(&field, sim_x, sim_z), sim_z))
    };

    // desired bars: (id, row) → (world pos, ratio, progress?)
    let mut want: HashMap<(u64, u8), (Vec3, f32, bool)> = HashMap::new();
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
        let base = anchor(g.0, p.pos.x.to_num::<f32>(), p.pos.y.to_num::<f32>());
        let lift = def.height.to_num::<f32>() + def.radius.to_num::<f32>() * 2.4 + 0.35;
        want.insert((g.0, 0), (base + Vec3::Y * lift, ratio.clamp(0.0, 1.0), false));
    }
    for (g, p, b, owner) in &q_buildings {
        let mask = owner.and_then(|o| owner_mask.get(&o.0).copied()).unwrap_or(0);
        let def = effective_building_def(b.kind, mask);
        if def.max_hp <= 0 {
            continue;
        }
        let base = anchor(g.0, p.pos.x.to_num::<f32>(), p.pos.y.to_num::<f32>());
        let top = base + Vec3::Y * (def.height.to_num::<f32>() + 0.6);
        let ratio = b.hp as f32 / def.max_hp as f32;
        if ratio < 0.999 {
            want.insert((g.0, 0), (top, ratio.clamp(0.0, 1.0), false));
        }
        // a rising site: the health bar says "frail", the build bar says "how
        // much longer" — without both, a 12%-hp foundation reads as a ruin
        if b.state != BuildState::Complete {
            let work = b.work.to_num::<f32>().clamp(0.0, 1.0);
            want.insert((g.0, 1), (top - Vec3::Y * (BAR_H * 1.9), work, true));
        }
    }

    let mut have: HashSet<(u64, u8)> = HashSet::new();
    for (e, bar, mut tf, mut mat) in &mut q_bars {
        match want.get(&(bar.of, bar.row)) {
            Some(&(pos, ratio, progress)) => {
                have.insert((bar.of, bar.row));
                tf.rotation = bill;
                if bar.fill {
                    // push the fill clearly in front of the backplate — a
                    // 1 mm gap z-fights at ortho distances and shimmers
                    tf.translation = pos + bill * Vec3::new(-(BAR_W * (1.0 - ratio)) / 2.0, 0.0, 0.025);
                    tf.scale = Vec3::new(ratio.max(0.001), 1.0, 1.0);
                    let want_mat = if progress {
                        rmats.bar_build.clone()
                    } else if ratio > 0.5 {
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
    for (&(id, row), &(pos, _, progress)) in &want {
        if have.contains(&(id, row)) {
            continue;
        }
        // backplate slightly oversized: reads as a crisp border
        commands.spawn((
            HpBar { of: id, fill: false, row },
            Mesh3d(assets.bar_quad.clone()),
            MeshMaterial3d(rmats.bar_bg.clone()),
            Transform::from_translation(pos).with_scale(Vec3::new(1.08, 1.45, 1.0)),
        ));
        commands.spawn((
            HpBar { of: id, fill: true, row },
            Mesh3d(assets.bar_quad.clone()),
            MeshMaterial3d(if progress {
                rmats.bar_build.clone()
            } else {
                rmats.bar_green.clone()
            }),
            Transform::from_translation(pos),
        ));
    }
}

/// Ring + rally flag on the selected building (updateBuildingHighlight port).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_building_highlight(
    mut commands: Commands,
    assets: Res<RenderAssets>,
    rmats: Res<RenderMaterials>,
    field: Res<HeightField>,
    selection: Res<Selection>,
    q_buildings: Query<(&GameId, &Pos, &Building)>,
    mut q_ring: Query<(Entity, &mut Transform), (With<BuildingSelRing>, Without<RallyFlag>)>,
    mut q_flag: Query<(Entity, &mut Transform), (With<RallyFlag>, Without<BuildingSelRing>)>,
    mut q_aura: Query<
        (Entity, &mut Transform, &mut Mesh3d),
        (With<AuraRing>, Without<BuildingSelRing>, Without<RallyFlag>),
    >,
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
            // work-radius circle for any building carrying an aura — the ring
            // is per-kind, so a Granary shows its fields and a hut its fishery
            match assets.aura_rings.get(b.kind as usize).and_then(|r| r.as_ref()) {
                Some(ring) => match q_aura.single_mut() {
                    Ok((_, mut tf, mut m)) => {
                        tf.translation = pos + Vec3::Y * 0.02;
                        if m.0 != *ring {
                            m.0 = ring.clone();
                        }
                    }
                    Err(_) => {
                        commands.spawn((
                            AuraRing,
                            Mesh3d(ring.clone()),
                            MeshMaterial3d(rmats.aura.clone()),
                            Transform::from_translation(pos + Vec3::Y * 0.02),
                        ));
                    }
                },
                None => {
                    if let Ok((e, ..)) = q_aura.single_mut() {
                        commands.entity(e).despawn();
                    }
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
            if let Ok((e, ..)) = q_aura.single_mut() {
                commands.entity(e).despawn();
            }
        }
    }
}

/// Build the shared mesh handles at match start.
pub fn build_assets(meshes: &mut Assets<Mesh>) -> RenderAssets {
    use crate::render::models::baked::{fish_node_mesh, resource_node_meshes};
    let mut nodes = HashMap::new();
    for r in [ResourceType::Wood, ResourceType::Stone, ResourceType::Food, ResourceType::Gold] {
        nodes.insert(r, resource_node_meshes(r).into_iter().map(|m| meshes.add(m)).collect());
    }
    let fish_node = meshes.add(fish_node_mesh());
    RenderAssets {
        unit_rigs: UnitKind::ALL
            .iter()
            .flat_map(|k| {
                [saladin_sim::Faction::Ayyubid, saladin_sim::Faction::Crusader].map(|f| {
                    crate::render::models::unit_rig(*k, f)
                        .into_iter()
                        .map(|p| RigHandle { group: p.group, pivot: p.pivot, mesh: meshes.add(p.mesh) })
                        .collect()
                })
            })
            .collect(),
        impostors: UnitKind::ALL
            .iter()
            .map(|k| meshes.add(crate::render::models::unit_impostor_mesh(*k)))
            .collect(),
        buildings: BuildingKind::ALL
            .iter()
            .flat_map(|k| {
                [saladin_sim::Faction::Ayyubid, saladin_sim::Faction::Crusader]
                    .map(|f| meshes.add(crate::render::models::building_mesh(*k, f)))
            })
            .collect(),
        wall_arm: [saladin_sim::Faction::Ayyubid, saladin_sim::Faction::Crusader]
            .map(|f| meshes.add(crate::render::models::baked::wall_arm_mesh(f))),
        team_rigs: HashMap::new(),
        team_impostors: HashMap::new(),
        nodes,
        fish_node,
        carry_sack: meshes.add(crate::render::models::units::carry_sack_mesh()),
        tools: crate::render::models::baked::tool_meshes().into_iter().map(|m| meshes.add(m)).collect(),
        puff: meshes.add(Sphere::new(1.0).mesh().uv(6, 5)),
        flame: meshes.add(Cone { radius: 0.5, height: 1.0 }.mesh().resolution(5).build()),
        aura_rings: BuildingKind::ALL
            .iter()
            .map(|k| {
                building_def(*k).aura.map(|a| {
                    meshes.add(
                        Torus { minor_radius: 0.06, major_radius: a.radius.to_num::<f32>() }
                            .mesh()
                            .minor_resolution(4)
                            .major_resolution(48)
                            .build(),
                    )
                })
            })
            .collect(),
        scaffold: meshes.add(crate::render::models::baked::scaffold_mesh()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_rises_from_a_visible_foundation_to_full_height() {
        assert_eq!(site_rise(0.0), SITE_FLOOR);
        assert!(site_rise(0.0) > 0.0, "a founded site is a target, so it must be visible");
        assert!((site_rise(1.0) - 1.0).abs() < 1e-5);
        assert!(site_rise(0.5) > site_rise(0.25));
        assert_eq!(site_rise(2.0), 1.0, "banked work never overshoots the model");
    }

    #[test]
    fn damage_dressing_stages_are_a_pure_function_of_health() {
        assert_eq!(dressing_stage(1.0), 0);
        assert_eq!(dressing_stage(0.6), 0);
        assert_eq!(dressing_stage(0.49), 1);
        assert_eq!(dressing_stage(0.2), 2);
        // the dressing is stamped monotonically and only ever removed by the
        // lifecycle pass noticing the stage FELL — which is only sound while
        // the stage depends on nothing but the current ratio
        assert!(dressing_stage(0.8) < dressing_stage(0.3));
    }

    #[test]
    fn shared_handle_tables_are_indexed_by_discriminant() {
        // buildings[kind * 2 + faction] and aura_rings[kind] are built by
        // mapping BuildingKind::ALL in order; a kind appended out of
        // discriminant order would silently render as its neighbour.
        for (i, k) in BuildingKind::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i, "{k:?} is not at its own discriminant");
        }
        assert!(
            BuildingKind::ALL.iter().any(|k| building_def(*k).aura.is_some()),
            "the per-kind aura ring table would be dead code otherwise"
        );
        for (i, k) in UnitKind::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i, "{k:?} is not at its own discriminant");
        }
    }

    /// The render yaw has to agree with the sim's own compass, or a man swings
    /// at an enemy standing behind him. The sim keeps `heading` as a sixteenth
    /// of a turn counter-clockwise from +X; models face +Z.
    #[test]
    fn render_facing_agrees_with_the_sims_compass() {
        for h in 0..saladin_sim::HEADINGS as u8 {
            let dir = saladin_sim::heading_dir(h);
            let (sx, sz) = (dir.x.to_num::<f32>(), dir.y.to_num::<f32>());
            // where the model's +Z ends up after the yaw
            let q = Quat::from_rotation_y(heading_yaw(h));
            let fwd = q * Vec3::Z;
            assert!((fwd.x - sx).abs() < 1e-3 && (fwd.z - sz).abs() < 1e-3, "heading {h}: {fwd:?}");
        }
        // and it wraps rather than indexing off the end
        assert!((heading_yaw(16) - heading_yaw(0)).abs() < 1e-6);
    }

    /// Every pose the animator can strike must be reachable from sim state the
    /// client actually mirrors: a flag with no writer is a dead branch.
    #[test]
    fn every_animation_flag_has_a_kind_that_can_raise_it() {
        use saladin_sim::unit_def as ud;
        assert!(UnitKind::ALL.iter().any(|k| ud(*k).brace), "nothing braces");
        assert!(
            UnitKind::ALL.iter().any(|k| ud(*k).charge_mult > saladin_sim::Fx::ONE),
            "nothing charges"
        );
        assert!(UnitKind::ALL.iter().any(|k| ud(*k).ranged), "nothing shoots");
        assert!(UnitKind::ALL.iter().any(|k| ud(*k).splash > saladin_sim::Fx::ZERO));
    }
}
