use super::{clamp_world, find_owned, occupancy_and_gates, player_match};
use crate::components::*;
use crate::{GameIndex, PathScratch, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::*;

/// A* path from `from` to `to` over terrain + building occupancy, as `viewer`
/// sees it — a gatehouse is a door for its owner and a wall for everyone else.
/// Shared by the AI brain's army-move / recall logic.
pub fn path_to(world: &mut World, viewer: u64, from: V2, to: V2) -> Vec<V2> {
    let seed = world.resource::<WorldConfig>().seed;
    let (occ, gates) = occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, viewer)
    };
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    let mut scratch = world.resource_mut::<PathScratch>();
    scratch.0.find_path_costed(&passable, &cost, from.x, from.y, to.x, to.y, MAX_EXPANSIONS)
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
    let (occ, gates) = occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    let path = {
        let mut scratch = world.resource_mut::<PathScratch>();
        scratch.0.find_path_costed(&passable, &cost, from.x, from.y, target.x, target.y, MAX_EXPANSIONS)
    };
    if let Some(mut u) = world.get_mut::<Unit>(e) {
        u.gather_state = GatherState::Idle;
        u.target_node = 0;
        u.job_site = 0;
        u.attack_target = 0;
        u.home = target;
        u.order = ORDER_MOVE;
        u.order_target = target;
        u.anchor = from;
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
        u.job_site = 0;
        u.attack_target = 0;
        u.has_target = false;
        u.order = ORDER_NONE;
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
        u.job_site = 0;
        u.has_target = false;
        u.order = ORDER_ATTACK;
    }
}

/// What one extra hand already working a node costs the next peasant to choose
/// it, in squared tiles. Four tiles of extra walk buys a node of your own —
/// without a price on the crowd, a food emergency lands every peasant the bot
/// owns on the same one or two nodes.
const CROWD_COST: Fx = fx!("16");
/// What working an already well-manned trade costs, in squared tiles. A PRICE,
/// not a rule: the trade that is short of hands wins any node within twelve
/// tiles of one that is not, and loses to anything nearer than that. Mandating
/// the short trade instead sent peasants across the map to the only quarry they
/// could reach and the haul rate collapsed.
const OFF_TRADE_COST: Fx = fx!("144");

