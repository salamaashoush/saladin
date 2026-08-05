//! Pointer + keyboard input (port of SaladinGame.ts bindEvents/onPointer*/
//! command/commitBuild/demolish + input.ts lineTiles/formation). Turns clicks
//! into lockstep `PlayerCommand`s — never mutates sim state directly.

use crate::camera::{GameCamera, pick_ground};
use crate::selection::{ControlGroups, Selection};
use crate::terrain::{HeightField, height_at};
use crate::ui::hud::HudRects;
use crate::{LocalInput, LocalPlayer};
use bevy::prelude::*;
use saladin_protocol::{Building, GameId, Owner, Player, PlayerCommand, Pos, ResourceNode, Unit};
use saladin_sim::{
    BuildingKind, Fx, GatherState, PlaceError, Stockpile, V2, building_def, can_garrison,
    can_host_garrison,
    check_build, footprint_center, garrison_free_slots, occupancy_set, operational, tile_key,
    unit_def,
};
use std::collections::HashSet;

pub const MAX_WALL_LEN: i32 = 40;

/// What the pointer currently does. Build/Demolish come from the build bar.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    #[default]
    Normal,
    Build(BuildingKind),
    Demolish,
}

#[derive(Resource, Default)]
pub struct DragState {
    pub start: Option<Vec2>,
    pub dragging: bool,
}

#[derive(Resource, Default)]
pub struct WallDrag(pub Option<(i32, i32)>);

/// Ghost orientation in quarter turns; R cycles it while placing a building.
#[derive(Resource, Default, Clone, Copy)]
pub struct GhostRot(pub u8);

/// Why the ghost under the cursor is red, published by the placement preview so
/// the mode hint can say it out loud instead of showing one silent red box for
/// ten different refusals.
#[derive(Resource, Default, Clone, Copy)]
pub struct PlaceHint(pub Option<PlaceError>);

#[derive(Resource, Default)]
pub struct DemolishDrag {
    pub painting: bool,
    pub done: HashSet<u64>,
}

/// Double-click detection for select-all-of-kind.
#[derive(Resource, Default)]
pub struct LastClick {
    pub at: f64,
    pub pos: Vec2,
}

/// The on-screen selection rectangle (a UI node toggled during drags).
#[derive(Component)]
pub struct DragBoxUi;

// ── small helpers (ports of input.ts) ────────────────────────────────────────

/// Straight tile line from s to e along the dominant axis (clamped length).
/// Orthogonally-connected staircase line from `s` to `e` (Bresenham with
/// either-or steps, never diagonal jumps) — walls drag in ANY direction and
/// still seal, because every consecutive pair shares an edge.
pub fn line_tiles(s: (i32, i32), e: (i32, i32)) -> Vec<(i32, i32)> {
    let mut out = vec![s];
    let (mut x, mut y) = s;
    let dx = (e.0 - s.0).abs();
    let dy = (e.1 - s.1).abs();
    let sx = (e.0 - s.0).signum();
    let sy = (e.1 - s.1).signum();
    let mut err = dx - dy;
    while (x, y) != e && out.len() <= MAX_WALL_LEN as usize {
        if 2 * err > -dy && x != e.0 {
            err -= dy;
            x += sx;
        } else if y != e.1 {
            err += dx;
            y += sy;
        } else {
            break;
        }
        out.push((x, y));
    }
    out
}

/// Grid offsets spreading `n` move targets around a click so units don't stack.
pub fn formation(n: usize) -> Vec<(f32, f32)> {
    let cols = (n as f32).sqrt().ceil().max(1.0) as usize;
    let rows = n.div_ceil(cols);
    let s = 0.85_f32;
    (0..n)
        .map(|i| {
            let c = (i % cols) as f32;
            let r = (i / cols) as f32;
            (
                (c - (cols as f32 - 1.0) / 2.0) * s,
                (r - (rows as f32 - 1.0) / 2.0) * s,
            )
        })
        .collect()
}

