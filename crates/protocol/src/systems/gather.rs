use crate::components::{Building, FieldOf, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit};
use crate::{GameIndex, MatchStatuses, PathScratch, WorldConfig};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    AuraTarget, DEPOSIT_RANGE, Fx, GatherState, harvest_reach,
    HARVEST_TIME, MAX_EXPANSIONS, Occupant, ResourceType, V2, WorkAura, approach_tile, building_def, dist, is_passable, move_cost_at,
    gate_blocks, nearest_reachable_passable_grid, occupancy_set, operational,
    reach_budget, res_bit, tile_key, unit_def,
};

const AI_DT: Fx = saladin_sim::AI_DT;

/// A computed move: a path, the first waypoint to head for, and the tile the
/// walk actually ends on (which is NOT the goal when the goal is unreachable).
struct MovePatch {
    path: Vec<V2>,
    target: V2,
    end: V2,
}

/// How far from the goal an approach tile may sit. A crop is sown at its farm's
/// own centre, under the whole footprint, so the ring has to clear it.
const APPROACH_RADIUS: i32 = 4;

/// Plan a walk to `to`.
///
/// The terrain regions answer "which tiles can this walker ever stand on"
/// exactly and in O(1), so the approach tile is a small ring scan around the
/// goal, not a flood. The flood is the fallback for the one case regions cannot
/// see — buildings sealing a pocket the terrain leaves open — and it now reports
/// whether it ran out of budget, because a walker must never conclude a node is
/// unreachable from a search that simply stopped looking.
fn move_patch(
    scratch: &mut PathScratch,
    seed: u32,
    occ: &std::collections::HashSet<i32>,
    gates: &[(i32, u64)],
    viewer: u64,
    from: V2,
    to: V2,
) -> Option<MovePatch> {
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(gates, k, viewer)
    };
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    if let Some(goal) = approach_tile(seed, &passable, from, to, APPROACH_RADIUS) {
        let path =
            scratch.0.find_path_costed(&passable, &cost, from.x, from.y, goal.x, goal.y, MAX_EXPANSIONS);
        if !path.is_empty() {
            return Some(MovePatch { target: path[0], path, end: goal });
        }
    }
    let mut reach =
        nearest_reachable_passable_grid(&mut scratch.1, &passable, from, to, reach_budget(dist(from, to)))?;
    // A truncated flood that found nothing better than the tile underfoot has
    // not answered the question, it has run out of time asking it. Escalate ONCE
    // to the whole map rather than hand back "you are already as close as you
    // can get" — that answer is what turns ToResource into an absorbing state.
    if reach.truncated && saladin_sim::dist2(reach.at, to) >= saladin_sim::dist2(tile_centre(from), to) {
        reach = nearest_reachable_passable_grid(&mut scratch.1, &passable, from, to, MAX_EXPANSIONS)?;
    }
    let path =
        scratch.0.find_path_costed(&passable, &cost, from.x, from.y, reach.at.x, reach.at.y, MAX_EXPANSIONS);
    if path.is_empty() {
        None
    } else {
        Some(MovePatch { target: path[0], path, end: reach.at })
    }
}

/// The centre of the tile a position sits in. Approach tiles are tile CENTRES,
/// so a progress test that measures them against a raw position is comparing two
/// different things and can be off by most of a tile.
fn tile_centre(p: V2) -> V2 {
    V2::new(p.x.floor() + saladin_sim::fx!("0.5"), p.y.floor() + saladin_sim::fx!("0.5"))
}

#[derive(Clone, Copy)]
struct Dropoff {
    owner: u64,
    pos: V2,
    footprint: i32,
    accepts: u8,
}

/// A standing work bonus a building projects over the nodes around it.
struct Aura {
    owner: u64,
    pos: V2,
    aura: WorkAura,
}

