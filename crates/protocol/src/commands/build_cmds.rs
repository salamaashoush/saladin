use super::garrison_cmds::eject_all;
use super::{building_occupancy, clamp_world, find_owned, spawn};
use crate::components::*;
use crate::{PathScratch, SimRng, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::*;
use std::collections::HashSet;

/// Tiles the town flood may visit. TOWN_RADIUS is 28, so a town's open ground
/// is a few thousand tiles; this is generous enough that the cap never decides
/// a placement and small enough to stay a rounding error once per context.
const TOWN_REACH_BUDGET: usize = 32768;

/// Population headroom for one more of `kind`. Queued units are charged at
/// ENQUEUE, and only OPERATIONAL structures grant housing — a hole in the
/// ground shelters nobody.
fn pop_room(world: &mut World, owner: u64, kind: UnitKind) -> bool {
    let mut bq = world.query::<(&Owner, &Building)>();
    let (mut cap, mut queued) = (0i32, 0i32);
    for (o, b) in bq.iter(world) {
        if o.0 != owner {
            continue;
        }
        if operational(b.state) {
            cap += building_def(b.kind).pop;
        }
        for i in 0..b.queue_len as usize {
            queued += UnitKind::from_u8(b.queue[i]).map_or(1, |k| unit_def(k).pop_cost);
        }
    }
    let mut uq = world.query::<(&Owner, &Unit)>();
    let pop: i32 = uq
        .iter(world)
        .filter(|(o, _)| o.0 == owner)
        .map(|(_, u)| unit_def(u.kind).pop_cost)
        .sum();
    pop + queued + unit_def(kind).pop_cost <= cap
}

/// Only workers raise buildings. ROLE, not `carry > 0`: a fishing skiff carries
/// more than a peasant and cannot reach a building site at all.
fn is_builder(kind: UnitKind) -> bool {
    unit_def(kind).builds()
}

/// The owner's lowest-`GameId` finished structure that trains `kind`. Raw ECS
/// iteration order differs between a live world and the same world restored
/// from a save (restore re-spawns sorted by id), so the pick is by id or it is
/// a desync waiting for someone to save.
fn lowest_trainer(world: &mut World, owner: u64, kind: UnitKind) -> Option<u64> {
    let mut q = world.query::<(&GameId, &Owner, &Building)>();
    q.iter(world)
        .filter(|(_, o, b)| {
            o.0 == owner && operational(b.state) && building_def(b.kind).trains.contains(&kind)
        })
        .map(|(g, _, _)| g.0)
        .min()
}

/// The legacy "train one of these somewhere" order: forwarded to the owner's
/// lowest-id operational hall, so production is per-building underneath while
/// every existing caller keeps working.
pub(crate) fn train(world: &mut World, owner: u64, kind: UnitKind) -> bool {
    let Some(building) = lowest_trainer(world, owner, kind) else { return false };
    train_at(world, owner, building, kind)
}

/// Queue `kind` at a NAMED building (prereq + pop + cost checked, paid up
/// front). The unit appears once the construction loop has worked the order
/// through the queue.
pub(crate) fn train_at(world: &mut World, owner: u64, building: u64, kind: UnitKind) -> bool {
    let def = unit_def(kind);
    // Faction exclusivity is a RULE, not a HUD filter. `BuildingDef.trains` is
    // the union of both rosters (it has to be — the discriminant is an index),
    // so without this the command layer happily queues a Mamluk for a Crusader
    // and the whole faction design is decoration a hand-built packet ignores.
    let faction = {
        let mut q = world.query::<&Player>();
        q.iter(world).find(|p| p.player_id == owner).map(|p| p.faction)
    };
    match faction {
        Some(f) if fields_unit(kind, f) => {}
        _ => return false,
    }
    let owned = super::owned_building_kinds(world, owner);
    if !has_prereq(&owned, def.requires) {
        return false;
    }
    let Some(be) = find_owned(world, owner, building) else { return false };
    let room = match world.get::<Building>(be) {
        Some(b) => {
            operational(b.state)
                && building_def(b.kind).trains.contains(&kind)
                && (b.queue_len as usize) < QUEUE_CAP
        }
        None => false,
    };
    if !room || !pop_room(world, owner, kind) {
        return false;
    }
    {
        let mut q = world.query::<&mut Player>();
        let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) else { return false };
        if !p.stock.can_afford(&def.cost) {
            return false;
        }
        p.stock.pay(&def.cost);
    }
    let Some(mut b) = world.get_mut::<Building>(be) else { return false };
    let slot = b.queue_len as usize;
    b.queue[slot] = kind as u8;
    b.queue_len += 1;
    true
}

