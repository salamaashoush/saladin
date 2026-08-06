use crate::{GameIndex, MatchStatuses, WorldConfig};
use crate::components::{GameId, MatchId, ORDER_NONE, ORDER_STOP, Pos, Unit};
use bevy_ecs::prelude::*;
use saladin_sim::{ARRIVE_EPS, Domain, Fx, MOVE_DT, V2, is_sailable, step_toward, unit_def};

/// tan(11.25 deg) and tan(33.75 deg) — the two sector walls inside one octant.
const T1: Fx = saladin_sim::fx!("0.19891237");
const T2: Fx = saladin_sim::fx!("0.66817864");

/// Nearest of sixteen compass points to `d`, counter-clockwise from +X, by
/// comparison alone. `saladin_sim::heading_of` answers the same question with
/// sixteen dot products, which is affordable once per group order and NOT
/// affordable for every moving unit on every 50 ms tick — this is four compares
/// and two multiplies. A test pins the two together over a dense sweep.
#[inline]
pub(crate) fn heading16(d: V2) -> u8 {
    let (ax, ay) = (d.x.abs(), d.y.abs());
    let sub = if ay <= T1 * ax {
        0u8
    } else if ay <= T2 * ax {
        1
    } else if ay * T2 <= ax {
        2
    } else if ay * T1 <= ax {
        3
    } else {
        4
    };
    match (d.x >= Fx::ZERO, d.y >= Fx::ZERO) {
        (true, true) => sub,
        (false, true) => 8 - sub,
        (false, false) => 8 + sub,
        (true, false) => (16 - sub) % 16,
    }
}

/// The step a hull may actually take. The whole step if it lands on water, then
/// one axis at a time, then nowhere — the same three-way landing `separation`
/// uses, and for the same reason: refusing outright would wedge a boat against
/// its own coastline.
fn afloat(seed: u32, from: V2, to: V2) -> V2 {
    let wet = |p: V2| is_sailable(seed, p.x.to_num::<i32>(), p.y.to_num::<i32>());
    if wet(to) {
        return to;
    }
    let x_only = V2::new(to.x, from.y);
    if wet(x_only) {
        return x_only;
    }
    let y_only = V2::new(from.x, to.y);
    if wet(y_only) { y_only } else { from }
}

