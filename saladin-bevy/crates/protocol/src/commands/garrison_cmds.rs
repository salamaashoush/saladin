use super::{building_occupancy, find_owned};
use crate::components::*;
use crate::WorldConfig;
use bevy_ecs::prelude::*;
use saladin_sim::*;

/// Twelve fixed eject directions (the TS version probed a cos/sin ring; replaced
/// with a constant table for determinism).
const DIRS12: [(Fx, Fx); 12] = [
    (saladin_sim::fx!("1"), saladin_sim::fx!("0")),
    (saladin_sim::fx!("0.87"), saladin_sim::fx!("0.5")),
    (saladin_sim::fx!("0.5"), saladin_sim::fx!("0.87")),
    (saladin_sim::fx!("0"), saladin_sim::fx!("1")),
    (saladin_sim::fx!("-0.5"), saladin_sim::fx!("0.87")),
    (saladin_sim::fx!("-0.87"), saladin_sim::fx!("0.5")),
    (saladin_sim::fx!("-1"), saladin_sim::fx!("0")),
    (saladin_sim::fx!("-0.87"), saladin_sim::fx!("-0.5")),
    (saladin_sim::fx!("-0.5"), saladin_sim::fx!("-0.87")),
    (saladin_sim::fx!("0"), saladin_sim::fx!("-1")),
    (saladin_sim::fx!("0.5"), saladin_sim::fx!("-0.87")),
    (saladin_sim::fx!("0.87"), saladin_sim::fx!("-0.5")),
];

/// Entities of every unit sheltered in `building`, in GameId order (occupancy is
/// derived from `Unit::garrisoned_in` — there is no separate garrison row here).
pub(crate) fn occupants_of(world: &mut World, building: u64) -> Vec<Entity> {
    let mut q = world.query::<(Entity, &GameId, &Unit)>();
    let mut v: Vec<(u64, Entity)> =
        q.iter(world).filter(|(_, _, u)| u.garrisoned_in == building).map(|(e, g, _)| (g.0, e)).collect();
    v.sort_by_key(|(id, _)| *id);
    v.into_iter().map(|(_, e)| e).collect()
}

pub(crate) fn occupant_count(world: &mut World, building: u64) -> i32 {
    let mut q = world.query::<&Unit>();
    q.iter(world).filter(|u| u.garrisoned_in == building).count() as i32
}

/// Snap a unit back onto the field at the host's edge: the nearest passable tile
/// around the structure so ejected occupants never land on water/inside a wall.
fn field_exit(world: &mut World, host_pos: V2, footprint: i32) -> V2 {
    let seed = world.resource::<WorldConfig>().seed;
    let occ = building_occupancy(world, false);
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty));
    let r = Fx::from_num(footprint) / Fx::from_num(2) + saladin_sim::fx!("0.6");
    for (dx, dy) in DIRS12 {
        let px = host_pos.x + dx * r;
        let py = host_pos.y + dy * r;
        if passable(px.to_num::<i32>(), py.to_num::<i32>()) {
            return nearest_passable_grid(&passable, px, py);
        }
    }
    nearest_passable_grid(&passable, host_pos.x, host_pos.y)
}

/// Shelter one of the caller's units inside one of the caller's structures. The
/// unit leaves the field (movement/combat loops skip it, the client hides it)
/// and, if ranged, lends fire to the host. Mirrors the `garrisonUnit` reducer.
pub(crate) fn garrison(world: &mut World, owner: u64, unit: u64, building: u64) {
    let Some(ue) = find_owned(world, owner, unit) else { return };
    let Some(be) = find_owned(world, owner, building) else { return };
    let (ukind, already) = match world.get::<Unit>(ue) {
        Some(u) => (u.kind, u.garrisoned_in != 0),
        None => return,
    };
    if already {
        return; // benign repeat
    }
    let Some(bkind) = world.get::<Building>(be).map(|b| b.kind) else { return };
    if !can_garrison(unit_def(ukind)) {
        return;
    }
    let bdef = building_def(bkind);
    if garrison_free_slots(bdef, occupant_count(world, building)) <= 0 {
        return;
    }
    if let Some(mut u) = world.get_mut::<Unit>(ue) {
        u.garrisoned_in = building;
        u.has_target = false;
        u.path = vec![];
        u.path_idx = 0;
        u.attack_target = 0;
        u.gather_state = GatherState::Idle;
        u.target_node = 0;
    }
}

/// Empty a structure: pop every occupant back onto the field at the host edge.
pub(crate) fn ungarrison(world: &mut World, owner: u64, building: u64) {
    if find_owned(world, owner, building).is_none() {
        return;
    }
    eject_all(world, building);
}

/// Return every occupant of `building` to the field. Used by ungarrison and by
/// voluntary demolition (occupants always survive a demolish).
pub(crate) fn eject_all(world: &mut World, building: u64) {
    let host = {
        let mut q = world.query::<(&GameId, &Pos, &Building)>();
        q.iter(world).find(|(g, _, _)| g.0 == building).map(|(_, p, b)| (p.pos, building_def(b.kind).footprint))
    };
    let Some((host_pos, footprint)) = host else { return };
    for e in occupants_of(world, building) {
        let exit = field_exit(world, host_pos, footprint);
        if let Some(mut pos) = world.get_mut::<Pos>(e) {
            pos.pos = exit;
        }
        if let Some(mut u) = world.get_mut::<Unit>(e) {
            u.garrisoned_in = 0;
            u.has_target = false;
            u.path = vec![];
            u.path_idx = 0;
            u.attack_target = 0;
            u.home = exit;
        }
    }
}