/// Drop the LAST order in a building's queue and hand its cost back.
pub(crate) fn cancel_train(world: &mut World, owner: u64, building: u64) {
    let Some(be) = find_owned(world, owner, building) else { return };
    let kind = {
        let Some(mut b) = world.get_mut::<Building>(be) else { return };
        if b.queue_len == 0 {
            return;
        }
        b.queue_len -= 1;
        let k = UnitKind::from_u8(b.queue[b.queue_len as usize]);
        if b.queue_len == 0 {
            b.train_work = Fx::ZERO;
        }
        k
    };
    let Some(kind) = kind else { return };
    let cost = unit_def(kind).cost;
    let mut q = world.query::<&mut Player>();
    if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
        p.stock.credit(&cost);
    }
}

/// Turn the head of a building's queue into a real unit: jittered beside the
/// hall's south edge, snapped onto passable ground, marching to the rally flag
/// when one was set away from the building. Called by the construction loop
/// once the order's training time is banked.
pub(crate) fn spawn_trained(world: &mut World, building: u64) -> bool {
    let Some(be) = find_by_id(world, building) else { return false };
    let Some(owner) = world.get::<Owner>(be).map(|o| o.0) else { return false };
    let Some(match_id) = world.get::<MatchId>(be).map(|m| m.0) else { return false };
    let Some(bpos) = world.get::<Pos>(be).map(|p| p.pos) else { return false };
    let (bkind, rally, kind) = {
        let Some(b) = world.get::<Building>(be) else { return false };
        if b.queue_len == 0 {
            return false;
        }
        let Some(kind) = UnitKind::from_u8(b.queue[0]) else { return false };
        (b.kind, b.rally, kind)
    };
    let fp = building_def(bkind).footprint;
    let (jx, jy) = {
        let mut rng = world.resource_mut::<SimRng>();
        ((rng.0.next_fx() - saladin_sim::fx!("0.5")) * Fx::from_num(2), rng.0.next_fx())
    };
    let raw_x = clamp_world(bpos.x + jx);
    let raw_y = clamp_world(bpos.y + Fx::from_num(fp) / Fx::from_num(2) + saladin_sim::fx!("0.8") + jy);
    let seed = world.resource::<WorldConfig>().seed;
    let (occ, gates) = super::occupancy_and_gates(world, false);
    let passable = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !gate_blocks(&gates, k, owner)
    };
    let afloat = unit_def(kind).afloat();
    let snap = if afloat {
        // A hull launches at its hall's berth or not at all. Snapping it onto
        // land instead would beach it forever: `movement` walks whatever path it
        // is handed and would never bring it back to water.
        match berth_of(seed, fp, bpos) {
            Some(b) => b,
            None => {
                pop_queue_head(world, be);
                let mut q = world.query::<&mut Player>();
                if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
                    p.stock.credit(&unit_def(kind).cost);
                }
                return false;
            }
        }
    } else {
        nearest_passable_grid(&passable, raw_x, raw_y)
    };
    let id = spawn::spawn_unit(world, owner, kind, snap, match_id, GatherState::Idle, 0);
    world.resource_mut::<crate::MatchStats>().of(owner).trained += 1;
    pop_queue_head(world, be);

    if dist(rally, bpos) > saladin_sim::fx!("1.2") {
        let sailable = |tx: i32, ty: i32| is_sailable(seed, tx, ty);
        let path = {
            let mut scratch = world.resource_mut::<PathScratch>();
            if afloat {
                if sea_reachable(seed, snap, rally) {
                    scratch.0.find_path_costed_in(
                        &sailable,
                        &|_, _| Fx::ONE,
                        snap.x,
                        snap.y,
                        rally.x,
                        rally.y,
                        MAX_EXPANSIONS,
                        Domain::Sea.smoothing(),
                    )
                } else {
                    Vec::new()
                }
            } else {
                scratch.0.find_path(&passable, snap.x, snap.y, rally.x, rally.y, MAX_EXPANSIONS)
            }
        };
        let mut q = world.query::<(&GameId, &mut Unit)>();
        if let Some((_, mut u)) = q.iter_mut(world).find(|(g, _)| g.0 == id) {
            // The flag is the ground this man is posted on. Without this his
            // home stays the hall door, so both leashes measure a rallied unit
            // against a building it was sent away from and it walks back to it
            // the moment it sees an enemy. A hull only takes the flag when it
            // can actually float to it — otherwise home is the berth.
            if !afloat || !path.is_empty() {
                u.home = rally;
            }
            if !path.is_empty() {
                u.target = path[0];
                u.path = path;
                u.path_idx = 0;
                u.has_target = true;
            }
        }
    }
    true
}

