use crate::constants::FOOD_PER_UNIT;
use crate::enums::UnitKind;
use crate::math::Fx;
use crate::units::unit_def;

// Supply, which is what hunger should have been.
//
// The old rule was `starving = bill > food`: ALL OR NOTHING. One soldier over
// the line and the ENTIRE army starved at once — every man's morale bled at the
// same rate, and after a fixed grace every man's hp did too. There was no
// rationing, no foraging, no supply line and no desertion, so it was not a
// decision a player could answer, only a punishment. Here a shortfall of one
// man's rations costs one man's rations, a deep push costs more to feed than a
// defence at home, troops in reach of a herd feed themselves, and the men who
// leave are the ones with no heart left rather than the whole army at once.

/// Ration a unit draws at full supply.
pub const FULL_RATION: Fx = crate::fx!("1");
/// How far from a friendly drop-off an army is still fed from the stores.
pub const SUPPLY_RADIUS: Fx = crate::fx!("34");
/// What a mouth beyond the supply radius costs to keep — carts, escorts, waste.
pub const OUT_OF_SUPPLY_DRAW: Fx = crate::fx!("1.5");
/// Food a hungry unit can strip from a wild node per economy tick.
pub const FORAGE_PER_TICK: i32 = 3;
/// Below this ration a man starts thinking about home.
pub const DESERT_RATION: Fx = crate::fx!("0.85");
/// Heart a man needs to stay when he is on nothing at all.
pub const DESERT_GRIT: Fx = crate::fx!("0.35");
/// Morale bled per economy tick at a ration of ZERO; a partial shortfall bleeds
/// its own fraction of this.
/// What one soldier draws per economy tick. A quarter of FOOD_PER_UNIT: at the
/// full rate a single soldier ate everything one gatherer could carry home, so
/// an army paid for itself twice over and any fight you won bankrupted you.
pub const RATION_DRAW: Fx = crate::fx!("0.25");

pub const STARVE_MORALE_DRAIN: Fx = crate::fx!("0.3");
/// Consecutive short-ration economy ticks before any hp attrition at all.
pub const STARVE_GRACE_TICKS: i32 = 5;
/// Ticks the attrition ramps over after the grace — a worsening famine, not a
/// plague.
pub const STARVE_RAMP_TICKS: i32 = 10;

/// Whether a kind draws rations. ROLE, not `attack > 0`: giving a peasant a
/// knife must never silently put it on the muster roll, and that is exactly what
/// the old test would have done.
pub fn draws_rations(kind: UnitKind) -> bool {
    unit_def(kind).draws_rations()
}

/// One mouth's claim on the larder, before rationing.
pub fn supply_draw(in_radius: bool) -> Fx {
    if in_radius { Fx::ONE } else { OUT_OF_SUPPLY_DRAW }
}

/// The bill a body of troops presents this economy tick. `out_of_supply` is how
/// many of `eaters` are beyond the reach of a friendly store.
pub fn supply_bill(eaters: i32, out_of_supply: i32) -> Fx {
    let far = out_of_supply.clamp(0, eaters.max(0));
    let near = (eaters.max(0) - far).max(0);
    RATION_DRAW * (Fx::from_num(near) + Fx::from_num(far) * OUT_OF_SUPPLY_DRAW)
}

/// PROPORTIONAL rationing: what fraction of a full ration each mouth gets. A
/// shortfall of one man's food costs one man's food, not the whole army's.
pub fn ration(food: i32, bill: Fx) -> Fx {
    if bill <= Fx::ZERO {
        return FULL_RATION;
    }
    (Fx::from_num(food.max(0)) / bill).min(FULL_RATION)
}

/// What troops standing on a wild food node strip from it. Foraging is thin and
/// it exhausts the node, so it buys a march, never a war.
pub fn forage_yield(node_remaining: i32) -> i32 {
    node_remaining.clamp(0, FORAGE_PER_TICK)
}