fn unit_world(field: &HeightField, p: V2) -> Vec3 {
    let x = p.x.to_num::<f32>();
    let z = p.y.to_num::<f32>();
    Vec3::new(x, height_at(field, x, z) + 0.5, z)
}

/// What a click resolves to, in priority order.
enum Picked {
    Unit(u64, u64),     // id, owner
    Building(u64, u64), // id, owner
    Node(u64),
    Ground(Vec3),
}

/// Resolve the cursor: nearest own/enemy unit blob on screen, else a building
/// whose footprint contains the ground point, else a node within reach, else
/// bare ground. (The TS client raycast meshes; screen-space + footprint tests
/// give the same result without a picking BVH.)
#[allow(clippy::too_many_arguments)]
fn pick(
    cursor: Vec2,
    camera: &Camera,
    cam_tf: &GlobalTransform,
    field: Option<&HeightField>,
    q_units: &Query<(&GameId, &Owner, &Pos, &Unit)>,
    q_buildings: &Query<(&GameId, &Owner, &Pos, &Building)>,
    q_nodes: &Query<(&GameId, &Pos, &ResourceNode)>,
) -> Option<Picked> {
    // units first — small targets beat big footprints
    let mut best: Option<(u64, u64)> = None;
    let mut bd = 18.0_f32;
    if let Some(f) = field {
        for (g, o, p, u) in q_units {
            if u.garrisoned_in != 0 {
                continue;
            }
            if let Ok(sp) = camera.world_to_viewport(cam_tf, unit_world(f, p.pos)) {
                let d = sp.distance(cursor);
                if d < bd {
                    bd = d;
                    best = Some((g.0, o.0));
                }
            }
        }
    }
    if let Some((id, o)) = best {
        return Some(Picked::Unit(id, o));
    }

    let ground = pick_ground(camera, cam_tf, cursor, field)?;
    let (gx, gz) = (ground.x, ground.z);

    for (g, o, p, b) in q_buildings {
        let half = building_def(b.kind).footprint as f32 / 2.0;
        let bx = p.pos.x.to_num::<f32>();
        let bz = p.pos.y.to_num::<f32>();
        if (gx - bx).abs() <= half && (gz - bz).abs() <= half {
            return Some(Picked::Building(g.0, o.0));
        }
    }
    for (g, p, _) in q_nodes {
        let nx = p.pos.x.to_num::<f32>();
        let nz = p.pos.y.to_num::<f32>();
        if (gx - nx).hypot(gz - nz) <= 0.8 {
            return Some(Picked::Node(g.0));
        }
    }
    Some(Picked::Ground(ground))
}

/// Voice of the selection: the first selected unit answers for the group.
fn selected_kind(
    selection: &Selection,
    q_units: &Query<(&GameId, &Owner, &Pos, &Unit)>,
) -> Option<saladin_sim::UnitKind> {
    let id = selection.units.iter().min()?;
    q_units.iter().find(|(g, ..)| g.0 == *id).map(|(_, _, _, u)| u.kind)
}

/// Everything `check_build` needs, gathered once — the client's mirror of the
/// sim's `BuildContext`. Built HERE and nowhere else so the ghost preview, the
/// mode hint and the command that ships all answer from one gathering: a
/// preview can never turn green on a placement the command will refuse.
///
/// A SITE counts toward the per-kind limit but satisfies no prereq, because an
/// unfinished barracks trains nothing.
pub struct BuildProbe {
    occ: HashSet<i32>,
    own: Vec<V2>,
    owned_kinds: HashSet<BuildingKind>,
    counts: [i32; 16],
    pub stock: Stockpile,
    seed: u32,
}

impl BuildProbe {
    pub fn check(&self, kind: BuildingKind, cx: f32, cy: f32) -> Result<(), PlaceError> {
        let occupied = |tx: i32, ty: i32| self.occ.contains(&tile_key(tx, ty));
        check_build(
            self.seed,
            kind,
            Fx::from_num(cx),
            Fx::from_num(cy),
            occupied,
            &self.own,
            &self.owned_kinds,
            &self.counts,
            &self.stock,
        )
    }
}

