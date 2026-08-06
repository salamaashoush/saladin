//! Soft unit separation: overlapping field units push each other apart a
//! little each pass, so gatherers crowding a node spread into a ring instead of
//! stacking into one sprite, and forty men in a melee occupy a frontage instead
//! of a dot. Deterministic: snapshots are id-sorted, pairs are visited in fixed
//! order, displacement math is pure fixed-point, and a push is dropped if it
//! would put a man on impassable ground OR inside a building.

use crate::components::{GameId, MatchId, Pos, Unit};
use crate::{MatchStatuses, Tick, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::{
    CELL_COUNT, CELL_SIZE, CELLS_PER_ROW, Domain, Fx, V2, WORLD_SIZE, cell_of, dist2,
    domain_passable, unit_def,
};

/// Push budget per pass — gentle nudges, not physics. A clump resolves over a
/// few passes without teleporting anyone out of harvest/fight range.
const MAX_PUSH: Fx = Fx::lit("0.18");
/// A man who is fighting still presses in, so his shove is a third of an idler's
/// — enough to stop forty bodies occupying 0.4 tiles, not enough to walk him out
/// of his own reach and start a push-versus-arrive oscillation.
const ENGAGED_PUSH: Fx = Fx::lit("0.15");
/// Widest pair of bodies in the roster (two Rams at 0.5). Nothing farther apart
/// than this can be overlapping, so the neighbour scan only ever visits the
/// cells this reaches: a 3x3 block of 4-tile cells is 144 tiles of candidates
/// to find neighbours inside ONE tile, and that ratio is what made separating
/// a packed melee unaffordable. A test pins it to the real radii.
pub(crate) const MAX_SEP: Fx = Fx::lit("1.0");
/// Fixed tie-break directions when two units share an exact position.
const DIRS8: [(Fx, Fx); 8] = [
    (Fx::lit("1"), Fx::lit("0")),
    (Fx::lit("0.7"), Fx::lit("0.7")),
    (Fx::lit("0"), Fx::lit("1")),
    (Fx::lit("-0.7"), Fx::lit("0.7")),
    (Fx::lit("-1"), Fx::lit("0")),
    (Fx::lit("-0.7"), Fx::lit("-0.7")),
    (Fx::lit("0"), Fx::lit("-1")),
    (Fx::lit("0.7"), Fx::lit("-0.7")),
];

struct Snap {
    id: u64,
    entity: Entity,
    pos: V2,
    radius: Fx,
    engaged: bool,
    dom: Domain,
}

#[derive(Resource, Default)]
pub struct SepScratch {
    grid: Vec<Vec<u32>>,
    snaps: Vec<Snap>,
    disp: Vec<V2>,
}

/// Base ticks between the passes that also spread the men who are FIGHTING.
/// Separation itself runs every 2 ticks; including a whole melee in every pass
/// doubled the pass and cost 22% of net_bench's 20k throughput, and a shove of
/// 0.06 delivered at 5 Hz opens a line just as well as one at 10 Hz.
const ENGAGED_EVERY: u64 = 4;

pub fn separation(
    cfg: Res<WorldConfig>,
    statuses: Res<MatchStatuses>,
    tick: Res<Tick>,
    ground: Res<super::combat::CombatScratch>,
    mut s: ResMut<SepScratch>,
    mut q: Query<(Entity, &GameId, &mut Pos, &MatchId, &Unit)>,
) {
    let seed = cfg.seed;
    let s = &mut *s;
    if s.grid.is_empty() {
        s.grid = vec![Vec::new(); CELL_COUNT as usize];
    }

    let spread_fighters = tick.0.is_multiple_of(ENGAGED_EVERY);
    s.snaps.clear();
    for (entity, g, pos, mid, u) in q.iter() {
        // ENGAGED means standing and trading blows. A man still WALKING at his
        // target is a pursuer, and pursuers get the full shove: reducing theirs
        // let a column press its own front rank down to 0.22 tiles.
        let engaged = u.attack_target != 0 && !u.has_target;
        if u.garrisoned_in != 0 || (engaged && !spread_fighters) || !statuses.simulates(mid.0) {
            continue;
        }
        let def = unit_def(u.kind);
        s.snaps.push(Snap {
            id: g.0,
            entity,
            pos: pos.pos,
            radius: def.radius,
            engaged,
            dom: def.domain,
        });
    }
    s.snaps.sort_unstable_by_key(|x| x.id);

    for bucket in s.grid.iter_mut() {
        bucket.clear();
    }
    for (i, sn) in s.snaps.iter().enumerate() {
        s.grid[cell_of(sn.pos.x, sn.pos.y) as usize].push(i as u32);
    }
    s.disp.clear();
    s.disp.resize(s.snaps.len(), V2::new(Fx::ZERO, Fx::ZERO));

    // pairwise, each pair once (i < j), over only the cells MAX_SEP reaches
    let cell_ix = |v: Fx| {
        let cs = Fx::from_num(CELL_SIZE);
        (v / cs).floor().to_num::<i32>().clamp(0, CELLS_PER_ROW - 1)
    };
    for i in 0..s.snaps.len() {
        let a = &s.snaps[i];
        let (x0, x1) = (cell_ix(a.pos.x - MAX_SEP), cell_ix(a.pos.x + MAX_SEP));
        let (y0, y1) = (cell_ix(a.pos.y - MAX_SEP), cell_ix(a.pos.y + MAX_SEP));
        for ny in y0..=y1 {
            for nx in x0..=x1 {
                for &j in &s.grid[(ny * CELLS_PER_ROW + nx) as usize] {
                    let j = j as usize;
                    if j <= i {
                        continue;
                    }
                    let b = &s.snaps[j];
                    // A hull and a man on the beach beside it are a tile apart
                    // and share no ground: shoving them off each other pushes
                    // the column into the water and the barge onto the shingle,
                    // and both landings are then refused, so the pair costs a
                    // shove nobody receives.
                    if a.dom != b.dom {
                        continue;
                    }
                    let min_sep = a.radius + b.radius;
                    let d2 = dist2(a.pos, b.pos);
                    if d2 >= min_sep * min_sep {
                        continue;
                    }
                    let (dirx, diry, d) = if d2 == Fx::ZERO {
                        // exact overlap: deterministic direction from the pair's ids
                        let (dx, dy) = DIRS8[((a.id ^ b.id) % 8) as usize];
                        (dx, dy, Fx::ZERO)
                    } else {
                        let d = saladin_sim::fx_sqrt(d2);
                        ((a.pos.x - b.pos.x) / d, (a.pos.y - b.pos.y) / d, d)
                    };
                    let cap_a = if a.engaged { ENGAGED_PUSH } else { MAX_PUSH };
                    let cap_b = if b.engaged { ENGAGED_PUSH } else { MAX_PUSH };
                    let half = (min_sep - d) / Fx::from_num(2);
                    s.disp[i].x += dirx * half.min(cap_a);
                    s.disp[i].y += diry * half.min(cap_a);
                    s.disp[j].x -= dirx * half.min(cap_b);
                    s.disp[j].y -= diry * half.min(cap_b);
                }
            }
        }
    }

    // apply, capped, clamped to the world, and never onto impassable ground or
    // into a standing building — nine of twenty-four measured peasants ended a
    // run inside their own keep before the second test existed. A push a wall
    // refuses SLIDES along it rather than being dropped: dropping it outright
    // jams a crowd against its own dropoff and cost seed 12345 a fifth of its
    // haul rate.
    let world_max = Fx::from_num(WORLD_SIZE);
    let free = |dom: Domain, x: Fx, y: Fx| {
        let (tx, ty) = (x.to_num::<i32>(), y.to_num::<i32>());
        domain_passable(seed, dom, tx, ty) && !ground.blocked_tile(tx, ty)
    };
    for (i, sn) in s.snaps.iter().enumerate() {
        let mut d = s.disp[i];
        if d.x == Fx::ZERO && d.y == Fx::ZERO {
            continue;
        }
        let cap = if sn.engaged { ENGAGED_PUSH } else { MAX_PUSH };
        d.x = d.x.clamp(-cap, cap);
        d.y = d.y.clamp(-cap, cap);
        let nx = (sn.pos.x + d.x).clamp(Fx::ZERO, world_max);
        let ny = (sn.pos.y + d.y).clamp(Fx::ZERO, world_max);
        let landed = if free(sn.dom, nx, ny) {
            Some(V2::new(nx, ny))
        } else if free(sn.dom, nx, sn.pos.y) {
            Some(V2::new(nx, sn.pos.y))
        } else if free(sn.dom, sn.pos.x, ny) {
            Some(V2::new(sn.pos.x, ny))
        } else {
            None
        };
        let Some(landed) = landed else { continue };
        if let Ok((_, _, mut pos, _, _)) = q.get_mut(sn.entity) {
            pos.pos = landed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saladin_sim::UnitKind;

    /// The neighbour scan only visits the cells `MAX_SEP` reaches, so a body
    /// wider than that would be pushed by men it never looked at.
    #[test]
    fn no_body_in_the_roster_is_wider_than_the_scan() {
        let widest = UnitKind::ALL.iter().map(|k| unit_def(*k).radius).fold(Fx::ZERO, Fx::max);
        assert!(widest * Fx::from_num(2) <= MAX_SEP, "the widest pair is {}", widest * Fx::from_num(2));
    }
}
