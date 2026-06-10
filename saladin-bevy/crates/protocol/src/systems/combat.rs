//! Combat — runs every combat tick (200 ms). Soldiers auto-acquire enemies,
//! close to range, and strike on cooldown; structures fire (their own bows plus
//! their garrison's); morale routs and recovers. Ported from the SpacetimeDB
//! `combatTick` reducer.
//!
//! Engineered for big battles: a FLAT spatial grid (324 cells, buckets with
//! retained capacity), index-based hp/output tables instead of per-tick
//! HashMaps, inline cell-block scans with zero per-unit allocation, and a
//! capped pursuit A* (straight-line fallback) so a mass first-contact can't
//! spike the tick. All state lives in `CombatScratch`, reused across ticks.

use crate::components::{Building, GameId, MatchId, Owner, Player, Pos, Unit};
use crate::{MatchStatuses, PathScratch, Shot, ShotEvents, WorldConfig};
use bevy_ecs::prelude::*;
use bevy_platform::collections::{HashMap, HashSet};
use saladin_sim::{
    Attacker, BuildingKind, CELL_COUNT, CELLS_PER_ROW, COMBAT_DT, CombatAct, DEFENSIVE_LEASH, Fx,
    GarrisonOccupant, MORALE_MAX, Stance, UnitKind, V2, building_def, cell_of, combat_action, dist,
    dist2, effective_building_def, effective_damage, effective_unit_def,
    elevation_at, elevation_range_bonus, garrison_fire_power, is_passable, is_routing, morale_after_hit,
    morale_recover, nearest_passable_grid, unit_def,
};

const DT: Fx = COMBAT_DT;
const ALLY_RADIUS: Fx = saladin_sim::fx!("5");
/// Pursuit pathfinding budget per unit per combat tick. The full-map budget made
/// a mass first-contact (thousands of units pathing at once) spike the tick;
/// a bounded search plus a straight-line fallback keeps worst-case flat —
/// blocked units simply re-path on a later tick when closer.
const PURSUIT_EXPANSIONS: usize = 1200;
/// Max pursuit paths computed per combat tick (id order, deterministic). A mass
/// first-contact charge staggers over a few ticks instead of spiking one.
const PURSUIT_BUDGET: u32 = 768;

#[derive(Clone)]
struct USnap {
    id: u64,
    entity: Entity,
    pos: V2,
    owner: u64,
    mtch: u64,
    kind: UnitKind,
    stance: Stance,
    home: V2,
    attack_target: u64,
    cooldown: Fx,
    morale: Fx,
    routing: bool,
    has_target: bool,
    garr: bool,
    garrisoned_in: u64,
}

#[derive(Clone, Copy)]
struct BSnap {
    id: u64,
    entity: Entity,
    pos: V2,
    owner: u64,
    mtch: u64,
    kind: BuildingKind,
}

/// What to write back to a unit after the decide pass.
#[derive(Default, Clone)]
struct UOut {
    attack_target: Option<u64>,
    cooldown: Option<Fx>,
    morale: Option<Fx>,
    routing: Option<bool>,
    clear_move: bool,
    mv: Option<(Vec<V2>, V2)>,
    eject_to: Option<V2>,
}

/// All per-tick combat working memory, retained across ticks so the hot path
/// never allocates once capacities warm up.
#[derive(Resource)]
pub struct CombatScratch {
    grid: Vec<Vec<u32>>, // flat cell id → snapshot indices
    units: Vec<USnap>,
    buildings: Vec<BSnap>,
    uhp: Vec<i32>,
    bhp: Vec<i32>,
    udead: Vec<bool>,
    bdead: Vec<bool>,
    out: Vec<UOut>,
    uidx: HashMap<u64, u32>,
    bidx: HashMap<u64, u32>,
    hit: Vec<bool>,
    garr_profile: HashMap<u64, Vec<GarrisonOccupant>>,
    garr_range_rate: HashMap<u64, (Fx, Fx)>,
}

impl Default for CombatScratch {
    fn default() -> Self {
        CombatScratch {
            grid: vec![Vec::new(); CELL_COUNT as usize],
            units: Vec::new(),
            buildings: Vec::new(),
            uhp: Vec::new(),
            bhp: Vec::new(),
            udead: Vec::new(),
            bdead: Vec::new(),
            out: Vec::new(),
            uidx: HashMap::default(),
            bidx: HashMap::default(),
            hit: Vec::new(),
            garr_profile: HashMap::default(),
            garr_range_rate: HashMap::default(),
        }
    }
}

