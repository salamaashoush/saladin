use crate::math::Fx;

/// World is a square of `WORLD_SIZE` tiles (TILE == 1 world unit).
pub const WORLD_SIZE: i32 = 288;
pub const TILE: Fx = crate::fx!("1");

// Scheduled-system rates (ms) and their derived dt in seconds.
pub const MOVE_TICK_MS: i64 = 50;
pub const MOVE_DT: Fx = crate::fx!("0.05");
pub const AI_TICK_MS: i64 = 200;
pub const AI_DT: Fx = crate::fx!("0.2");
pub const COMBAT_TICK_MS: i64 = 200;
pub const COMBAT_DT: Fx = crate::fx!("0.2");
pub const AI_BRAIN_TICK_MS: i64 = 1000;
pub const AI_BRAIN_DT: Fx = crate::fx!("1");
pub const ECONOMY_TICK_MS: i64 = 2000;
pub const ECONOMY_DT: Fx = crate::fx!("2");
pub const RESEARCH_TICK_MS: i64 = 1000;
pub const RESEARCH_DT: Fx = crate::fx!("1");

pub const ARRIVE_EPS: Fx = crate::fx!("0.05");
/// Buildings must rise within this range of an existing own building — towns
/// grow outward instead of teleporting structures across the map.
pub const TOWN_RADIUS: Fx = crate::fx!("28");
pub const HARVEST_RANGE: Fx = crate::fx!("0.7");
pub const DEPOSIT_RANGE: Fx = crate::fx!("1.1");
pub const HARVEST_TIME: Fx = crate::fx!("1.2");
/// Fishing-hut work aura: fish nodes within this range of a friendly hut are
/// harvested at double speed (nets + boats).
pub const FISHING_HUT_RANGE: Fx = crate::fx!("6");

// Resource node counts per map and per-node yields.
pub const TREE_COUNT: i32 = 2160;
pub const TREE_WOOD: i32 = 120;
pub const STONE_NODES: i32 = 540;
pub const STONE_YIELD: i32 = 200;
pub const GOLD_NODES: i32 = 160;
pub const GOLD_YIELD: i32 = 140;
pub const FOOD_NODES: i32 = 360;
pub const FOOD_YIELD: i32 = 160;

// Food economy: every owned unit eats FOOD_PER_UNIT per economy tick; an empty
// stockpile bleeds STARVE_DPS hp/sec.
pub const FOOD_PER_UNIT: i32 = 1;
pub const STARVE_DPS: Fx = crate::fx!("4");

// Market: sell MARKET_RATE units of a good for one gold.
pub const MARKET_RATE: i32 = 2;

pub const START_PEASANTS: i32 = 5;
pub const START_WOOD: i32 = 60;
pub const START_STONE: i32 = 30;
pub const START_FOOD: i32 = 100;
pub const START_GOLD: i32 = 0;
pub const PEASANT_COST: i32 = 20;

pub const MAX_PLAYERS: usize = 8;
pub const SPAWN_MARGIN: i32 = 40;
pub const SPAWN_CLUSTER: Fx = crate::fx!("2.2");