/// `buildings` yields (pos, kind, owner, state-is-operational); `nodes` yields
/// node positions. Own wall tiles are transparent to a composing gate or tower
/// — the sim absorbs the segment on build, so the preview must agree.
pub fn build_probe(
    kind: BuildingKind,
    me: u64,
    seed: u32,
    stock: Stockpile,
    buildings: impl Iterator<Item = (V2, BuildingKind, u64, bool)>,
    nodes: impl Iterator<Item = V2>,
) -> BuildProbe {
    let composes = saladin_sim::composes_with_walls(kind);
    let mut occ_list = Vec::new();
    let mut own = Vec::new();
    let mut owned_kinds = HashSet::new();
    let mut counts = [0i32; 16];
    let mut own_walls = Vec::new();
    for (pos, k, owner, live) in buildings {
        occ_list.push(saladin_sim::Occupant { kind: k, pos });
        if owner != me {
            continue;
        }
        own.push(pos);
        counts[k as usize] += 1;
        if live {
            owned_kinds.insert(k);
        }
        if k == BuildingKind::Wall {
            own_walls.push(tile_key(pos.x.to_num::<i32>(), pos.y.to_num::<i32>()));
        }
    }
    let mut occ = occupancy_set(&occ_list, true);
    for p in nodes {
        occ.insert(tile_key(p.x.to_num::<i32>(), p.y.to_num::<i32>()));
    }
    if composes {
        for k in own_walls {
            occ.remove(&k);
        }
    }
    BuildProbe { occ, own, owned_kinds, counts, stock, seed }
}

/// Placement cells under the cursor: one footprint, or the dragged wall line.
pub fn build_cells(kind: BuildingKind, hx: f32, hz: f32, wall_drag: Option<(i32, i32)>) -> Vec<(f32, f32)> {
    if kind == BuildingKind::Wall {
        let hov = (hx.floor() as i32, hz.floor() as i32);
        let tiles = match wall_drag {
            Some(s) => line_tiles(s, hov),
            None => vec![hov],
        };
        return tiles.iter().map(|&(tx, ty)| (tx as f32 + 0.5, ty as f32 + 0.5)).collect();
    }
    let def = building_def(kind);
    let c = footprint_center(def.footprint, Fx::from_num(hx), Fx::from_num(hz));
    vec![(c.x.to_num::<f32>(), c.y.to_num::<f32>())]
}

/// `build_probe` fed from the pointer system's own queries.
fn probe_from_world(
    kind: BuildingKind,
    me: u64,
    seed: u32,
    q_players: &Query<&Player>,
    q_buildings: &Query<(&GameId, &Owner, &Pos, &Building)>,
    q_nodes: &Query<(&GameId, &Pos, &ResourceNode)>,
) -> BuildProbe {
    let stock = q_players.iter().find(|p| p.player_id == me).map(|p| p.stock).unwrap_or_default();
    build_probe(
        kind,
        me,
        seed,
        stock,
        q_buildings.iter().map(|(_, o, p, b)| (p.pos, b.kind, o.0, operational(b.state))),
        q_nodes.iter().map(|(_, p, _)| p.pos),
    )
}

