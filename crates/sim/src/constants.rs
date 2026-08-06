use crate::math::Fx;

/// World is a square of `WORLD_SIZE` tiles (TILE == 1 world unit).
pub const WORLD_SIZE: i32 = 384;
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
/// Granary work aura: friendly fields within this range are worked and regrow
/// faster — a hub is worthless alone and transformative over a cluster. Sized
/// against measured granary-to-farm spacing inside `TOWN_RADIUS` (3.6 to 18.4
/// tiles): at 8 it reached two farms out of nine.
pub const GRANARY_RANGE: Fx = crate::fx!("14");
/// Mosque morale aura: the ground a standing mosque steadies.
pub const MOSQUE_MORALE_RANGE: Fx = crate::fx!("14");

// ── construction ────────────────────────────────────────────────────────────
// Building, repairing and upgrading are ONE loop: a builder adds `work` to the
// job and `hp` to anything below full. hp is authoritative and additive (work
// adds, damage subtracts), so a site under fire needs no special case.

/// How close a peasant must stand to a job site to work on it.
pub const BUILD_RANGE: Fx = crate::fx!("1.4");
/// Builders past this add nothing — a foundation only has so many edges.
pub const MAX_BUILDERS: i32 = 8;
/// Work rate in hundredths per builder count (index == builders, clamped to
/// MAX_BUILDERS). A table, so the diminishing-returns curve never costs an
/// `fx_sqrt` in a per-tick loop.
pub const BUILDER_RATE: [i32; 9] = [0, 100, 175, 235, 285, 325, 355, 375, 385];
/// A founded site starts at this percentage of the finished structure's health,
/// so an unguarded build is worth raiding. Integer percent, not a fraction:
/// 0.10 has no exact fixed-point representation and would floor a 1500 hp keep
/// site to 149 on some rounding paths.
pub const SITE_HP_PCT: i32 = 10;
/// Repairing a structure from zero to full costs this percentage of its build
/// cost — damage is recoverable, never free.
pub const REPAIR_COST_PCT: i32 = 50;
/// Demolishing returns this percentage of the build cost, SCALED by remaining
/// health: a burnt-out shell is worth what it looks like.
pub const DEMOLISH_REFUND_PCT: i32 = 50;
/// How far a builder will look for another job when its own site finishes.
pub const SITE_REASSIGN_RADIUS: Fx = crate::fx!("14");
/// Units a production building may hold in its queue.
pub const QUEUE_CAP: usize = 5;

// Resource node counts per map and per-node yields.
pub const TREE_COUNT: i32 = 2160;
pub const TREE_WOOD: i32 = 120;
pub const STONE_NODES: i32 = 540;
pub const STONE_YIELD: i32 = 200;
pub const GOLD_NODES: i32 = 160;
pub const GOLD_YIELD: i32 = 140;
pub const FOOD_NODES: i32 = 360;
pub const FOOD_YIELD: i32 = 160;
/// Gravel gold in the channels below the highlands: cheap, safe, early.
pub const PLACER_NODES: i32 = 60;
/// The exploration prize in the high country. Sized to the Hills/Alpine band
/// the elevation curve actually grows — a few percent of land, so these land
/// as roughly half a dozen patches per map rather than one lucky pocket.
pub const MOTHERLODE_GOLD_NODES: i32 = 40;
pub const MOTHERLODE_STONE_NODES: i32 = 36;
/// Fish replenished per economy tick on every water food node inside a
/// fishing hut's reach — huts sustain their fishery; unmanaged waters fish out.
pub const FISH_REGEN_PER_TICK: i32 = 2;

