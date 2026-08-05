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

/// New morale after a hit, scaled by the unit's own RESOLVE. Without this every
/// kind in the game broke at exactly the same ~50% of health lost, regardless of
/// what it was, how many friends it had, or who was leading it — the flat
/// `1.5 * dmg_frac` made morale a second health bar rather than a stat.
pub fn morale_after_hit_resolve(morale: Fx, dmg_frac: Fx, resolve: Fx) -> Fx {
    let r = resolve.max(crate::fx!("0.1"));
    clamp(morale - dmg_frac.max(Fx::ZERO) * MORALE_HIT_WEIGHT / r)
}

/// Fraction of its own health a unit can lose before it breaks.
pub fn breaking_damage(resolve: Fx) -> Fx {
    let r = resolve.max(crate::fx!("0.1"));
    (MORALE_MAX - ROUT_THRESHOLD) * r / MORALE_HIT_WEIGHT
}

/// How hard a shell landing on a garrisoned structure shakes the men inside it.
/// A keep barely notices; a tower coming down around them does not need to kill
/// anyone to empty it.
pub const BOMBARD_MORALE_WEIGHT: Fx = crate::fx!("0.8");

pub fn bombard_morale(dmg: i32, host_max_hp: i32) -> Fx {
    if host_max_hp <= 0 || dmg <= 0 {
        return Fx::ZERO;
    }
    (Fx::from_num(dmg) / Fx::from_num(host_max_hp) * BOMBARD_MORALE_WEIGHT).min(MORALE_MAX)
}

/// A Chaplain's verb: men in his radius are steadier and rally sooner. This is
/// deliberately a DIFFERENT mechanic from the Imam's wide passive recovery, not
/// a smaller radius of the same one.
pub const DISCIPLINE_BONUS: Fx = crate::fx!("0.35");

pub fn disciplined_resolve(resolve: Fx, in_rally_aura: bool) -> Fx {
    if in_rally_aura { resolve + DISCIPLINE_BONUS } else { resolve }
}

pub fn rally_cooldown(base: i32, in_rally_aura: bool) -> i32 {
    if in_rally_aura { (base + 1) / 2 } else { base }
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

    /// Every kind broke at the same half-health before resolve existed.
    #[test]
    fn discipline_decides_when_a_line_breaks() {
        use crate::enums::UnitKind;
        use crate::units::unit_def;
        let levy = unit_def(UnitKind::Spearman).morale_resolve;
        let professional = unit_def(UnitKind::Sergeant).morale_resolve;
        let raw = unit_def(UnitKind::Naffatun).morale_resolve;
        assert!(breaking_damage(professional) > breaking_damage(levy));
        assert!(breaking_damage(levy) > breaking_damage(raw));
        // a Crusader line does not break at 50%
        assert!(breaking_damage(professional) > crate::fx!("0.6"));
        // half health lost, and only the raw troops are broken
        let half = crate::fx!("0.5");
        assert!(should_rout(morale_after_hit_resolve(MORALE_MAX, half, raw)));
        assert!(!should_rout(morale_after_hit_resolve(MORALE_MAX, half, professional)));
        // a chaplain's ground is steadier still
        assert!(
            breaking_damage(disciplined_resolve(levy, true)) > breaking_damage(levy),
            "the chaplain steadied nobody"
        );
        assert_eq!(rally_cooldown(9, true), 5);
        assert_eq!(rally_cooldown(9, false), 9);
    }

    /// Siege on a garrisoned structure has to bleed onto the men inside, or a
    /// mangonel can never do the one thing a ram cannot.
    #[test]
    fn a_shell_on_the_parapet_shakes_the_men_under_it() {
        use crate::buildings_defs::building_def;
        use crate::enums::BuildingKind;
        let keep = building_def(BuildingKind::Keep).max_hp;
        let tower = building_def(BuildingKind::Tower).max_hp;
        let shell = 75;
        assert!(bombard_morale(shell, tower) > bombard_morale(shell, keep) * crate::fx!("2"));
        assert!(bombard_morale(shell, keep) > Fx::ZERO);
        assert_eq!(bombard_morale(0, tower), Fx::ZERO);
        assert_eq!(bombard_morale(10, 0), Fx::ZERO);
        assert!(bombard_morale(9_000_000, tower) <= MORALE_MAX);
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
