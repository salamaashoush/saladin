//! Soft unit separation: overlapping field units push each other apart a
//! little each pass, so gatherers crowding a node or a dropoff spread into a
//! ring instead of stacking into one sprite. Deterministic: snapshots are
//! id-sorted, pairs are visited in fixed order, displacement math is pure
//! fixed-point, and pushes onto impassable tiles are dropped.

use crate::components::{GameId, MatchId, Pos, Unit};
use crate::{MatchStatuses, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::{CELL_COUNT, CELLS_PER_ROW, Fx, V2, WORLD_SIZE, cell_of, dist2, is_passable, unit_def};

/// Push budget per pass — gentle nudges, not physics. A clump resolves over a
/// few passes without teleporting anyone out of harvest/fight range.
const MAX_PUSH: Fx = Fx::lit("0.18");
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
}

#[derive(Resource, Default)]
pub struct SepScratch {
    grid: Vec<Vec<u32>>,
    snaps: Vec<Snap>,
    disp: Vec<V2>,
}

pub fn separation(
    cfg: Res<WorldConfig>,
    statuses: Res<MatchStatuses>,
    mut s: ResMut<SepScratch>,
    mut q: Query<(Entity, &GameId, &mut Pos, &MatchId, &Unit)>,
) {
    let seed = cfg.seed;
    let s = &mut *s;
    if s.grid.is_empty() {
        s.grid = vec![Vec::new(); CELL_COUNT as usize];
    }

    s.snaps.clear();
    for (entity, g, pos, mid, u) in q.iter() {
        // fighters press into melee on purpose — only off-combat units (the
        // gatherers/idlers that visibly stack) get spread apart
        if u.garrisoned_in != 0 || u.attack_target != 0 || !statuses.simulates(mid.0) {
            continue;
        }
        s.snaps.push(Snap { id: g.0, entity, pos: pos.pos, radius: unit_def(u.kind).radius });
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

    // pairwise within the 3×3 cell block, each pair once (i < j)
    for i in 0..s.snaps.len() {
        let a = &s.snaps[i];
        let cell = cell_of(a.pos.x, a.pos.y);
        let (cx, cy) = (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW);
        for dy in -1i32..=1 {
            let ny = cy + dy;
            if ny < 0 || ny >= CELLS_PER_ROW {
                continue;
            }
            for dx in -1i32..=1 {
                let nx = cx + dx;
                if nx < 0 || nx >= CELLS_PER_ROW {
                    continue;
                }
                for &j in &s.grid[(ny * CELLS_PER_ROW + nx) as usize] {
                    let j = j as usize;
                    if j <= i {
                        continue;
                    }
                    let b = &s.snaps[j];
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
                    let push = ((min_sep - d) / Fx::from_num(2)).min(MAX_PUSH);
                    s.disp[i].x += dirx * push;
                    s.disp[i].y += diry * push;
                    s.disp[j].x -= dirx * push;
                    s.disp[j].y -= diry * push;
                }
            }
        }
    }

    // apply, capped, clamped to the world, and never onto an impassable tile
    let cap = MAX_PUSH;
    let world_max = Fx::from_num(WORLD_SIZE);
    for (i, sn) in s.snaps.iter().enumerate() {
        let mut d = s.disp[i];
        if d.x == Fx::ZERO && d.y == Fx::ZERO {
            continue;
        }
        d.x = d.x.clamp(-cap, cap);
        d.y = d.y.clamp(-cap, cap);
        let nx = (sn.pos.x + d.x).clamp(Fx::ZERO, world_max);
        let ny = (sn.pos.y + d.y).clamp(Fx::ZERO, world_max);
        if !is_passable(seed, nx.to_num::<i32>(), ny.to_num::<i32>()) {
            continue;
        }
        if let Ok((_, _, mut pos, _, _)) = q.get_mut(sn.entity) {
            pos.pos = V2::new(nx, ny);
        }
    }
}