/// Visit every snapshot index in the `r`-cell Chebyshev block around `pos` —
/// inline, no allocation.
#[inline]
fn for_near(grid: &[Vec<u32>], pos: V2, r: i32, mut visit: impl FnMut(u32)) {
    let cell = cell_of(pos.x, pos.y);
    let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
    for dy in -r..=r {
        let ny = cy + dy;
        if ny < 0 || ny >= CELLS_PER_ROW {
            continue;
        }
        for dx in -r..=r {
            let nx = cx + dx;
            if nx < 0 || nx >= CELLS_PER_ROW {
                continue;
            }
            for &i in &grid[(ny * CELLS_PER_ROW + nx) as usize] {
                visit(i);
            }
        }
    }
}

/// Nearest matching unit by RING-ordered cell scan with early exit: once a ring
/// finds a candidate, any farther ring's cells cannot beat it if their minimum
/// possible distance exceeds the best found — in a dense melee this stops at
/// ring 0/1 instead of scanning the whole block. Deterministic: full rings are
/// always finished, ties broken by lowest distance then scan order (fixed).
#[inline]
fn nearest_in_rings(
    grid: &[Vec<u32>],
    pos: V2,
    max_r: i32,
    range2: Fx,
    mut accept: impl FnMut(u32) -> bool,
    units: &[USnap],
) -> (u64, Fx, V2) {
    let cell = cell_of(pos.x, pos.y);
    let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
    let mut best = 0u64;
    let mut best_d = Fx::MAX;
    let mut best_pos = pos;
    let cs = Fx::from_num(saladin_sim::CELL_SIZE);
    for r in 0..=max_r {
        // a ring at Chebyshev distance r is at least (r-1)*CELL away
        if best != 0 {
            let min_d = Fx::from_num((r - 1).max(0)) * cs;
            if min_d * min_d > best_d {
                break;
            }
        }
        for dy in -r..=r {
            let ny = cy + dy;
            if ny < 0 || ny >= CELLS_PER_ROW {
                continue;
            }
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue; // ring perimeter only
                }
                let nx = cx + dx;
                if nx < 0 || nx >= CELLS_PER_ROW {
                    continue;
                }
                for &i in &grid[(ny * CELLS_PER_ROW + nx) as usize] {
                    if !accept(i) {
                        continue;
                    }
                    let e = &units[i as usize];
                    let d = dist2(pos, e.pos);
                    if d <= range2 && d < best_d {
                        best_d = d;
                        best = e.id;
                        best_pos = e.pos;
                    }
                }
            }
        }
    }
    (best, best_d, best_pos)
}