/// Does this individual walk away? Discipline is the answer to hunger: a
/// Sergeant on half rations holds where a Naffatun does not.
pub fn deserts(morale: Fx, morale_resolve: Fx, ration: Fx) -> bool {
    if ration >= DESERT_RATION {
        return false;
    }
    let grit = morale.max(Fx::ZERO) * morale_resolve.max(crate::fx!("0.1"));
    grit < DESERT_GRIT * (FULL_RATION - ration)
}

pub struct SupplyResult {
    pub food: i32,
    /// Fraction of a full ration every mouth drew this tick.
    pub ration: Fx,
    /// Morale each ration-drawer loses — proportional to the SHORTFALL.
    pub morale_drain: Fx,
    /// Hp each ration-drawer loses, once the grace has run out.
    /// True while anybody is on short rations at all.
    pub short: bool,
}

/// One economy tick of supply. `hunger` counts consecutive short ticks and is
/// persisted by the caller.
pub fn apply_supply(food: i32, bill: Fx, _hunger: i32, _dt: Fx) -> SupplyResult {
    let r = ration(food, bill);
    let shortfall = (FULL_RATION - r).max(Fx::ZERO);
    let drawn = (bill * r).ceil().to_num::<i32>().min(food.max(0));
    SupplyResult {
        food: (food - drawn).max(0),
        ration: r,
        morale_drain: STARVE_MORALE_DRAIN * shortfall,
        short: shortfall > Fx::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ECONOMY_DT;

    /// The whole complaint, in one test: one man over the line used to starve
    /// twenty. Now twenty men one ration short lose one twentieth of a ration
    /// each.
    #[test]
    fn a_one_unit_shortfall_costs_one_unit_of_rations() {
        // 80 mouths, all in supply: at RATION_DRAW the bill is 20, so a single
        // integer of food is one man's share and the shortfall is expressible
        let bill = supply_bill(80, 0);
        let short_food = bill.to_num::<i32>() - 1;
        let r = ration(short_food, bill);
        assert!(r > crate::fx!("0.94") && r < FULL_RATION, "ration was {r}");
        let out = apply_supply(short_food, bill, 0, ECONOMY_DT);
        assert!(out.short);
        assert_eq!(out.food, 0);
        // the morale bite is the SHORTFALL, a fraction of the old blanket hit
        assert!(out.morale_drain < STARVE_MORALE_DRAIN / Fx::from_num(15), "{}", out.morale_drain);
        assert!(out.morale_drain > Fx::ZERO);
        // and an army with an empty larder takes the full bite
        let empty = apply_supply(0, bill, 0, ECONOMY_DT);
        assert_eq!(empty.ration, Fx::ZERO);
        assert_eq!(empty.morale_drain, STARVE_MORALE_DRAIN);
    }

    #[test]
    fn a_fed_army_pays_nothing() {
        let bill = supply_bill(10, 0);
        let out = apply_supply(500, bill, 99, ECONOMY_DT);
        assert!(!out.short);
        assert_eq!(out.ration, FULL_RATION);
        assert_eq!(out.morale_drain, Fx::ZERO);
        assert_eq!(out.food, 500 - bill.ceil().to_num::<i32>());
    }

    /// Hunger costs an army its spirit and then its men. It never costs them
    /// their lives: no game in this genre kills a soldier with an empty larder,
    /// and a starving force that cannot even walk away is a punishment, not a
    /// decision. This test is the guarantee that replaced attrition-by-hp.
    #[test]
    fn hunger_breaks_spirits_never_bodies() {
        let bill = supply_bill(10, 0);
        for hunger in [0, STARVE_GRACE_TICKS, STARVE_GRACE_TICKS + STARVE_RAMP_TICKS, 10_000] {
            let out = apply_supply(0, bill, hunger, ECONOMY_DT);
            assert!(out.short, "an empty larder is a shortfall at hunger {hunger}");
            assert!(out.morale_drain > Fx::ZERO, "hunger must cost morale at {hunger}");
        }
        // and the morale bite scales with HOW short the ration is
        let starved = apply_supply(0, bill, STARVE_GRACE_TICKS, ECONOMY_DT);
        let mild = apply_supply(9, bill, STARVE_GRACE_TICKS, ECONOMY_DT);
        assert!(mild.morale_drain < starved.morale_drain, "a thin ration is not a famine");
    }

    /// The men leave instead. Desertion is what an empty larder actually cost a
    /// medieval army, and it is the ONLY way hunger removes a soldier now.
    #[test]
    fn the_hungry_walk_away_and_the_stubborn_stay() {
        let broken = crate::fx!("0.1");
        let steady = crate::fx!("0.9");
        assert!(deserts(broken, crate::fx!("0.1"), Fx::ZERO), "starving and broken: he goes");
        assert!(!deserts(steady, crate::fx!("0.9"), FULL_RATION), "fed and steady: he stays");
        assert!(!deserts(broken, crate::fx!("0.1"), FULL_RATION), "a fed man does not desert");
    }

    /// A deep push prices itself. This is the positioning decision the flat poll
    /// tax never offered.
    #[test]
    fn an_army_far_from_its_stores_eats_more() {
        let home = supply_bill(10, 0);
        let away = supply_bill(10, 10);
        assert!(away > home);
        assert_eq!(away, home * OUT_OF_SUPPLY_DRAW);
        // half out of supply is half the penalty
        let half = supply_bill(10, 5);
        assert!(half > home && half < away);
        assert!(SUPPLY_RADIUS > crate::constants::TOWN_RADIUS, "a town cannot be out of its own supply");
    }

    /// Desertion is an INDIVIDUAL leaving, and discipline is the answer to it.
    #[test]
    fn the_men_who_leave_are_the_ones_with_no_heart_left() {
        let broken = crate::fx!("0.3");
        let levy = unit_def(UnitKind::Spearman).morale_resolve;
        let professional = unit_def(UnitKind::Sergeant).morale_resolve;
        assert!(deserts(broken, levy, Fx::ZERO), "a broken levy on nothing stays?");
        assert!(!deserts(broken, professional, Fx::ZERO), "a sergeant deserts?");
        // fed men never desert, however grim it is
        assert!(!deserts(Fx::ZERO, levy, FULL_RATION));
        // and a full-hearted man holds on short rations
        assert!(!deserts(crate::fx!("0.9"), levy, crate::fx!("0.5")));
    }

    #[test]
    fn foraging_buys_a_march_not_a_war() {
        assert_eq!(forage_yield(100), FORAGE_PER_TICK);
        assert_eq!(forage_yield(1), 1);
        assert_eq!(forage_yield(0), 0);
        assert_eq!(forage_yield(-5), 0);
        // measured against the SMALLEST field that can now exist, not the median
        assert!(
            forage_yield(i32::MAX) * 20 < crate::constants::FARM_CAP_MIN,
            "foraging replaces a farm"
        );
    }

    /// The muster roll is a ROLE question. Workers and preachers do not draw a
    /// soldier's rations, and arming a peasant must not change that.
    #[test]
    fn only_soldiers_draw_rations() {
        assert!(draws_rations(UnitKind::Spearman));
        assert!(draws_rations(UnitKind::Naffatun));
        // timber, rope and iron: an engine has no stomach
        assert!(!draws_rations(UnitKind::Ram));
        assert!(!draws_rations(UnitKind::Mangonel));
        assert!(!draws_rations(UnitKind::Peasant));
        assert!(!draws_rations(UnitKind::Imam));
        assert!(!draws_rations(UnitKind::Chaplain));
        // there is no supply line at sea, so there is no famine on a hull
        assert!(!draws_rations(UnitKind::FishingSkiff));
        assert!(!draws_rations(UnitKind::Barge));
    }
}
