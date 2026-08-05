//! Combat — runs every combat tick (200 ms). Soldiers auto-acquire enemies,
//! close to range, and strike on cooldown; structures fire (their own bows plus
//! their garrison's); morale routs and recovers.
//!
//! Two rules make this a battle rather than a damage exchange:
//! FORTIFICATION — every standing structure raises a bit in a tile bitset that
//! pursuit, line of fire and the separation push all read, so a wall stops a
//! man, an arrow and a shove alike; a pursuit the wall refuses makes the wall
//! the objective. FACING — a `heading` maintained as one of sixteen compass
//! points decides whether a blow lands on the face, the flank or the back, and
//! whether a charge is taken on set spears.
//!
//! Engineered for big battles: a FLAT spatial grid with a per-cell owner mask
//! (an idle army skips every friendly cell instead of scanning it), buildings in
//! their own grid, binary search over id-sorted snapshots instead of hash maps,
//! one retained path arena instead of a `Vec` per pursuit, squared-distance
//! compares throughout. All state lives in `CombatScratch`, reused across ticks.

use crate::components::{
    Building, GameId, MatchId, ORDER_ATTACK, ORDER_ATTACK_MOVE, ORDER_MOVE, Owner, Player, Pos,
    Unit,
};
use crate::{MatchStatuses, PathScratch, Shot, ShotEvents, WorldConfig};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashSet;
use saladin_sim::{
    AStar, Attacker, BuildState, BuildingKind, CELL_COUNT, CELLS_PER_ROW, COMBAT_DT, CombatAct,
    DEFENSIVE_LEASH, DamageType, Fx, GarrisonShooter, GarrisonShot, GarrisonTarget, MORALE_MAX,
    ROUT_THRESHOLD, Sight, Stance, UnitKind, V2, WORLD_SIZE, applied_bonus, bombard_morale,
    building_damage, building_def, cell_of, charge_multiplier, combat_action, disciplined_resolve,
    dist2, effective_building_def, effective_damage_vs, effective_unit_def, elevation_at,
    elevation_range_bonus, facing_multiplier, fx_sqrt, garrison_volley, gate_blocks,
    has_line_of_fire, is_frontal, is_routing, morale_after_hit_resolve, morale_recover,
    move_cost_at, nearest_passable_grid, operational, passable_grid, unit_def,
};

use super::movement::heading16;

const DT: Fx = COMBAT_DT;
const ALLY_RADIUS: Fx = saladin_sim::fx!("5");
/// Pursuit pathfinding budget per unit per combat tick. The full-map budget made
/// a mass first-contact (thousands of units pathing at once) spike the tick;
/// a bounded search plus a ray fallback keeps worst-case flat — blocked units
/// simply re-path on a later tick when closer.
const PURSUIT_EXPANSIONS: usize = 1200;
/// Max pursuit paths computed per combat tick (id order, deterministic). A mass
/// first-contact charge staggers over a few ticks instead of spiking one.
const PURSUIT_BUDGET: u32 = 768;
/// How far an Aggressive unit chases from the ground it was posted on. Without
/// it one scout drags a whole army across the map, burning the pursuit budget
/// every tick of the walk.
const AGGRESSIVE_LEASH: Fx = saladin_sim::fx!("14");
/// Beyond this, a shot needs line of fire. A melee blow at arm's length does
/// not, or two men either side of a doorway could never fight.
const LOS_MIN_RANGE: Fx = saladin_sim::fx!("2");
/// How many attackers one body can hold off at once. Frontage: without it forty
/// men pile onto one and the rest of the line stands idle.
const MELEE_SLOTS: u8 = 5;
/// Combat ticks a rider needs between charges.
const CHARGE_COOLDOWN: i32 = 40;
/// Combat ticks a rallied unit spends walking back to its post. Without it a
/// broken line recovers to full morale and stands where it stopped running,
/// which is how a 40v40 mirror froze at 20v20 for 230 seconds.
const RALLY_RETURN: i32 = 90;
/// Tiles a routing unit runs before it stops. Fleeing all the way to spawn is
/// what took the survivors permanently out of the battle.
const ROUT_FLEE: Fx = saladin_sim::fx!("6");
/// A routing unit runs to a friendly morale anchor if one is this close.
const ROUT_ANCHOR_RANGE: Fx = saladin_sim::fx!("12");
/// Squared tiles from the end of an attack-move inside which the march counts
/// as finished. Wider than `ARRIVE_EPS` on purpose: a formation slot is not the
/// order's destination, so a man who has taken his place must not keep repathing
/// at the centre of the line and burning the pursuit budget every tick.
const RESUME_EPS2: Fx = saladin_sim::fx!("9");
/// Tiles of a straight-line pursuit step when the capped A* gives up but the
/// ground ahead is open.
const RAY_STEP: i32 = 10;
/// How far a quarry may drift from the end of the path chasing it before the
/// chaser asks for a new one, squared.
const REPATH_DRIFT2: Fx = saladin_sim::fx!("9");
/// Splash lands at this fraction on everything that is not the target.
const SPLASH_SHARE: Fx = saladin_sim::fx!("0.5");

const NO_BLD: u32 = u32::MAX;
const TILES: usize = (WORLD_SIZE * WORLD_SIZE) as usize;
const WORDS: usize = TILES.div_ceil(64);
/// The per-cell owner mask is a `u16`; the last slot is shared by any overflow,
/// and a unit sitting in it simply skips the optimisation.
const OVERFLOW_SLOT: u8 = 15;

#[derive(Clone)]
struct USnap {
    id: u64,
    entity: Entity,
    pos: V2,
    target: V2,
    home: V2,
    owner: u64,
    oslot: u8,
    mtch: u64,
    kind: UnitKind,
    stance: Stance,
    attack_target: u64,
    cd: i32,
    morale: Fx,
    setup_timer: Fx,
    charge_cd: i32,
    rally_cd: i32,
    heading: u8,
    order: u8,
    /// Where the standing order ends, so a march can resume after the fight it
    /// walked into is over.
    order_target: V2,
    routing: bool,
    has_target: bool,
    garr: bool,
    garrisoned_in: u64,
    hp: i32,
    /// Where the path this unit is walking actually ENDS. A pursuit that only
    /// repaths when the stale path runs out walks to where its quarry WAS.
    path_end: V2,
}

#[derive(Clone, Copy)]
struct BSnap {
    id: u64,
    entity: Entity,
    pos: V2,
    owner: u64,
    oslot: u8,
    mtch: u64,
    kind: BuildingKind,
    state: BuildState,
    hp: i32,
}

/// What to write back to a unit after the decide pass. `mv` is (offset, len)
/// into the tick's path arena, not a `Vec` — the old shape allocated and freed
/// up to ~1500 heap blocks per combat tick, all dropped on the next one.
#[derive(Default, Clone)]
struct UOut {
    attack_target: Option<u64>,
    cooldown: Option<i32>,
    morale: Option<Fx>,
    routing: Option<bool>,
    heading: Option<u8>,
    setup: Option<Fx>,
    charge_cd: Option<i32>,
    rally_cd: Option<i32>,
    slot: Option<u8>,
    clear_move: bool,
    mv: Option<(u32, u32, V2)>,
    eject_to: Option<V2>,
}

