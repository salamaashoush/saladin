// Single source of truth for sim + render. Imported by the SpacetimeDB module
// (authoritative) and the Three.js client (render/interpolation).

export const WORLD_SIZE = 96; // world units == grid tiles (TILE = 1)
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

export const ARRIVE_EPS = 0.05;
export const HARVEST_RANGE = 0.7;
export const DEPOSIT_RANGE = 1.1;
export const HARVEST_TIME = 1.2; // seconds to fill one carry load

export const TREE_COUNT = 240;
export const TREE_WOOD = 120;

export const START_PEASANTS = 5;
export const START_WOOD = 60;
export const START_STONE = 0;
export const START_FOOD = 20;
export const START_GOLD = 0;
export const PEASANT_COST = 20;

export const MAX_PLAYERS = 4;
export const SPAWN_MARGIN = 16;
export const SPAWN_CLUSTER = 2.2;
