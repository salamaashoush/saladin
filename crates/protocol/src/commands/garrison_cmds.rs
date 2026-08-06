use super::{find_owned, occupancy_and_gates};
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
fn field_exit(world: &mut World, owner: u64, host_pos: V2, footprint: i32) -> V2 {
    let seed = world.resource::<WorldConfig>().seed;
    let (occ, gates) = occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
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
    let Some(state) = world.get::<Building>(be).map(|b| b.state) else { return };
    if !operational(state) {
        return; // a foundation shelters nobody
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

/// Take a landing party aboard. The hold is `Unit::garrisoned_in` pointed at a
/// hull, so a passenger leaves the field exactly the way a tower garrison does —
/// and `carry_cargo` then keeps its `Pos` on the hull, which is what makes every
/// other reader of a position (supply, foraging, desertion, the state hash) come
/// out right without a special case anywhere.
pub(crate) fn embark(world: &mut World, owner: u64, units: &[u64], boat: u64) {
    let Some(be) = find_owned(world, owner, boat) else { return };
    let cap = match world.get::<Unit>(be) {
        Some(u) if u.garrisoned_in == 0 => unit_def(u.kind).cargo_cap,
        _ => return,
    };
    if cap <= 0 {
        return;
    }
    let Some(bpos) = world.get::<Pos>(be).map(|p| p.pos) else { return };
    let mut aboard = occupant_count(world, boat);
    let mut want: Vec<u64> = units.to_vec();
    want.sort_unstable();
    want.dedup();
    for id in want {
        if aboard >= cap {
            break;
        }
        if id == boat {
            continue;
        }
        let Some(ue) = find_owned(world, owner, id) else { continue };
        // A hull is not freight. Nesting one would give `carry_cargo` a cycle to
        // walk and the drowning pass a chain to follow.
        let ok = match world.get::<Unit>(ue) {
            Some(u) => u.garrisoned_in == 0 && unit_def(u.kind).domain == Domain::Land,
            None => false,
        };
        if !ok {
            continue;
        }
        let Some(upos) = world.get::<Pos>(ue).map(|p| p.pos) else { continue };
        if dist(upos, bpos) > EMBARK_RANGE {
            continue;
        }
        if let Some(mut pos) = world.get_mut::<Pos>(ue) {
            pos.pos = bpos;
        }
        if let Some(mut u) = world.get_mut::<Unit>(ue) {
            u.garrisoned_in = boat;
            u.has_target = false;
            u.path = vec![];
            u.path_idx = 0;
            u.attack_target = 0;
            u.gather_state = GatherState::Idle;
            u.target_node = 0;
            u.job_site = 0;
            u.home = bpos;
        }
        aboard += 1;
    }
}

/// Put the party ashore. The landing is the legal land tile nearest `target`
/// within `LANDING_REACH` of the hull — no harbour at the far end, which is what
/// makes a beach a second front instead of a docking manoeuvre.
pub(crate) fn disembark(world: &mut World, owner: u64, boat: u64, target: V2) {
    let Some(be) = find_owned(world, owner, boat) else { return };
    if world.get::<Unit>(be).is_none_or(|u| unit_def(u.kind).cargo_cap <= 0) {
        return;
    }
    let Some(bpos) = world.get::<Pos>(be).map(|p| p.pos) else { return };
    let riders = occupants_of(world, boat);
    if riders.is_empty() {
        return;
    }
    let Some(shore) = landing_spot(world, owner, bpos, target) else { return };
    for e in riders {
        if let Some(mut pos) = world.get_mut::<Pos>(e) {
            pos.pos = shore;
        }
        if let Some(mut u) = world.get_mut::<Unit>(e) {
            u.garrisoned_in = 0;
            u.has_target = false;
            u.path = vec![];
            u.path_idx = 0;
            u.attack_target = 0;
            u.home = shore;
        }
    }
}

/// The land tile within `LANDING_REACH` of the hull that lies nearest `target`,
/// ties broken by tile key. `nearest_passable_grid` alone would happily answer
/// with the far side of a headland — a landing has to be off THIS hull.
fn landing_spot(world: &mut World, owner: u64, from: V2, target: V2) -> Option<V2> {
    let seed = world.resource::<WorldConfig>().seed;
    let (occ, gates) = occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
    let r = LANDING_REACH;
    let reach2 = Fx::from_num(r) * Fx::from_num(r);
    let (bx, by) = (from.x.to_num::<i32>(), from.y.to_num::<i32>());
    let half = saladin_sim::fx!("0.5");
    let mut best: Option<(Fx, i32, V2)> = None;
    for dy in -r..=r {
        for dx in -r..=r {
            let (tx, ty) = (bx + dx, by + dy);
            if !passable(tx, ty) {
                continue;
            }
            let c = V2::new(Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            if dist2(from, c) > reach2 {
                continue;
            }
            let d = dist2(c, target);
            let k = tile_key(tx, ty);
            match best {
                Some((bd, bk, _)) if d > bd || (d == bd && k >= bk) => {}
                _ => best = Some((d, k, c)),
            }
        }
    }
    best.map(|(_, _, c)| c)
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
        let mut q = world.query::<(&GameId, &Pos, &Building, &Owner)>();
        q.iter(world)
            .find(|(g, _, _, _)| g.0 == building)
            .map(|(_, p, b, o)| (p.pos, building_def(b.kind).footprint, o.0))
    };
    let Some((host_pos, footprint, owner)) = host else { return };
    for e in occupants_of(world, building) {
        let exit = field_exit(world, owner, host_pos, footprint);
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