/// Drop the finished (or refused) order at the head of a queue and reset the
/// work clock. Leaving it in place would jam the hall forever, retrying the
/// same impossible spawn every tick.
fn pop_queue_head(world: &mut World, be: Entity) {
    let Some(mut b) = world.get_mut::<Building>(be) else { return };
    for i in 1..b.queue_len as usize {
        b.queue[i - 1] = b.queue[i];
    }
    b.queue_len = b.queue_len.saturating_sub(1);
    b.train_work = Fx::ZERO;
}

fn find_by_id(world: &mut World, id: u64) -> Option<Entity> {
    let mut q = world.query::<(Entity, &GameId)>();
    q.iter(world).find(|(_, g)| g.0 == id).map(|(e, _)| e)
}

/// Tile keys occupied by resource nodes (no building on a tree/quarry/etc.).
pub(crate) fn node_occupancy(world: &World) -> HashSet<i32> {
    let Some(mut q) = world.try_query::<(&Pos, &ResourceNode)>() else { return HashSet::new() };
    q.iter(world)
        .map(|(p, _)| tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()))
        .collect()
}

/// Everything a placement decision needs, gathered ONCE. `place_near` probes up
/// to ~800 spots for a waterside or fertility building; rebuilding building AND
/// node occupancy per probe walked every node on the map each time.
pub(crate) struct BuildContext {
    occ: HashSet<i32>,
    /// Open ground the owner's builders can actually reach, flooded ONCE from
    /// where they stand. The approach rule reads this, never `occ`: what covers
    /// ground and what stops a walker are different sets, and a plot your own
    /// keep has sealed off is neither occupied nor reachable.
    reach: HashSet<i32>,
    /// What a walker cannot cross: buildings, passable ones excluded. Kept
    /// beside `reach` because founding has to ask what the ground looks like
    /// AFTER this footprint lands on it.
    walk_occ: HashSet<i32>,
    /// Whether the owner has any hand that could raise anything at all. With
    /// none, the rule is waived — there is nobody to be cut off.
    has_hands: bool,
    wall_keys: HashSet<i32>,
    walls: Vec<(u64, i32)>,
    own: Vec<V2>,
    owned_kinds: HashSet<BuildingKind>,
    counts: [i32; BuildingKind::ALL.len()],
    stock: Stockpile,
    match_id: u64,
}

pub(crate) fn build_context(world: &World, owner: u64) -> Option<BuildContext> {
    let (stock, match_id) = {
        let mut q = world.try_query::<(&Player, &MatchId)>()?;
        q.iter(world).find(|(p, _)| p.player_id == owner).map(|(p, m)| (p.stock, m.0))?
    };
    let mut occ = building_occupancy(world, true);
    let walk_occ = building_occupancy(world, false);
    let seed = world.resource::<WorldConfig>().seed;
    occ.extend(node_occupancy(world));
    let mut walls = Vec::new();
    let mut own = Vec::new();
    let mut owned_kinds = HashSet::new();
    let mut counts = [0i32; BuildingKind::ALL.len()];
    if let Some(mut q) = world.try_query::<(&GameId, &Pos, &Owner, &Building)>() {
        for (g, p, o, b) in q.iter(world) {
            if o.0 != owner {
                continue;
            }
            own.push(p.pos);
            counts[b.kind as usize] += 1;
            if operational(b.state) {
                owned_kinds.insert(b.kind);
            }
            if b.kind == BuildingKind::Wall {
                walls.push((g.0, tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>())));
            }
        }
    }
    let wall_keys = walls.iter().map(|(_, k)| *k).collect();
    let hands: Vec<V2> = match world.try_query::<(&Owner, &Pos, &Unit)>() {
        Some(mut q) => q
            .iter(world)
            .filter(|(o, _, u)| o.0 == owner && u.garrisoned_in == 0 && is_builder(u.kind))
            .map(|(_, p, _)| p.pos)
            .collect(),
        None => Vec::new(),
    };
    let reach = town_reach(
        |tx, ty| is_passable(seed, tx, ty) && !walk_occ.contains(&tile_key(tx, ty)),
        &hands,
        &own,
        TOWN_REACH_BUDGET,
    );
    let has_hands = !hands.is_empty();
    Some(BuildContext {
        occ,
        reach,
        walk_occ,
        has_hands,
        wall_keys,
        walls,
        own,
        owned_kinds,
        counts,
        stock,
        match_id,
    })
}

