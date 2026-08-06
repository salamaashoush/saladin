use crate::enums::UnitKind;
use crate::math::Fx;
use crate::morale::{MORALE_MAX, ROUT_THRESHOLD};
use crate::units::unit_def;

// THE BAGGAGE TRAIN.
//
// A soldier's bread is bought twice: once at the muster, and again every day he
// stands somewhere his own stores cannot reach. Between them — in garrison, in
// his own country, within reach of a drop-off — he costs NOTHING, because the
// ground he is standing on is already yours and already feeding him.
//
// Two models were tried and both failed the same way. `bill = men *
// FOOD_PER_UNIT` with an all-or-nothing failure killed armies outright; the same
// bill at a quarter of the rate stopped mattering (measured: 10 soldiers, 1.25
// food/s, against an 1868 stockpile). A flat per-head drain on a STOCK has no
// stable band — it is a flow subtracted from a pile, so either income beats it
// and it is decoration, or it beats income and it is a death spiral. Which one
// you get is a knife edge, not a design.
//
// What limits army size is the pop cap and what a soldier costs to raise
// (`UnitDef.cost`, three quarters of it bread). What supply limits is DEPTH AND
// DURATION: how far from your stores you can campaign, and for how long. Those
// are different questions and only the second one is spatial, which is the half
// of the old system that was worth keeping.

/// Within this of a friendly drop-off a soldier is fed FROM the stores and draws
/// nothing at all. A garrison is free — the whole model turns on that.
pub const SUPPLY_RADIUS: Fx = crate::fx!("34");
/// Tiles of road past the supply radius that add one full ration to a man's
/// draw. The ramp is CONTINUOUS: a cliff at the radius would be the same
/// all-or-nothing failure in new clothes, one step over the line flipping an
/// army from free to fully billed.
pub const SUPPLY_SPAN: Fx = crate::fx!("34");
/// The road never costs more than this many rations a man. A march to the far
/// corner of the map is dear, not infinite.
pub const MAX_STRAIN: Fx = crate::fx!("3");
/// THE ONE RATE. Food a soldier draws per economy tick per unit of strain —
/// every other number in the model is a shape, and sim, AI, HUD and tooling all
/// price the road through this and nothing else.
pub const FIELD_RATION: Fx = crate::fx!("0.1");

/// A man's full claim. Rations are issued as a FRACTION of this.
pub const FULL_RATION: Fx = crate::fx!("1");
/// Food a hungry unit can strip from a wild node per economy tick.
pub const FORAGE_PER_TICK: i32 = 3;
/// Below this ration a man starts thinking about home.
pub const DESERT_RATION: Fx = crate::fx!("0.85");
/// Heart a man needs to stay when he is on nothing at all.
pub const DESERT_GRIT: Fx = crate::fx!("0.35");
/// Consecutive short-ration economy ticks before anybody walks out. Men do not
/// leave on the first evening without supper.
pub const STARVE_GRACE_TICKS: i32 = 5;
/// Ration below which the famine clock runs at all. Above it the column is
/// tired, not breaking.
pub const FAMINE_RATION: Fx = crate::fx!("0.5");
/// Combat ticks of extra rest a man on nothing at all takes between blows, added
/// once per economy tick. Hungry troops swing slower before they walk away.
pub const FATIGUE_TICKS: i32 = 3;
/// Economy ticks of campaigning a commander should be able to pay for. What
/// `campaign_reserve` is measured in, and the only place the answer to "how much
/// food does a war need" is written down.
pub const CAMPAIGN_TICKS: i32 = 90;

/// Whether a kind draws rations. ROLE, not `attack > 0`: giving a peasant a
/// knife must never silently put it on the muster roll.
pub fn draws_rations(kind: UnitKind) -> bool {
    unit_def(kind).draws_rations()
}

/// How hard the road pulls on one man standing `dist` from his nearest store.
/// Zero inside the supply radius, and that zero is exact: a defence at home is
/// not "cheap", it is free.
pub fn strain(dist_to_store: Fx) -> Fx {
    ((dist_to_store - SUPPLY_RADIUS) / SUPPLY_SPAN).clamp(Fx::ZERO, MAX_STRAIN)
}

/// The bill a body of troops presents this economy tick, given the summed strain
/// over every mouth. An army wholly in supply sums to zero and bills nothing.
pub fn supply_bill(total_strain: Fx) -> Fx {
    FIELD_RATION * total_strain.max(Fx::ZERO)
}

/// What one man at `strain` claims — the same number `supply_bill` sums, so the
/// forage path cannot drift from the billing path the way the old per-man draw
/// did.
pub fn man_draw(strain: Fx) -> Fx {
    supply_bill(strain)
}

