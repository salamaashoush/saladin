use crate::commands::{finish_building, occupancy_and_gates, spawn_trained};
use crate::components::*;
use crate::{MatchStatuses, PathScratch, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::*;
use std::collections::HashSet;

const DT: Fx = AI_DT;
/// Labour a full repair costs when the def has no build time to scale against
/// (the Keep, and the Watchtower a Tower becomes). Without it `work_step`
/// returns a whole job per tick and a wrecked keep snaps back on one hammer.
const MEND_REFERENCE: Fx = saladin_sim::fx!("30");

struct Job {
    id: u64,
    entity: Entity,
    owner: u64,
    mtch: u64,
    pos: V2,
    row: Building,
    max_hp: i32,
    build_time: Fx,
    crew: i32,
}

struct Hand {
    entity: Entity,
    pos: V2,
    owner: u64,
    site: u64,
}

/// Construction, repair, upgrade and production — one loop, because they are
/// one mechanic. A builder adds `work` to an unfinished job and `hp` to
/// anything below full; `hp` is authoritative and ADDITIVE (work adds, damage
/// subtracts), so a site under fire needs no special case and a half-built hall
/// is a real half-health target.
///
/// Exclusive because completing a job spawns rows (a farm's field, a queued
/// unit) and both need the deterministic id allocator.
pub fn construction(world: &mut World) {
    let jobs = snapshot_jobs(world);
    let hands = snapshot_hands(world);
    // A town with nobody holding a hammer, no queue and no stale crew count costs
    // two queries per construction tick and not one pathfind. `builders == 0` is
    // the load-bearing half now that a farm always wants hands: without it a farm
    // whose crew walked away would keep a phantom crew, and its field would grow
    // forever on labour nobody is doing. With no hands anywhere there is nothing
    // for `crew_up` to place and nothing for `advance` to bank — `labour` and
    // `mend` both need a crew — so skipping is exact, not an approximation.
    if hands.is_empty() && jobs.iter().all(|j| j.row.builders == 0 && j.row.queue_len == 0) {
        return;
    }
    let jobs = crew_up(world, jobs, &hands);
    advance(world, &jobs);
}

fn snapshot_jobs(world: &mut World) -> Vec<Job> {
    let mask_of: bevy_platform::collections::HashMap<u64, u64> = {
        let mut q = world.query::<&Player>();
        q.iter(world).map(|p| (p.player_id, p.tech_mask)).collect()
    };
    let mut jobs: Vec<Job> = {
        let mut q = world.query::<(Entity, &GameId, &Owner, &MatchId, &Pos, &Building)>();
        q.iter(world)
            .map(|(entity, g, o, m, p, b)| {
                let mask = mask_of.get(&o.0).copied().unwrap_or(0);
                let def = effective_building_def(b.kind, mask);
                let build_time = match b.state {
                    BuildState::Upgrading => building_def(b.kind).upgrade_time,
                    _ => def.build_time,
                };
                Job {
                    id: g.0,
                    entity,
                    owner: o.0,
                    mtch: m.0,
                    pos: p.pos,
                    row: *b,
                    max_hp: def.max_hp,
                    build_time,
                    crew: 0,
                }
            })
            .collect()
    };
    jobs.sort_by_key(|j| j.id);
    jobs
}

fn snapshot_hands(world: &mut World) -> Vec<Hand> {
    let mut hands: Vec<(u64, Hand)> = {
        let mut q = world.query::<(Entity, &GameId, &Owner, &Pos, &Unit)>();
        q.iter(world)
            .filter(|(_, _, _, _, u)| u.gather_state == GatherState::Constructing)
            .map(|(entity, g, o, p, u)| {
                (g.0, Hand { entity, pos: p.pos, owner: o.0, site: u.job_site })
            })
            .collect()
    };
    hands.sort_by_key(|(id, _)| *id);
    hands.into_iter().map(|(_, h)| h).collect()
}

/// Count the hands standing at each job, walk the rest toward theirs, and send
/// anyone whose site is gone or finished looking for other work nearby.
fn crew_up(world: &mut World, mut jobs: Vec<Job>, hands: &[Hand]) -> Vec<Job> {
    let seed = world.resource::<WorldConfig>().seed;
    let statuses: Vec<(u64, bool)> = jobs
        .iter()
        .map(|j| (j.mtch, world.resource::<MatchStatuses>().simulates(j.mtch)))
        .collect();
    let index: bevy_platform::collections::HashMap<u64, usize> =
        jobs.iter().enumerate().map(|(i, j)| (j.id, i)).collect();

    // Building the occupancy set is a walk over every structure on the map, and
    // a farm crew that never leaves its field would otherwise pay for it every
    // construction tick for the rest of the match. Only a hand that needs a NEW
    // path needs it, so the walkers are collected first and the set is built at
    // most once, only if there are any.
    let mut walkers: Vec<(&Hand, V2)> = Vec::new();
    for h in hands {
        let Some(&ji) = index.get(&h.site) else {
            reassign(world, h, &jobs);
            continue;
        };
        if !statuses[ji].1 {
            continue;
        }
        let j = &jobs[ji];
        if j.owner != h.owner || !wants_work(j) {
            reassign(world, h, &jobs);
            continue;
        }
        let half = Fx::from_num(building_def(j.row.kind).footprint) / Fx::from_num(2);
        if dist(h.pos, j.pos) <= BUILD_RANGE + half {
            jobs[ji].crew += 1;
            if let Some(mut u) = world.get_mut::<Unit>(h.entity) {
                u.has_target = false;
            }
            continue;
        }
        if world.get::<Unit>(h.entity).is_some_and(|u| u.has_target) {
            continue; // already walking there
        }
        walkers.push((h, j.pos));
    }
    if !walkers.is_empty() {
        let (occ, gates) = occupancy_and_gates(world, false);
        for (h, to) in walkers {
            walk_to(world, h, to, seed, &occ, &gates);
        }
    }
    jobs
}

/// A job wants hands while it is unfinished or hurt — and a standing FARM always
/// does. Tending a field is the same committed-builder loop as raising a wall:
/// the crew that finished the plot stays in it, `Repair` is the order that puts
/// more hands in, and an explicit Move/Gather/Attack is how the player takes one
/// back. No new command verb, no second crew mechanic.
fn wants_work(j: &Job) -> bool {
    needs_raising(j) || (operational(j.row.state) && tends_a_field(j.row.kind))
}

/// What a HOMELESS hand goes looking for. Deliberately not `wants_work`: a wall
/// crew whose segment just finished must not be silently drained into the
/// nearest wheat field.
fn needs_raising(j: &Job) -> bool {
    j.row.state != BuildState::Complete || j.row.hp < j.max_hp
}

fn tends_a_field(kind: BuildingKind) -> bool {
    building_def(kind).min_fertility > Fx::ZERO
}

/// The nearest other job of this hand's owner still wanting work, within
/// `SITE_REASSIGN_RADIUS`; ties break on the lowest GameId, never on iteration
/// order. Nothing in reach means back to the fields.
fn reassign(world: &mut World, h: &Hand, jobs: &[Job]) {
    let mut best: Option<(Fx, u64)> = None;
    let r2 = SITE_REASSIGN_RADIUS * SITE_REASSIGN_RADIUS;
    for j in jobs {
        if j.owner != h.owner || !needs_raising(j) {
            continue;
        }
        let d = dist2(h.pos, j.pos);
        if d > r2 {
            continue;
        }
        match best {
            Some((bd, bid)) if d > bd || (d == bd && j.id >= bid) => {}
            _ => best = Some((d, j.id)),
        }
    }
    if let Some(mut u) = world.get_mut::<Unit>(h.entity) {
        match best {
            Some((_, id)) => {
                u.job_site = id;
                u.has_target = false;
            }
            None => {
                u.job_site = 0;
                u.gather_state = GatherState::Idle;
                u.has_target = false;
            }
        }
    }
}

fn walk_to(
    world: &mut World,
    h: &Hand,
    to: V2,
    seed: u32,
    occ: &HashSet<i32>,
    gates: &[(i32, u64)],
) {
    let owner = h.owner;
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(gates, k, owner)
    };
    // the approach tile must be one the builder can actually WALK to: snapping
    // to the nearest passable tile alone puts a crew on the far side of the
    // very wall it is raising
    let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
    let path = {
        let mut scratch = world.resource_mut::<PathScratch>();
        let snap = approach_tile(seed, &passable, h.pos, to, 3).or_else(|| {
            nearest_reachable_passable_grid(
                &mut scratch.1,
                &passable,
                h.pos,
                to,
                reach_budget(dist(h.pos, to)),
            )
            .map(|r| r.at)
        });
        match snap {
            Some(s) => {
                scratch.0.find_path_costed(&passable, &cost, h.pos.x, h.pos.y, s.x, s.y, MAX_EXPANSIONS)
            }
            None => Vec::new(),
        }
    };
    if path.is_empty() {
        // no route to the job at all — back to the fields rather than a failing
        // A* every tick forever
        if let Some(mut u) = world.get_mut::<Unit>(h.entity) {
            u.job_site = 0;
            u.gather_state = GatherState::Idle;
        }
        return;
    }
    if let Some(mut u) = world.get_mut::<Unit>(h.entity) {
        u.target = path[0];
        u.path = path;
        u.path_idx = 0;
        u.has_target = true;
    }
}

