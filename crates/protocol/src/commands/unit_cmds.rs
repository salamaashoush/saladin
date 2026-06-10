use super::{building_occupancy, clamp_world, find_owned, player_match};
use crate::components::*;
use crate::{PathScratch, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::*;

/// A* path from `from` to `to` over terrain + building occupancy. Shared by the
/// AI brain's army-move / recall logic.
pub(crate) fn path_to(world: &mut World, from: V2, to: V2) -> Vec<V2> {
    let seed = world.resource::<WorldConfig>().seed;
    let occ = building_occupancy(world, false);
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty));
    let mut scratch = world.resource_mut::<PathScratch>();
    scratch.0.find_path(&passable, from.x, from.y, to.x, to.y, MAX_EXPANSIONS)
}

/// Manual move order: cancels gathering and combat pursuit, re-homes the unit at
/// the destination (so Defensive stance leashes there).
pub(crate) fn move_unit(world: &mut World, owner: u64, unit: u64, target: V2) {
    let Some(e) = find_owned(world, owner, unit) else { return };
    if world.get::<Unit>(e).is_none_or(|u| u.garrisoned_in != 0) {
        return;
    }
    let target = V2::new(clamp_world(target.x), clamp_world(target.y));
    let from = world.get::<Pos>(e).map(|p| p.pos);
    let Some(from) = from else { return };
    let seed = world.resource::<WorldConfig>().seed;
    let occ = building_occupancy(world, false);
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty));
    let path = {
        let mut scratch = world.resource_mut::<PathScratch>();
        scratch.0.find_path(&passable, from.x, from.y, target.x, target.y, MAX_EXPANSIONS)
    };
    if let Some(mut u) = world.get_mut::<Unit>(e) {
        u.gather_state = GatherState::Idle;
        u.target_node = 0;
        u.attack_target = 0;
        u.home = target;
        if path.is_empty() {
            u.has_target = false;
        } else {
            u.target = path[0];
            u.path = path;
            u.path_idx = 0;
            u.has_target = true;
        }
    }
}

/// Set combat posture; posts the unit's home at its current position so
/// Defensive units leash to where they were set.
pub(crate) fn set_stance(world: &mut World, owner: u64, unit: u64, stance: Stance) {
    let Some(e) = find_owned(world, owner, unit) else { return };
    let here = world.get::<Pos>(e).map(|p| p.pos);
    if let Some(mut u) = world.get_mut::<Unit>(e) {
        u.stance = stance;
        if let Some(here) = here {
            u.home = here;
        }
    }
}

/// Send a carrier unit to harvest `node`. Mirrors the `gatherResource` reducer:
/// only units with carry capacity, only live nodes; cancels combat pursuit.
pub(crate) fn gather(world: &mut World, owner: u64, unit: u64, node: u64) {
    let Some(e) = find_owned(world, owner, unit) else { return };
    let kind = match world.get::<Unit>(e) {
        Some(u) if u.garrisoned_in == 0 => u.kind,
        _ => return,
    };
    if unit_def(kind).carry <= 0 {
        return;
    }
    let node_alive = {
        let mut q = world.query::<(&GameId, &ResourceNode)>();
        q.iter(world).any(|(g, _)| g.0 == node)
    };
    if !node_alive {
        return;
    }
    if let Some(mut u) = world.get_mut::<Unit>(e) {
        u.gather_state = GatherState::ToResource;
        u.target_node = node;
        u.attack_target = 0;
        u.has_target = false;
    }
}

/// Order an explicit attack on an enemy unit or building. Mirrors `attackUnit`.
pub(crate) fn attack(world: &mut World, owner: u64, unit: u64, target: u64) {
    let Some(e) = find_owned(world, owner, unit) else { return };
    if world.get::<Unit>(e).is_none_or(|u| u.garrisoned_in != 0) {
        return;
    }
    // the target must exist and belong to someone else
    let target_enemy = {
        let mut q = world.query::<(&GameId, &Owner)>();
        q.iter(world).any(|(g, o)| g.0 == target && o.0 != owner)
    };
    if !target_enemy {
        return;
    }
    if let Some(mut u) = world.get_mut::<Unit>(e) {
        u.attack_target = target;
        u.gather_state = GatherState::Idle;
        u.target_node = 0;
        u.has_target = false;
    }
}

/// Assign every idle peasant of `owner` to the nearest node of its balanced
/// (food-first) resource type — or all-in on `prefer` when given (the AI and the
/// auto-gather button steer the economy toward what is short).
pub(crate) fn assign_idle_gatherers(world: &mut World, owner: u64, prefer: Option<ResourceType>) {
    let seed = world.resource::<crate::WorldConfig>().seed;
    let Some(match_id) = player_match(world, owner) else { return };
    let idle: Vec<(Entity, V2)> = {
        let mut q = world.query::<(Entity, &Owner, &Pos, &Unit)>();
        q.iter(world)
            .filter(|(_, o, _, u)| {
                o.0 == owner
                    && u.garrisoned_in == 0
                    && u.gather_state == GatherState::Idle
                    && unit_def(u.kind).carry > 0
            })
            .map(|(e, _, p, _)| (e, p.pos))
            .collect()
    };
    if idle.is_empty() {
        return;
    }
    let nodes: Vec<(u64, V2, ResourceType)> = {
        let mut q = world.query::<(&GameId, &Pos, &ResourceNode, &MatchId)>();
        q.iter(world).filter(|(_, _, _, m)| m.0 == match_id).map(|(g, p, n, _)| (g.0, p.pos, n.res_type)).collect()
    };
    if nodes.is_empty() {
        return;
    }
    let available: Vec<ResourceType> = {
        let mut s = Vec::new();
        for (_, _, rt) in &nodes {
            if !s.contains(rt) {
                s.push(*rt);
            }
        }
        s
    };
    let types = balanced_gather_types(&available, idle.len());
    for (i, (e, pos)) in idle.iter().enumerate() {
        let want = prefer.or_else(|| types.get(i).copied());
        let typed: Vec<(u64, V2)> = match want {
            Some(w) => nodes.iter().filter(|(_, _, rt)| *rt == w).map(|(id, p, _)| (*id, *p)).collect(),
            None => Vec::new(),
        };
        let pool = if typed.is_empty() { nodes.iter().map(|(id, p, _)| (*id, *p)).collect::<Vec<_>>() } else { typed };
        let mut best: Option<u64> = None;
        let mut bd = Fx::MAX;
        for (id, p) in &pool {
            if !node_reachable(seed, *pos, *p) {
                continue;
            }
            let d = dist2(*pos, *p);
            if d < bd {
                bd = d;
                best = Some(*id);
            }
        }
        if let Some(node) = best {
            if let Some(mut u) = world.get_mut::<Unit>(*e) {
                u.gather_state = GatherState::ToResource;
                u.target_node = node;
                u.has_target = false;
            }
        }
    }
}

/// Send every idle gatherer to work — balanced food-first, but all-in on food
/// when the larder is low so a "Gather" click can't starve the base.
pub(crate) fn auto_gather(world: &mut World, owner: u64) {
    let (food, pop) = {
        let food = {
            let mut q = world.query::<&Player>();
            match q.iter(world).find(|p| p.player_id == owner) {
                Some(p) => p.stock.food,
                None => return,
            }
        };
        let mut uq = world.query::<&Owner>();
        let pop = uq.iter(world).filter(|o| o.0 == owner).count() as i32;
        (food, pop)
    };
    let prefer = if food_low(food, pop) { Some(ResourceType::Food) } else { None };
    assign_idle_gatherers(world, owner, prefer);
}