impl BuildContext {
    /// The reachability overlay, for devctl's terrain map. Read-only: the set
    /// that decides placements is the set a debugger must be able to see.
    pub(crate) fn reach_set(&self) -> &HashSet<i32> {
        &self.reach
    }

    /// The full `check_build` rule set against this gathering. The command asks
    /// through here and so does devctl's dry-run probe, so a probe can never
    /// answer differently from the order it is standing in for.
    pub(crate) fn check(
        &self,
        seed: u32,
        kind: BuildingKind,
        pos: V2,
    ) -> Result<(), PlaceError> {
        let composes = composes_with_walls(kind);
        let occupied = |tx: i32, ty: i32| {
            let k = tile_key(tx, ty);
            self.occ.contains(&k) && !(composes && self.wall_keys.contains(&k))
        };
        // no hands, no rule: nobody can be cut off from a plot they were never
        // going to reach
        let reachable =
            |tx: i32, ty: i32| !self.has_hands || self.reach.contains(&tile_key(tx, ty));
        check_build(
            seed,
            kind,
            pos.x,
            pos.y,
            occupied,
            reachable,
            &self.own,
            &self.owned_kinds,
            &self.counts,
            &self.stock,
        )
    }
}

/// Ground the cut-off test may walk. The town box plus a margin: a man past
/// that is not in the town, and one footprint is not what cut him off.
const CUTOFF_FLOOD_MAX: usize = 8192;

/// Everything of the owner's that this ground can be walked to from the keep.
fn keep_reach<W: Fn(i32, i32) -> bool>(keep: V2, walkable: &W) -> HashSet<i32> {
    let margin = TOWN_RADIUS.ceil().to_num::<i32>() * 2;
    let (kx, ky) = (keep.x.to_num::<i32>(), keep.y.to_num::<i32>());
    let inside = |tx: i32, ty: i32| (tx - kx).abs() <= margin && (ty - ky).abs() <= margin;
    let mut seen: HashSet<i32> = HashSet::new();
    let mut queue: Vec<(i32, i32)> = Vec::new();
    for dy in -3..=3 {
        for dx in -3..=3 {
            let (tx, ty) = (kx + dx, ky + dy);
            if inside(tx, ty) && walkable(tx, ty) && seen.insert(tile_key(tx, ty)) {
                queue.push((tx, ty));
            }
        }
    }
    let mut head = 0;
    while head < queue.len() && seen.len() < CUTOFF_FLOOD_MAX {
        let (tx, ty) = queue[head];
        head += 1;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (tx + dx, ty + dy);
            if inside(nx, ny) && walkable(nx, ny) && seen.insert(tile_key(nx, ny)) {
                queue.push((nx, ny));
            }
        }
    }
    seen
}

/// Would raising this footprint wall the owner's own people off from their keep?
///
/// A bot's town is dense, and each building is legal on its own. Measured on
/// River Valley seed 3, one grew around its peasants and left EIGHT of fourteen
/// standing in two-tile pockets for the rest of the match: nothing crushes
/// them, nothing tells them, and every gather gate they are put through says
/// "no standing room the hand can reach" forever.
///
/// The test is the DIFFERENCE the footprint makes: flood the keep's ground with
/// and without it, and refuse if any of the owner's men fall out of the second
/// set. Checking only the men standing near the plot — which is what this did
/// first — misses the ones who walked into the bay five tiles away before the
/// neck was closed, which is how Continental seed 22 still lost nine hands.
///
/// Two bounded floods per FOUNDING, never per candidate: `place_near` probes a
/// perimeter every decision window and a search per probe cost 28% of the sim
/// tick, measured. A founding happens a few times a minute.
fn seals_own_units(
    world: &World,
    ctx: &BuildContext,
    owner: u64,
    seed: u32,
    kind: BuildingKind,
    pos: V2,
) -> bool {
    let def = building_def(kind);
    if def.passable {
        return false;
    }
    // The KEEP is the town. Any owned building will not do: the walls that seal
    // a man in are owned buildings too, and standing beside one is not
    // connectivity. A hand-built world (every test) leaves `Player.keep` at 0,
    // so the hall itself is the fallback.
    let keep = {
        let mut pq = match world.try_query::<&Player>() {
            Some(q) => q,
            None => return false,
        };
        let id = pq.iter(world).find(|p| p.player_id == owner).map(|p| p.keep).unwrap_or(0);
        let mut bq = match world.try_query::<(&GameId, &Owner, &Pos, &Building)>() {
            Some(q) => q,
            None => return false,
        };
        let named =
            bq.iter(world).find(|(g, _, _, _)| g.0 == id && id != 0).map(|(_, _, p, _)| p.pos);
        match named.or_else(|| {
            bq.iter(world)
                .filter(|(_, o, _, b)| o.0 == owner && b.kind == BuildingKind::Keep)
                .min_by_key(|(g, _, _, _)| g.0)
                .map(|(_, _, p, _)| p.pos)
        }) {
            Some(p) => p,
            None => return false,
        }
    };

    let laid: HashSet<i32> = footprint_tiles(def.footprint, pos.x, pos.y)
        .iter()
        .map(|t| tile_key(t.tx, t.ty))
        .collect();
    let before = keep_reach(keep, &|tx, ty| {
        is_passable(seed, tx, ty) && !ctx.walk_occ.contains(&tile_key(tx, ty))
    });
    let after = keep_reach(keep, &|tx, ty| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !ctx.walk_occ.contains(&k) && !laid.contains(&k)
    });

    let Some(mut q) = world.try_query::<(&Owner, &Pos, &Unit)>() else { return false };
    q.iter(world).any(|(o, p, u)| {
        if o.0 != owner || u.garrisoned_in != 0 || unit_def(u.kind).afloat() {
            return false;
        }
        let key = tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>());
        // standing ON the plot is `shove_clear`'s job, not a refusal
        !laid.contains(&key) && before.contains(&key) && !after.contains(&key)
    })
}