/// Bank this tick's labour and this tick's training time, in GameId order so
/// the ids handed to newly spawned rows are identical on every peer.
fn advance(world: &mut World, jobs: &[Job]) {
    for j in jobs {
        if !world.resource::<MatchStatuses>().simulates(j.mtch) {
            continue;
        }
        if j.row.builders != j.crew
            && let Some(mut b) = world.get_mut::<Building>(j.entity)
        {
            b.builders = j.crew;
        }
        match j.row.state {
            BuildState::Site | BuildState::Upgrading => labour(world, j),
            BuildState::Complete => {
                if j.row.hp < j.max_hp && j.crew > 0 {
                    mend(world, j);
                }
            }
        }
        if operational(j.row.state) && j.row.queue_len > 0 {
            produce(world, j);
        }
    }
}

fn labour(world: &mut World, j: &Job) {
    let step = work_step(j.crew, DT, j.build_time);
    if step <= Fx::ZERO {
        return;
    }
    let work = j.row.work + step;
    let gain = hp_step(j.max_hp, step);
    if work < Fx::ONE {
        if let Some(mut b) = world.get_mut::<Building>(j.entity) {
            b.work = work;
            b.hp = (b.hp + gain).min(j.max_hp);
        }
        return;
    }
    // finished: an upgrade becomes what it was rising into, and both stand at
    // the full health of what they now are
    let becomes = if j.row.state == BuildState::Upgrading { j.row.target_kind } else { j.row.kind };
    let mask = {
        let mut q = world.query::<&Player>();
        q.iter(world).find(|p| p.player_id == j.owner).map(|p| p.tech_mask).unwrap_or(0)
    };
    let full = effective_building_def(becomes, mask).max_hp;
    if let Some(mut b) = world.get_mut::<Building>(j.entity) {
        b.kind = becomes;
        b.target_kind = becomes;
        b.state = BuildState::Complete;
        b.work = Fx::ZERO;
        b.builders = 0;
        b.hp = full;
    }
    // the crew is NOT released here: next tick `crew_up` sees a job that wants
    // nothing and hands each hand the nearest job still standing, which is what
    // walks a wall drag segment by segment instead of dropping the line.
    finish_building(world, j.id);
}