/// Gather AI state machine — runs every AI tick (200 ms). Sets movement targets,
/// harvests nodes, deposits at the keep / food drop-offs. Ported from the
/// SpacetimeDB `unitAi` reducer. Occupancy + paths reuse the shared A*.
#[allow(clippy::too_many_arguments)]
pub fn gather(
    cfg: Res<WorldConfig>,
    statuses: Res<MatchStatuses>,
    mut scratch: ResMut<PathScratch>,
    index: Res<GameIndex>,
    mut commands: Commands,
    q_buildings: Query<(&GameId, &Pos, &Building, &Owner)>,
    mut q_nodes: Query<(&GameId, &Pos, &mut ResourceNode, &MatchId)>,
    mut q_players: Query<(Entity, &mut Player)>,
    mut q_units: Query<(&GameId, &Pos, &Owner, &MatchId, &mut Unit)>,
    q_fields: Query<(&GameId, &FieldOf)>,
    mut stats: ResMut<crate::MatchStats>,
) {
    let seed = cfg.seed;

    // ── read-only snapshots ──────────────────────────────────────────────────
    let occupants: Vec<Occupant> =
        q_buildings.iter().map(|(_, p, b, _)| Occupant { kind: b.kind, pos: p.pos }).collect();
    let occ = occupancy_set(&occupants, false);
    // a finished gatehouse is a door for its owner and a wall for everyone else
    let gates: Vec<(i32, u64)> = q_buildings
        .iter()
        .filter(|(_, _, b, _)| building_def(b.kind).passable && operational(b.state))
        .map(|(_, p, _, o)| (tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()), o.0))
        .collect();

    let dropoffs: Vec<Dropoff> = q_buildings
        .iter()
        .filter(|(_, _, b, _)| operational(b.state))
        .filter_map(|(_, p, b, owner)| {
            let def = building_def(b.kind);
            (def.accepts != 0).then_some(Dropoff {
                owner: owner.0,
                pos: p.pos,
                footprint: def.footprint,
                accepts: def.accepts,
            })
        })
        .collect();

    // work auras: the hut's nets over its fishery, the granary's hands over the
    // fields. One field on the def, one loop here.
    let auras: Vec<Aura> = q_buildings
        .iter()
        .filter(|(_, _, b, _)| operational(b.state))
        .filter_map(|(_, p, b, o)| {
            building_def(b.kind).aura.map(|aura| Aura { owner: o.0, pos: p.pos, aura })
        })
        .collect();
    // A crop is sown at its farm's own centre, which on an even footprint is the
    // corner four blocked tiles share, so a reaper stands back by the farm's
    // whole footprint.
    let footprint_of: HashMap<u64, i32> =
        q_buildings.iter().map(|(g, _, b, _)| (g.0, building_def(b.kind).footprint)).collect();
    let field_nodes: HashMap<u64, i32> = q_fields
        .iter()
        .map(|(g, f)| (g.0, footprint_of.get(&f.0).copied().unwrap_or(1)))
        .collect();
    let aura_mult = |owner: u64, at: V2, target: AuraTarget| -> Fx {
        let mut best = Fx::ONE;
        for a in &auras {
            if a.owner != owner || a.aura.target != target || a.aura.harvest_mult <= best {
                continue;
            }
            if dist(a.pos, at) <= a.aura.radius {
                best = a.aura.harvest_mult;
            }
        }
        best
    };

    // Position + the reach a harvester needs to work it. The span of unstandable
    // ground under a node is what the reach has to clear: a farm's whole
    // footprint for a crop, the water tile itself for a school of fish.
    let mut node_map: HashMap<u64, (V2, Fx)> = HashMap::new();
    let mut nodes_list: Vec<(u64, V2, u64)> = Vec::new();
    for (gid, p, _n, m) in q_nodes.iter() {
        node_map.insert(gid.0, (p.pos, node_reach(seed, p.pos, field_nodes.get(&gid.0).copied())));
        nodes_list.push((gid.0, p.pos, m.0));
    }

    // player_id → entity
    let player_ent: HashMap<u64, Entity> =
        q_players.iter().map(|(e, p)| (p.player_id, e)).collect();

    // nearest dropoff for (owner, carry_type) from a position
    let nearest_dropoff = |owner: u64, carry: ResourceType, from: V2| -> Option<Dropoff> {
        let mut best: Option<Dropoff> = None;
        let mut best_d = Fx::MAX;
        for d in &dropoffs {
            if d.owner != owner {
                continue;
            }
            if d.accepts & res_bit(carry) == 0 {
                continue;
            }
            let dd = dist(from, d.pos);
            if dd < best_d {
                best_d = dd;
                best = Some(*d);
            }
        }
        best
    };


    let look =
        Look { seed, occ: &occ, gates: &gates, nodes: &nodes_list, node_map: &node_map };

    // ── mutate units ─────────────────────────────────────────────────────────
    for (_gid, pos, owner, mid, mut u) in &mut q_units {
        if u.garrisoned_in != 0 || u.gather_state == GatherState::Idle || !statuses.simulates(mid.0) {
            continue;
        }
        let here = pos.pos;
        let mid = mid.0;

        match u.gather_state {
            GatherState::ToResource => {
                let Some((node_pos, reach)) = node_map.get(&u.target_node).copied() else {
                    retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, 0));
                    continue;
                };
                if !saladin_sim::node_reachable(seed, here, node_pos) {
                    // across water — retarget (region-filtered) instead of
                    // marching to the shore and discovering it there
                    let skip = u.target_node;
                    retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, skip));
                    continue;
                }
                if dist(here, node_pos) <= reach {
                    u.gather_state = GatherState::Harvesting;
                    u.harvest_timer = Fx::ZERO;
                    u.has_target = false;
                } else if !u.has_target {
                    let patch = move_patch(&mut scratch, seed, &occ, &gates, owner.0, here, node_pos)
                        .filter(|p| !stuck(here, p.end, node_pos, reach));
                    match patch {
                        Some(p) => {
                            u.path = p.path;
                            u.path_idx = 0;
                            u.target = p.target;
                            u.has_target = true;
                        }
                        // Out of reach for good — the region says the ground
                        // connects, so something BUILT is in the way. Take other
                        // work. This used to ping-pong between two siblings
                        // forever, but only because a truncated flood called
                        // both of them unreachable when neither was; the
                        // judgement here is now A* actually failing.
                        None => {
                            let skip = u.target_node;
                            retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, skip));
                        }
                    }
                }
            }
            GatherState::Harvesting => {
                let Some(node_e) = index.get(u.target_node) else {
                    retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, 0));
                    continue;
                };
                let Ok((_, npos, mut node, _)) = q_nodes.get_mut(node_e) else {
                    retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, 0));
                    continue;
                };
                // another harvester may have emptied it earlier THIS tick (its
                // despawn is deferred): treat 0-remaining as gone, never dupe
                if node.remaining <= 0 {
                    retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, 0));
                    continue;
                }
                // a hut's nets over its fishery, a granary's hands over its
                // fields: whichever aura covers this node speeds the work
                let target = if field_nodes.contains_key(&u.target_node) {
                    Some(AuraTarget::Field)
                } else if node.res_type == ResourceType::Food
                    && !is_passable(seed, npos.pos.x.to_num::<i32>(), npos.pos.y.to_num::<i32>())
                {
                    Some(AuraTarget::WaterFood)
                } else {
                    None
                };
                let step = match target {
                    Some(t) => AI_DT * aura_mult(owner.0, npos.pos, t),
                    None => AI_DT,
                };
                let timer = u.harvest_timer + step;
                if timer < HARVEST_TIME {
                    u.harvest_timer = timer;
                    continue;
                }
                let def = unit_def(u.kind);
                let take = def.carry.min(node.remaining);
                let rem = node.remaining - take;
                u.carry_type = node.res_type;
                node.remaining = rem;
                if rem <= 0 {
                    commands.entity(node_e).despawn();
                }
                u.carrying = take;
                u.harvest_timer = Fx::ZERO;
                u.gather_state = GatherState::ToStockpile;
            }
            GatherState::ToStockpile => {
                let Some(drop) = nearest_dropoff(owner.0, u.carry_type, here) else {
                    u.gather_state = GatherState::Idle;
                    u.has_target = false;
                    continue;
                };
                if !saladin_sim::node_reachable(seed, here, drop.pos) {
                    // the dropoff sits in a region this carrier can never walk
                    // to — idle now rather than failing pathfinds forever
                    u.gather_state = GatherState::Idle;
                    u.has_target = false;
                    continue;
                }
                // banked when standing by the building's edge — movement already
                // walked us to the nearest reachable tile, so a radius test
                // against the footprint is exact and costs nothing
                let half_fp = Fx::from_num(drop.footprint) / Fx::from_num(2);
                let at_building = dist(here, drop.pos) <= DEPOSIT_RANGE + half_fp;
                if at_building {
                    if let Some(&pe) = player_ent.get(&owner.0) {
                        if let Ok((_, mut player)) = q_players.get_mut(pe) {
                            player.stock.add(u.carry_type, u.carrying);
                            stats.of(owner.0).gathered += u.carrying as u64;
                        }
                    }
                    u.carrying = 0;
                    u.has_target = false;
                    if node_map.contains_key(&u.target_node) {
                        u.gather_state = GatherState::ToResource;
                    } else {
                        retarget(&mut u, best_node(&mut scratch.1, &look, here, mid, owner.0, 0));
                    }
                } else if !u.has_target {
                    match move_patch(&mut scratch, seed, &occ, &gates, owner.0, here, drop.pos) {
                        Some(p) => {
                            u.path = p.path;
                            u.path_idx = 0;
                            u.target = p.target;
                            u.has_target = true;
                        }
                        None => {
                            // no route to any deposit from here — idle instead of
                            // re-running a failing A* every tick forever (the
                            // auto-gather / player order will re-task the unit)
                            u.gather_state = GatherState::Idle;
                            u.has_target = false;
                        }
                    }
                }
            }
            // a builder is driven by the construction loop, not the gather one
            GatherState::Idle | GatherState::Constructing => {}
        }
    }
}