/// Put the owner's own people off the ground a new footprint just took, the
/// way a hall displaces the villagers standing in it. Without this the man is
/// inside a solid block: nothing crushes him, and every A* out of it returns
/// nothing for the rest of the match.
fn shove_clear(world: &mut World, owner: u64, seed: u32, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    if def.passable {
        return;
    }
    let tiles = footprint_tiles(def.footprint, pos.x, pos.y);
    let laid: HashSet<i32> = tiles.iter().map(|t| tile_key(t.tx, t.ty)).collect();
    let occ = super::building_occupancy(world, false);
    let free = |tx: i32, ty: i32| {
        let k = tile_key(tx, ty);
        is_passable(seed, tx, ty) && !occ.contains(&k) && !laid.contains(&k)
    };
    let inside: Vec<Entity> = {
        let Some(mut q) = world.try_query::<(Entity, &Owner, &Pos, &Unit)>() else { return };
        q.iter(world)
            .filter(|(_, o, p, u)| {
                o.0 == owner
                    && u.garrisoned_in == 0
                    && !unit_def(u.kind).afloat()
                    && laid.contains(&tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()))
            })
            .map(|(e, _, _, _)| e)
            .collect()
    };
    for e in inside {
        let Some(at) = world.get::<Pos>(e).map(|p| p.pos) else { continue };
        let to = nearest_passable_grid(&free, at.x, at.y);
        if let Some(mut p) = world.get_mut::<Pos>(e) {
            p.pos = to;
        }
        if let Some(mut u) = world.get_mut::<Unit>(e) {
            u.has_target = false;
            u.path.clear();
            u.path_idx = 0;
        }
    }
}

