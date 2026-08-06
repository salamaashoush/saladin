use crate::components::{
    Building, Crop, FieldOf, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit, reapable,
};
use crate::{GameIndex, MatchStatuses, PathScratch, WorldConfig};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    AuraTarget, DEPOSIT_RANGE, Domain, Fx, GatherState, harvest_reach,
    HARVEST_TIME, MAX_EXPANSIONS, Occupant, ResourceType, V2, WorkAura, approach_tile_in, berth_of, building_def, dist, domain_passable, is_sailable, move_cost_at,
    gate_blocks, nearest_reachable_passable_grid, occupancy_set, operational,
    reach_budget, region_at, region_grid, res_bit, sea_reachable, tile_key, unit_def,
    water_region_at, water_region_grid,
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
#[allow(clippy::too_many_arguments)]
fn move_patch(
    scratch: &mut PathScratch,
    seed: u32,
    dom: Domain,
    occ: &std::collections::HashSet<i32>,
    gates: &[(i32, u64)],
    viewer: u64,
    from: V2,
    to: V2,
) -> Option<MovePatch> {
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        domain_passable(seed, dom, tx, ty) && !occ.contains(&k) && !gate_blocks(gates, k, viewer)
    };
    // Open water costs the same everywhere: there is no naval move-cost grid,
    // and the flat cost is what keeps the straight-line fast path — which IS
    // the common case for a haul that never leaves the bay.
    let cost = |tx: i32, ty: i32| match dom {
        Domain::Land => move_cost_at(seed, tx, ty),
        Domain::Sea => Fx::ONE,
    };
    let (regions, region) = match dom {
        Domain::Land => (region_grid(seed), region_at(seed, from.x, from.y)),
        Domain::Sea => (water_region_grid(seed), water_region_at(seed, from.x, from.y)),
    };
    if let Some(goal) = approach_tile_in(regions, region, &passable, from, to, APPROACH_RADIUS) {
        let path =
            scratch.0.find_path_costed_in(
                &passable,
                &cost,
                from.x,
                from.y,
                goal.x,
                goal.y,
                MAX_EXPANSIONS,
                dom.smoothing(),
            );
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
        scratch.0.find_path_costed_in(
            &passable,
            &cost,
            from.x,
            from.y,
            reach.at.x,
            reach.at.y,
            MAX_EXPANSIONS,
            dom.smoothing(),
        );
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
    /// Where a hull ties up. A haul that crosses the land/sea boundary is the
    /// one deposit the region filter cannot answer on its own — the store is on
    /// ground the boat can never enter, and its berth is the address that means
    /// the same thing in both domains.
    berth: Option<V2>,
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
    mut q_nodes: Query<(&GameId, &Pos, &mut ResourceNode, &MatchId, Option<&mut Crop>)>,
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
            (def.accepts != 0).then(|| Dropoff {
                owner: owner.0,
                pos: p.pos,
                footprint: def.footprint,
                accepts: def.accepts,
                // only a waterside structure can ever have one, and only those
                // pay for the lookup
                berth: def
                    .requires_water
                    .then(|| berth_of(seed, def.footprint, p.pos))
                    .flatten(),
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
    // One walk, both directions. Field to footprint is what a reaper's reach
    // needs; farm to field is what lets the hand tending a plot be handed the
    // very crop he has been working the moment it comes in. Construction cannot
    // make that handoff — a ripe crop is a NODE, and it only knows buildings.
    let mut field_nodes: HashMap<u64, i32> = HashMap::new();
    let mut farm_field: HashMap<u64, u64> = HashMap::new();
    for (g, f) in q_fields.iter() {
        field_nodes.insert(g.0, footprint_of.get(&f.0).copied().unwrap_or(1));
        farm_field.insert(f.0, g.0);
    }
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

    // Position, the reach a harvester needs to work it, and whether there is
    // anything to take. The span of unstandable ground under a node is what the
    // reach has to clear: a farm's whole footprint for a crop, the water tile
    // itself for a school of fish. A GROWING crop stays in the map — a reaper
    // already walking to its own field keeps it — but never enters the candidate
    // list, so `best_node` can no longer offer anybody a field that is not in.
    let mut node_map: HashMap<u64, (V2, Fx, bool, bool)> = HashMap::new();
    let mut nodes_list: Vec<(u64, V2, u64, Domain)> = Vec::new();
    for (gid, p, n, m, crop) in q_nodes.iter() {
        let field = field_nodes.get(&gid.0).copied();
        let cut = reapable(n, field.is_some(), crop);
        node_map.insert(gid.0, (p.pos, node_reach(seed, p.pos, field), cut, n.regen > 0));
        if cut {
            nodes_list.push((gid.0, p.pos, m.0, node_domain(seed, p.pos)));
        }
    }

    // player_id → entity
    let player_ent: HashMap<u64, Entity> =
        q_players.iter().map(|(e, p)| (p.player_id, e)).collect();

    // nearest dropoff for (owner, carry_type) from a position. A hull can only
    // bank where it can tie up, and only on water its own hull can cross —
    // otherwise a skiff picks the keep two tiles inland and strands itself.
    let nearest_dropoff = |owner: u64, carry: ResourceType, from: V2, dom: Domain| -> Option<Dropoff> {
        let mut best: Option<Dropoff> = None;
        let mut best_d = Fx::MAX;
        for d in &dropoffs {
            if d.owner != owner {
                continue;
            }
            if d.accepts & res_bit(carry) == 0 {
                continue;
            }
            if dom == Domain::Sea && !d.berth.is_some_and(|b| sea_reachable(seed, from, b)) {
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
        // An idle hand still booked on a job site falls THROUGH to the arm below:
        // he is the one case the tend -> reap -> haul -> tend loop can lose, and
        // nothing else in the sim is looking for him.
        if u.garrisoned_in != 0
            || (u.gather_state == GatherState::Idle && u.job_site == 0)
            || !statuses.simulates(mid.0)
        {
            continue;
        }
        let here = pos.pos;
        let mid = mid.0;
        let dom = unit_def(u.kind).domain;

        match u.gather_state {
            GatherState::ToResource => {
                let Some((node_pos, reach, _, _)) = node_map.get(&u.target_node).copied() else {
                    retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, 0));
                    continue;
                };
                // A fishery is worked from a boat and a seam from the shore.
                // The filter is here as well as in `best_node` because an order
                // and a save both put a node in `target_node` without asking,
                // and `harvest_reach` on a water node is 1.7 tiles — enough for
                // a peasant on the beach to net the first tile of sea.
                if node_domain(seed, node_pos) != dom
                    || !reachable(seed, dom, here, node_pos)
                {
                    let skip = u.target_node;
                    retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, skip));
                    continue;
                }
                if dist(here, node_pos) <= reach {
                    u.gather_state = GatherState::Harvesting;
                    u.harvest_timer = Fx::ZERO;
                    u.has_target = false;
                } else if !u.has_target {
                    let patch = move_patch(&mut scratch, seed, dom, &occ, &gates, owner.0, here, node_pos)
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
                            retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, skip));
                        }
                    }
                }
            }
            GatherState::Harvesting => {
                let Some(node_e) = index.get(u.target_node) else {
                    if !back_to_work(&mut u) {
                        retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, 0));
                    }
                    continue;
                };
                let Ok((_, npos, mut node, _, mut crop)) = q_nodes.get_mut(node_e) else {
                    if !back_to_work(&mut u) {
                        retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, 0));
                    }
                    continue;
                };
                if node_domain(seed, npos.pos) != dom {
                    let skip = u.target_node;
                    if !back_to_work(&mut u) {
                        retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, skip));
                    }
                    continue;
                }
                let is_field = field_nodes.contains_key(&u.target_node);
                // A crop that is still growing cannot be cut. A hand who has a
                // plot of his own goes back to TENDING it — that is the whole
                // point of standing there. A hand who has NOT takes other work:
                // a crop can go from ripe to stubble while he is still walking to
                // it, because somebody else cut it or because it lodged, and a
                // man stood over it is doing nothing while calling it Harvesting
                // — which no idle census, no balancer and no `construction` pass
                // will ever find. That is a strand wearing a working man's hat.
                if is_field && crop.as_deref().is_some_and(|c| !c.ripe) {
                    if u.harvest_timer != Fx::ZERO {
                        u.harvest_timer = Fx::ZERO;
                    }
                    if !back_to_work(&mut u) {
                        retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, 0));
                    }
                    continue;
                }
                // another harvester may have emptied it earlier THIS tick (its
                // despawn is deferred): treat 0-remaining as gone, never dupe
                if node.remaining <= 0 {
                    // A boat over a school it has just fished out WAITS for it.
                    // `gather` never looks at an idle hand with no job site, so
                    // a hull that stands down here never fishes again — and the
                    // node itself leaves and re-enters the candidate list, so
                    // holding needs no state of its own. A boat on station is
                    // always empty; the harvest step always banks.
                    if dom == Domain::Sea && node.regen > 0 {
                        if u.harvest_timer != Fx::ZERO {
                            u.harvest_timer = Fx::ZERO;
                        }
                        continue;
                    }
                    if !back_to_work(&mut u) {
                        retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, 0));
                    }
                    continue;
                }
                // a hut's nets over its fishery, a granary's hands over its
                // fields: whichever aura covers this node speeds the work
                let target = if is_field {
                    Some(AuraTarget::Field)
                } else if node.res_type == ResourceType::Food
                    && is_sailable(seed, npos.pos.x.to_num::<i32>(), npos.pos.y.to_num::<i32>())
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
                // A crop being cut is not a crop standing neglected: the sickle
                // resets the clock that lodges it, so a field nobody can reap in
                // one grace is not punished for being big. The last sheaf ends
                // the season on the spot — waiting for the economy tick would
                // leave a bare plot drawn as standing gold for two seconds.
                if let Some(c) = crop.as_deref_mut() {
                    let next = Crop { ripe: c.ripe && rem > 0, standing: 0 };
                    if *c != next {
                        *c = next;
                    }
                }
                // ONLY finite deposits retire. A field reaped bare is stubble,
                // not a hole: it re-sows itself next economy tick, and deleting
                // it here is what turned worked farms into 50 wood of scenery.
                // A fishery is renewable in its own right, so `regen` answers
                // this on its own — the old "is a hut near it" shim answered it
                // for the WOOD and STONE beside a hut too, and every one of
                // those became a permanent zero-remaining row.
                if rem <= 0 && node.regen == 0 {
                    commands.entity(node_e).despawn();
                }
                u.carrying = take;
                u.harvest_timer = Fx::ZERO;
                u.gather_state = GatherState::ToStockpile;
            }
            GatherState::ToStockpile => {
                let Some(drop) = nearest_dropoff(owner.0, u.carry_type, here, dom) else {
                    u.gather_state = GatherState::Idle;
                    u.has_target = false;
                    continue;
                };
                // A hull steers for the berth, not the door. The banking test
                // below is unchanged and still measures the BUILDING: a
                // waterside store always has a sailable tile abutting it, so
                // standing on the berth is standing at the store.
                let goal = match dom {
                    Domain::Sea => drop.berth.unwrap_or(drop.pos),
                    Domain::Land => drop.pos,
                };
                if !reachable(seed, dom, here, goal) {
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
                    // back to the same node while there is still something in it
                    // — a field cut to stubble is not that, so the carrier goes
                    // back to the plot he belongs to (or takes other work) rather
                    // than standing in the furrows waiting out a whole season
                    // A boat goes back to its school even when the school is
                    // empty: it holds station over the water it fished until the
                    // fish come back. Standing it down instead loses it for the
                    // match — `gather` never looks at an idle hand with no job
                    // site, and a boat has none.
                    let mine = node_map.get(&u.target_node);
                    let back = mine.is_some_and(|(_, _, cut, renews)| {
                        *cut || (dom == Domain::Sea && *renews)
                    });
                    if back {
                        u.gather_state = GatherState::ToResource;
                    } else if !back_to_work(&mut u) {
                        retarget(&mut u, best_node(&mut scratch.1, &look, dom, here, mid, owner.0, 0));
                    }
                } else if !u.has_target {
                    match move_patch(&mut scratch, seed, dom, &occ, &gates, owner.0, here, goal) {
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
            // A builder is driven by the construction loop — until the field he
            // is tending comes in. Then the same crew, on the same `job_site`,
            // with no order from anybody, becomes the reaping crew. This runs at
            // the 200 ms gather cadence rather than the construction one because
            // the thing being watched is a NODE.
            GatherState::Constructing => {
                let Some(&field) = farm_field.get(&u.job_site) else { continue };
                if unit_def(u.kind).carry > 0
                    && node_map.get(&field).is_some_and(|(_, _, cut, _)| *cut)
                {
                    u.target_node = field;
                    u.gather_state = GatherState::ToResource;
                    u.has_target = false;
                }
            }
            // A hand still booked on a standing job site is not out of work. The
            // reaping excursion keeps `job_site`, and every way it can end badly
            // — the crop gone, the drop-off razed, no route home — stands the man
            // down HOLDING it. Construction can never pick him back up (it only
            // ever looks at hands already in `Constructing`), so this is the one
            // place that closes the loop: without it a farmhand who lost his
            // stockpile stands in the furrows for the rest of the match.
            GatherState::Idle => {
                if footprint_of.contains_key(&u.job_site) {
                    back_to_work(&mut u);
                } else {
                    u.job_site = 0; // the site he was booked on is rubble
                }
            }
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

/// A hand with a job site is not homeless: he goes back to the plot he was put
/// on instead of looking for the nearest rock. This is the return leg of
/// tend -> reap -> haul -> tend, and the reason the round trip costs no orders —
/// nothing on the reaping path clears `job_site`, so the return address survives
/// the whole excursion. An explicit Move / Gather / Attack is what gives a hand
/// back to the town.
///
/// Checked BEFORE `best_node`, never after: the flood that answers "what else
/// could this man work" is the expensive half, and a farmhand never needs it.
fn back_to_work(u: &mut Unit) -> bool {
    if u.job_site == 0 {
        return false;
    }
    u.gather_state = GatherState::Constructing;
    u.target_node = 0;
    u.has_target = false;
    true
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
/// WATER, not "impassable" — a cliff is impassable and dry, and a herd grazing
/// under one was being handed the fishery's span.
pub(crate) fn node_reach(seed: u32, pos: V2, field_footprint: Option<i32>) -> Fx {
    let span = match field_footprint {
        Some(fp) => fp,
        None if is_sailable(seed, pos.x.to_num::<i32>(), pos.y.to_num::<i32>()) => 1,
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

/// Which domain a node belongs to, and therefore who may work it. "Is this
/// aquatic" is a pure function of the GROUND — it never changes, it is already
/// cached O(1), and it needs no field on the row, no word in the StateHash and
/// no save migration. The old test was `!is_passable`, which called a cliff
/// face the sea.
pub(crate) fn node_domain(seed: u32, at: V2) -> Domain {
    if is_sailable(seed, at.x.to_num::<i32>(), at.y.to_num::<i32>()) {
        Domain::Sea
    } else {
        Domain::Land
    }
}

/// "Could this mover ever get there" — the region filter of its own domain.
fn reachable(seed: u32, dom: Domain, from: V2, to: V2) -> bool {
    match dom {
        Domain::Land => saladin_sim::node_reachable(seed, from, to),
        Domain::Sea => sea_reachable(seed, from, to),
    }
}

/// Everything `best_node` needs to judge a node without touching the world.
struct Look<'a> {
    seed: u32,
    occ: &'a std::collections::HashSet<i32>,
    gates: &'a [(i32, u64)],
    nodes: &'a [(u64, V2, u64, Domain)],
    node_map: &'a HashMap<u64, (V2, Fx, bool, bool)>,
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
#[allow(clippy::too_many_arguments)]
fn best_node(
    flood: &mut saladin_sim::Flood,
    look: &Look,
    dom: Domain,
    from: V2,
    match_id: u64,
    owner: u64,
    skip: u64,
) -> Option<u64> {
    let seed = look.seed;
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        domain_passable(seed, dom, tx, ty)
            && !look.occ.contains(&k)
            && !gate_blocks(look.gates, k, owner)
    };
    if !flood.explore(&passable, from, MAX_EXPANSIONS) {
        return None;
    }
    let mut rejected = [0u64; NODE_TRIES];
    let mut n_rej = 0usize;
    loop {
        let mut best: Option<u64> = None;
        let mut best_d = Fx::MAX;
        for (id, pos, mid, ndom) in look.nodes {
            // Domain first, and BEFORE the retry budget: `NODE_TRIES` is 8, and
            // a shore start with eight schools nearer than any rock would burn
            // every retry on water and stand the peasant down.
            if *ndom != dom || *mid != match_id || *id == skip || rejected[..n_rej].contains(id) {
                continue;
            }
            if !reachable(seed, dom, from, *pos) {
                continue;
            }
            let dd = saladin_sim::dist2(from, *pos);
            if dd < best_d {
                best_d = dd;
                best = Some(*id);
            }
        }
        let id = best?;
        let (npos, reach, _, _) = look.node_map.get(&id).copied()?;
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