// ── the main pointer system ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn pointer_input(
    (mouse, keys, time): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>, Res<Time>),
    windows: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    field: Option<Res<HeightField>>,
    local: Res<LocalPlayer>,
    cfg: Res<saladin_protocol::WorldConfig>,
    (mut mode, mut input, mut selection): (ResMut<InputMode>, ResMut<LocalInput>, ResMut<Selection>),
    mut voice: ResMut<crate::audio::VoiceQueue>,
    ghost_rot: Res<GhostRot>,
    (mut drag, mut wall_drag, mut demolish, mut last_click): (
        ResMut<DragState>,
        ResMut<WallDrag>,
        ResMut<DemolishDrag>,
        ResMut<LastClick>,
    ),
    hud: Res<HudRects>,
    q_units: Query<(&GameId, &Owner, &Pos, &Unit)>,
    q_buildings: Query<(&GameId, &Owner, &Pos, &Building)>,
    q_nodes: Query<(&GameId, &Pos, &ResourceNode)>,
    q_players: Query<&Player>,
) {
    let me = local.0;
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let field_ref = field.as_deref();
    // the HUD's OWN measured rects, not a guessed band
    let on_hud = hud.hit(cursor);

    // ── demolish mode ─────────────────────────────────────────────────────────
    if *mode == InputMode::Demolish {
        if mouse.just_pressed(MouseButton::Right) || keys.just_pressed(KeyCode::Escape) {
            *mode = InputMode::Normal;
            return;
        }
        if mouse.just_pressed(MouseButton::Left) && !on_hud {
            demolish.painting = true;
            demolish.done.clear();
        }
        if mouse.just_released(MouseButton::Left) {
            demolish.painting = false;
        }
        if demolish.painting {
            if let Some(Picked::Building(id, owner)) =
                pick(cursor, camera, cam_tf, field_ref, &q_units, &q_buildings, &q_nodes)
            {
                if owner == me && !demolish.done.contains(&id) {
                    demolish.done.insert(id);
                    input.0.push(PlayerCommand::Demolish { player_id: me, building: id });
                }
            }
        }
        return;
    }

    // ── build mode ────────────────────────────────────────────────────────────
    if let InputMode::Build(kind) = *mode {
        if mouse.just_pressed(MouseButton::Right) || keys.just_pressed(KeyCode::Escape) {
            *mode = InputMode::Normal;
            wall_drag.0 = None;
            return;
        }
        let ground = pick_ground(camera, cam_tf, cursor, field_ref);
        if mouse.just_pressed(MouseButton::Left)
            && !on_hud
            && let Some(g) = ground
        {
            if kind == BuildingKind::Wall {
                wall_drag.0 = Some((g.x.floor() as i32, g.z.floor() as i32));
            } else {
                let crew = selected_builders(&selection, &q_units, me, Vec2::new(g.x, g.z));
                let probe = probe_from_world(kind, me, cfg.seed, &q_players, &q_buildings, &q_nodes);
                commit_build(kind, g.x, g.z, None, me, &probe, ghost_rot.0, &crew, &mut input);
            }
        }
        if mouse.just_released(MouseButton::Left)
            && kind == BuildingKind::Wall
            && let (Some(start), Some(g)) = (wall_drag.0.take(), ground)
        {
            let crew = selected_builders(
                &selection,
                &q_units,
                me,
                Vec2::new(start.0 as f32 + 0.5, start.1 as f32 + 0.5),
            );
            let probe = probe_from_world(kind, me, cfg.seed, &q_players, &q_buildings, &q_nodes);
            commit_build(kind, g.x, g.z, Some(start), me, &probe, ghost_rot.0, &crew, &mut input);
        }
        return;
    }

    // ── normal mode ───────────────────────────────────────────────────────────
    // right-click: rally (building selected) or context command
    if mouse.just_pressed(MouseButton::Right) && !on_hud {
        if let Some(bid) = selection.building {
            if let Some(g) = pick_ground(camera, cam_tf, cursor, field_ref) {
                input.0.push(PlayerCommand::SetRally {
                    player_id: me,
                    building: bid,
                    target: V2::new(Fx::from_num(g.x), Fx::from_num(g.z)),
                });
            }
            return;
        }
        if selection.units.is_empty() {
            return;
        }
        match pick(cursor, camera, cam_tf, field_ref, &q_units, &q_buildings, &q_nodes) {
            Some(Picked::Unit(target, owner)) if owner != me => {
                command_attack(&selection, &q_units, me, target, &mut input);
                if let Some(k) = selected_kind(&selection, &q_units) {
                    voice.0.push((k, crate::audio::Bark::Attack));
                }
            }
            Some(Picked::Node(node)) => {
                let mut any = false;
                for &id in &selection.units {
                    if let Some((_, _, _, u)) = q_units.iter().find(|(g, ..)| g.0 == id) {
                        if unit_def(u.kind).carry > 0 {
                            input.0.push(PlayerCommand::Gather { player_id: me, unit: id, node });
                            any = true;
                        }
                    }
                }
                if any {
                    let bark = match q_nodes.iter().find(|(g, ..)| g.0 == node).map(|(_, _, n)| n.res_type) {
                        Some(saladin_sim::ResourceType::Wood) => crate::audio::Bark::Wood,
                        Some(saladin_sim::ResourceType::Food) => crate::audio::Bark::Food,
                        Some(saladin_sim::ResourceType::Stone) => crate::audio::Bark::Stone,
                        Some(saladin_sim::ResourceType::Gold) => crate::audio::Bark::Gold,
                        None => crate::audio::Bark::Ack,
                    };
                    voice.0.push((saladin_sim::UnitKind::Peasant, bark));
                }
            }
            Some(Picked::Building(target, owner)) => {
                if owner != me {
                    command_attack(&selection, &q_units, me, target, &mut input);
                    if let Some(k) = selected_kind(&selection, &q_units) {
                        voice.0.push((k, crate::audio::Bark::Attack));
                    }
                } else {
                    // hands first: a structure that is unfinished, rising or
                    // hurt takes the peasants in the selection and puts them on
                    // it — the same right-click that gathers a tree
                    let row = q_buildings.iter().find(|(g, ..)| g.0 == target).map(|(_, _, _, b)| *b);
                    let mask = q_players.iter().find(|p| p.player_id == me).map(|p| p.tech_mask).unwrap_or(0);
                    let wants_work = row.is_some_and(|b| {
                        !operational(b.state)
                            || b.state == saladin_sim::BuildState::Upgrading
                            || b.hp < saladin_sim::effective_building_def(b.kind, mask).max_hp
                    });
                    let mut crew: HashSet<u64> = HashSet::new();
                    if wants_work {
                        let mut ids: Vec<u64> = selection.units.iter().copied().collect();
                        ids.sort_unstable();
                        for id in ids {
                            if crew.len() >= saladin_sim::MAX_BUILDERS as usize {
                                break;
                            }
                            if let Some((_, _, _, u)) = q_units.iter().find(|(g, ..)| g.0 == id)
                                && unit_def(u.kind).carry > 0
                                && u.garrisoned_in == 0
                            {
                                input.0.push(PlayerCommand::Repair { player_id: me, unit: id, building: target });
                                crew.insert(id);
                            }
                        }
                        if !crew.is_empty() {
                            voice.0.push((saladin_sim::UnitKind::Peasant, crate::audio::Bark::Ack));
                        }
                    }
                    let host = row.map(|b| building_def(b.kind)).filter(|d| can_host_garrison(d));
                    if let Some(def) = host {
                        let occupants =
                            q_units.iter().filter(|(_, _, _, u)| u.garrisoned_in == target).count() as i32;
                        let mut free = garrison_free_slots(def, occupants);
                        let mut any = false;
                        for &id in &selection.units {
                            if free <= 0 {
                                break;
                            }
                            // a peasant already sent to the hammer is not also
                            // sent inside — the later order would just win
                            if crew.contains(&id) {
                                continue;
                            }
                            if let Some((_, _, _, u)) = q_units.iter().find(|(g, ..)| g.0 == id)
                                && can_garrison(unit_def(u.kind))
                            {
                                input.0.push(PlayerCommand::Garrison { player_id: me, unit: id, building: target });
                                free -= 1;
                                any = true;
                            }
                        }
                        if !any
                            && crew.is_empty()
                            && let Some((_, _, p, _)) =
                                q_buildings.iter().find(|(g, ..)| g.0 == target)
                        {
                            command_move(&selection, me, p.pos.x.to_num(), p.pos.y.to_num(), &mut input);
                        }
                    } else if crew.is_empty()
                        && let Some((_, _, p, _)) = q_buildings.iter().find(|(g, ..)| g.0 == target)
                    {
                        command_move(&selection, me, p.pos.x.to_num(), p.pos.y.to_num(), &mut input);
                    }
                }
            }
            Some(Picked::Ground(g)) => {
                command_move(&selection, me, g.x, g.z, &mut input);
                if let Some(k) = selected_kind(&selection, &q_units) {
                    voice.0.push((k, crate::audio::Bark::Ack));
                }
            }
            _ => {}
        }
        return;
    }

    // left: drag-select / click-pick / double-click same-kind
    if mouse.just_pressed(MouseButton::Left) && !on_hud {
        drag.start = Some(cursor);
        drag.dragging = false;
    }
    if let Some(start) = drag.start {
        if mouse.pressed(MouseButton::Left) && !drag.dragging && start.distance(cursor) > 4.0 {
            drag.dragging = true;
        }
    }
    if mouse.just_released(MouseButton::Left) {
        let Some(start) = drag.start.take() else { return };
        let additive = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if drag.dragging {
            // box select own field units
            let (lo, hi) = (start.min(cursor), start.max(cursor));
            if !additive {
                selection.units.clear();
            }
            selection.building = None;
            if let Some(f) = field_ref {
                for (g, o, p, u) in &q_units {
                    if o.0 != me || u.garrisoned_in != 0 {
                        continue;
                    }
                    if let Ok(sp) = camera.world_to_viewport(cam_tf, unit_world(f, p.pos)) {
                        if sp.x >= lo.x && sp.x <= hi.x && sp.y >= lo.y && sp.y <= hi.y {
                            selection.units.insert(g.0);
                        }
                    }
                }
            }
        } else {
            let now = time.elapsed_secs_f64();
            let double = now - last_click.at < 0.35 && last_click.pos.distance(cursor) < 8.0;
            last_click.at = now;
            last_click.pos = cursor;
            match pick(cursor, camera, cam_tf, field_ref, &q_units, &q_buildings, &q_nodes) {
                Some(Picked::Unit(id, owner)) if owner == me => {
                    selection.building = None;
                    if double {
                        // select every own unit of the same kind on screen
                        let kind = q_units.iter().find(|(g, ..)| g.0 == id).map(|(_, _, _, u)| u.kind);
                        if let (Some(kind), Some(f)) = (kind, field_ref) {
                            selection.units.clear();
                            for (g, o, p, u) in &q_units {
                                if o.0 != me || u.kind != kind || u.garrisoned_in != 0 {
                                    continue;
                                }
                                if let Ok(sp) = camera.world_to_viewport(cam_tf, unit_world(f, p.pos)) {
                                    if sp.x >= 0.0
                                        && sp.y >= 0.0
                                        && sp.x <= window.width()
                                        && sp.y <= window.height()
                                    {
                                        selection.units.insert(g.0);
                                    }
                                }
                            }
                        }
                    } else {
                        if !additive {
                            selection.units.clear();
                        }
                        selection.units.insert(id);
                    }
                }
                Some(Picked::Building(id, owner)) if owner == me => {
                    selection.units.clear();
                    selection.building = Some(id);
                }
                _ => {
                    if !additive {
                        selection.units.clear();
                        selection.building = None;
                    }
                }
            }
        }
        drag.dragging = false;
    }
}