/// Found `kind` at `pos` against an already-gathered context — the full
/// `check_build` rule set (buildable biome, node/building occupancy, waterside,
/// town radius, approach, prereqs, per-kind limit, cost). The cost is paid in
/// full up front and a SITE goes up: frail, inert, and worth raiding until a
/// peasant finishes it. `facing` = quarter turns; square footprints make
/// rotation purely visual, but it rides the command so every client renders the
/// same yaw.
///
/// Defense composition: a gate or tower placed on the player's OWN wall tile
/// absorbs that segment (full refund) instead of being refused — walls are a
/// canvas the other defense pieces slot into. A gate dropped into a wall run
/// also auto-orients its passage across the run.
///
/// `ctx` is updated in place on success, so a caller may keep probing.
pub(crate) fn build_with(
    world: &mut World,
    ctx: &mut BuildContext,
    owner: u64,
    kind: BuildingKind,
    pos: V2,
    facing: u8,
) -> Result<u64, PlaceError> {
    let def = building_def(kind);
    let seed = world.resource::<WorldConfig>().seed;
    let composes = composes_with_walls(kind);
    ctx.check(seed, kind, pos)?;
    if seals_own_units(world, ctx, owner, seed, kind, pos) {
        return Err(PlaceError::NoApproach);
    }
    {
        let mut q = world.query::<&mut Player>();
        let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) else {
            return Err(PlaceError::CannotAfford);
        };
        if !p.stock.can_afford(&def.cost) {
            return Err(PlaceError::CannotAfford);
        }
        p.stock.pay(&def.cost);
    }
    ctx.stock.pay(&def.cost);

    // absorb the overlapped segment: refund in full, pop any parapet garrison
    let fp: Vec<i32> =
        footprint_tiles(def.footprint, pos.x, pos.y).iter().map(|t| tile_key(t.tx, t.ty)).collect();
    let mut absorbed_run = (false, false); // own wall continues along (x, z)
    if composes && !ctx.walls.is_empty() {
        let (tx, ty) = (pos.x.floor().to_num::<i32>(), pos.y.floor().to_num::<i32>());
        let wall_at = |dx: i32, dy: i32| ctx.wall_keys.contains(&tile_key(tx + dx, ty + dy));
        absorbed_run = (wall_at(1, 0) || wall_at(-1, 0), wall_at(0, 1) || wall_at(0, -1));
        let absorbed: Vec<(u64, i32)> =
            ctx.walls.iter().filter(|(_, k)| fp.contains(k)).copied().collect();
        for (wid, wkey) in absorbed {
            raze_building(world, owner, wid, RazeCause::Absorbed);
            ctx.walls.retain(|(id, _)| *id != wid);
            ctx.wall_keys.remove(&wkey);
            ctx.occ.remove(&wkey);
            ctx.counts[BuildingKind::Wall as usize] -= 1;
            ctx.stock.credit(&building_def(BuildingKind::Wall).cost);
        }
    }

    let center = footprint_center(def.footprint, pos.x, pos.y);
    let state = if def.build_time > Fx::ZERO { BuildState::Site } else { BuildState::Complete };
    shove_clear(world, owner, seed, kind, center);
    let id = spawn::spawn_building(world, owner, kind, center, ctx.match_id, state);
    if state == BuildState::Complete {
        finish_building(world, id);
    }
    // a gate in a clear X- or Z-run turns its passage across the run; the
    // player's chosen facing wins when the neighborhood is ambiguous
    let facing = if kind == BuildingKind::Gatehouse {
        match absorbed_run {
            (true, false) => 0,
            (false, true) => 1,
            _ => facing,
        }
    } else {
        facing
    };
    if facing % 4 != 0 {
        let yaw = saladin_sim::fx!("1.5707963") * Fx::from_num(facing % 4);
        let mut q = world.query::<(&GameId, &mut Pos)>();
        if let Some((_, mut p)) = q.iter_mut(world).find(|(g, _)| g.0 == id) {
            p.facing = yaw;
        }
    }

    ctx.own.push(center);
    ctx.counts[kind as usize] += 1;
    if state != BuildState::Site {
        ctx.owned_kinds.insert(kind);
    }
    for k in fp {
        ctx.occ.insert(k);
    }
    if kind == BuildingKind::Wall {
        let k = tile_key(pos.x.floor().to_num::<i32>(), pos.y.floor().to_num::<i32>());
        ctx.walls.push((id, k));
        ctx.wall_keys.insert(k);
    }
    Ok(id)
}

/// What a structure gains the moment it is finished. A farm IS its field, and
/// sowing at COMPLETION rather than at siting is what makes completion a real
/// event: how BIG the harvest will be is the soil's business, how fast it comes
/// in is the crew's.
pub(crate) fn finish_building(world: &mut World, id: u64) {
    let Some(e) = find_by_id(world, id) else { return };
    let Some(kind) = world.get::<Building>(e).map(|b| b.kind) else { return };
    let def = building_def(kind);
    if def.min_fertility <= Fx::ZERO {
        return;
    }
    let sown = {
        let mut q = world.query::<&FieldOf>();
        q.iter(world).any(|f| f.0 == id)
    };
    if sown {
        return;
    }
    let Some(owner) = world.get::<Owner>(e).map(|o| o.0) else { return };
    let Some(match_id) = world.get::<MatchId>(e).map(|m| m.0) else { return };
    let Some(pos) = world.get::<Pos>(e).map(|p| p.pos) else { return };
    let seed = world.resource::<WorldConfig>().seed;
    let soil = saladin_sim::soil_quality(seed, def.footprint, pos.x, pos.y);
    spawn::spawn_field(world, owner, id, pos, saladin_sim::field_cap(soil), match_id);
}

