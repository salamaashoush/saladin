//! The season a sown field runs through, as pure fixed-point math.
//!
//! A rock's output is a function of how many hands you put on it. A field's is a
//! function of TIME AND CARE: the soil decides how big the harvest is, the crew
//! decides how fast it comes in, and hands only set the pace and carry the
//! result home. Two axes, one player input each, both already computed by the
//! worldgen and both free.

use crate::buildings::work_step;
use crate::constants::{
    ECONOMY_DT, FARM_CAP_MAX, FARM_CAP_MIN, FARM_LODGE_DIVISOR, FARM_MIN_FERTILITY,
    FARM_REGEN_IDLE, FARM_SOIL_RICH, FARM_TEND_TIME,
};
use crate::math::Fx;

/// How much standing crop ground of this fertility carries at harvest.
///
/// ROUNDED, never truncated: truncation is exactly what collapsed the old model
/// into two distinct farms across every soil the gate lets through.
pub fn field_cap(soil: Fx) -> i32 {
    let span = FARM_SOIL_RICH - FARM_MIN_FERTILITY;
    let t = if span <= Fx::ZERO {
        Fx::ONE
    } else {
        ((soil - FARM_MIN_FERTILITY) / span).clamp(Fx::ZERO, Fx::ONE)
    };
    FARM_CAP_MIN + (t * Fx::from_num(FARM_CAP_MAX - FARM_CAP_MIN)).round().to_num::<i32>()
}

/// Standing crop a field gains in one economy tick: the rain-fed creep every
/// field gets for free, plus what `hands` tending it bring in, plus a farm hub's
/// aura. Hands ride `BUILDER_RATE`, so three hands on three fields beat three on
/// one and the allocation is a real decision.
pub fn field_growth(hands: i32, cap: i32, aura_regen: i32) -> i32 {
    let labour = (Fx::from_num(cap.max(0)) * work_step(hands, ECONOMY_DT, FARM_TEND_TIME))
        .floor()
        .to_num::<i32>();
    (FARM_REGEN_IDLE + labour + aura_regen.max(0)).max(1)
}

/// Whether a ripe field still carries a harvest worth PLANNING around.
///
/// `Crop.ripe` latches, and it has to: a reaper drawing the field down must not
/// un-ripen it at the first sheaf. But that means it stays true all the way to
/// an empty plot, so a field with two sheaves left reads as "the harvest is in"
/// — and it reads that way for a long time, because a ripe crop stops growing
/// and then holds its value for the whole `FARM_RIPE_GRACE` before it starts to
/// bleed. A planner that answers a famine with "there is a harvest standing"
/// would sit through that window refusing to buy grain or sow again.
///
/// This is a decision threshold, NOT a harvest gate: a stripped field is still
/// perfectly legal to cut (see `reapable`), it is just no longer an answer to a
/// famine.
pub fn harvest_standing(remaining: i32, cap: i32) -> bool {
    remaining * 2 >= cap.max(1)
}

