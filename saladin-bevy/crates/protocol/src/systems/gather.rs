use crate::components::{Building, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit};
use crate::{GameIndex, MatchStatuses, PathScratch, WorldConfig};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    AStar, BuildingKind, DEPOSIT_RANGE, Fx, GatherState, HARVEST_RANGE, HARVEST_TIME, MAX_EXPANSIONS,
    Occupant, ResourceType, V2, building_def, dist, is_passable, nearest_passable_grid,
    nearest_reachable_passable_grid, occupancy_set, tile_key, unit_def,
};

const AI_DT: Fx = saladin_sim::AI_DT;

/// A computed move: a path plus the first waypoint to head for.
struct MovePatch {
    path: Vec<V2>,
    target: V2,
}

/// Cap on the reachable-region flood. The snapped target/approach tile is always
/// near the goal, so a bounded flood finds it; the full-map flood (MAX_EXPANSIONS)
/// is what made large maps crawl — it visited the whole connected region on EVERY
/// gather/deposit tick.
const REACH_CAP: usize = 1024;

fn move_patch(astar: &mut AStar, seed: u32, occ: &std::collections::HashSet<i32>, from: V2, to: V2) -> Option<MovePatch> {
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty));
    let snap = nearest_reachable_passable_grid(&passable, from, to, REACH_CAP)
        .unwrap_or_else(|| nearest_passable_grid(&passable, to.x, to.y));
    let path = astar.find_path(&passable, from.x, from.y, snap.x, snap.y, MAX_EXPANSIONS);
    if path.is_empty() {
        None
    } else {
        let target = path[0];
        Some(MovePatch { path, target })
    }
}

#[derive(Clone, Copy)]
struct Dropoff {
    owner: u64,
    pos: V2,
    footprint: i32,
    is_keep: bool,
    food_dropoff: bool,
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
    q_buildings: Query<(&Pos, &Building, &Owner)>,
    mut q_nodes: Query<(&GameId, &Pos, &mut ResourceNode, &MatchId)>,
    mut q_players: Query<(Entity, &mut Player)>,
    mut q_units: Query<(&GameId, &Pos, &Owner, &MatchId, &mut Unit)>,
    mut stats: ResMut<crate::MatchStats>,
) {
    let seed = cfg.seed;

    // ── read-only snapshots ──────────────────────────────────────────────────
    let occupants: Vec<Occupant> =
        q_buildings.iter().map(|(p, b, _)| Occupant { kind: b.kind, pos: p.pos }).collect();
    let occ = occupancy_set(&occupants, false);

    let dropoffs: Vec<Dropoff> = q_buildings
        .iter()
        .map(|(p, b, owner)| {
            let def = building_def(b.kind);
            Dropoff {
                owner: owner.0,
                pos: p.pos,
                footprint: def.footprint,
                is_keep: b.kind == BuildingKind::Keep,
                food_dropoff: def.food_dropoff,
            }
        })
        .collect();

    let mut node_map: HashMap<u64, V2> = HashMap::new();
    let mut nodes_list: Vec<(u64, V2, u64)> = Vec::new();
    for (gid, p, _n, m) in q_nodes.iter() {
        node_map.insert(gid.0, p.pos);
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
            let accepts = d.is_keep || (d.food_dropoff && carry == ResourceType::Food);
            if !accepts {
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

    // nearest node id in `match_id`, optionally skipping `skip`. Filtered to the
    // walker's connected region so a gatherer never locks onto an island it can
    // not reach (the old skip-one retarget ping-ponged between two unreachable
    // nodes forever).
    let nearest_node = |from: V2, match_id: u64, skip: u64| -> Option<u64> {
        let mut best: Option<u64> = None;
        let mut best_d = Fx::MAX;
        for (id, pos, mid) in &nodes_list {
            if *mid != match_id || *id == skip {
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
        best
    };

    // ── mutate units ─────────────────────────────────────────────────────────
    for (_gid, pos, owner, mid, mut u) in &mut q_units {
        if u.garrisoned_in != 0 || u.gather_state == GatherState::Idle || !statuses.simulates(mid.0) {
            continue;
        }
        let here = pos.pos;
        let mid = mid.0;

        match u.gather_state {
            GatherState::ToResource => {
                let Some(node_pos) = node_map.get(&u.target_node).copied() else {
                    retarget(&mut u, here, mid, 0, &nearest_node);
                    continue;
                };
                if !saladin_sim::node_reachable(seed, here, node_pos) {
                    // across water — retarget (region-filtered) instead of
                    // marching to the shore and discovering it there
                    let skip = u.target_node;
                    retarget(&mut u, here, mid, skip, &nearest_node);
                    continue;
                }
                if dist(here, node_pos) <= HARVEST_RANGE {
                    u.gather_state = GatherState::Harvesting;
                    u.harvest_timer = Fx::ZERO;
                    u.has_target = false;
                } else if !u.has_target {
                    match move_patch(&mut scratch.0, seed, &occ, here, node_pos) {
                        Some(p) => {
                            u.path = p.path;
                            u.path_idx = 0;
                            u.target = p.target;
                            u.has_target = true;
                        }
                        None => {
                            let skip = u.target_node;
                            retarget(&mut u, here, mid, skip, &nearest_node);
                        }
                    }
                }
            }
            GatherState::Harvesting => {
                let Some(node_e) = index.get(u.target_node) else {
                    retarget(&mut u, here, mid, 0, &nearest_node);
                    continue;
                };
                let Ok((_, _, mut node, _)) = q_nodes.get_mut(node_e) else {
                    retarget(&mut u, here, mid, 0, &nearest_node);
                    continue;
                };
                // another harvester may have emptied it earlier THIS tick (its
                // despawn is deferred): treat 0-remaining as gone, never dupe
                if node.remaining <= 0 {
                    retarget(&mut u, here, mid, 0, &nearest_node);
                    continue;
                }
                let timer = u.harvest_timer + AI_DT;
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
                        retarget(&mut u, here, mid, 0, &nearest_node);
                    }
                } else if !u.has_target {
                    match move_patch(&mut scratch.0, seed, &occ, here, drop.pos) {
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
            GatherState::Idle => {}
        }
    }
}

/// Head to the nearest remaining node in the unit's own match, else idle.
fn retarget(
    u: &mut Unit,
    here: V2,
    match_id: u64,
    skip: u64,
    nearest_node: &impl Fn(V2, u64, u64) -> Option<u64>,
) {
    match nearest_node(here, match_id, skip) {
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