/// Assign every idle peasant of `owner` to the nearest node of its balanced
/// (food-first) resource type — or all-in on `prefer` when given (the AI and the
/// auto-gather button steer the economy toward what is short).
pub(crate) fn assign_idle_gatherers(world: &mut World, owner: u64, prefer: Option<ResourceType>) {
    let seed = world.resource::<crate::WorldConfig>().seed;
    let Some(match_id) = player_match(world, owner) else { return };
    // GameId order, never ECS iteration order: the balanced round-robin below
    // indexes by position, so the walk order decides who digs what.
    let mut idle: Vec<(u64, Entity, V2)> = {
        let mut q = world.query::<(Entity, &GameId, &Owner, &Pos, &Unit)>();
        q.iter(world)
            .filter(|(_, _, o, _, u)| {
                o.0 == owner
                    && u.garrisoned_in == 0
                    && u.gather_state == GatherState::Idle
                    && unit_def(u.kind).carry > 0
            })
            .map(|(e, g, _, p, _)| (g.0, e, p.pos))
            .collect()
    };
    if idle.is_empty() {
        return;
    }
    idle.sort_unstable_by_key(|(g, _, _)| *g);
    let mut load: bevy_platform::collections::HashMap<u64, i32> = Default::default();
    {
        let mut q = world.query::<(&Owner, &Unit)>();
        for (o, u) in q.iter(world) {
            if o.0 == owner && u.target_node != 0 && u.gather_state != GatherState::Idle {
                *load.entry(u.target_node).or_insert(0) += 1;
            }
        }
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
    // Balance the STANDING workforce, not the batch. `balanced_gather_types`
    // round-robins from food, and a peasant whose node ran out arrives here
    // alone — so a one-hand batch asked for the ideal split of one hand and got
    // "food", every time, and the whole town drifted onto the larder while the
    // timber ran out.
    let committed: Vec<ResourceType> = {
        let by_node: bevy_platform::collections::HashMap<u64, ResourceType> =
            nodes.iter().map(|(id, _, rt)| (*id, *rt)).collect();
        let mut q = world.query::<(&Owner, &Unit)>();
        q.iter(world)
            .filter(|(o, u)| o.0 == owner && u.gather_state != GatherState::Idle)
            .filter_map(|(_, u)| by_node.get(&u.target_node).copied())
            .collect()
    };
    let ideal = balanced_gather_types(&available, committed.len() + idle.len());
    let count_of = |list: &[ResourceType], t: ResourceType| list.iter().filter(|x| **x == t).count();
    let mut have: Vec<(ResourceType, usize, usize)> = available
        .iter()
        .map(|t| (*t, count_of(&committed, *t), count_of(&ideal, *t)))
        .collect();
    let short_of_hands = |have: &[(ResourceType, usize, usize)], rt: ResourceType| {
        have.iter().find(|(t, _, _)| *t == rt).is_none_or(|(_, hv, want)| hv < want)
    };
    // How close a harvester has to get to each node, and what the walker can
    // actually reach through the town's own masonry. Without this a hand that
    // the gather loop just stood down — because its node was sealed or its only
    // approach tile was a hair out of reach — is handed the very same node back
    // one tick later, forever.
    let field_footprints: bevy_platform::collections::HashMap<u64, i32> = {
        let sizes: bevy_platform::collections::HashMap<u64, i32> = {
            let mut q = world.query::<(&GameId, &Building)>();
            q.iter(world).map(|(g, b)| (g.0, building_def(b.kind).footprint)).collect()
        };
        let mut q = world.query::<(&GameId, &FieldOf)>();
        q.iter(world).map(|(g, f)| (g.0, sizes.get(&f.0).copied().unwrap_or(1))).collect()
    };
    let (occ, gates) = occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
    let mut flood = world.remove_resource::<crate::PathScratch>();
    for (_, e, pos) in idle.iter() {
        if let Some(s) = flood.as_mut() {
            s.1.explore(&passable, *pos, MAX_EXPANSIONS);
        }
        // One pass over every node, scored on walk plus two prices: the crowd
        // already on it, and the trade being one the town is not short of. An
        // explicit `prefer` overrides both, and falls back to any node at all
        // when the preferred resource has none left.
        let mut best: Option<u64> = None;
        let mut bd = Fx::MAX;
        for pass in 0..2 {
            for (id, p, rt) in &nodes {
                if pass == 0 && prefer.is_some_and(|w| w != *rt) {
                    continue;
                }
                if !node_reachable(seed, *pos, *p) {
                    continue;
                }
                if let Some(s) = flood.as_ref() {
                    let reach = crate::systems::node_reach(seed, *p, field_footprints.get(id).copied());
                    if !crate::systems::workable(&s.1, *p, reach) {
                        continue;
                    }
                }
                let crowd = Fx::from_num(load.get(id).copied().unwrap_or(0)) * CROWD_COST;
                let off_trade = if prefer.is_none() && !short_of_hands(&have, *rt) {
                    OFF_TRADE_COST
                } else {
                    Fx::ZERO
                };
                let d = dist2(*pos, *p).saturating_add(crowd).saturating_add(off_trade);
                if d < bd {
                    bd = d;
                    best = Some(*id);
                }
            }
            if best.is_some() || prefer.is_none() {
                break;
            }
        }
        if let Some(node) = best {
            if let Some(mut u) = world.get_mut::<Unit>(*e) {
                u.gather_state = GatherState::ToResource;
                u.target_node = node;
                u.has_target = false;
                u.order = ORDER_NONE;
                *load.entry(node).or_insert(0) += 1;
                let taken = nodes.iter().find(|(id, _, _)| *id == node).map(|(_, _, rt)| *rt);
                if let Some(slot) =
                    taken.and_then(|rt| have.iter_mut().find(|(t, _, _)| *t == rt))
                {
                    slot.1 += 1;
                }
            }
        }
    }
    if let Some(s) = flood {
        world.insert_resource(s);
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

// ── group orders ─────────────────────────────────────────────────────────────

/// Full-map A* runs one tick's command batch may spend on group orders. Past it
/// a group still marches; it just gets the cheap ceiling. `move_unit` runs an
/// UNBUDGETED full-map A* per unit, and 200 of those measured 29.79 ms inside a
/// SINGLE exclusive `apply_commands` pass against a 50 ms tick — paid at the
/// same moment by every peer, because lockstep applies the same batch on all of
/// them. Combat bounds mass pursuit the same way (`PURSUIT_BUDGET`).
pub(crate) const GROUP_PATHS_PER_TICK: usize = 4;
const CHEAP_EXPANSIONS: usize = 6_144;
/// Corners of the shared route a man will try before deciding he is not with
/// the group. The route is string-pulled, so it is corners only.
const JOIN_PROBES: usize = 4;

struct Member {
    id: u64,
    e: Entity,
    pos: V2,
    kind: UnitKind,
}

/// Resolve a selection to owned, ungarrisoned units through `GameIndex` — not
/// one full-world scan per id, which is what `find_owned` costs. Sorted by
/// `GameId` and deduped, so two peers handed the same click in a different
/// order build the same group and hand out the same slots.
fn resolve_group(world: &mut World, owner: u64, ids: &[u64]) -> Vec<Member> {
    let mut want: Vec<u64> = ids.to_vec();
    want.sort_unstable();
    want.dedup();
    let mut ents: Vec<(u64, Entity)> = Vec::with_capacity(want.len());
    let mut missing: Vec<u64> = Vec::new();
    {
        let index = world.resource::<GameIndex>();
        for id in &want {
            match index.get(*id) {
                Some(e) => ents.push((*id, e)),
                None => missing.push(*id),
            }
        }
    }
    // The index is rebuilt every fourth tick, so a unit trained moments ago is
    // not in it yet. ONE scan resolves every miss at once.
    if !missing.is_empty() {
        let mut q = world.query::<(Entity, &GameId)>();
        for (e, g) in q.iter(world) {
            if missing.binary_search(&g.0).is_ok() {
                ents.push((g.0, e));
            }
        }
        ents.sort_unstable_by_key(|(id, _)| *id);
    }
    ents.into_iter()
        .filter_map(|(id, e)| {
            if world.get::<Owner>(e).is_none_or(|o| o.0 != owner) {
                return None;
            }
            let (kind, free) = match world.get::<Unit>(e) {
                Some(u) => (u.kind, u.garrisoned_in == 0),
                None => return None,
            };
            let pos = world.get::<Pos>(e)?.pos;
            free.then_some(Member { id, e, pos, kind })
        })
        .collect()
}

/// Group sizes above this skip the untangling pass — it is O(n^2) per round,
/// and a click that selects a thousand men is not a formation.
const UNTANGLE_MAX: usize = 384;
/// Pair comparisons the untangling may spend, whatever the group size. This is
/// what keeps ONE click's cost flat: 30 men converge in a few rounds and 200
/// simply get fewer of them.
const UNTANGLE_PAIRS: usize = 80_000;
const UNTANGLE_ROUNDS: usize = 8;

/// Trade places between any two men who would walk across each other. Sorting
/// the men and the slots by ONE march key cannot remove the crossings on its
/// own — measured, it removes none: the block a player has selected is rarely
/// aligned to the line he is sending it to, so the men's depths are all
/// distinct and the cross-axis tie-break never fires. Swapping a crossing pair
/// always shortens the total walk, so this terminates.
///
/// `members` and `slots` are both in `GameId` order, so index `i` is one man.
fn untangle(members: &[Member], slots: &mut [(u64, V2)]) {
    let n = members.len();
    if !(2..=UNTANGLE_MAX).contains(&n) {
        return;
    }
    // the walk each man currently faces, kept alongside so a pair costs two
    // square roots instead of four
    let mut walk: Vec<Fx> = members.iter().zip(slots.iter()).map(|(m, s)| dist(m.pos, s.1)).collect();
    let pairs = n * (n - 1) / 2;
    let rounds = (UNTANGLE_PAIRS / pairs.max(1)).clamp(1, UNTANGLE_ROUNDS);
    for _ in 0..rounds {
        let mut traded = false;
        for i in 0..n {
            for j in i + 1..n {
                let (pi, pj) = (members[i].pos, members[j].pos);
                let (si, sj) = (slots[i].1, slots[j].1);
                let (ci, cj) = (dist(pi, sj), dist(pj, si));
                if ci.saturating_add(cj) < walk[i].saturating_add(walk[j]) {
                    slots[i].1 = sj;
                    slots[j].1 = si;
                    walk[i] = ci;
                    walk[j] = cj;
                    traded = true;
                }
            }
        }
        if !traded {
            break;
        }
    }
}

/// March a whole group on ONE path: one occupancy snapshot, one A* from the
/// group's centre, formation slots around the destination, and each man joining
/// the shared route at the nearest corner he can see.
#[allow(clippy::too_many_arguments)]
fn lay_march(
    world: &mut World,
    owner: u64,
    members: &[Member],
    target: V2,
    shape: Option<FormationShape>,
    order: u8,
    attack_target: u64,
    budget: &mut usize,
) {
    let seed = world.resource::<WorldConfig>().seed;
    let (occ, gates) = occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    let free = |p: V2| passable(p.x.to_num::<i32>(), p.y.to_num::<i32>());

    let raw = V2::new(clamp_world(target.x), clamp_world(target.y));
    // an order dropped on water or on a roof left the whole group walking at a
    // tile not one of them can stand on
    let dest = if free(raw) { raw } else { nearest_passable_grid(&passable, raw.x, raw.y) };

    // A column marches at the pace of its slowest man. Without it a group order
    // is only a shared destination: the horse arrives, the engine it was sent to
    // escort turns up a minute later, and the two never fight the same battle.
    // `Unit.speed` is pure duplication of `unit_def(kind).speed` — no research
    // touches it (a test pins that) — so `movement` restores it the moment the
    // march ends and nothing has to remember what it was.
    let march_speed = members
        .iter()
        .map(|m| unit_def(m.kind).speed)
        .min()
        .unwrap_or(Fx::ONE)
        .max(fx!("0.1"));

    let mut sum = V2::ZERO;
    for m in members {
        sum = V2::new(sum.x + m.pos.x, sum.y + m.pos.y);
    }
    let n = Fx::from_num(members.len() as i32);
    let centroid = V2::new(sum.x / n, sum.y / n);

    let cap = if *budget > 0 {
        *budget -= 1;
        MAX_EXPANSIONS
    } else {
        CHEAP_EXPANSIONS
    };
    let route = {
        let mut scratch = world.resource_mut::<PathScratch>();
        scratch.0.find_path_costed(&passable, &cost, centroid.x, centroid.y, dest.x, dest.y, cap)
    };

    let mut slots: Vec<(u64, V2)> = Vec::with_capacity(members.len());
    match shape {
        Some(shape) => {
            let radii: Vec<Fx> = members.iter().map(|m| unit_def(m.kind).radius).collect();
            let pitch = formation_pitch(&radii);
            let heading = heading_of(V2::new(dest.x - centroid.x, dest.y - centroid.y));
            let pairs: Vec<(u64, V2)> = members.iter().map(|m| (m.id, m.pos)).collect();
            let (mut keyed, mut places) = (Vec::new(), Vec::new());
            assign_slots(&pairs, dest, heading, shape, pitch, &mut keyed, &mut places, &mut slots);
        }
        None => slots.extend(members.iter().map(|m| (m.id, dest))),
    }
    for (_, s) in slots.iter_mut() {
        let c = V2::new(clamp_world(s.x), clamp_world(s.y));
        *s = if free(c) { c } else { dest };
    }
    slots.sort_unstable_by_key(|(id, _)| *id);
    untangle(members, &mut slots);

    let tail = route.len().saturating_sub(1);
    let mut near: Vec<(Fx, usize)> = Vec::with_capacity(route.len());
    let mut path: Vec<V2> = Vec::new();
    for m in members {
        let slot =
            slots.binary_search_by_key(&m.id, |(id, _)| *id).map(|i| slots[i].1).unwrap_or(dest);
        path.clear();
        let mut walks = !route.is_empty();
        if walks {
            near.clear();
            near.extend((0..route.len()).map(|i| (dist2(m.pos, route[i]), i)));
            near.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            match near
                .iter()
                .take(JOIN_PROBES)
                .find(|(_, i)| line_of_sight(&passable, m.pos, route[*i]))
            {
                // the men already ahead never walk back to the centre, and a
                // stray on the wrong side of a wall never cuts through it
                Some((_, j)) if *j < tail => path.extend_from_slice(&route[*j..tail]),
                Some(_) => {}
                None => walks = false,
            }
        }
        if walks {
            path.push(slot);
        }
        let Some(mut u) = world.get_mut::<Unit>(m.e) else { continue };
        u.gather_state = GatherState::Idle;
        u.target_node = 0;
        u.job_site = 0;
        u.attack_target = attack_target;
        u.order = order;
        u.order_target = dest;
        u.anchor = m.pos;
        if order != ORDER_ATTACK {
            u.home = slot;
        }
        u.path.clear();
        u.path_idx = 0;
        u.has_target = walks;
        if walks {
            u.target = path[0];
            u.path.extend_from_slice(&path);
            if order != ORDER_ATTACK {
                u.speed = march_speed;
            }
        }
    }
}

pub(crate) fn group_move(
    world: &mut World,
    owner: u64,
    ids: &[u64],
    target: V2,
    formation: u8,
    budget: &mut usize,
) {
    let members = resolve_group(world, owner, ids);
    if members.is_empty() {
        return;
    }
    let shape = FormationShape::from_u8(formation);
    lay_march(world, owner, &members, target, shape, ORDER_MOVE, 0, budget);
}

/// March, but fight what turns up on the way. The march RESUMES when whatever
/// was acquired dies — `order_target` is what combat walks back onto — which is
/// what lets two bodies more than an aggro range apart reach each other at all.
pub(crate) fn attack_move(
    world: &mut World,
    owner: u64,
    ids: &[u64],
    target: V2,
    formation: u8,
    budget: &mut usize,
) {
    let members = resolve_group(world, owner, ids);
    if members.is_empty() {
        return;
    }
    let shape = FormationShape::from_u8(formation);
    lay_march(world, owner, &members, target, shape, ORDER_ATTACK_MOVE, 0, budget);
}

pub(crate) fn group_attack(
    world: &mut World,
    owner: u64,
    ids: &[u64],
    target: u64,
    budget: &mut usize,
) {
    let members = resolve_group(world, owner, ids);
    if members.is_empty() {
        return;
    }
    let tpos = {
        let mut q = world.query::<(&GameId, &Owner, &Pos)>();
        q.iter(world).find(|(g, o, _)| g.0 == target && o.0 != owner).map(|(_, _, p)| p.pos)
    };
    let Some(tpos) = tpos else { return };
    lay_march(world, owner, &members, tpos, None, ORDER_ATTACK, target, budget);
}

/// Halt: path, pursuit, gathering and building labour all released at once.
/// Before this the only way to stop a unit was to order it onto its own feet.
pub(crate) fn stop(world: &mut World, owner: u64, ids: &[u64]) {
    for m in resolve_group(world, owner, ids) {
        let Some(mut u) = world.get_mut::<Unit>(m.e) else { continue };
        u.has_target = false;
        u.path.clear();
        u.path_idx = 0;
        u.attack_target = 0;
        u.gather_state = GatherState::Idle;
        u.target_node = 0;
        u.job_site = 0;
        u.order = ORDER_STOP;
        u.order_target = m.pos;
        u.anchor = m.pos;
        u.home = m.pos;
    }
}