/// Standing crop a lodged field loses per economy tick. A ripe crop nobody cuts
/// falls over; it is never deleted, and it can be salvaged at any point.
///
/// ROUNDED UP, for the same reason `field_cap` rounds. Truncating made the slide
/// a step function of the yield with its worst step where real farms land: a
/// 99-crop field bled over 198 s and a 100-crop field over 100 s.
pub fn lodge_loss(cap: i32) -> i32 {
    ((cap.max(0) + FARM_LODGE_DIVISOR - 1) / FARM_LODGE_DIVISOR).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{FARM_RIPE_GRACE, FARM_SOW_DIVISOR};

    /// The commonest gate-clearing soil, and the reference every tuning number
    /// in the plan is quoted against.
    const MEDIAN_SOIL: Fx = crate::fx!("0.32");

    #[test]
    fn richer_soil_carries_a_bigger_harvest() {
        assert_eq!(field_cap(Fx::ZERO), FARM_CAP_MIN, "barren ground still clamps to the floor");
        assert_eq!(field_cap(FARM_MIN_FERTILITY), FARM_CAP_MIN);
        assert_eq!(field_cap(FARM_SOIL_RICH), FARM_CAP_MAX);
        assert_eq!(field_cap(crate::fx!("0.9")), FARM_CAP_MAX, "past rich is not richer");
        let mut last = 0;
        for n in 22..=60 {
            let cap = field_cap(Fx::from_num(n) / Fx::from_num(100));
            assert!(cap >= last, "soil {n}/100 carried less than poorer ground");
            last = cap;
        }
    }

    /// The anti-truncation assertion the old `1 + soil * FARM_REGEN_MAX` model
    /// would have failed: it landed on 2 or 3 over this whole measured range.
    #[test]
    fn soil_is_not_two_farms_wearing_a_hat() {
        let mut seen: Vec<i32> = Vec::new();
        // the measured fertility range of gate-clearing land, 0.22 .. 0.74
        for n in 22..=74 {
            let cap = field_cap(Fx::from_num(n) / Fx::from_num(100));
            if !seen.contains(&cap) {
                seen.push(cap);
            }
        }
        assert!(seen.len() >= 6, "soil buys only {} distinct farms: {seen:?}", seen.len());
    }

    #[test]
    fn a_field_nobody_works_still_creeps_in() {
        for cap in [FARM_CAP_MIN, field_cap(MEDIAN_SOIL), FARM_CAP_MAX] {
            assert_eq!(field_growth(0, cap, 0), FARM_REGEN_IDLE);
            assert!(field_growth(0, cap, 0) > 0, "a field that cannot grow is a dead field");
        }
        assert!(field_growth(0, 0, 0) > 0, "growth is never zero, whatever the ground");
    }

    #[test]
    fn hands_bring_the_season_in_faster() {
        for cap in [FARM_CAP_MIN, field_cap(MEDIAN_SOIL), FARM_CAP_MAX] {
            // the band the decision actually lives in
            for h in 0..3 {
                assert!(
                    field_growth(h + 1, cap, 0) > field_growth(h, cap, 0),
                    "hand {} added nothing on cap {cap}",
                    h + 1
                );
            }
            // and past it, more hands never make a field grow SLOWER
            for h in 0..crate::constants::MAX_BUILDERS + 2 {
                assert!(field_growth(h + 1, cap, 0) >= field_growth(h, cap, 0));
            }
        }
    }

    /// `BUILDER_RATE` is why the allocation is a decision: spreading hands over
    /// fields beats stacking them on one.
    #[test]
    fn three_hands_on_one_field_are_worth_less_than_three_fields() {
        let cap = field_cap(MEDIAN_SOIL);
        let one = field_growth(1, cap, 0) - FARM_REGEN_IDLE;
        let three = field_growth(3, cap, 0) - FARM_REGEN_IDLE;
        assert!(three < one * 3, "the tending curve does not diminish ({three} vs {})", one * 3);
        assert!(three > one, "a third hand is worth nothing at all");
    }

    /// `ripe` latches down to an empty plot, and a ripe crop stops growing, so a
    /// stripped field reads as "the harvest is in" for the whole grace AND the
    /// whole bleed after it. Measured on seed 777, a starving bot spent 47 s of
    /// its famine holding that reading; on seed 101 its fields lodged while it
    /// sat on 388 wood and 140 gold it would not spend.
    #[test]
    fn a_field_reaped_down_to_stubble_is_not_a_standing_harvest() {
        for soil in [FARM_MIN_FERTILITY, MEDIAN_SOIL, FARM_SOIL_RICH] {
            let cap = field_cap(soil);
            assert!(harvest_standing(cap, cap), "a full field is a harvest on any soil");
            assert!(!harvest_standing(0, cap), "an empty plot is not a harvest");
            assert!(!harvest_standing(1, cap), "one sheaf is not a harvest");
            // the line itself, from both sides
            assert!(harvest_standing((cap + 1) / 2, cap));
            assert!(!harvest_standing(cap / 2 - 1, cap));
            // and it is monotone, so a crew cutting a field never re-crosses it
            let mut seen_gone = false;
            for rem in (0..=cap).rev() {
                let standing = harvest_standing(rem, cap);
                if !standing {
                    seen_gone = true;
                } else {
                    assert!(!seen_gone, "cap {cap} rem {rem}: the reading came BACK");
                }
            }
        }
        // richer ground has to carry more before it counts, or the threshold
        // would just be a constant wearing a fraction's clothes
        let thin = FARM_CAP_MIN;
        let rich = FARM_CAP_MAX;
        let half_thin = (thin + 1) / 2;
        assert!(harvest_standing(half_thin, thin));
        assert!(!harvest_standing(half_thin, rich), "the bar must scale with the soil");
        // a field with no yield at all must never divide by zero or read true
        assert!(!harvest_standing(0, 0));
    }

    #[test]
    fn a_hub_adds_its_own_hands() {
        let cap = field_cap(MEDIAN_SOIL);
        assert_eq!(field_growth(1, cap, 3), field_growth(1, cap, 0) + 3);
        assert_eq!(field_growth(1, cap, -5), field_growth(1, cap, 0), "an aura never takes");
    }

    /// The whole point of the two axes: the median field, one hand, is the
    /// number every other target in the plan is quoted against.
    #[test]
    fn the_measured_season_holds() {
        let cap = field_cap(MEDIAN_SOIL);
        assert_eq!(cap, 102, "the median field moved");
        assert_eq!(field_growth(1, cap, 0), 4, "one hand on the median field");
        assert_eq!(field_growth(1, FARM_CAP_MAX, 0), 7, "one hand on the richest field");
        assert_eq!(field_growth(1, FARM_CAP_MIN, 0), 3, "one hand on the thinnest field");
        // sown at a quarter, one hand: a season inside 40 s
        let step = field_growth(1, cap, 0);
        let ticks = (cap - cap / FARM_SOW_DIVISOR + step - 1) / step;
        assert!((15..=22).contains(&ticks), "the first season takes {ticks} economy ticks");
    }

    /// Swept over EVERY yield a field can have, not sampled at three: an integer
    /// loss per tick is a step function of the cap, and the old truncating
    /// version put its worst step (198 s against 100 s) right where real farms
    /// land — soil 0.311 and soil 0.315, indistinguishable ground.
    #[test]
    fn a_standing_crop_falls_over_slowly_enough_to_be_saved() {
        let (mut fastest, mut slowest) = (i32::MAX, 0);
        for cap in FARM_CAP_MIN..=FARM_CAP_MAX {
            let loss = lodge_loss(cap);
            assert!(loss >= 1, "a lodged field must actually lose crop");
            let ticks = (cap + loss - 1) / loss;
            assert!(
                (30..=50).contains(&ticks),
                "cap {cap} bleeds out in {ticks} economy ticks - too {} to price",
                if ticks < 30 { "fast" } else { "slow" }
            );
            // and the grace comes first, so neglect is always visible before it costs
            assert!(FARM_RIPE_GRACE * 2 < ticks * 3, "the grace dwarfs the bleed");
            fastest = fastest.min(ticks);
            slowest = slowest.max(ticks);
        }
        assert!(
            slowest * 2 < fastest * 3,
            "the slide is {fastest}..{slowest} ticks across the soil range - a cliff, not a slope"
        );
        assert_eq!(lodge_loss(0), 1, "even a nothing field loses something");
        assert_eq!(lodge_loss(-5), 1, "and one that somehow went negative still does");
    }
}