/// One-shot placement: gather the context, found one site, put the named
/// peasants on it.
pub(crate) fn build(
    world: &mut World,
    owner: u64,
    kind: BuildingKind,
    pos: V2,
    facing: u8,
    builders: &[u64],
) -> Result<u64, PlaceError> {
    let Some(mut ctx) = build_context(world, owner) else { return Err(PlaceError::CannotAfford) };
    let id = build_with(world, &mut ctx, owner, kind, pos, facing)?;
    assign_builders(world, owner, id, builders);
    Ok(id)
}

/// Put `units` to work on `site`. Ownership-checked per unit; one that is not
/// the caller's, not a carrier, or already sheltered is skipped.
pub(crate) fn assign_builders(world: &mut World, owner: u64, site: u64, units: &[u64]) {
    for &u in units {
        repair(world, owner, u, site);
    }
}

/// Send a peasant to work on one of the caller's structures. ONE command covers
/// founding labour, repair labour and upgrade labour — they are the same loop,
/// and `job_site` survives the retargeting that clears `target_node`.
pub(crate) fn repair(world: &mut World, owner: u64, unit: u64, building: u64) -> bool {
    if find_owned(world, owner, building).is_none() {
        return false;
    }
    let Some(ue) = find_owned(world, owner, unit) else { return false };
    let ok = match world.get::<Unit>(ue) {
        Some(u) => u.garrisoned_in == 0 && is_builder(u.kind),
        None => false,
    };
    if !ok {
        return false;
    }
    if let Some(mut u) = world.get_mut::<Unit>(ue) {
        u.gather_state = GatherState::Constructing;
        u.job_site = building;
        u.target_node = 0;
        u.attack_target = 0;
        u.has_target = false;
    }
    true
}

/// Abandon an unfinished site: the labour already sunk into it is gone, the
/// rest of the cost comes back, and the crew looks for other work.
pub(crate) fn cancel_site(world: &mut World, owner: u64, building: u64) {
    let Some(e) = find_owned(world, owner, building) else { return };
    if world.get::<Building>(e).is_none_or(|b| b.state != BuildState::Site) {
        return;
    }
    raze_building(world, owner, building, RazeCause::Cancelled);
}

/// Raise a structure into what it becomes. The entity keeps its GameId, owner,
/// garrison, rally and facing — which is why `Player::keep` and the defeat
/// check need no special case, and why the tower keeps FIRING while it rises.
pub(crate) fn upgrade_building(world: &mut World, owner: u64, building: u64) -> bool {
    let Some(e) = find_owned(world, owner, building) else { return false };
    let Some(b) = world.get::<Building>(e).copied() else { return false };
    if b.state != BuildState::Complete {
        return false;
    }
    let def = building_def(b.kind);
    let Some(target) = def.upgrades_to else { return false };
    {
        let mut q = world.query::<&mut Player>();
        let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) else { return false };
        if !p.stock.can_afford(&def.upgrade_cost) {
            return false;
        }
        p.stock.pay(&def.upgrade_cost);
    }
    if let Some(mut b) = world.get_mut::<Building>(e) {
        b.state = BuildState::Upgrading;
        b.target_kind = target;
        b.work = Fx::ZERO;
    }
    true
}