/// PROPORTIONAL rationing: what fraction of a full ration each mouth in the
/// field gets. A shortfall of one man's food costs one man's food.
pub fn ration(food: i32, bill: Fx) -> Fx {
    if bill <= Fx::ZERO {
        return FULL_RATION;
    }
    (Fx::from_num(food.max(0)) / bill).min(FULL_RATION)
}

/// The heart a man keeps on a given ration. Full commons cost nothing; an empty
/// larder pins him exactly ON the breaking point, so the next blow breaks him.
///
/// A CEILING, not a drain, and the reason is measured: a packed formation
/// recovers about +0.34 morale per economy tick against a largest-possible
/// drain of 0.30, so a drain-based hunger is invisible in exactly the formation
/// that most needs feeding.
pub fn morale_ceiling(r: Fx) -> Fx {
    ROUT_THRESHOLD + (MORALE_MAX - ROUT_THRESHOLD) * r.clamp(Fx::ZERO, FULL_RATION)
}

/// Combat ticks of extra rest a man on `r` rations takes between blows.
pub fn fatigue_ticks(r: Fx) -> i32 {
    (Fx::from_num(FATIGUE_TICKS) * (FULL_RATION - r).max(Fx::ZERO)).round().to_num::<i32>()
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

/// Food a commander wants banked before marching `soldiers` men out to the far
/// end of the map — a full campaign at maximum strain. The bot's war chest and
/// the HUD's warning both read this, so neither can misprice the road.
pub fn campaign_reserve(soldiers: i32) -> i32 {
    if soldiers <= 0 {
        return 0;
    }
    // ROUNDED, not ceiled: 0.1 has no exact fixed-point representation, and a
    // ceiling turns the last bit of that into a whole loaf of war chest.
    (supply_bill(MAX_STRAIN * Fx::from_num(soldiers)) * Fx::from_num(CAMPAIGN_TICKS))
        .round()
        .to_num::<i32>()
}

pub struct SupplyResult {
    pub food: i32,
    /// Fraction of a full ration every mouth in the field drew this tick.
    pub ration: Fx,
}

/// One economy tick of supply: issue what the larder covers and debit it.
pub fn apply_supply(food: i32, bill: Fx) -> SupplyResult {
    let r = ration(food, bill);
    let drawn = (bill * r).ceil().to_num::<i32>().min(food.max(0));
    SupplyResult { food: (food - drawn).max(0), ration: r }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TOWN_RADIUS;

    /// THE WHOLE MODEL, IN ONE TEST. An army at home is free. The flat per-head
    /// tax this replaces was charged wherever a man stood, which is what made it
    /// either crushing or irrelevant with no band in between.
    #[test]
    fn a_garrison_costs_nothing_at_all() {
        assert_eq!(strain(Fx::ZERO), Fx::ZERO);
        assert_eq!(strain(SUPPLY_RADIUS), Fx::ZERO);
        assert_eq!(supply_bill(Fx::ZERO), Fx::ZERO);
        // a thousand men beside the keep bill nothing
        let out = apply_supply(500, supply_bill(Fx::ZERO));
        assert_eq!(out.food, 500);
        assert_eq!(out.ration, FULL_RATION);
        assert!(SUPPLY_RADIUS > TOWN_RADIUS, "a town cannot be out of its own supply");
    }

    /// The road is a RAMP, not a cliff. One step past the line must cost about
    /// one step's worth — a step function there is the all-or-nothing failure
    /// wearing a distance check.
    #[test]
    fn the_road_prices_itself_by_the_tile() {
        let just_out = strain(SUPPLY_RADIUS + crate::fx!("1"));
        assert!(just_out > Fx::ZERO);
        assert!(just_out < crate::fx!("0.05"), "a cliff at the supply line: {just_out}");
        assert_eq!(strain(SUPPLY_RADIUS + SUPPLY_SPAN), FULL_RATION);
        assert_eq!(strain(SUPPLY_RADIUS + SUPPLY_SPAN * Fx::from_num(2)), crate::fx!("2"));
        // and it caps, so the far corner of the map is dear rather than infinite
        assert_eq!(strain(crate::fx!("400")), MAX_STRAIN);
    }

    /// A deep push prices itself against a shallow one. This is the positioning
    /// decision the flat poll tax never offered.
    #[test]
    fn a_deep_push_costs_more_than_a_raid_over_the_fence() {
        let raid = supply_bill(strain(SUPPLY_RADIUS + crate::fx!("10")) * Fx::from_num(10));
        let march = supply_bill(strain(SUPPLY_RADIUS + crate::fx!("40")) * Fx::from_num(10));
        let siege = supply_bill(strain(crate::fx!("250")) * Fx::from_num(10));
        assert!(Fx::ZERO < raid && raid < march && march < siege);
        // ten men at the far end of the map, per economy tick
        assert_eq!(siege, FIELD_RATION * MAX_STRAIN * Fx::from_num(10));
    }

    /// A one-man shortfall costs one man's rations. The rule this replaced was
    /// `bill > food`: one loaf short and every soldier starved at once.
    #[test]
    fn a_one_unit_shortfall_costs_one_unit_of_rations() {
        // 80 men at full strain bill 24
        let bill = supply_bill(MAX_STRAIN * Fx::from_num(80));
        let short_food = bill.to_num::<i32>() - 1;
        let r = ration(short_food, bill);
        assert!(r > crate::fx!("0.94") && r < FULL_RATION, "ration was {r}");
        let out = apply_supply(short_food, bill);
        assert_eq!(out.food, 0);
        // and an army with an empty larder is on nothing
        assert_eq!(apply_supply(0, bill).ration, Fx::ZERO);
    }

    /// Hunger costs an army its spirit and then its men. It never costs them
    /// their lives: no game in this genre kills a soldier with an empty larder,
    /// and a starving force that cannot even walk away is a punishment, not a
    /// decision. There is no hp term anywhere in this module — that is the
    /// guarantee, and it is structural rather than asserted.
    #[test]
    fn hunger_breaks_spirits_never_bodies() {
        assert_eq!(morale_ceiling(FULL_RATION), MORALE_MAX);
        assert_eq!(morale_ceiling(Fx::ZERO), ROUT_THRESHOLD);
        assert!(morale_ceiling(crate::fx!("0.5")) < MORALE_MAX);
        assert!(morale_ceiling(crate::fx!("0.5")) > ROUT_THRESHOLD);
        assert_eq!(fatigue_ticks(FULL_RATION), 0);
        assert_eq!(fatigue_ticks(Fx::ZERO), FATIGUE_TICKS);
    }

    /// The men leave instead. Desertion is what an empty larder actually cost a
    /// medieval army, and it is the ONLY way hunger removes a soldier.
    #[test]
    fn the_hungry_walk_away_and_the_stubborn_stay() {
        let broken = crate::fx!("0.1");
        let steady = crate::fx!("0.9");
        assert!(deserts(broken, crate::fx!("0.1"), Fx::ZERO), "starving and broken: he goes");
        assert!(!deserts(steady, crate::fx!("0.9"), FULL_RATION), "fed and steady: he stays");
        assert!(!deserts(broken, crate::fx!("0.1"), FULL_RATION), "a fed man does not desert");
    }

    /// Desertion is an INDIVIDUAL leaving, and discipline is the answer to it.
    #[test]
    fn the_men_who_leave_are_the_ones_with_no_heart_left() {
        let broken = crate::fx!("0.3");
        let levy = unit_def(UnitKind::Spearman).morale_resolve;
        let professional = unit_def(UnitKind::Sergeant).morale_resolve;
        assert!(deserts(broken, levy, Fx::ZERO), "a broken levy on nothing stays?");
        assert!(!deserts(broken, professional, Fx::ZERO), "a sergeant deserts?");
        assert!(!deserts(Fx::ZERO, levy, FULL_RATION));
        assert!(!deserts(crate::fx!("0.9"), levy, crate::fx!("0.5")));
    }

    #[test]
    fn foraging_buys_a_march_not_a_war() {
        assert_eq!(forage_yield(100), FORAGE_PER_TICK);
        assert_eq!(forage_yield(1), 1);
        assert_eq!(forage_yield(0), 0);
        assert_eq!(forage_yield(-5), 0);
        assert!(
            forage_yield(i32::MAX) * 20 < crate::constants::FARM_CAP_MIN,
            "foraging replaces a farm"
        );
    }

    /// The forage path and the billing path must read the SAME number. The last
    /// rework found the per-man draw computed in three places that had drifted.
    #[test]
    fn one_mans_draw_is_the_army_bill_divided_by_the_army() {
        for s in ["0.3", "1", "2.5", "3"] {
            let st = Fx::lit(s);
            assert_eq!(man_draw(st) * Fx::from_num(40), supply_bill(st * Fx::from_num(40)));
        }
    }

    /// The war chest is priced off the one rate, so a bot can never misprice the
    /// road. Ninety economy ticks is three minutes of campaigning at full depth.
    #[test]
    fn the_war_chest_is_the_road_priced_in_advance() {
        assert_eq!(campaign_reserve(0), 0);
        assert_eq!(campaign_reserve(20), 540);
        let bill = supply_bill(MAX_STRAIN * Fx::from_num(20));
        assert_eq!(campaign_reserve(20), (bill * Fx::from_num(CAMPAIGN_TICKS)).to_num::<i32>());
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