/// All per-tick combat working memory, retained across ticks so the hot path
/// never allocates once capacities warm up.
#[derive(Resource)]
pub struct CombatScratch {
    grid: Vec<Vec<u32>>,
    /// Per cell, a bit per owner slot. A ring scan skips any cell holding only
    /// the scanner's own men — the packed-idle case (no enemy anywhere) used to
    /// pay a full 7x7 scan per unit forever.
    cell_owners: Vec<u16>,
    bgrid: Vec<Vec<u32>>,
    bcell_owners: Vec<u16>,
    owners: Vec<u64>,
    units: Vec<USnap>,
    buildings: Vec<BSnap>,
    uhp: Vec<i32>,
    bhp: Vec<i32>,
    udead: Vec<bool>,
    bdead: Vec<bool>,
    out: Vec<UOut>,
    hit: Vec<bool>,
    engage: Vec<u8>,
    /// One bit per tile: a standing structure that a man cannot walk through and
    /// an arrow cannot fly through.
    blocked: Vec<u64>,
    /// Words touched last tick — the bitset is cleared by replaying these, never
    /// by a 18 KB memset.
    blocked_words: Vec<u32>,
    /// Tile -> building snapshot index, so a blocked pursuit can ask WHAT is in
    /// the way. Cleared by touched-tile list.
    tile_bld: Vec<u32>,
    tile_dirty: Vec<u32>,
    gates: Vec<(i32, u64)>,
    /// Buildings with `morale_radius > 0`, so the support scan stops visiting
    /// every wall segment on the map.
    aura_blds: Vec<u32>,
    /// Units with a rally aura (Chaplain) — usually empty, so the per-hit
    /// discipline lookup costs nothing.
    rally_auras: Vec<(V2, Fx, u64)>,
    bgarr: Vec<Vec<u32>>,
    shooters: Vec<GarrisonShooter>,
    gtargets: Vec<GarrisonTarget>,
    gt_idx: Vec<u32>,
    gshots: Vec<GarrisonShot>,
    path_arena: Vec<V2>,
    caught: Vec<u32>,
    bcd: Vec<(Entity, Fx)>,
}

impl Default for CombatScratch {
    fn default() -> Self {
        CombatScratch {
            grid: vec![Vec::new(); CELL_COUNT as usize],
            cell_owners: vec![0; CELL_COUNT as usize],
            bgrid: vec![Vec::new(); CELL_COUNT as usize],
            bcell_owners: vec![0; CELL_COUNT as usize],
            owners: Vec::new(),
            units: Vec::new(),
            buildings: Vec::new(),
            uhp: Vec::new(),
            bhp: Vec::new(),
            udead: Vec::new(),
            bdead: Vec::new(),
            out: Vec::new(),
            hit: Vec::new(),
            engage: Vec::new(),
            blocked: vec![0; WORDS],
            blocked_words: Vec::new(),
            tile_bld: vec![NO_BLD; TILES],
            tile_dirty: Vec::new(),
            gates: Vec::new(),
            aura_blds: Vec::new(),
            rally_auras: Vec::new(),
            bgarr: Vec::new(),
            shooters: Vec::new(),
            gtargets: Vec::new(),
            gt_idx: Vec::new(),
            gshots: Vec::new(),
            path_arena: Vec::new(),
            caught: Vec::new(),
            bcd: Vec::new(),
        }
    }
}

impl CombatScratch {
    /// Does a standing structure cover this tile? Read by `separation` so a
    /// shove cannot push a man inside a keep (9 of 24 measured peasants ended a
    /// run inside one).
    pub fn blocked_tile(&self, tx: i32, ty: i32) -> bool {
        if !in_world(tx, ty) {
            return false;
        }
        bit(&self.blocked, ty * WORLD_SIZE + tx)
    }
}

#[inline]
fn in_grid(c: i32) -> bool {
    (0..CELLS_PER_ROW).contains(&c)
}

#[inline]
fn in_world(tx: i32, ty: i32) -> bool {
    (0..WORLD_SIZE).contains(&tx) && (0..WORLD_SIZE).contains(&ty)
}

#[inline]
fn bit(words: &[u64], k: i32) -> bool {
    let k = k as usize;
    words[k >> 6] & (1u64 << (k & 63)) != 0
}

#[inline]
fn set_bit(words: &mut [u64], dirty: &mut Vec<u32>, k: i32) {
    let k = k as usize;
    let w = k >> 6;
    if words[w] == 0 {
        dirty.push(w as u32);
    }
    words[w] |= 1u64 << (k & 63);
}

#[inline]
fn floor_i(v: Fx) -> i32 {
    v.floor().to_num::<i32>()
}

/// Visit every snapshot index in the `r`-cell Chebyshev block around `pos` —
/// inline, no allocation.
#[inline]
fn for_near(grid: &[Vec<u32>], pos: V2, r: i32, mut visit: impl FnMut(u32)) {
    let cell = cell_of(pos.x, pos.y);
    let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
    for dy in -r..=r {
        let ny = cy + dy;
        if !in_grid(ny) {
            continue;
        }
        for dx in -r..=r {
            let nx = cx + dx;
            if !in_grid(nx) {
                continue;
            }
            for &i in &grid[(ny * CELLS_PER_ROW + nx) as usize] {
                visit(i);
            }
        }
    }
}

/// `for_near` that stops the moment the visitor says it has seen enough.
#[inline]
fn for_near_until(grid: &[Vec<u32>], pos: V2, r: i32, mut visit: impl FnMut(u32) -> bool) {
    let cell = cell_of(pos.x, pos.y);
    let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
    for dy in -r..=r {
        let ny = cy + dy;
        if !in_grid(ny) {
            continue;
        }
        for dx in -r..=r {
            let nx = cx + dx;
            if !in_grid(nx) {
                continue;
            }
            for &i in &grid[(ny * CELLS_PER_ROW + nx) as usize] {
                if !visit(i) {
                    return;
                }
            }
        }
    }
}

/// Nearest matching thing by RING-ordered cell scan. Two exits, not one: a ring
/// whose nearest possible tile is farther than the best hit so far stops the
/// scan, AND — the case that mattered — a ring farther than the search RADIUS
/// stops it even when nothing has been found, so an idle packed army no longer
/// pays a full block scan every tick for having no enemies at all. Cells whose
/// owner mask holds nothing but `own` are skipped outright.
#[inline]
#[allow(clippy::too_many_arguments)]
fn nearest_in_rings(
    grid: &[Vec<u32>],
    masks: &[u16],
    own: u16,
    pos: V2,
    max_r: i32,
    range2: Fx,
    mut accept: impl FnMut(u32) -> bool,
    pos_of: impl Fn(u32) -> (u64, V2),
) -> (u64, Fx, V2) {
    let cell = cell_of(pos.x, pos.y);
    let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
    let mut best = 0u64;
    let mut best_d = Fx::MAX;
    let mut best_pos = pos;
    let cs = Fx::from_num(saladin_sim::CELL_SIZE);
    for r in 0..=max_r {
        // a ring at Chebyshev distance r is at least (r-1)*CELL away
        let min_d = Fx::from_num((r - 1).max(0)) * cs;
        let min_d2 = min_d * min_d;
        if min_d2 > range2 || (best != 0 && min_d2 > best_d) {
            break;
        }
        for dy in -r..=r {
            let ny = cy + dy;
            if !in_grid(ny) {
                continue;
            }
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue; // ring perimeter only
                }
                let nx = cx + dx;
                if !in_grid(nx) {
                    continue;
                }
                let ci = (ny * CELLS_PER_ROW + nx) as usize;
                if own != 0 && masks[ci] & !own == 0 {
                    continue; // nothing but our own men in there
                }
                for &i in &grid[ci] {
                    if !accept(i) {
                        continue;
                    }
                    let (id, p) = pos_of(i);
                    let d = dist2(pos, p);
                    if d <= range2 && d < best_d {
                        best_d = d;
                        best = id;
                        best_pos = p;
                    }
                }
            }
        }
    }
    (best, best_d, best_pos)
}