/// Batched wall placement for a dragged line: founds every affordable, valid
/// Wall tile and skips the rest silently.
///
/// The anchor set is snapshotted ONCE. Letting each placed segment extend it
/// turned a 120-tile drag into a 115-tile reach against a TOWN_RADIUS of 28 —
/// the only spatial containment rule in the game, bought for a few wood a tile.
pub(crate) fn place_wall(world: &mut World, owner: u64, tiles: &[(i32, i32)], builders: &[u64]) {
    let Some(mut ctx) = build_context(world, owner) else { return };
    let def = building_def(BuildingKind::Wall);
    let seed = world.resource::<WorldConfig>().seed;
    let state = if def.build_time > Fx::ZERO { BuildState::Site } else { BuildState::Complete };
    let anchors = std::mem::take(&mut ctx.own);
    let mut first = 0u64;
    let mut spent = false;
    for &(tx, ty) in tiles {
        if !ctx.stock.can_afford(&def.cost) {
            break;
        }
        let x = Fx::from_num(tx);
        let y = Fx::from_num(ty);
        let occupied = |px: i32, py: i32| ctx.occ.contains(&tile_key(px, py));
        let reachable = |px: i32, py: i32| !ctx.has_hands || ctx.reach.contains(&tile_key(px, py));
        if check_build(
            seed,
            BuildingKind::Wall,
            x,
            y,
            occupied,
            reachable,
            &anchors,
            &ctx.owned_kinds,
            &ctx.counts,
            &ctx.stock,
        )
        .is_err()
        {
            continue;
        }
        let c = footprint_center(def.footprint, x, y);
        // A WALL is the classic sealer, and a drag lays a whole line of them.
        // Continental seed 22: the bot's own line closed on nine of its fifteen
        // peasants at tick 10400 and they never worked again.
        if seals_own_units(world, &ctx, owner, seed, BuildingKind::Wall, c) {
            continue;
        }
        shove_clear(world, owner, seed, BuildingKind::Wall, c);
        let id = spawn::spawn_building(world, owner, BuildingKind::Wall, c, ctx.match_id, state);
        if first == 0 {
            first = id;
        }
        for k in occupancy_set(&[Occupant { kind: BuildingKind::Wall, pos: V2::new(x, y) }], true) {
            ctx.occ.insert(k);
        }
        ctx.counts[BuildingKind::Wall as usize] += 1;
        ctx.stock.pay(&def.cost);
        spent = true;
    }
    if spent {
        let mut q = world.query::<&mut Player>();
        if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
            p.stock = ctx.stock;
        }
    }
    // the crew starts at the head of the line; finishing one segment hands it
    // the next one within reach
    if first != 0 {
        assign_builders(world, owner, first, builders);
    }
}

/// Why a structure left the map. The refund policy is a property of the CAUSE,
/// so every voluntary route out of the world funnels through one place. A
/// structure KILLED in combat is razed by the combat loop's own deferred
/// despawn — it refunds nothing, which is the whole point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RazeCause {
    /// Torn down on purpose: half the cost back, scaled by what still stands.
    Demolished,
    /// Overbuilt by a composing defense piece: paid for once, refunded in full.
    Absorbed,
    /// A site abandoned before it rose: the unspent remainder comes back.
    Cancelled,
}

/// The single exit: refund by cause, hand back the production queue, return the
/// garrison to the field, release the crew, despawn.
pub(crate) fn raze_building(world: &mut World, owner: u64, id: u64, cause: RazeCause) {
    let Some(e) = find_owned(world, owner, id) else { return };
    let Some(b) = world.get::<Building>(e).copied() else { return };
    let def = building_def(b.kind);
    let mut refund = match cause {
        RazeCause::Demolished => demolish_refund(&def.cost, b.hp, def.max_hp),
        RazeCause::Absorbed => def.cost,
        RazeCause::Cancelled => cancel_refund(&def.cost, b.work),
    };
    for i in 0..b.queue_len as usize {
        if let Some(kind) = UnitKind::from_u8(b.queue[i]) {
            let c = unit_def(kind).cost;
            refund = ResourceCost::new(
                refund.wood + c.wood,
                refund.stone + c.stone,
                refund.food + c.food,
                refund.gold + c.gold,
            );
        }
    }
    if refund != ResourceCost::ZERO {
        let mut q = world.query::<&mut Player>();
        if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
            p.stock.credit(&refund);
        }
    }
    eject_all(world, id);
    release_crew(world, id);
    world.despawn(e);
}

/// Take every builder off `site` — it is finished, gone, or needs no more work.
pub(crate) fn release_crew(world: &mut World, site: u64) {
    let mut q = world.query::<&mut Unit>();
    for mut u in q.iter_mut(world) {
        if u.job_site == site {
            u.job_site = 0;
            if u.gather_state == GatherState::Constructing {
                u.gather_state = GatherState::Idle;
                u.has_target = false;
            }
        }
    }
}

/// Tear down an owned building (never the Keep): an unfinished site hands back
/// its unspent remainder, a standing one half its cost scaled by health.
pub(crate) fn demolish(world: &mut World, owner: u64, building: u64) {
    let Some(e) = find_owned(world, owner, building) else { return };
    let Some(b) = world.get::<Building>(e).copied() else { return };
    if b.kind == BuildingKind::Keep {
        return;
    }
    let cause =
        if b.state == BuildState::Site { RazeCause::Cancelled } else { RazeCause::Demolished };
    raze_building(world, owner, building, cause);
}

/// Move a building's rally flag. Trained units march there on spawn.
pub(crate) fn set_rally(world: &mut World, owner: u64, building: u64, target: V2) {
    let Some(e) = find_owned(world, owner, building) else { return };
    if let Some(mut b) = world.get_mut::<Building>(e) {
        b.rally = V2::new(clamp_world(target.x), clamp_world(target.y));
    }
}