/// True when walking cannot help: the best tile this walker can reach is itself
/// out of working range, and the walker is already standing on it.
///
/// The test is about `end`, the approach tile — not about how far the walker
/// currently is. Asking "am I still far from the node" instead needs a slack
/// term to stop a walker that is practically there from throwing its node away
/// over a rounding bit, and that slack blinds it to the case that actually
/// happens: a node whose every standable neighbour is a full tile off while
/// `harvest_reach` is seven tenths, because something got BUILT on the one tile
/// that would have worked. That node can never be harvested by anyone, and the
/// walker used to march at it for the rest of the match.
///
/// Both distances come from TILE CENTRES. Measuring an approach tile's centre
/// against a walker's raw position compares two different things — a walker
/// standing a little off-centre reads as closer than any tile it can reach, so
/// every patch looks like a step backwards and `ToResource` becomes absorbing.
fn stuck(here: V2, end: V2, node: V2, reach: Fx) -> bool {
    dist(end, node) > reach
        && saladin_sim::dist2(end, node) >= saladin_sim::dist2(tile_centre(here), node)
}

/// Take the offered node, or stand down.
fn retarget(u: &mut Unit, node: Option<u64>) {
    match node {
        Some(id) => {
            u.gather_state = GatherState::ToResource;
            u.target_node = id;
            u.has_target = false;
        }
        None => {
            u.gather_state = GatherState::Idle;
            u.has_target = false;
            u.target_node = 0;
        }
    }
}