/// Everything a pursuit needs to know about the ground.
struct Field<'a> {
    seed: u32,
    pgrid: &'a [bool],
    blocked: &'a [u64],
    tile_bld: &'a [u32],
    gates: &'a [(i32, u64)],
}

impl Field<'_> {
    #[inline]
    fn walkable(&self, tx: i32, ty: i32, owner: u64) -> bool {
        if !in_world(tx, ty) {
            return false;
        }
        let k = ty * WORLD_SIZE + tx;
        self.pgrid[k as usize]
            && !bit(self.blocked, k)
            && (self.gates.is_empty() || !gate_blocks(self.gates, k, owner))
    }

    /// What a shot cannot pass. Terrain is deliberately NOT in here: an arrow
    /// crosses a river, a wall stops it.
    #[inline]
    fn opaque(&self, tx: i32, ty: i32) -> bool {
        in_world(tx, ty) && bit(self.blocked, ty * WORLD_SIZE + tx)
    }
}

/// Walk the tile line from `from` toward `to`, at most `max` tiles. Returns the
/// last tile that can be stood on and, if the line ran into a structure, that
/// structure's snapshot index.
///
/// This replaced the old straight-line pursuit fallback, which is how a Spearman
/// walked into a SEALED wall ring and killed the peasant inside it in 5.8 s with
/// all 32 segments standing: movement integrates position without ever asking
/// the terrain, so a waypoint on the far side of a wall IS a hole in the wall.
fn ray_step(f: &Field, owner: u64, from: V2, to: V2, max: i32) -> (Option<V2>, Option<u32>) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len2 = dx * dx + dy * dy;
    if len2 <= Fx::ZERO {
        return (None, None);
    }
    let len = fx_sqrt(len2);
    let (ux, uy) = (dx / len, dy / len);
    let steps = max.min(floor_i(len).max(1));
    let mut last: Option<V2> = None;
    for s in 1..=steps {
        let t = Fx::from_num(s);
        let p = V2::new(from.x + ux * t, from.y + uy * t);
        let (tx, ty) = (floor_i(p.x), floor_i(p.y));
        if !f.walkable(tx, ty, owner) {
            let blocker = if in_world(tx, ty) {
                let b = f.tile_bld[(ty * WORLD_SIZE + tx) as usize];
                if b == NO_BLD { None } else { Some(b) }
            } else {
                None
            };
            return (last, blocker);
        }
        last = Some(p);
    }
    (last, None)
}