fn command_attack(
    selection: &Selection,
    q_units: &Query<(&GameId, &Owner, &Pos, &Unit)>,
    me: u64,
    target: u64,
    input: &mut LocalInput,
) {
    for &id in &selection.units {
        if let Some((_, _, _, u)) = q_units.iter().find(|(g, ..)| g.0 == id) {
            if unit_def(u.kind).attack > 0 {
                input.0.push(PlayerCommand::Attack { player_id: me, unit: id, target });
            }
        }
    }
}

fn command_move(selection: &Selection, me: u64, gx: f32, gz: f32, input: &mut LocalInput) {
    let ids: Vec<u64> = {
        let mut v: Vec<u64> = selection.units.iter().copied().collect();
        v.sort();
        v
    };
    let offs = formation(ids.len());
    for (i, id) in ids.iter().enumerate() {
        let (ox, oz) = offs[i];
        input.0.push(PlayerCommand::Move {
            player_id: me,
            unit: *id,
            target: V2::new(Fx::from_num(gx + ox), Fx::from_num(gz + oz)),
        });
    }
}

/// R rotates the placement ghost a quarter turn (build mode, non-wall).
pub fn rotate_ghost(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<InputMode>,
    mut rot: ResMut<GhostRot>,
) {
    if matches!(*mode, InputMode::Build(k) if k != BuildingKind::Wall)
        && keys.just_pressed(KeyCode::KeyR)
    {
        rot.0 = (rot.0 + 1) % 4;
    }
}

