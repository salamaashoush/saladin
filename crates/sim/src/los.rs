use crate::math::{Fx, V2};

// Line of fire. Until this existed an archer shot a spearman THROUGH a wall
// segment from two tiles away, a mangonel was no different from a bow, and a
// tower was a wall that happened to have hit points.
//
// Integer DDA over the tile grid, bounded by the shooter's own range: at most
// `LOS_MAX_STEPS` tiles are ever touched, there is no allocation, no sqrt and
// no trig. Corner-safe, matching A*'s diagonal rule — a shot may not slip
// between two blocked tiles that meet at a corner, or arrows thread gaps a man
// cannot walk through.

/// The longest sight line anything in the game has, plus the diagonal slack.
/// A Mangonel reaches 10 tiles; nothing needs more than this.
pub const LOS_MAX_STEPS: i32 = 24;

/// Height a shooter needs above its target before it can see over one
/// obstruction — standing on the hill behind your own wall.
pub const PARAPET_ELEV: Fx = crate::fx!("0.06");

/// A garrisoned shooter is ON the parapet, so its own host never blocks it and
/// it looks over one course of wall.
pub const GARRISON_OVERLOOK: i32 = 1;

fn floor_i(v: Fx) -> i32 {
    v.floor().to_num::<i32>()
}

/// Does a shot from `from` to `to` reach? `overlook` blocked tiles may be
/// ignored (a parapet, a garrison slit); the shooter's own tile and the
/// target's tile never block.
pub fn clear_line<B: Fn(i32, i32) -> bool>(blocked: &B, from: V2, to: V2, overlook: i32) -> bool {
    let mut budget = overlook.max(0);
    let (mut cx, mut cy) = (floor_i(from.x), floor_i(from.y));
    let (ex, ey) = (floor_i(to.x), floor_i(to.y));
    if cx == ex && cy == ey {
        return true;
    }
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    // An axis whose delta is below EPS never crosses a grid line; this also
    // keeps the reciprocal from overflowing fixed-point, where a near-zero
    // divisor blows past Fx::MAX rather than merely going large.
    const EPS: Fx = crate::fx!("0.0001");
    let small_x = dx.abs() < EPS;
    let small_y = dy.abs() < EPS;
    if small_x && small_y {
        return true;
    }
    let step_x = if dx > Fx::ZERO { 1 } else { -1 };
    let step_y = if dy > Fx::ZERO { 1 } else { -1 };
    let inf = Fx::MAX;
    let t_delta_x = if small_x { inf } else { (Fx::ONE / dx).abs() };
    let t_delta_y = if small_y { inf } else { (Fx::ONE / dy).abs() };
    let mut t_max_x = if small_x {
        inf
    } else {
        let f = from.x.floor();
        (if dx > Fx::ZERO { f + Fx::ONE - from.x } else { from.x - f }) * t_delta_x
    };
    let mut t_max_y = if small_y {
        inf
    } else {
        let f = from.y.floor();
        (if dy > Fx::ZERO { f + Fx::ONE - from.y } else { from.y - f }) * t_delta_y
    };

    let max_steps = ((ex - cx).abs() + (ey - cy).abs() + 2).min(LOS_MAX_STEPS);
    let mut steps = 0;
    while cx != ex || cy != ey {
        if steps >= max_steps {
            return false;
        }
        steps += 1;
        if t_max_x < t_max_y {
            cx += step_x;
            t_max_x += t_delta_x;
        } else if t_max_y < t_max_x {
            cy += step_y;
            t_max_y += t_delta_y;
        } else {
            if blocked(cx + step_x, cy) && blocked(cx, cy + step_y) {
                if budget == 0 {
                    return false;
                }
                budget -= 1;
            }
            cx += step_x;
            cy += step_y;
            t_max_x += t_delta_x;
            t_max_y += t_delta_y;
        }
        if cx == ex && cy == ey {
            break;
        }
        if blocked(cx, cy) {
            if budget == 0 {
                return false;
            }
            budget -= 1;
        }
    }
    true
}

/// How many obstructions the ground itself lets a shooter see over.
pub fn parapet_overlook(shooter_elev: Fx, target_elev: Fx) -> i32 {
    if shooter_elev - target_elev >= PARAPET_ELEV { 1 } else { 0 }
}