/// Bounded pursuit path into the tick's arena. Returns (offset, len, first
/// waypoint) or, when there is no way through at all, the structure standing in
/// the way — which is what turns "attack that man" into "besiege that wall".
#[allow(clippy::too_many_arguments)]
fn pursuit_patch(
    astar: &mut AStar,
    arena: &mut Vec<V2>,
    f: &Field,
    owner: u64,
    from: V2,
    to: V2,
) -> (Option<(u32, u32, V2)>, Option<u32>) {
    let passable = |tx: i32, ty: i32| f.walkable(tx, ty, owner);
    let cost = |tx: i32, ty: i32| move_cost_at(f.seed, tx, ty);
    let path = astar.find_path_costed(&passable, &cost, from.x, from.y, to.x, to.y, PURSUIT_EXPANSIONS);
    if path.is_empty() {
        let (step, blocker) = ray_step(f, owner, from, to, RAY_STEP);
        let mv = step.map(|p| {
            let off = arena.len() as u32;
            arena.push(p);
            (off, 1u32, p)
        });
        return (mv, blocker);
    }
    let off = arena.len() as u32;
    let t = path[0];
    arena.extend_from_slice(&path);
    (Some((off, path.len() as u32, t)), None)
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn combat(
    cfg: Res<WorldConfig>,
    statuses: Res<MatchStatuses>,
    mut shots: ResMut<ShotEvents>,
    mut path_scratch: ResMut<PathScratch>,
    mut s: ResMut<CombatScratch>,
    mut commands: Commands,
    mut q_units: Query<(Entity, &GameId, &mut Pos, &Owner, &MatchId, &mut Unit), Without<Building>>,
    mut q_buildings: Query<(Entity, &GameId, &Pos, &Owner, &MatchId, &mut Building), Without<Unit>>,
    mut q_players: Query<&mut Player>,
    mut stats: ResMut<crate::MatchStats>,
) {
    let seed = cfg.seed;
    shots.0.clear();
    let CombatScratch {
        grid,
        cell_owners,
        bgrid,
        bcell_owners,
        owners,
        units,
        buildings,
        uhp,
        bhp,
        udead,
        bdead,
        out,
        hit,
        engage,
        blocked,
        blocked_words,
        tile_bld,
        tile_dirty,
        gates,
        aura_blds,
        rally_auras,
        bgarr,
        shooters,
        gtargets,
        gt_idx,
        gshots,
        path_arena,
        caught,
        bcd,
    } = &mut *s;

    // tech mask per owner (for effective stats)
    let mut mask: bevy_platform::collections::HashMap<u64, u64> = Default::default();
    for p in &q_players {
        mask.insert(p.player_id, p.tech_mask);
    }
    let mask_of = |o: u64| mask.get(&o).copied().unwrap_or(0);

    // ── snapshots (sorted by id for deterministic processing) ────────────────
    units.clear();
    uhp.clear();
    for (ent, g, pos, owner, mid, u) in q_units.iter() {
        units.push(USnap {
            id: g.0,
            entity: ent,
            pos: pos.pos,
            target: u.target,
            home: u.home,
            owner: owner.0,
            oslot: 0,
            mtch: mid.0,
            kind: u.kind,
            stance: u.stance,
            attack_target: u.attack_target,
            cd: u.attack_cd,
            morale: u.morale,
            setup_timer: u.setup_timer,
            charge_cd: u.charge_cd,
            rally_cd: u.rally_cd,
            heading: u.heading,
            order: u.order,
            order_target: u.order_target,
            routing: u.routing,
            has_target: u.has_target,
            garr: u.garrisoned_in != 0,
            garrisoned_in: u.garrisoned_in,
            // hp read HERE, in the pass that already touches every row — the old
            // second `q.get()` per snapshot was a full random-access walk of 20k
            // archetype rows for one field.
            hp: u.hp,
            path_end: u.path.last().copied().unwrap_or(u.target),
        });
    }
    units.sort_unstable_by_key(|x| x.id);
    uhp.extend(units.iter().map(|u| u.hp));

    buildings.clear();
    bhp.clear();
    for (ent, g, pos, owner, mid, b) in q_buildings.iter() {
        buildings.push(BSnap {
            id: g.0,
            entity: ent,
            pos: pos.pos,
            owner: owner.0,
            oslot: 0,
            mtch: mid.0,
            kind: b.kind,
            state: b.state,
            hp: b.hp,
        });
    }
    buildings.sort_unstable_by_key(|x| x.id);
    bhp.extend(buildings.iter().map(|b| b.hp));

    let n = units.len();
    let m = buildings.len();
    udead.clear();
    hit.clear();
    out.clear();
    engage.clear();
    out.resize(n, UOut::default());
    udead.resize(n, false);
    hit.resize(n, false);
    engage.resize(n, 0);
    bdead.clear();
    bdead.resize(m, false);
    path_arena.clear();
    bcd.clear();

    // ── owner slots (a per-cell mask needs a small dense index) ──────────────
    owners.clear();
    for u in units.iter() {
        if !owners.contains(&u.owner) {
            owners.push(u.owner);
        }
    }
    for b in buildings.iter() {
        if !owners.contains(&b.owner) {
            owners.push(b.owner);
        }
    }
    owners.sort_unstable();
    let slot_of = |o: u64| -> u8 {
        match owners.binary_search(&o) {
            Ok(i) if i < OVERFLOW_SLOT as usize => i as u8,
            _ => OVERFLOW_SLOT,
        }
    };
    for u in units.iter_mut() {
        u.oslot = slot_of(u.owner);
    }
    for b in buildings.iter_mut() {
        b.oslot = slot_of(b.owner);
    }

    let uid_of = |units: &[USnap], id: u64| -> Option<usize> {
        units.binary_search_by_key(&id, |u| u.id).ok()
    };
    let bid_of = |bs: &[BSnap], id: u64| -> Option<usize> {
        bs.binary_search_by_key(&id, |b| b.id).ok()
    };

    // ── flat spatial grids (buckets keep their capacity across ticks) ────────
    for bucket in grid.iter_mut() {
        bucket.clear();
    }
    cell_owners.iter_mut().for_each(|c| *c = 0);
    for (i, u) in units.iter().enumerate() {
        let c = cell_of(u.pos.x, u.pos.y) as usize;
        grid[c].push(i as u32);
        cell_owners[c] |= 1u16 << u.oslot;
    }
    for bucket in bgrid.iter_mut() {
        bucket.clear();
    }
    bcell_owners.iter_mut().for_each(|c| *c = 0);
    for (i, b) in buildings.iter().enumerate() {
        let c = cell_of(b.pos.x, b.pos.y) as usize;
        bgrid[c].push(i as u32);
        bcell_owners[c] |= 1u16 << b.oslot;
    }

    // ── the ground: one bitset every pursuit, shot and shove reads ───────────
    for &w in blocked_words.iter() {
        blocked[w as usize] = 0;
    }
    blocked_words.clear();
    for &t in tile_dirty.iter() {
        tile_bld[t as usize] = NO_BLD;
    }
    tile_dirty.clear();
    gates.clear();
    aura_blds.clear();
    for (bi, b) in buildings.iter().enumerate() {
        let def = building_def(b.kind);
        if def.morale_radius > Fx::ZERO {
            aura_blds.push(bi as u32);
        }
        let cx = floor_i(b.pos.x);
        let cy = floor_i(b.pos.y);
        let r = def.footprint / 2;
        if def.passable {
            // a gate is a door in YOUR line, not a breach in it
            if operational(b.state) {
                gates.push((cy * WORLD_SIZE + cx, b.owner));
            }
            continue;
        }
        for i in 0..def.footprint {
            for j in 0..def.footprint {
                let (tx, ty) = (cx - r + i, cy - r + j);
                if !in_world(tx, ty) {
                    continue;
                }
                let k = ty * WORLD_SIZE + tx;
                set_bit(blocked, blocked_words, k);
                if tile_bld[k as usize] == NO_BLD {
                    tile_dirty.push(k as u32);
                }
                tile_bld[k as usize] = bi as u32;
            }
        }
    }
    let field = Field { seed, pgrid: passable_grid(seed), blocked, tile_bld, gates };

    // ── garrisons, by host ───────────────────────────────────────────────────
    bgarr.resize(m, Vec::new());
    for g in bgarr.iter_mut() {
        g.clear();
    }
    rally_auras.clear();
    let mut any_aura = false;
    for (i, u) in units.iter().enumerate() {
        if u.garrisoned_in != 0 {
            if let Some(bi) = bid_of(buildings, u.garrisoned_in) {
                bgarr[bi].push(i as u32);
            }
            continue;
        }
        let d = unit_def(u.kind);
        if d.rally_aura > Fx::ZERO {
            rally_auras.push((u.pos, d.rally_aura, u.owner));
        }
        any_aura |= d.morale_aura > Fx::ZERO;
    }
    let mut defeated_owners: HashSet<u64> = HashSet::default();
    let mut pursuit_budget = PURSUIT_BUDGET;

    // ── soldier loop ─────────────────────────────────────────────────────────
    for i in 0..n {
        let a = &units[i];
        if a.garr || udead[i] || !statuses.simulates(a.mtch) {
            continue;
        }
        let def = effective_unit_def(a.kind, mask_of(a.owner));
        if def.attack <= 0 {
            continue;
        }
        let cd = (a.cd - 1).max(0);
        let own_bit = if a.oslot == OVERFLOW_SLOT { 0 } else { 1u16 << a.oslot };

        // an engine that moved has to be emplaced again before it can shoot
        if def.setup_time > Fx::ZERO {
            let next =
                if a.has_target { def.setup_time } else { (a.setup_timer - DT).max(Fx::ZERO) };
            out[i].setup = Some(next);
        }
        let emplaced = def.setup_time <= Fx::ZERO || a.setup_timer <= Fx::ZERO;
        if a.charge_cd > 0 {
            out[i].charge_cd = Some(a.charge_cd - 1);
        }

        // routing: run for a rally point, not for the spawn corner
        if is_routing(a.routing, a.morale) {
            out[i].routing = Some(true);
            out[i].attack_target = Some(0);
            out[i].cooldown = Some(cd);
            out[i].rally_cd = Some(RALLY_RETURN);
            if !a.has_target && pursuit_budget > 0 {
                pursuit_budget -= 1;
                let threat = if a.attack_target != 0 {
                    uid_of(units, a.attack_target).map(|j| units[j].pos)
                } else {
                    None
                };
                let dest = flee_point(&field, buildings, aura_blds, a, threat);
                let (mv, _) = pursuit_patch(
                    &mut path_scratch.0,
                    path_arena,
                    &field,
                    a.owner,
                    a.pos,
                    dest,
                );
                out[i].mv = mv;
            }
            continue;
        }
        out[i].routing = Some(false);
        if a.rally_cd > 0 {
            out[i].rally_cd = Some(a.rally_cd - 1);
        }

        // ── acquire ─────────────────────────────────────────────────────────
        let capped = def.range <= LOS_MIN_RANGE;
        let melee_cap = if capped { MELEE_SLOTS } else { u8::MAX };
        // A man struck earlier THIS tick already has an answer waiting in `out`;
        // reading the snapshot alone would overwrite it with "no target" and the
        // retaliation would never survive the tick it was written on.
        let mut target_id = match out[i].attack_target {
            Some(t) if t != 0 => t,
            _ => a.attack_target,
        };
        if target_id == 0 && def.aggro_range > Fx::ZERO {
            // the high ground sees farther, not just shoots farther
            let here_elev = elevation_at(seed, a.pos.x, a.pos.y);
            let aggro = def.aggro_range * (Fx::ONE + saladin_sim::ELEV_BONUS_MAX);
            let r2 = aggro * aggro;
            let mut best = 0u64;
            if def.prefers_buildings {
                let max_r = (aggro.to_num::<i32>() / saladin_sim::CELL_SIZE + 1).clamp(1, 3);
                let (found, _, _) = nearest_in_rings(
                    bgrid,
                    bcell_owners,
                    own_bit,
                    a.pos,
                    max_r,
                    r2,
                    |j| {
                        let b = &buildings[j as usize];
                        b.owner != a.owner && b.mtch == a.mtch && !bdead[j as usize]
                    },
                    |j| {
                        let b = &buildings[j as usize];
                        (b.id, b.pos)
                    },
                );
                best = found;
            }
            if best == 0 {
                let max_r = (aggro.to_num::<i32>() / saladin_sim::CELL_SIZE + 1).clamp(1, 3);
                // High ground EXTENDS what a man notices; low ground does not
                // shrink it. A symmetric penalty reads well until you measure it:
                // two blocks exactly `aggro_range` apart on ground that differs
                // by a fortieth of the elevation span stopped seeing each other
                // at all, and three of four measured pairings lost contact
                // outright at a 6-tile gap.
                //
                // So inside `sure2` no slope can matter, and only the band
                // between it and the best-case reach pays for an elevation
                // lookup. A packed melee is entirely inside `sure2` — doing that
                // lookup per candidate unconditionally cost more than everything
                // else in this pass put together.
                let sure2 = def.aggro_range * def.aggro_range;
                let (found, _, _) = nearest_in_rings(
                    grid,
                    cell_owners,
                    own_bit,
                    a.pos,
                    max_r,
                    r2,
                    |j| {
                        let e = &units[j as usize];
                        // ordered by how often each test says no: in a two-sided
                        // melee the owner check rejects half the block outright
                        e.owner != a.owner
                            && e.mtch == a.mtch
                            && !e.garr
                            && !udead[j as usize]
                            && (!capped || engage[j as usize] < melee_cap)
                            && {
                                let d2 = dist2(a.pos, e.pos);
                                d2 <= sure2 || {
                                    let r = def.aggro_range
                                        * elevation_range_bonus(
                                            here_elev,
                                            elevation_at(seed, e.pos.x, e.pos.y),
                                        )
                                        .max(Fx::ONE);
                                    d2 <= r * r
                                }
                            }
                    },
                    |j| {
                        let e = &units[j as usize];
                        (e.id, e.pos)
                    },
                );
                best = found;
            }
            target_id = best;
        }

        // ── resolve target ──────────────────────────────────────────────────
        let (tpos, tunit, tbld) = if target_id == 0 {
            (None, None, None)
        } else if let Some(j) = uid_of(units, target_id) {
            if udead[j] { (None, None, None) } else { (Some(units[j].pos), Some(j), None) }
        } else if let Some(j) = bid_of(buildings, target_id) {
            if bdead[j] { (None, None, None) } else { (Some(buildings[j].pos), None, Some(j)) }
        } else {
            (None, None, None)
        };
        let Some(tpos) = tpos else {
            out[i].attack_target = Some(0);
            out[i].cooldown = Some(cd);
            // a rallied man walks back to the ground he was posted on instead of
            // standing at morale 1.00 where the rout stopped him
            if a.rally_cd > 0 && !a.has_target && pursuit_budget > 0 {
                let back = dist2(a.pos, a.home);
                if back > saladin_sim::fx!("4") {
                    pursuit_budget -= 1;
                    let (mv, _) = pursuit_patch(
                        &mut path_scratch.0,
                        path_arena,
                        &field,
                        a.owner,
                        a.pos,
                        a.home,
                    );
                    out[i].mv = mv;
                }
            } else if a.order == ORDER_ATTACK_MOVE
                && !a.has_target
                && pursuit_budget > 0
                && dist2(a.pos, a.order_target) > RESUME_EPS2
            {
                // nothing left to fight: the march RESUMES. Without this a body
                // that wins its first contact stands where it stopped, and two
                // armies more than one aggro range apart never meet again.
                pursuit_budget -= 1;
                let (mv, blocker) = pursuit_patch(
                    &mut path_scratch.0,
                    path_arena,
                    &field,
                    a.owner,
                    a.pos,
                    a.order_target,
                );
                out[i].mv = mv;
                // a wall between a man and where he was told to go IS the
                // objective — the same rule pursuit already applies
                if let Some(bi) = blocker {
                    let b = buildings[bi as usize];
                    if b.owner != a.owner && b.mtch == a.mtch && !bdead[bi as usize] {
                        out[i].attack_target = Some(b.id);
                    }
                }
            }
            continue;
        };

        let target_r = match tbld {
            Some(j) => Fx::from_num(building_def(buildings[j].kind).footprint) / Fx::from_num(2),
            None => Fx::ZERO,
        };
        let d2 = dist2(a.pos, tpos);
        let elev_mul =
            elevation_range_bonus(elevation_at(seed, a.pos.x, a.pos.y), elevation_at(seed, tpos.x, tpos.y));
        // squared compare: two fx_sqrt per unit per tick were 2.2% of total CPU
        let reach = def.range * elev_mul + target_r;
        let too_close = def.min_range > Fx::ZERO && d2 < def.min_range * def.min_range;
        let sighted = !def.ranged
            || def.arcs
            || d2 <= LOS_MIN_RANGE * LOS_MIN_RANGE
            || has_line_of_fire(&|x, y| field.opaque(x, y), a.pos, tpos, Sight::ground());
        let in_range = !too_close && d2 <= reach * reach && sighted;

        // FRONTAGE: a body can only be reached by so many men at once. The
        // overflow KEEPS its target and waits for a slot rather than dropping
        // it — releasing it made every man in the press re-run the ring
        // acquisition every single tick, which cost more than the whole rest of
        // the loop. A slot frees the moment a man in front of him falls.
        if let (true, Some(j)) = (in_range, tunit) {
            if engage[j] >= melee_cap {
                out[i].attack_target = Some(target_id);
                out[i].cooldown = Some(cd);
                out[i].clear_move = true;
                out[i].slot = Some(u8::MAX);
                continue;
            }
            engage[j] += 1;
            out[i].slot = Some(engage[j]);
        }

        let leash = if a.stance == Stance::Defensive { DEFENSIVE_LEASH } else { AGGRESSIVE_LEASH };
        let dh2 = dist2(a.pos, a.home);
        // `combat_action` only ever compares `dist_from_home >= leash`, so the
        // squared test can stand in for the sqrt. An attack-move is not leashed:
        // `home` is the far end of the march, so every man on the road is past
        // the leash and would turn back instead of closing on what he just saw.
        // An EXPLICIT order is never drift. The leash exists to stop one scout
        // dragging an army across the map, and that is an AGGRO pickup — but it
        // was measured against `home`, which nothing but a group order keeps
        // current, so a man told to kill something 30 tiles away dropped the
        // order on arrival and walked back. `Attack` and `AttackMove` are the
        // player naming a target; only a pickup is leashed.
        let leashed = a.order != ORDER_ATTACK_MOVE && a.order != ORDER_ATTACK;
        let drift = if leashed && dh2 >= leash * leash { leash } else { Fx::ZERO };
        let mut act = combat_action(a.stance, in_range, drift, leash);
        if act == CombatAct::Approach && a.stance == Stance::Aggressive && drift > Fx::ZERO {
            act = CombatAct::Return;
        }
        if too_close {
            act = CombatAct::Hold; // an engine inside its own dead zone
        }

        match act {
            CombatAct::Attack => {
                out[i].attack_target = Some(target_id);
                out[i].clear_move = true;
                out[i].heading = Some(heading16(V2::new(tpos.x - a.pos.x, tpos.y - a.pos.y)));
                if cd > 0 || !emplaced {
                    out[i].cooldown = Some(cd);
                    continue;
                }
                // ── the blow ────────────────────────────────────────────────
                // set spears only bite armour while they are SET; the same
                // standstill is what a charge is measured against
                let braced = def.brace && !a.has_target;
                let atk = Attacker {
                    attack: Fx::from_num(def.attack),
                    damage_type: def.damage_type,
                    bonus_vs_armor: applied_bonus(&def, braced),
                };
                let charging = def.charge_mult > Fx::ONE && a.charge_cd <= 0 && a.has_target;
                let mut killed = false;
                if let Some(j) = tunit {
                    let t = &units[j];
                    let tdef = effective_unit_def(t.kind, mask_of(t.owner));
                    let base = effective_damage_vs(&atk, &tdef);
                    let facing = facing_multiplier(t.heading, t.pos, a.pos);
                    let charge = if charging {
                        let frontal = is_frontal(t.heading, t.pos, a.pos);
                        let set = tdef.brace && !t.has_target;
                        charge_multiplier(def.charge_mult, set, frontal)
                    } else {
                        Fx::ONE
                    };
                    let dmg = (Fx::from_num(base) * facing * charge).floor().to_num::<i32>().max(1);
                    if def.ranged {
                        shots.0.push(Shot { from: a.pos, to: tpos, stone: a.kind == UnitKind::Mangonel });
                    }
                    let shock = facing * charge;
                    apply_hit(
                        Damage { j, dmg, from: a.id, from_pos: a.pos, shock },
                        uhp,
                        udead,
                        out,
                        hit,
                        units,
                        rally_auras,
                        &mask_of,
                    );
                    if def.splash > Fx::ZERO {
                        let share = (Fx::from_num(dmg) * SPLASH_SHARE).floor().to_num::<i32>().max(1);
                        let r2 = def.splash * def.splash;
                        caught.clear();
                        let br = (def.splash.to_num::<i32>() / saladin_sim::CELL_SIZE + 1).clamp(1, 2);
                        for_near(grid, tpos, br, |k| {
                            let e = &units[k as usize];
                            if k as usize == j
                                || e.owner == a.owner
                                || e.mtch != a.mtch
                                || e.garr
                                || udead[k as usize]
                                || dist2(tpos, e.pos) > r2
                            {
                                return;
                            }
                            caught.push(k);
                        });
                        for &k in caught.iter() {
                            apply_hit(
                                Damage {
                                    j: k as usize,
                                    dmg: share,
                                    from: a.id,
                                    from_pos: a.pos,
                                    shock: Fx::ONE,
                                },
                                uhp,
                                udead,
                                out,
                                hit,
                                units,
                                rally_auras,
                                &mask_of,
                            );
                        }
                    }
                    killed = udead[j];
                } else if let Some(j) = tbld {
                    let t = buildings[j];
                    let bdef = effective_building_def(t.kind, mask_of(t.owner));
                    let dmg = building_damage(&atk, &bdef);
                    if def.ranged {
                        shots.0.push(Shot { from: a.pos, to: tpos, stone: a.kind == UnitKind::Mangonel });
                    }
                    bhp[j] = (bhp[j] - dmg).max(0);
                    // a shell on the parapet shakes the men under it, and a
                    // broken man leaves the tower still standing
                    if def.damage_type == DamageType::Siege && !bgarr[j].is_empty() {
                        let drop = bombard_morale(dmg, bdef.max_hp);
                        for &g in bgarr[j].iter() {
                            let ui = g as usize;
                            if udead[ui] {
                                continue;
                            }
                            let base = out[ui].morale.unwrap_or(units[ui].morale);
                            let mo = (base - drop).max(Fx::ZERO);
                            out[ui].morale = Some(mo);
                            hit[ui] = true;
                            if mo < ROUT_THRESHOLD {
                                out[ui].routing = Some(true);
                                let exit = nearest_passable_grid(
                                    &|tx, ty| field.walkable(tx, ty, units[ui].owner),
                                    t.pos.x,
                                    t.pos.y,
                                );
                                out[ui].eject_to = Some(exit);
                            }
                        }
                    }
                    if bhp[j] <= 0 {
                        bdead[j] = true;
                        if building_def(t.kind).defeat_on_death {
                            defeated_owners.insert(t.owner);
                        }
                        killed = true;
                    }
                }
                if charging {
                    out[i].charge_cd = Some(CHARGE_COOLDOWN);
                }
                out[i].attack_target = Some(if killed { 0 } else { target_id });
                out[i].cooldown = Some(def.attack_ticks.max(1));
            }
            CombatAct::Approach => {
                out[i].cooldown = Some(cd);
                out[i].attack_target = Some(target_id);
                // Repath when the way we were walking has since been walled off,
                // and when the quarry has MOVED away from where this path ends —
                // a pursuit that only repaths on arrival chases a ghost.
                let waypoint_gone = a.has_target
                    && !field.walkable(floor_i(a.target.x), floor_i(a.target.y), a.owner);
                let quarry_moved = a.has_target && dist2(a.path_end, tpos) > REPATH_DRIFT2;
                if (!a.has_target || waypoint_gone || quarry_moved) && pursuit_budget > 0 {
                    pursuit_budget -= 1;
                    let (mv, blocker) = pursuit_patch(
                        &mut path_scratch.0,
                        path_arena,
                        &field,
                        a.owner,
                        a.pos,
                        tpos,
                    );
                    out[i].mv = mv;
                    if mv.is_none() {
                        out[i].clear_move = true;
                    }
                    // a wall in the way IS the objective — this one rule is what
                    // makes besieging emerge from attacking
                    if let Some(bi) = blocker {
                        let b = buildings[bi as usize];
                        if b.owner != a.owner && b.mtch == a.mtch && !bdead[bi as usize] {
                            out[i].attack_target = Some(b.id);
                        }
                    }
                }
            }
            CombatAct::Return => {
                out[i].cooldown = Some(cd);
                out[i].attack_target = Some(0);
                if !a.has_target && pursuit_budget > 0 {
                    pursuit_budget -= 1;
                    let (mv, _) =
                        pursuit_patch(&mut path_scratch.0, path_arena, &field, a.owner, a.pos, a.home);
                    out[i].mv = mv;
                }
            }
            CombatAct::Hold => {
                out[i].cooldown = Some(cd);
                out[i].attack_target = Some(if in_range { target_id } else { 0 });
                // Hold Ground means hold the ground: stance was ignored
                // entirely, so the only stop in the game was a Move onto your
                // own feet. An explicit order still overrides it.
                if a.order != ORDER_MOVE && a.order != ORDER_ATTACK_MOVE {
                    out[i].clear_move = true;
                }
            }
        }
    }

    // ── structure fire: the host's own bow plus every man on the parapet ─────
    for bi in 0..m {
        let b = buildings[bi];
        // a foundation has no parapet: an unfinished tower fires nothing
        if bdead[bi] || !operational(b.state) || !statuses.simulates(b.mtch) {
            continue;
        }
        let bdef = effective_building_def(b.kind, mask_of(b.owner));
        shooters.clear();
        let mut garr_range = Fx::ZERO;
        let mut garr_rate = Fx::MAX;
        for &g in bgarr[bi].iter() {
            let ui = g as usize;
            if udead[ui] {
                continue;
            }
            let d = unit_def(units[ui].kind);
            shooters.push(GarrisonShooter::of(d));
            if d.ranged && d.attack > 0 {
                garr_range = garr_range.max(d.range);
                garr_rate = garr_rate.min(d.attack_rate);
            }
        }
        let host_fires = bdef.attack > 0 && bdef.range > Fx::ZERO;
        let garr_fires = bdef.garrison_cap > 0 && garr_range > Fx::ZERO;
        if !host_fires && !garr_fires {
            continue;
        }
        let fire_range = if bdef.range > Fx::ZERO { bdef.range } else { garr_range };
        let fire_rate = if bdef.attack_rate > Fx::ZERO {
            bdef.attack_rate
        } else if garr_rate < Fx::MAX {
            garr_rate
        } else {
            Fx::ONE
        };
        let cooldown = q_buildings.get(b.entity).map(|(_, _, _, _, _, bb)| bb.cooldown).unwrap_or(Fx::ZERO);
        let cd = (cooldown - DT).max(Fx::ZERO);
        if cd > Fx::ZERO {
            bcd.push((b.entity, cd));
            continue;
        }

        // nearest enemies within best-case elevation reach, then line of fire.
        // A tower that shoots around its own corner is a wall with hit points.
        let reach = fire_range * (Fx::ONE + saladin_sim::ELEV_BONUS_MAX);
        let reach2 = reach * reach;
        let own_bit = if b.oslot == OVERFLOW_SLOT { 0 } else { 1u16 << b.oslot };
        let max_r = (reach.to_num::<i32>() / saladin_sim::CELL_SIZE + 1).clamp(1, 3);
        const TOPK: usize = 16;
        let mut top: [(Fx, u64, u32); TOPK] = [(Fx::MAX, 0, 0); TOPK];
        let mut ntop = 0usize;
        {
            let cell = cell_of(b.pos.x, b.pos.y);
            let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
            for dy in -max_r..=max_r {
                let ny = cy + dy;
                if !in_grid(ny) {
                    continue;
                }
                for dx in -max_r..=max_r {
                    let nx = cx + dx;
                    if !in_grid(nx) {
                        continue;
                    }
                    let ci = (ny * CELLS_PER_ROW + nx) as usize;
                    if own_bit != 0 && cell_owners[ci] & !own_bit == 0 {
                        continue;
                    }
                    for &j in &grid[ci] {
                        let e = &units[j as usize];
                        if e.owner == b.owner || e.mtch != b.mtch || e.garr || udead[j as usize] {
                            continue;
                        }
                        let d = dist2(b.pos, e.pos);
                        if d > reach2 {
                            continue;
                        }
                        // insertion sort by (distance, id) — deterministic, and
                        // no allocation for a tower that fires every tick
                        let key = (d, e.id);
                        if ntop == TOPK && key >= (top[TOPK - 1].0, top[TOPK - 1].1) {
                            continue;
                        }
                        let mut p = ntop.min(TOPK - 1);
                        while p > 0 && (top[p - 1].0, top[p - 1].1) > key {
                            top[p] = top[p - 1];
                            p -= 1;
                        }
                        top[p] = (d, e.id, j);
                        if ntop < TOPK {
                            ntop += 1;
                        }
                    }
                }
            }
        }
        let tower_elev = elevation_at(seed, b.pos.x, b.pos.y);
        gtargets.clear();
        gt_idx.clear();
        let cap = bdef.garrison_cap.clamp(1, 8) as usize;
        for t in top.iter().take(ntop) {
            if gtargets.len() >= cap {
                break;
            }
            let e = &units[t.2 as usize];
            let telev = elevation_at(seed, e.pos.x, e.pos.y);
            if fx_sqrt(t.0) > fire_range * elevation_range_bonus(tower_elev, telev) {
                continue;
            }
            if !has_line_of_fire(
                &|x, y| field.opaque(x, y),
                b.pos,
                e.pos,
                Sight::from_parapet(tower_elev, telev),
            ) {
                continue;
            }
            let tdef = effective_unit_def(e.kind, mask_of(e.owner));
            gtargets.push(GarrisonTarget {
                armor: tdef.armor_class,
                damage_reduction: tdef.damage_reduction,
            });
            gt_idx.push(t.2);
        }
        if gtargets.is_empty() {
            bcd.push((b.entity, cd));
            continue;
        }

        let mut fired = false;
        if host_fires {
            let j = gt_idx[0] as usize;
            let t = units[j].clone();
            let tdef = effective_unit_def(t.kind, mask_of(t.owner));
            let atk = Attacker {
                attack: Fx::from_num(bdef.attack),
                damage_type: bdef.damage_type,
                bonus_vs_armor: [Fx::ONE; 4],
            };
            let dmg = effective_damage_vs(&atk, &tdef);
            shots.0.push(Shot { from: b.pos, to: t.pos, stone: false });
            apply_hit(
                Damage { j, dmg, from: 0, from_pos: b.pos, shock: Fx::ONE },
                uhp,
                udead,
                out,
                hit,
                units,
                rally_auras,
                &mask_of,
            );
            fired = true;
        }
        // one arrow per man, each with its OWN damage type and bonus: a
        // garrisoned Crossbowman is still a Crossbowman
        garrison_volley(shooters, &bdef, gtargets, gshots);
        for shot in gshots.iter() {
            let j = gt_idx[shot.target] as usize;
            if udead[j] {
                continue;
            }
            shots.0.push(Shot { from: b.pos, to: units[j].pos, stone: false });
            apply_hit(
                Damage { j, dmg: shot.damage, from: 0, from_pos: b.pos, shock: Fx::ONE },
                uhp,
                udead,
                out,
                hit,
                units,
                rally_auras,
                &mask_of,
            );
            fired = true;
        }
        bcd.push((b.entity, if fired { fire_rate } else { cd }));
    }

    // ── dying hosts: evacuate or entomb their garrison ───────────────────────
    for bi in 0..m {
        if !bdead[bi] {
            continue;
        }
        let b = buildings[bi];
        let bdef = building_def(b.kind);
        for &g in bgarr[bi].iter() {
            let ui = g as usize;
            if udead[ui] {
                continue;
            }
            if bdef.garrison_survives_death {
                let exit = nearest_passable_grid(
                    &|tx, ty| field.walkable(tx, ty, units[ui].owner),
                    b.pos.x,
                    b.pos.y,
                );
                out[ui].eject_to = Some(exit);
            } else {
                udead[ui] = true;
            }
        }
    }

    // ── morale recovery (units not hit this tick) ────────────────────────────
    // Workers and support kinds were skipped here, so their morale was
    // permanently-decaying dead state: nothing ever put it back.
    for i in 0..n {
        let a = &units[i];
        if a.garr || udead[i] || hit[i] || !statuses.simulates(a.mtch) {
            continue;
        }
        let routing_now = out[i].routing.unwrap_or(a.routing);
        if a.morale >= MORALE_MAX && !routing_now {
            continue;
        }
        let mut allies = 0i32;
        let mut support = false;
        {
            let r2 = ALLY_RADIUS * ALLY_RADIUS;
            const ALLY_R: i32 = 5 / saladin_sim::CELL_SIZE + 1;
            // Recovery caps the ally count, so with no aura anywhere on the map
            // the scan can stop at the cap instead of counting a whole packed
            // block per shaken man.
            for_near_until(grid, a.pos, ALLY_R, |j| {
                let e = &units[j as usize];
                if j as usize == i || e.owner != a.owner || e.garr || udead[j as usize] {
                    return true;
                }
                let d = dist2(a.pos, e.pos);
                if d <= r2 {
                    allies += 1;
                }
                if any_aura {
                    let aura = unit_def(e.kind).morale_aura;
                    if aura > Fx::ZERO && d <= aura * aura {
                        support = true;
                    }
                    return true;
                }
                allies < saladin_sim::morale::MORALE_ALLY_CAP
            });
        }
        // the ground a keep or a mosque steadies: `morale_radius` is the rule,
        // not a hardcoded kind — and only the handful of structures that HAVE
        // one are visited (a wall line used to cost 36% of the tick here)
        if !support {
            for &bi in aura_blds.iter() {
                let b = &buildings[bi as usize];
                if b.owner != a.owner || !operational(b.state) || bdead[bi as usize] {
                    continue;
                }
                let r = building_def(b.kind).morale_radius;
                if dist2(a.pos, b.pos) <= r * r {
                    support = true;
                    break;
                }
            }
        }
        let morale = morale_recover(a.morale, DT, allies, support);
        out[i].morale = Some(morale);
        out[i].routing = Some(is_routing(a.routing, morale));
    }

    // ── apply ────────────────────────────────────────────────────────────────
    for i in 0..n {
        let snap = &units[i];
        let Ok((ent, _g, mut p, _o, _m, mut u)) = q_units.get_mut(snap.entity) else { continue };
        if udead[i] {
            stats.of(snap.owner).lost += 1;
            commands.entity(ent).despawn();
            continue;
        }
        u.hp = uhp[i];
        let o = &out[i];
        if let Some(exit) = o.eject_to {
            p.pos = exit;
            u.garrisoned_in = 0;
            u.has_target = false;
            u.path.clear();
            u.path_idx = 0;
            u.attack_target = 0;
            u.home = exit;
        }
        if let Some(t) = o.attack_target {
            u.attack_target = t;
        }
        if let Some(cd) = o.cooldown {
            u.attack_cd = cd;
        }
        if let Some(mo) = o.morale {
            u.morale = mo;
        }
        if let Some(r) = o.routing {
            u.routing = r;
        }
        if let Some(h) = o.heading {
            u.heading = h;
        }
        if let Some(st) = o.setup {
            u.setup_timer = st;
        }
        if let Some(c) = o.charge_cd {
            u.charge_cd = c;
        }
        if let Some(r) = o.rally_cd {
            u.rally_cd = r;
        }
        u.engage_slot = o.slot.unwrap_or(0);
        if let Some((off, len, target)) = o.mv {
            u.path.clear();
            u.path.extend_from_slice(&path_arena[off as usize..(off + len) as usize]);
            u.path_idx = 0;
            u.target = target;
            u.has_target = true;
        } else if o.clear_move {
            u.has_target = false;
        }
    }
    for i in 0..m {
        let snap = &buildings[i];
        let Ok((ent, _g, _p, _o, _m, mut b)) = q_buildings.get_mut(snap.entity) else { continue };
        if bdead[i] {
            commands.entity(ent).despawn();
            continue;
        }
        b.hp = bhp[i];
    }
    for &(ent, cd) in bcd.iter() {
        if let Ok((_, _, _, _, _, mut b)) = q_buildings.get_mut(ent) {
            b.cooldown = cd;
        }
    }
    if !defeated_owners.is_empty() {
        for mut p in &mut q_players {
            if defeated_owners.contains(&p.player_id) {
                p.defeated = true;
            }
        }
    }
}

/// Where a broken man runs. Not to the spawn corner: a rout that leaves the
/// field permanently is how a 40v40 self-terminated at 20v20 and then stood
/// still for 230 seconds.
fn flee_point(
    field: &Field,
    buildings: &[BSnap],
    aura_blds: &[u32],
    a: &USnap,
    threat: Option<V2>,
) -> V2 {
    let anchor_r2 = ROUT_ANCHOR_RANGE * ROUT_ANCHOR_RANGE;
    let mut best: Option<(Fx, V2)> = None;
    for &bi in aura_blds {
        let b = &buildings[bi as usize];
        if b.owner != a.owner || !operational(b.state) {
            continue;
        }
        let d = dist2(a.pos, b.pos);
        if d <= anchor_r2 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, b.pos));
        }
    }
    if let Some((_, p)) = best {
        return p;
    }
    let away = match threat {
        Some(t) => V2::new(a.pos.x - t.x, a.pos.y - t.y),
        None => V2::new(a.home.x - a.pos.x, a.home.y - a.pos.y),
    };
    let len2 = away.x * away.x + away.y * away.y;
    if len2 <= Fx::ZERO {
        return a.pos;
    }
    let len = fx_sqrt(len2);
    let run = ROUT_FLEE.min(len);
    let raw = V2::new(a.pos.x + away.x / len * run, a.pos.y + away.y / len * run);
    let world = Fx::from_num(WORLD_SIZE) - Fx::ONE;
    let clamped = V2::new(raw.x.clamp(Fx::ONE, world), raw.y.clamp(Fx::ONE, world));
    nearest_passable_grid(&|tx, ty| field.walkable(tx, ty, a.owner), clamped.x, clamped.y)
}