/// The crew a build order ships with: the peasants the player has selected, or
/// — when none are — the nearest hands not already on a job.
///
/// The fallback is not a nicety. A site is paid for in FULL the tick it is
/// founded and does nothing at all until a hammer reaches it, so an order sent
/// with an empty crew spends the whole cost on a foundation that never rises
/// and says so only if the player thinks to click it. The bot already staffs
/// every site it pays for (`staff_jobs`) and the command card's Send Builders
/// already picks by the same rule; this is the third caller of one policy, not
/// a new one. Ids ride in the command, so every peer still agrees.
fn selected_builders(
    selection: &Selection,
    q_units: &Query<(&GameId, &Owner, &Pos, &Unit)>,
    me: u64,
    at: Vec2,
) -> Vec<u64> {
    let hands = || {
        q_units.iter().filter(move |(_, o, _, u)| {
            o.0 == me && u.garrisoned_in == 0 && unit_def(u.kind).carry > 0
        })
    };
    let mut ids: Vec<u64> =
        hands().filter(|(g, ..)| selection.units.contains(&g.0)).map(|(g, ..)| g.0).collect();
    if !ids.is_empty() {
        ids.sort_unstable();
        ids.truncate(saladin_sim::MAX_BUILDERS as usize);
        return ids;
    }
    let here = V2::new(Fx::from_num(at.x), Fx::from_num(at.y));
    let mut idle: Vec<(Fx, u64)> = hands()
        .filter(|(_, _, _, u)| {
            matches!(u.gather_state, GatherState::Idle | GatherState::ToResource) && u.job_site == 0
        })
        .map(|(g, _, p, _)| (saladin_sim::dist2(p.pos, here), g.0))
        .collect();
    idle.sort_unstable();
    idle.truncate(DEFAULT_CREW);
    idle.into_iter().map(|(_, id)| id).collect()
}