// Farms: the only renewable food that scales, and the only node whose output is
// a function of TIME AND CARE rather than of how many hands you put on it. Soil
// sets how big the harvest is (`field_cap`), labour sets how fast it comes in
// (`field_growth`), and nothing at all grows below FARM_MIN_FERTILITY — which is
// what makes floodplains, deltas and oases worth taking and holding. The season
// math is `farming.rs`.
/// Superseded by `field_cap`: the one flat store every field used to carry,
/// whatever it was sown on. Kept as the yardstick the soil harnesses plot the
/// old model against — measured, real farms now land between 96 and 113.
pub const FARM_STORE: i32 = 90;
/// Superseded by `field_cap`: the old truncated integer regen, kept because the
/// soil harnesses plot the model it produced against the one that replaced it.
pub const FARM_REGEN_MAX: i32 = 7;
pub const FARM_MIN_FERTILITY: Fx = crate::fx!("0.22");
/// Standing crop the poorest ground that clears the gate carries, and what the
/// richest carries. A truncated `1 + soil * FARM_REGEN_MAX` landed on 2 or 3 for
/// 84-94% of all sowable land, so soil 0.30 and soil 0.42 built the same farm.
pub const FARM_CAP_MIN: i32 = 70;
pub const FARM_CAP_MAX: i32 = 190;
/// Soil at or above this carries a full harvest. Measured p99 fertility is
/// 0.44-0.56 and the world maximum 0.60-0.74, so pinning higher would spend most
/// of the range on ground no seed grows.
pub const FARM_SOIL_RICH: Fx = crate::fx!("0.60");
/// Seconds ONE pair of hands takes to bring a full field in from bare furrows.
pub const FARM_TEND_TIME: Fx = crate::fx!("60");
/// Rain-fed growth per economy tick on a field nobody is working — slow, but a
/// neglected farm is never worthless and never deleted.
pub const FARM_REGEN_IDLE: i32 = 1;
/// A new field is sown at `cap / this`, so the first season pays back fast
/// without handing over a free harvest the moment the plot is finished.
pub const FARM_SOW_DIVISOR: i32 = 4;
/// Standing crop lost per economy tick once it lodges, as `cap / this` rounded
/// UP: 60 to 100 s from a full field to a bare one whatever the soil,
/// salvageable at any point along it.
pub const FARM_LODGE_DIVISOR: i32 = 50;
/// Economy ticks a ripe crop may stand uncut before it starts to lodge
/// (30 x 2 s = 60 s of grace, then a slow visible bleed).
pub const FARM_RIPE_GRACE: i32 = 30;

// Food economy: every ration-drawing unit eats FOOD_PER_UNIT per economy tick.
// A short larder is PROPORTIONAL from here on (`supply.rs`) — an empty one
// bleeds STARVE_DPS hp/sec, a nearly-full one costs almost nothing.
pub const FOOD_PER_UNIT: i32 = 1;
pub const STARVE_DPS: Fx = crate::fx!("4");

// ── shock ────────────────────────────────────────────────────────────────────
// A charge is a once-per-approach event, not a passive damage stat, so it needs
// a run-up and a recovery. Both are counted in COMBAT ticks (200 ms).
/// Ground a rider must cover unobstructed before the next blow counts as a charge.
pub const CHARGE_MIN_RUN: Fx = crate::fx!("4");
/// Combat ticks before the same rider may charge again.
pub const CHARGE_COOLDOWN_TICKS: i32 = 50;
/// Combat ticks a broken unit needs before it will listen to an order again.
pub const RALLY_COOLDOWN_TICKS: i32 = 25;

// Market: sell MARKET_RATE units of a good for one gold; buying costs
// MARKET_BUY_RATE gold per unit — the spread is the merchant's cut.
pub const MARKET_RATE: i32 = 2;
pub const MARKET_BUY_RATE: i32 = 2;

pub const START_PEASANTS: i32 = 5;
pub const START_WOOD: i32 = 60;
pub const START_STONE: i32 = 30;
pub const START_FOOD: i32 = 100;
pub const START_GOLD: i32 = 0;
pub const PEASANT_COST: i32 = 20;

pub const MAX_PLAYERS: usize = 8;
pub const SPAWN_MARGIN: i32 = 40;
pub const SPAWN_CLUSTER: Fx = crate::fx!("2.2");