/// Everything a shot needs to know about who is taking it.
#[derive(Clone, Copy, Debug)]
pub struct Sight {
    /// Artillery lobs over walls and heads alike — the one exemption.
    pub arcs: bool,
    /// Extra obstructions this shooter sees past (garrison slit, high ground).
    pub overlook: i32,
}

impl Sight {
    pub fn ground() -> Self {
        Sight { arcs: false, overlook: 0 }
    }
    pub fn arcing() -> Self {
        Sight { arcs: true, overlook: 0 }
    }
    pub fn from_parapet(shooter_elev: Fx, target_elev: Fx) -> Self {
        Sight { arcs: false, overlook: GARRISON_OVERLOOK + parapet_overlook(shooter_elev, target_elev) }
    }
}

/// The one call combat makes: can this shooter hit that spot?
pub fn has_line_of_fire<B: Fn(i32, i32) -> bool>(blocked: &B, from: V2, to: V2, sight: Sight) -> bool {
    sight.arcs || clear_line(blocked, from, to, sight.overlook)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: &str, y: &str) -> V2 {
        V2::new(Fx::lit(x), Fx::lit(y))
    }

    /// The measured hole: an archer and a spearman two tiles apart with ONE wall
    /// segment between them, and the archer shot through stone and killed.
    #[test]
    fn one_wall_segment_stops_the_arrow() {
        let wall = |x: i32, _y: i32| x == 5;
        let shooter = at("4.5", "10.5");
        let target = at("6.5", "10.5");
        assert!(!clear_line(&wall, shooter, target, 0));
        // the same shot from the parapet, or lobbed, gets there
        assert!(clear_line(&wall, shooter, target, 1));
        assert!(has_line_of_fire(&wall, shooter, target, Sight::arcing()));
        assert!(!has_line_of_fire(&wall, shooter, target, Sight::ground()));
    }

    #[test]
    fn open_ground_is_always_clear() {
        let open = |_: i32, _: i32| false;
        assert!(clear_line(&open, at("2.5", "2.5"), at("9.5", "6.5"), 0));
        assert!(clear_line(&open, at("9.5", "6.5"), at("2.5", "2.5"), 0));
        // straight along an axis, both directions
        assert!(clear_line(&open, at("2.5", "2.5"), at("2.5", "9.5"), 0));
        assert!(clear_line(&open, at("9.5", "2.5"), at("2.5", "2.5"), 0));
        // a target standing IN a blocked tile is still shootable
        let one = |x: i32, y: i32| x == 6 && y == 2;
        assert!(clear_line(&one, at("2.5", "2.5"), at("6.5", "2.5"), 0));
    }

    /// A*'s diagonal rule, applied to fire: an arrow must not thread the corner
    /// where two blocked tiles meet, or shots pass through a wall junction that
    /// no man can walk through.
    #[test]
    fn an_arrow_cannot_thread_a_wall_corner() {
        let corner = |x: i32, y: i32| (x == 5 && y == 4) || (x == 4 && y == 5);
        assert!(!clear_line(&corner, at("4.5", "4.5"), at("5.5", "5.5"), 0));
        // the other diagonal through the same pair is open ground
        assert!(clear_line(&corner, at("5.5", "4.5"), at("4.5", "5.5"), 0));
    }

    #[test]
    fn a_shooter_on_the_high_ground_sees_over_one_course() {
        let low = crate::fx!("0.30");
        let high = crate::fx!("0.42");
        assert_eq!(parapet_overlook(high, low), 1);
        assert_eq!(parapet_overlook(low, high), 0);
        assert_eq!(parapet_overlook(low, low), 0);
        let sight = Sight::from_parapet(high, low);
        assert_eq!(sight.overlook, GARRISON_OVERLOOK + 1);
        // two courses of wall still stop it
        let two_courses = |x: i32, _y: i32| x == 5 || x == 7;
        assert!(clear_line(&two_courses, at("4.5", "3.5"), at("8.5", "3.5"), 2));
        assert!(!clear_line(&two_courses, at("4.5", "3.5"), at("8.5", "3.5"), 1));
    }

    /// Bounded by construction: a sight line longer than anything in the game
    /// gives up rather than walking the map.
    #[test]
    fn the_walk_is_bounded() {
        let open = |_: i32, _: i32| false;
        assert!(!clear_line(&open, at("2.5", "2.5"), at("120.5", "2.5"), 0));
        assert!(clear_line(&open, at("2.5", "2.5"), at("12.5", "2.5"), 0));
    }
}
