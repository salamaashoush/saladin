use crate::math::Fx;

/// Morale is a 0..1 scalar per combat unit: it sinks on damage and recovers when
/// not hit, faster among allies and near a keep/Imam aura. Below ROUT a fresh
/// unit breaks; a routing unit only rallies once above the higher RALLY (the gap
/// is hysteresis to stop boundary flicker). Deterministic.
pub const MORALE_MAX: Fx = crate::fx!("1");
pub const MORALE_MIN: Fx = crate::fx!("0");
pub const ROUT_THRESHOLD: Fx = crate::fx!("0.25");
pub const RALLY_THRESHOLD: Fx = crate::fx!("0.5");
pub const MORALE_HIT_WEIGHT: Fx = crate::fx!("1.5");
pub const MORALE_RECOVER_BASE: Fx = crate::fx!("0.05");
pub const MORALE_RECOVER_PER_ALLY: Fx = crate::fx!("0.02");
pub const MORALE_ALLY_CAP: i32 = 6;
pub const MORALE_RECOVER_SUPPORT: Fx = crate::fx!("0.12");

fn clamp(v: Fx) -> Fx {
    v.clamp(MORALE_MIN, MORALE_MAX)
}

/// New morale after a hit that removed `dmg_frac` (0..1) of max hp.
pub fn morale_after_hit(morale: Fx, dmg_frac: Fx) -> Fx {
    let drop = dmg_frac.max(Fx::ZERO) * MORALE_HIT_WEIGHT;
    clamp(morale - drop)
}

/// New morale after `dt` seconds of not being hit.
pub fn morale_recover(morale: Fx, dt: Fx, near_allies: i32, near_keep_or_imam: bool) -> Fx {
    let allies = near_allies.clamp(0, MORALE_ALLY_CAP);
    let support = if near_keep_or_imam { MORALE_RECOVER_SUPPORT } else { Fx::ZERO };
    let rate = MORALE_RECOVER_BASE + Fx::from_num(allies) * MORALE_RECOVER_PER_ALLY + support;
    clamp(morale + rate * dt.max(Fx::ZERO))
}

pub fn should_rout(morale: Fx) -> bool {
    morale < ROUT_THRESHOLD
}

pub fn has_rallied(morale: Fx) -> bool {
    morale > RALLY_THRESHOLD
}

/// Resolve routing with hysteresis: a routing unit keeps routing until it
/// rallies; a steady unit breaks only once it drops below ROUT.
pub fn is_routing(was_routing: bool, morale: Fx) -> bool {
    if was_routing { !has_rallied(morale) } else { should_rout(morale) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_lowers_recover_raises() {
        // non-dyadic constants (0.2/0.05) round at the last bit — compare with
        // a tolerance, not bit-equality.
        let eps = crate::fx!("0.0001");
        let m = morale_after_hit(crate::fx!("1"), crate::fx!("0.2")); // -0.3
        assert!((m - crate::fx!("0.7")).abs() < eps);
        let r = morale_recover(crate::fx!("0.5"), crate::fx!("1"), 0, false); // +0.05
        assert!((r - crate::fx!("0.55")).abs() < eps);
    }

    #[test]
    fn hysteresis() {
        // dropping below ROUT breaks a steady unit
        assert!(is_routing(false, crate::fx!("0.2")));
        // a routing unit at 0.3 (below RALLY) keeps routing
        assert!(is_routing(true, crate::fx!("0.3")));
        // a routing unit above RALLY rallies
        assert!(!is_routing(true, crate::fx!("0.6")));
    }
}
