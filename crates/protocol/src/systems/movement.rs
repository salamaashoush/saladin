use crate::MatchStatuses;
use crate::components::{MatchId, ORDER_NONE, ORDER_STOP, Pos, Unit};
use bevy_ecs::prelude::*;
use saladin_sim::{ARRIVE_EPS, Fx, MOVE_DT, V2, step_toward, unit_def};

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

/// Integrate every active mover one base tick toward its target, advancing along
/// its path on arrival, and keep its `heading` — which is what decides whether
/// the next blow it takes lands on its face or its back. Garrisoned units are
/// off the field; paused matches freeze in place. A Stop order is a real order:
/// before it existed the only way to halt a unit was to move it onto its own
/// feet.
pub fn movement(statuses: Res<MatchStatuses>, mut q: Query<(&mut Pos, &mut Unit, &MatchId)>) {
    for (mut pos, mut u, mid) in &mut q {
        if u.garrisoned_in != 0 || !statuses.simulates(mid.0) {
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
        pos.pos = r.pos;
        let d = V2::new(r.pos.x - from.x, r.pos.y - from.y);
        if d.x != Fx::ZERO || d.y != Fx::ZERO {
            u.heading = heading16(d);
        }
        if !r.arrived {
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
