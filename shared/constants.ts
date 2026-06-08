// Single source of truth for sim + render. Imported by the SpacetimeDB module
// (authoritative) and the Three.js client (render/interpolation).

export const WORLD_SIZE = 144; // world units == grid tiles (TILE = 1)
export const TILE = 1;

// Two scheduled reducers at different rates: smooth movement, slower AI.
export const MOVE_TICK_MS = 50;
export const MOVE_DT = MOVE_TICK_MS / 1000;
export const AI_TICK_MS = 200;
export const AI_DT = AI_TICK_MS / 1000;
export const COMBAT_TICK_MS = 200;
export const COMBAT_DT = COMBAT_TICK_MS / 1000;
export const AI_BRAIN_TICK_MS = 1000;
export const AI_BRAIN_DT = AI_BRAIN_TICK_MS / 1000;
export const ECONOMY_TICK_MS = 2000;
export const ECONOMY_DT = ECONOMY_TICK_MS / 1000;

export const ARRIVE_EPS = 0.05;
export const HARVEST_RANGE = 0.7;
export const DEPOSIT_RANGE = 1.1;
export const HARVEST_TIME = 1.2; // seconds to fill one carry load

// Resource node counts scattered per map, and how much each node holds. Wood is
// the staple (forests), the rest sparser. Yields feed scatterNodes + the client
// scale curve. Counts scale ~with map area (144²/96² ≈ 2.25×) so density stays
// roughly constant as the map grows.
export const TREE_COUNT = 540;
export const TREE_WOOD = 120;
export const STONE_NODES = 135;
export const STONE_YIELD = 200;
export const GOLD_NODES = 40;
export const GOLD_YIELD = 140;
export const FOOD_NODES = 90;
export const FOOD_YIELD = 160;

// Food economy: every owned unit eats FOOD_PER_UNIT per economy tick. When the
// stockpile hits zero, units bleed STARVE_DPS hit points per second until fed.
export const FOOD_PER_UNIT = 1;
export const STARVE_DPS = 4;

// Market: sell two units of wood or stone for one gold. Buildings/towers later
// can demand gold, so a wood-rich player can convert surplus into coin.
export const MARKET_RATE = 2;

export const START_PEASANTS = 5;
export const START_WOOD = 60;
export const START_STONE = 30;
export const START_FOOD = 60;
export const START_GOLD = 0;
export const PEASANT_COST = 20;

export const MAX_PLAYERS = 4;
export const SPAWN_MARGIN = 24; // scales with WORLD_SIZE — keeps keeps off the coast
export const SPAWN_CLUSTER = 2.2;