struct Damage {
    j: usize,
    dmg: i32,
    from: u64,
    from_pos: V2,
    /// Facing and charge multiplier — a blow from behind shakes harder than it
    /// wounds.
    shock: Fx,
}

#[allow(clippy::too_many_arguments)]
fn apply_hit(
    d: Damage,
    uhp: &mut [i32],
    udead: &mut [bool],
    out: &mut [UOut],
    hit: &mut [bool],
    units: &[USnap],
    rally_auras: &[(V2, Fx, u64)],
    mask_of: &impl Fn(u64) -> u64,
) {
    let j = d.j;
    let old = uhp[j];
    if old <= 0 {
        return;
    }
    let new = (old - d.dmg).max(0);
    uhp[j] = new;
    if new <= 0 {
        udead[j] = true;
        return;
    }
    let t = &units[j];
    let tdef = effective_unit_def(t.kind, mask_of(t.owner));
    let maxhp = tdef.max_hp;
    let frac = if maxhp > 0 {
        Fx::from_num(old - new) / Fx::from_num(maxhp) * d.shock
    } else {
        Fx::ZERO
    };
    hit[j] = true;
    let in_aura = rally_auras.iter().any(|(p, r, o)| *o == t.owner && dist2(t.pos, *p) <= *r * *r);
    let resolve = disciplined_resolve(tdef.morale_resolve, in_aura);
    let base = out[j].morale.unwrap_or(t.morale);
    out[j].morale = Some(morale_after_hit_resolve(base, frac, resolve));
    // A man shot from beyond his own aggro range used to have no answer at all:
    // taking damage only lowered morale. One field write ends ranged units
    // farming a standing line.
    if d.from != 0 && tdef.attack > 0 && t.attack_target == 0 && out[j].attack_target.is_none_or(|v| v == 0)
    {
        let _ = d.from_pos;
        out[j].attack_target = Some(d.from);
    }
}