/// Integrate every active mover one base tick toward its target, advancing along
/// its path on arrival, and keep its `heading` — which is what decides whether
/// the next blow it takes lands on its face or its back. Garrisoned units are
/// off the field; paused matches freeze in place. A Stop order is a real order:
/// before it existed the only way to halt a unit was to move it onto its own
/// feet.
///
/// Then cargo rides with its hull. A host that MOVES is new — a tower does not —
/// and a passenger whose `Pos` froze at the beach it boarded from would have its
/// supply band, its foraging draw and its desertion roll computed at a place it
/// left minutes ago. Making the position TRUTHFUL is what gets every reader of
/// one right at once, including the state hash that already digests it.
///
/// The riders are collected in the loop that already skips them, and their hosts
/// are resolved through `GameIndex` — a second full pass over every unit on the
/// map, every tick, to find the handful that are aboard something is not
/// affordable at twenty thousand.
pub fn movement(
    statuses: Res<MatchStatuses>,
    cfg: Res<WorldConfig>,
    index: Res<GameIndex>,
    mut riders: Local<Vec<(Entity, u64)>>,
    mut hulls: Local<Vec<(u64, V2)>>,
    mut q: Query<(Entity, &GameId, &mut Pos, &mut Unit, &MatchId)>,
) {
    let seed = cfg.seed;
    riders.clear();
    for (e, _, mut pos, mut u, mid) in &mut q {
        if !statuses.simulates(mid.0) {
            continue;
        }
        if u.garrisoned_in != 0 {
            riders.push((e, u.garrisoned_in));
            continue;
        }
        // The halt is spent once it has been served. Leaving it standing vetoes
        // every later `has_target` write in the tree — a stopped man could
        // never pursue, never build and never gather again, which is HoldGround
        // applied to the whole game rather than a Stop order.
        if u.order == ORDER_STOP {
            if u.has_target {
                u.has_target = false;
                u.path.clear();
                u.path_idx = 0;
            }
            u.order = ORDER_NONE;
            continue;
        }
        if !u.has_target {
            // the march is over, so the column pace is over with it — compared
            // before writing so an idle army does not dirty its `Unit` rows
            // every tick for nothing
            let base = unit_def(u.kind).speed;
            if u.speed != base {
                u.speed = base;
            }
            continue;
        }
        let step = u.speed * MOVE_DT;
        let from = pos.pos;
        let r = step_toward(pos.pos, u.target, step, ARRIVE_EPS);
        // A hull is the one mover whose ground is VISIBLE, and the only one whose
        // path being right is not enough. Every sea leg is cleared with the exact
        // DDA when it is laid, but `step_toward` is fixed point and `separation`
        // nudges: a leg that runs along a tile boundary crosses it on a hair of
        // drift, and the boat then stands on a headland with a perfectly legal
        // path still in hand. Slide along whichever axis still floats, exactly as
        // `separation` lands a push. Land is untouched — one enum compare — both
        // because a man a hair inside a wall is invisible and because every land
        // path in the game is measured against the behaviour it has now.
        pos.pos = match unit_def(u.kind).domain {
            Domain::Land => r.pos,
            Domain::Sea => afloat(seed, from, r.pos),
        };
        let d = V2::new(pos.pos.x - from.x, pos.pos.y - from.y);
        if d.x != Fx::ZERO || d.y != Fx::ZERO {
            u.heading = heading16(d);
        }
        // Arrival is the step's own verdict: a slid hull is still walking its
        // leg, and promoting it to the next waypoint would leave it a boat-width
        // short of every corner it ever turns.
        if !r.arrived || pos.pos != r.pos {
            continue;
        }
        let next = u.path_idx + 1;
        if next < u.path.len() {
            u.path_idx = next;
            u.target = u.path[next];
        } else {
            u.has_target = false;
        }
    }

    if riders.is_empty() {
        return;
    }
    hulls.clear();
    let mut unindexed = false;
    for (_, host) in riders.iter() {
        if hulls.iter().any(|(h, _)| h == host) {
            continue;
        }
        // A hall is a host too and it does not move. Only a hull needs this, and
        // a building has no `Unit` row, so the query filters it out for free —
        // which is also why a resolved-but-unmatched host must NOT fall through
        // to the scan below, or every tower garrison on the map would pay for it.
        match index.get(*host) {
            Some(he) => {
                if let Ok((_, _, hp, _, _)) = q.get(he) {
                    hulls.push((*host, hp.pos));
                }
            }
            None => unindexed = true,
        }
    }
    // The index is rebuilt on a four-tick cadence, so a hull spawned or restored
    // moments ago is not in it yet and its passengers would ride at the beach
    // they boarded from until it is.
    if unindexed {
        for (_, g, p, u, _) in q.iter() {
            if unit_def(u.kind).cargo_cap > 0
                && riders.iter().any(|(_, h)| *h == g.0)
                && !hulls.iter().any(|(h, _)| *h == g.0)
            {
                hulls.push((g.0, p.pos));
            }
        }
    }
    for (e, host) in riders.iter() {
        let Some((_, at)) = hulls.iter().find(|(h, _)| h == host).copied() else { continue };
        if let Ok((_, _, mut p, _, _)) = q.get_mut(*e)
            && p.pos != at
        {
            p.pos = at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saladin_sim::{HEADING_DIRS, heading_of};

    /// The cheap compass has to be the SAME compass the sim's flank maths uses,
    /// or a blow could land on the flank by one function and the face by the
    /// other.
    #[test]
    fn the_cheap_compass_agrees_with_the_sim_one() {
        for (i, d) in HEADING_DIRS.iter().enumerate() {
            assert_eq!(heading16(*d), i as u8, "canonical direction {i}");
        }
        let mut checked = 0;
        for dx in -60i32..=60 {
            for dy in -60i32..=60 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let v = V2::new(Fx::from_num(dx), Fx::from_num(dy));
                assert_eq!(
                    heading16(v),
                    heading_of(v),
                    "({dx},{dy}) split the two compasses"
                );
                checked += 1;
            }
        }
        assert!(checked > 14_000);
    }

    #[test]
    fn the_cardinals_are_where_they_should_be() {
        let at = |x: &str, y: &str| V2::new(Fx::lit(x), Fx::lit(y));
        assert_eq!(heading16(at("1", "0")), 0);
        assert_eq!(heading16(at("0", "1")), 4);
        assert_eq!(heading16(at("-1", "0")), 8);
        assert_eq!(heading16(at("0", "-1")), 12);
        assert_eq!(heading16(at("1", "1")), 2);
        assert_eq!(heading16(at("-1", "-1")), 10);
    }
}