/// Hands pulled off the fields when a build order names none. Small on purpose:
/// enough that the site rises, few enough that the economy notices it left.
const DEFAULT_CREW: usize = 2;

#[allow(clippy::too_many_arguments)]
fn commit_build(
    kind: BuildingKind,
    hx: f32,
    hz: f32,
    wall_start: Option<(i32, i32)>,
    me: u64,
    probe: &BuildProbe,
    facing: u8,
    builders: &[u64],
    input: &mut LocalInput,
) {
    let cells = build_cells(kind, hx, hz, wall_start);
    if kind == BuildingKind::Wall {
        // send the whole dragged line — the sim re-validates per segment with
        // the chain-extended anchor set the client cannot predict
        let tiles: Vec<(i32, i32)> =
            cells.iter().map(|&(cx, cy)| (cx.floor() as i32, cy.floor() as i32)).collect();
        if !tiles.is_empty() {
            input.0.push(PlayerCommand::PlaceWall {
                player_id: me,
                tiles,
                builders: builders.to_vec(),
            });
        }
        return;
    }
    for (cx, cy) in cells {
        if probe.check(kind, cx, cy).is_ok() {
            input.0.push(PlayerCommand::Build {
                player_id: me,
                kind,
                pos: V2::new(Fx::from_num(cx), Fx::from_num(cy)),
                facing,
                builders: builders.to_vec(),
            });
        }
    }
}

// ── keyboard: control groups + mode cancel ───────────────────────────────────

pub fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut groups: ResMut<ControlGroups>,
    mut selection: ResMut<Selection>,
) {
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    for (i, key) in DIGITS.iter().enumerate() {
        if !keys.just_pressed(*key) {
            continue;
        }
        if ctrl {
            groups.0[i + 1] = selection.units.iter().copied().collect();
        } else if !groups.0[i + 1].is_empty() {
            selection.building = None;
            selection.units = groups.0[i + 1].iter().copied().collect();
        }
    }
}