/// How far a harvester must be able to get to work a node. The span of
/// unstandable ground UNDER the node is what the reach has to clear: a farm's
/// whole footprint for a crop, the water tile itself for a school of fish.
pub(crate) fn node_reach(seed: u32, pos: V2, field_footprint: Option<i32>) -> Fx {
    let span = match field_footprint {
        Some(fp) => fp,
        None if !is_passable(seed, pos.x.to_num::<i32>(), pos.y.to_num::<i32>()) => 1,
        None => 0,
    };
    harvest_reach(span)
}

/// Can a walker whose reachable region is already flooded into `flood` stand
/// close enough to work this node?
pub(crate) fn workable(flood: &saladin_sim::Flood, npos: V2, reach: Fx) -> bool {
    let r = reach.ceil().to_num::<i32>().max(1);
    let tx = npos.x.floor().to_num::<i32>();
    let ty = npos.y.floor().to_num::<i32>();
    let half = saladin_sim::fx!("0.5");
    for dy in -r..=r {
        for dx in -r..=r {
            let (nx, ny) = (tx + dx, ty + dy);
            if !flood.saw(nx, ny) {
                continue;
            }
            let c = V2::new(Fx::from_num(nx) + half, Fx::from_num(ny) + half);
            if dist(c, npos) <= reach {
                return true;
            }
        }
    }
    false
}

/// Everything `best_node` needs to judge a node without touching the world.
struct Look<'a> {
    seed: u32,
    occ: &'a std::collections::HashSet<i32>,
    gates: &'a [(i32, u64)],
    nodes: &'a [(u64, V2, u64)],
    node_map: &'a HashMap<u64, (V2, Fx)>,
}

/// How many nearest candidates to try before standing the walker down. Each
/// rejection is a node it can see and cannot work.
const NODE_TRIES: usize = 8;

/// The nearest node in `match_id` this walker can actually WORK, skipping
/// `skip`.
///
/// "Nearest reachable node" is not the same question as "nearest node I can
/// stand close enough to". A node whose every standable neighbour is out of
/// harvest range — because a hut got raised on the one tile that would have
/// worked — or one sealed behind a wall ring reads as a perfectly good target
/// to a distance test, and a walker handed it will march at it, give up, and be
/// handed its equally hopeless neighbour, forever.
///
/// So this floods the walker's region ONCE, occupancy and gates included, and
/// then asks of each candidate in ascending distance whether any tile it reached
/// is inside that node's harvest reach. One flood answers it for every
/// candidate; an A* per candidate would not be affordable.
fn best_node(
    flood: &mut saladin_sim::Flood,
    look: &Look,
    from: V2,
    match_id: u64,
    owner: u64,
    skip: u64,
) -> Option<u64> {
    let seed = look.seed;
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !look.occ.contains(&k) && !gate_blocks(look.gates, k, owner)
    };
    if !flood.explore(&passable, from, MAX_EXPANSIONS) {
        return None;
    }
    let mut rejected = [0u64; NODE_TRIES];
    let mut n_rej = 0usize;
    loop {
        let mut best: Option<u64> = None;
        let mut best_d = Fx::MAX;
        for (id, pos, mid) in look.nodes {
            if *mid != match_id || *id == skip || rejected[..n_rej].contains(id) {
                continue;
            }
            if !saladin_sim::node_reachable(seed, from, *pos) {
                continue;
            }
            let dd = saladin_sim::dist2(from, *pos);
            if dd < best_d {
                best_d = dd;
                best = Some(*id);
            }
        }
        let id = best?;
        let (npos, reach) = look.node_map.get(&id).copied()?;
        if workable(flood, npos, reach) {
            return Some(id);
        }
        if n_rej == NODE_TRIES {
            return None;
        }
        rejected[n_rej] = id;
        n_rej += 1;
    }
}