/// Repair rides the same builder curve as construction, charged against the
/// CUMULATIVE cost curve so a hundred small floors cannot add up to a free
/// rebuild.
fn mend(world: &mut World, j: &Job) {
    let reference = if j.build_time > Fx::ZERO { j.build_time } else { MEND_REFERENCE };
    let step = work_step(j.crew, DT, reference);
    let gain = hp_step(j.max_hp, step).min(j.max_hp - j.row.hp);
    if gain <= 0 {
        return;
    }
    let def = building_def(j.row.kind);
    let done = j.max_hp - j.row.hp;
    let before = repair_charge(&def.cost, done, j.max_hp);
    let after = repair_charge(&def.cost, done - gain, j.max_hp);
    let charge = ResourceCost::new(
        before.wood - after.wood,
        before.stone - after.stone,
        before.food - after.food,
        before.gold - after.gold,
    );
    {
        let mut q = world.query::<&mut Player>();
        let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == j.owner) else { return };
        if !p.stock.can_afford(&charge) {
            return;
        }
        p.stock.pay(&charge);
    }
    if let Some(mut b) = world.get_mut::<Building>(j.entity) {
        b.hp = (b.hp + gain).min(j.max_hp);
    }
}

fn produce(world: &mut World, j: &Job) {
    let Some(kind) = UnitKind::from_u8(j.row.queue[0]) else { return };
    let work = j.row.train_work + DT;
    if work < unit_def(kind).train_time {
        if let Some(mut b) = world.get_mut::<Building>(j.entity) {
            b.train_work = work;
        }
        return;
    }
    spawn_trained(world, j.id);
}