/// Sync the on-screen drag rectangle with the active drag.
pub fn update_drag_box(
    drag: Res<DragState>,
    windows: Query<&Window>,
    mut q: Query<(&mut Node, &mut Visibility), With<DragBoxUi>>,
) {
    let Ok((mut node, mut vis)) = q.single_mut() else { return };
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    match (drag.start, cursor, drag.dragging) {
        (Some(start), Some(c), true) => {
            let lo = start.min(c);
            let hi = start.max(c);
            node.left = Val::Px(lo.x);
            node.top = Val::Px(lo.y);
            node.width = Val::Px(hi.x - lo.x);
            node.height = Val::Px(hi.y - lo.y);
            *vis = Visibility::Visible;
        }
        _ => *vis = Visibility::Hidden,
    }
}

/// One world-spawn for the drag rectangle UI node.
pub fn spawn_drag_box(mut commands: Commands) {
    commands.spawn((
        DragBoxUi,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::srgb_u8(0xff, 0xec, 0x80)),
        BackgroundColor(Color::srgba(1.0, 0.93, 0.5, 0.15)),
        Visibility::Hidden,
        ZIndex(5),
        Pickable::IGNORE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use saladin_sim::Stockpile;

    #[test]
    fn wall_lines_go_any_direction_and_stay_connected() {
        for end in [(10, 4), (4, 10), (-6, 3), (7, -7), (-5, -9), (0, 8), (9, 0)] {
            let tiles = line_tiles((0, 0), end);
            assert_eq!(*tiles.first().unwrap(), (0, 0));
            assert_eq!(*tiles.last().unwrap(), end, "line reaches {end:?}");
            for w in tiles.windows(2) {
                let (a, b) = (w[0], w[1]);
                let d = (a.0 - b.0).abs() + (a.1 - b.1).abs();
                assert_eq!(d, 1, "orthogonally connected (seals): {a:?} -> {b:?}");
            }
        }
    }

    #[test]
    fn wall_line_is_capped() {
        let tiles = line_tiles((0, 0), (500, 500));
        assert!(tiles.len() <= super::MAX_WALL_LEN as usize + 1);
    }

    /// The ghost asks the SAME gate the command does, from the same gathering:
    /// a hole in the ground is not a barracks, so it unlocks nothing.
    #[test]
    fn a_site_previews_as_locked_until_it_is_finished() {
        let rich = Stockpile { wood: 999, stone: 999, food: 999, gold: 999 };
        let at = V2::new(Fx::from_num(10), Fx::from_num(10));
        let unfinished =
            build_probe(BuildingKind::Stable, 1, 1, rich, [(at, BuildingKind::Barracks, 1, false)].into_iter(), [].into_iter());
        assert_eq!(
            unfinished.check(BuildingKind::Stable, 11.5, 10.5),
            Err(PlaceError::MissingPrereq(BuildingKind::Barracks)),
        );
        // and a finished one gets past the prereq gate (terrain may still say no)
        let finished =
            build_probe(BuildingKind::Stable, 1, 1, rich, [(at, BuildingKind::Barracks, 1, true)].into_iter(), [].into_iter());
        assert_ne!(
            finished.check(BuildingKind::Stable, 11.5, 10.5),
            Err(PlaceError::MissingPrereq(BuildingKind::Barracks)),
        );
    }

    /// The Watchtower is EARNED on a standing Tower, never bought — the ghost
    /// must refuse it the same way the command does.
    #[test]
    fn the_watchtower_cannot_be_sited() {
        let rich = Stockpile { wood: 999, stone: 999, food: 999, gold: 999 };
        let probe = build_probe(BuildingKind::Watchtower, 1, 1, rich, [].into_iter(), [].into_iter());
        assert_eq!(probe.check(BuildingKind::Watchtower, 10.5, 10.5), Err(PlaceError::NotBuildable));
    }
}