/// Bounded pursuit path: capped A*, straight-line fallback (blocked units
/// re-path on a later combat tick once they are closer).
fn pursuit_patch(scratch: &mut PathScratch, seed: u32, from: V2, to: V2) -> Option<(Vec<V2>, V2)> {
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty);
    let path = scratch.0.find_path(&passable, from.x, from.y, to.x, to.y, PURSUIT_EXPANSIONS);
    if path.is_empty() {
        let snap = nearest_passable_grid(&passable, to.x, to.y);
        Some((vec![snap], snap))
    } else {
        let t = path[0];
        Some((path, t))
    }
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
) {
    let seed = cfg.seed;
    shots.0.clear();
    let s = &mut *s;

    // tech mask per owner (for effective stats)
    let mut mask: HashMap<u64, u64> = HashMap::default();
    for p in &q_players {
        mask.insert(p.player_id, p.tech_mask);
    }
    let mask_of = |o: u64| mask.get(&o).copied().unwrap_or(0);

    // ── snapshots (sorted by id for deterministic processing) ────────────────
    s.units.clear();
    for (ent, g, pos, owner, mid, u) in q_units.iter() {
        s.units.push(USnap {
            id: g.0,
            entity: ent,
            pos: pos.pos,
            owner: owner.0,
            mtch: mid.0,
            kind: u.kind,
            stance: u.stance,
            home: u.home,
            attack_target: u.attack_target,
            cooldown: u.attack_cooldown,
            morale: u.morale,
            routing: u.routing,
            has_target: u.has_target,
            garr: u.garrisoned_in != 0,
            garrisoned_in: u.garrisoned_in,
        });
    }
    s.units.sort_unstable_by_key(|x| x.id);

    s.buildings.clear();
    for (ent, g, pos, owner, mid, b) in q_buildings.iter() {
        s.buildings.push(BSnap { id: g.0, entity: ent, pos: pos.pos, owner: owner.0, mtch: mid.0, kind: b.kind });
    }
    s.buildings.sort_unstable_by_key(|x| x.id);

    let n = s.units.len();
    let m = s.buildings.len();
    s.uidx.clear();
    s.bidx.clear();
    for (i, u) in s.units.iter().enumerate() {
        s.uidx.insert(u.id, i as u32);
    }
    for (i, b) in s.buildings.iter().enumerate() {
        s.bidx.insert(b.id, i as u32);
    }
    s.uhp.clear();
    s.udead.clear();
    s.hit.clear();
    s.out.clear();
    s.out.resize(n, UOut::default());
    s.udead.resize(n, false);
    s.hit.resize(n, false);
    s.bhp.clear();
    s.bdead.clear();
    s.bdead.resize(m, false);
    // hp tables in snapshot order
    {
        // snapshots were built from the same iteration set; read hp via entity
        for u in &s.units {
            let hp = q_units.get(u.entity).map(|(_, _, _, _, _, uu)| uu.hp).unwrap_or(0);
            s.uhp.push(hp);
        }
        for b in &s.buildings {
            let hp = q_buildings.get(b.entity).map(|(_, _, _, _, _, bb)| bb.hp).unwrap_or(0);
            s.bhp.push(hp);
        }
    }

    // ── flat spatial grid (buckets keep their capacity across ticks) ─────────
    for bucket in s.grid.iter_mut() {
        bucket.clear();
    }
    for (i, u) in s.units.iter().enumerate() {
        s.grid[cell_of(u.pos.x, u.pos.y) as usize].push(i as u32);
    }

    // ── garrison fire profiles ───────────────────────────────────────────────
    s.garr_profile.clear();
    s.garr_range_rate.clear();
    for u in &s.units {
        if u.garrisoned_in == 0 {
            continue;
        }
        let def = unit_def(u.kind);
        s.garr_profile
            .entry(u.garrisoned_in)
            .or_default()
            .push(GarrisonOccupant { attack: def.attack, ranged: def.ranged });
        if def.ranged && def.attack > 0 {
            let e = s.garr_range_rate.entry(u.garrisoned_in).or_insert((Fx::ZERO, Fx::MAX));
            e.0 = e.0.max(def.range);
            e.1 = e.1.min(def.attack_rate);
        }
    }

    let mut defeated_owners: HashSet<u64> = HashSet::default();
    let mut pursuit_budget = PURSUIT_BUDGET;

    // ── soldier loop ─────────────────────────────────────────────────────────
    for i in 0..n {
        let a = s.units[i].clone();
        if a.garr || s.udead[i] || !statuses.simulates(a.mtch) {
            continue;
        }
        let def = effective_unit_def(a.kind, mask_of(a.owner));
        if def.attack <= 0 {
            continue;
        }
        let cd = (a.cooldown - DT).max(Fx::ZERO);
        let out = &mut s.out[i];

        // routing: flee toward home, suppress attacks
        if is_routing(a.routing, a.morale) {
            out.routing = Some(true);
            out.attack_target = Some(0);
            out.cooldown = Some(cd);
            if !a.has_target && pursuit_budget > 0 {
                pursuit_budget -= 1;
                out.mv = pursuit_patch(&mut path_scratch, seed, a.pos, a.home);
            }
            continue;
        }
        out.routing = Some(false);

        // acquire a target if none — inline block scan, squared distances
        let mut target_id = a.attack_target;
        if target_id == 0 && def.aggro_range > Fx::ZERO {
            let r2 = def.aggro_range * def.aggro_range;
            let mut best = 0u64;
            let mut best_d = Fx::MAX;
            // siege prefers buildings: scan the (small) building list first
            if def.prefers_buildings {
                for b in s.buildings.iter() {
                    let bi = s.bidx[&b.id] as usize;
                    if b.owner == a.owner || b.mtch != a.mtch || s.bdead[bi] {
                        continue;
                    }
                    let d = dist2(a.pos, b.pos);
                    if d <= r2 && d < best_d {
                        best_d = d;
                        best = b.id;
                    }
                }
            }
            if best == 0 {
                let units = &s.units;
                let udead = &s.udead;
                // block radius sized to the actual aggro range
                let max_r = (def.aggro_range.to_num::<i32>() / saladin_sim::CELL_SIZE + 1).clamp(1, 3);
                let (found, _, _) = nearest_in_rings(
                    &s.grid,
                    a.pos,
                    max_r,
                    r2,
                    |j| {
                        let e = &units[j as usize];
                        e.id != a.id && e.owner != a.owner && e.mtch == a.mtch && !e.garr && !udead[j as usize]
                    },
                    units,
                );
                let _ = best_d;
                best = found;
            }
            target_id = best;
        }

        // resolve target position + liveness
        let (tpos, t_is_unit) = {
            if target_id == 0 {
                (None, false)
            } else if let Some(&j) = s.uidx.get(&target_id) {
                if s.udead[j as usize] { (None, false) } else { (Some(s.units[j as usize].pos), true) }
            } else if let Some(&j) = s.bidx.get(&target_id) {
                if s.bdead[j as usize] { (None, false) } else { (Some(s.buildings[j as usize].pos), false) }
            } else {
                (None, false)
            }
        };
        let Some(tpos) = tpos else {
            let out = &mut s.out[i];
            out.attack_target = Some(0);
            out.cooldown = Some(cd);
            continue;
        };
        let target_r = if t_is_unit {
            Fx::ZERO
        } else {
            let k = s.buildings[s.bidx[&target_id] as usize].kind;
            Fx::from_num(building_def(k).footprint) / Fx::from_num(2)
        };
        let d = dist(a.pos, tpos);
        let elev_mul = elevation_range_bonus(elevation_at(seed, a.pos.x, a.pos.y), elevation_at(seed, tpos.x, tpos.y));

        if d <= def.range * elev_mul + target_r {
            if cd > Fx::ZERO {
                let out = &mut s.out[i];
                out.attack_target = Some(target_id);
                out.cooldown = Some(cd);
                out.clear_move = true;
                continue;
            }
            // strike
            let armor = if t_is_unit {
                let t = &s.units[s.uidx[&target_id] as usize];
                effective_unit_def(t.kind, mask_of(t.owner)).armor_class
            } else {
                let t = &s.buildings[s.bidx[&target_id] as usize];
                effective_building_def(t.kind, mask_of(t.owner)).armor_class
            };
            let atk = Attacker {
                attack: Fx::from_num(def.attack),
                damage_type: def.damage_type,
                bonus_vs_armor: def.bonus_vs_armor,
            };
            let dmg = effective_damage(&atk, armor);
            if def.ranged {
                shots.0.push(Shot { from: a.pos, to: tpos });
            }
            if t_is_unit {
                let j = s.uidx[&target_id] as usize;
                let old = s.uhp[j];
                let new = (old - dmg).max(0);
                s.uhp[j] = new;
                if new <= 0 {
                    s.udead[j] = true;
                } else {
                    apply_dent(&mut s.out, &mut s.hit, &s.units, j, old, new, &mask_of);
                }
            } else {
                let j = s.bidx[&target_id] as usize;
                s.bhp[j] = (s.bhp[j] - dmg).max(0);
                if s.bhp[j] <= 0 {
                    s.bdead[j] = true;
                    if s.buildings[j].kind == BuildingKind::Keep {
                        defeated_owners.insert(s.buildings[j].owner);
                    }
                }
            }
            let killed = if t_is_unit {
                s.udead[s.uidx[&target_id] as usize]
            } else {
                s.bdead[s.bidx[&target_id] as usize]
            };
            let out = &mut s.out[i];
            out.attack_target = Some(if killed { 0 } else { target_id });
            out.cooldown = Some(def.attack_rate);
            out.clear_move = true;
        } else {
            // out of range — posture decides
            let act = combat_action(a.stance, false, dist(a.pos, a.home), DEFENSIVE_LEASH);
            let mv = match act {
                CombatAct::Approach if !a.has_target && pursuit_budget > 0 => {
                    pursuit_budget -= 1;
                    pursuit_patch(&mut path_scratch, seed, a.pos, tpos)
                }
                CombatAct::Return if !a.has_target && pursuit_budget > 0 => {
                    pursuit_budget -= 1;
                    pursuit_patch(&mut path_scratch, seed, a.pos, a.home)
                }
                _ => None,
            };
            let out = &mut s.out[i];
            out.cooldown = Some(cd);
            out.attack_target = Some(match act {
                CombatAct::Approach => target_id,
                _ => 0,
            });
            if mv.is_some() {
                out.mv = mv;
            }
        }
    }

    // ── structure fire: tower self-fire + garrisoned shooters ────────────────
    let mut bcd: Vec<(Entity, Fx)> = Vec::with_capacity(m);
    for (bi, b) in s.buildings.iter().enumerate() {
        if s.bdead[bi] || !statuses.simulates(b.mtch) {
            continue;
        }
        let bdef = effective_building_def(b.kind, mask_of(b.owner));
        let garr_fire = s.garr_profile.get(&b.id).map(|o| garrison_fire_power(o, &bdef)).unwrap_or(0);
        let fire_attack = bdef.attack + garr_fire;
        if fire_attack <= 0 {
            continue; // neither the host nor its garrison can fire
        }
        let gr = if bdef.attack <= 0 { s.garr_range_rate.get(&b.id).copied() } else { None };
        let fire_range = if bdef.range > Fx::ZERO { bdef.range } else { gr.map(|(r, _)| r).unwrap_or(Fx::ZERO) };
        let fire_rate = if bdef.attack_rate > Fx::ZERO { bdef.attack_rate } else { gr.map(|(_, r)| r).unwrap_or(Fx::ONE) };
        if fire_range <= Fx::ZERO {
            continue;
        }
        let cooldown = q_buildings.get(b.entity).map(|(_, _, _, _, _, bb)| bb.cooldown).unwrap_or(Fx::ZERO);
        let cd = (cooldown - DT).max(Fx::ZERO);

        // nearest enemy within best-case elevation reach (3-cell block)
        let reach = fire_range * (Fx::ONE + saladin_sim::ELEV_BONUS_MAX);
        let reach2 = reach * reach;
        let (best, _, best_pos) = {
            let units = &s.units;
            let udead = &s.udead;
            let max_r = (reach.to_num::<i32>() / saladin_sim::CELL_SIZE + 1).clamp(1, 3);
            nearest_in_rings(
                &s.grid,
                b.pos,
                max_r,
                reach2,
                |j| {
                    let e = &units[j as usize];
                    e.owner != b.owner && e.mtch == b.mtch && !e.garr && !udead[j as usize]
                },
                units,
            )
        };
        let tower_elev = elevation_at(seed, b.pos.x, b.pos.y);
        let in_elev = best != 0
            && dist(b.pos, best_pos)
                <= fire_range * elevation_range_bonus(tower_elev, elevation_at(seed, best_pos.x, best_pos.y));
        if best != 0 && in_elev && cd <= Fx::ZERO {
            let j = s.uidx[&best] as usize;
            let t = &s.units[j];
            let armor = effective_unit_def(t.kind, mask_of(t.owner)).armor_class;
            let atk = Attacker {
                attack: Fx::from_num(fire_attack),
                damage_type: bdef.damage_type,
                bonus_vs_armor: [Fx::ONE; 4],
            };
            let dmg = effective_damage(&atk, armor);
            shots.0.push(Shot { from: b.pos, to: best_pos });
            let old = s.uhp[j];
            let new = (old - dmg).max(0);
            s.uhp[j] = new;
            if new <= 0 {
                s.udead[j] = true;
            } else {
                apply_dent(&mut s.out, &mut s.hit, &s.units, j, old, new, &mask_of);
            }
            bcd.push((b.entity, fire_rate));
        } else {
            bcd.push((b.entity, cd));
        }
    }

    // ── dying hosts: evacuate or entomb their garrison ───────────────────────
    for (bi, b) in s.buildings.iter().enumerate() {
        if !s.bdead[bi] {
            continue;
        }
        let bdef = building_def(b.kind);
        for (ui, u) in s.units.iter().enumerate() {
            if u.garrisoned_in != b.id || s.udead[ui] {
                continue;
            }
            if bdef.garrison_survives_death {
                let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty);
                let exit = nearest_passable_grid(&passable, b.pos.x, b.pos.y);
                s.out[ui].eject_to = Some(exit);
            } else {
                s.udead[ui] = true;
            }
        }
    }

    // ── morale recovery (units not hit this tick) ────────────────────────────
    for i in 0..n {
        let a = &s.units[i];
        if a.garr || s.udead[i] || s.hit[i] || !statuses.simulates(a.mtch) {
            continue;
        }
        let def = unit_def(a.kind);
        if def.attack <= 0 {
            continue;
        }
        let routing_now = s.out[i].routing.unwrap_or(a.routing);
        if a.morale >= MORALE_MAX && !routing_now {
            continue;
        }
        // allies nearby + morale support, in one inline block scan
        let mut allies = 0i32;
        let mut support = false;
        {
            let units = &s.units;
            let udead = &s.udead;
            let r2 = ALLY_RADIUS * ALLY_RADIUS;
            // block sized to ALLY_RADIUS at the current cell granularity
            const ALLY_R: i32 = 5 / saladin_sim::CELL_SIZE + 1;
            for_near(&s.grid, a.pos, ALLY_R, |j| {
                let e = &units[j as usize];
                if j as usize == i || e.owner != a.owner || e.garr || udead[j as usize] {
                    return;
                }
                let d = dist2(a.pos, e.pos);
                if d <= r2 {
                    allies += 1;
                }
                let aura = unit_def(e.kind).morale_aura;
                if aura > Fx::ZERO && d <= aura * aura {
                    support = true;
                }
            });
        }
        if !support {
            for b in &s.buildings {
                if b.kind == BuildingKind::Keep
                    && b.owner == a.owner
                    && dist2(a.pos, b.pos) <= ALLY_RADIUS * ALLY_RADIUS
                {
                    support = true;
                    break;
                }
            }
        }
        let morale = morale_recover(a.morale, DT, allies, support);
        let out = &mut s.out[i];
        out.morale = Some(morale);
        out.routing = Some(is_routing(a.routing, morale));
    }

    // ── apply ────────────────────────────────────────────────────────────────
    for (i, snap) in s.units.iter().enumerate() {
        let Ok((ent, _g, mut p, _o, _m, mut u)) = q_units.get_mut(snap.entity) else { continue };
        if s.udead[i] {
            commands.entity(ent).despawn();
            continue;
        }
        u.hp = s.uhp[i];
        let o = &s.out[i];
        if let Some(exit) = o.eject_to {
            p.pos = exit;
            u.garrisoned_in = 0;
            u.has_target = false;
            u.path = vec![];
            u.path_idx = 0;
            u.attack_target = 0;
            u.home = exit;
        }
        if let Some(t) = o.attack_target {
            u.attack_target = t;
        }
        if let Some(cd) = o.cooldown {
            u.attack_cooldown = cd;
        }
        if let Some(mo) = o.morale {
            u.morale = mo;
        }
        if let Some(r) = o.routing {
            u.routing = r;
        }
        if let Some((path, target)) = &o.mv {
            u.path = path.clone();
            u.path_idx = 0;
            u.target = *target;
            u.has_target = true;
        } else if o.clear_move {
            u.has_target = false;
        }
    }
    for (i, snap) in s.buildings.iter().enumerate() {
        let Ok((ent, _g, _p, _o, _m, mut b)) = q_buildings.get_mut(snap.entity) else { continue };
        if s.bdead[i] {
            commands.entity(ent).despawn();
            continue;
        }
        b.hp = s.bhp[i];
    }
    for (ent, cd) in bcd {
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

fn apply_dent(
    out: &mut [UOut],
    hit: &mut [bool],
    units: &[USnap],
    j: usize,
    old: i32,
    new: i32,
    mask_of: &impl Fn(u64) -> u64,
) {
    let t = &units[j];
    let maxhp = effective_unit_def(t.kind, mask_of(t.owner)).max_hp;
    let frac = if maxhp > 0 { Fx::from_num(old - new) / Fx::from_num(maxhp) } else { Fx::ZERO };
    hit[j] = true;
    let o = &mut out[j];
    let base = o.morale.unwrap_or(t.morale);
    o.morale = Some(morale_after_hit(base, frac));
}
