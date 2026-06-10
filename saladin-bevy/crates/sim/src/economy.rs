use crate::constants::{ECONOMY_DT, FOOD_PER_UNIT, MARKET_RATE, STARVE_DPS};
use crate::enums::ResourceType;
use crate::math::Fx;
use serde::{Deserialize, Serialize};

/// The cost of a thing in the four resources (missing == 0).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCost {
    pub wood: i32,
    pub stone: i32,
    pub food: i32,
    pub gold: i32,
}

impl ResourceCost {
    pub const ZERO: ResourceCost = ResourceCost { wood: 0, stone: 0, food: 0, gold: 0 };

    pub const fn new(wood: i32, stone: i32, food: i32, gold: i32) -> Self {
        ResourceCost { wood, stone, food, gold }
    }
}

/// Anything carrying the four balances — the player stockpile.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stockpile {
    pub wood: i32,
    pub stone: i32,
    pub food: i32,
    pub gold: i32,
}

impl Stockpile {
    pub fn get(&self, r: ResourceType) -> i32 {
        match r {
            ResourceType::Wood => self.wood,
            ResourceType::Stone => self.stone,
            ResourceType::Food => self.food,
            ResourceType::Gold => self.gold,
        }
    }

    pub fn add(&mut self, r: ResourceType, amt: i32) {
        match r {
            ResourceType::Wood => self.wood += amt,
            ResourceType::Stone => self.stone += amt,
            ResourceType::Food => self.food += amt,
            ResourceType::Gold => self.gold += amt,
        }
    }

    pub fn can_afford(&self, cost: &ResourceCost) -> bool {
        self.wood >= cost.wood
            && self.stone >= cost.stone
            && self.food >= cost.food
            && self.gold >= cost.gold
    }

    /// Spend `cost`, flooring each balance at zero so an over-spend can't go negative.
    pub fn pay(&mut self, cost: &ResourceCost) {
        self.wood = (self.wood - cost.wood).max(0);
        self.stone = (self.stone - cost.stone).max(0);
        self.food = (self.food - cost.food).max(0);
        self.gold = (self.gold - cost.gold).max(0);
    }

    /// Refund `frac` of `cost`, floored per-resource so refunds stay integral.
    pub fn refund(&mut self, cost: &ResourceCost, frac: Fx) {
        let f = |c: i32| (Fx::from_num(c) * frac).floor().to_num::<i32>();
        self.wood += f(cost.wood);
        self.stone += f(cost.stone);
        self.food += f(cost.food);
        self.gold += f(cost.gold);
    }
}

/// Gather priority: food first (units starve without it), then wood/stone/gold.
pub const GATHER_PRIORITY: [ResourceType; 4] =
    [ResourceType::Food, ResourceType::Wood, ResourceType::Stone, ResourceType::Gold];

/// Per-pop food cushion below which the economy biases hard toward food.
pub const FOOD_RESERVE_PER_POP: i32 = 6;

pub fn food_low(food: i32, pop: i32) -> bool {
    food < pop * FOOD_RESERVE_PER_POP
}

/// Round-robin a resource type to each of `n` idle gatherers over the types
/// actually present (food-first), spreading peasants instead of clumping.
pub fn balanced_gather_types(available: &[ResourceType], n: usize) -> Vec<ResourceType> {
    let order: Vec<ResourceType> =
        GATHER_PRIORITY.iter().copied().filter(|t| available.contains(t)).collect();
    if order.is_empty() {
        return Vec::new();
    }
    (0..n).map(|i| order[i % order.len()]).collect()
}

pub struct UpkeepResult {
    pub food: i32,
    pub starving: bool,
    pub hp_drain: i32,
}

/// One economy tick of food upkeep. Every owned unit eats FOOD_PER_UNIT; when
/// the bill exceeds the stockpile the player starves and each unit bleeds
/// STARVE_DPS over `dt`.
pub fn apply_upkeep(food: i32, unit_count: i32, dt: Fx) -> UpkeepResult {
    let bill = unit_count * FOOD_PER_UNIT;
    let starving = bill > food;
    let new_food = (food - bill).max(0);
    let hp_drain = if starving { (STARVE_DPS * dt).round().to_num::<i32>() } else { 0 };
    UpkeepResult { food: new_food, starving, hp_drain }
}

pub fn apply_upkeep_default(food: i32, unit_count: i32) -> UpkeepResult {
    apply_upkeep(food, unit_count, ECONOMY_DT)
}

pub struct TradeResult {
    pub ok: bool,
    pub spent: i32,
    pub gold: i32,
}

/// Sell `amount` of a tradeable good for gold at MARKET_RATE input:1. Rounds the
/// sale down to whole lots and refuses a sale it can't cover.
pub fn market_sale(balance: i32, amount: i32) -> TradeResult {
    if amount <= 0 || balance <= 0 {
        return TradeResult { ok: false, spent: 0, gold: 0 };
    }
    let affordable = amount.min(balance);
    let gold = affordable / MARKET_RATE;
    if gold <= 0 {
        return TradeResult { ok: false, spent: 0, gold: 0 };
    }
    TradeResult { ok: true, spent: gold * MARKET_RATE, gold }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afford_pay_floor() {
        let mut p = Stockpile { wood: 50, stone: 10, food: 0, gold: 0 };
        let cost = ResourceCost::new(70, 0, 0, 0);
        assert!(!p.can_afford(&cost));
        p.pay(&cost);
        assert_eq!(p.wood, 0); // floored, not negative
    }

    #[test]
    fn refund_floors_fraction() {
        let mut p = Stockpile::default();
        p.refund(&ResourceCost::new(45, 0, 0, 0), crate::fx!("0.5"));
        assert_eq!(p.wood, 22); // floor(22.5)
    }

    #[test]
    fn upkeep_starves() {
        let r = apply_upkeep(5, 10, ECONOMY_DT); // bill 10 > 5
        assert!(r.starving);
        assert_eq!(r.food, 0);
        assert_eq!(r.hp_drain, 8); // round(4 * 2)
        let ok = apply_upkeep(100, 10, ECONOMY_DT);
        assert!(!ok.starving);
        assert_eq!(ok.food, 90);
        assert_eq!(ok.hp_drain, 0);
    }

    #[test]
    fn market_rounds_down() {
        let t = market_sale(100, 25);
        assert!(t.ok);
        assert_eq!(t.gold, 12); // 25 / 2
        assert_eq!(t.spent, 24);
        assert!(!market_sale(100, 1).ok); // less than one lot
    }

    #[test]
    fn balanced_gather_round_robins_present() {
        let avail = [ResourceType::Wood, ResourceType::Stone];
        let g = balanced_gather_types(&avail, 4);
        // food absent -> wood, stone, wood, stone
        assert_eq!(g, vec![ResourceType::Wood, ResourceType::Stone, ResourceType::Wood, ResourceType::Stone]);
        assert!(balanced_gather_types(&[], 3).is_empty());
    }
}
