// Shapes for the player-driven match actions, shared by the actions hook and the
// UI store (kept here to avoid a store↔hook import cycle).

export interface SkirmishConfig {
  name: string;
  faction: number;
  enemies: number[]; // one AiDifficulty per opponent
  seed: number; // 0 = let the server roll a fresh map seed
  preset: string; // map preset id (render flavor)
}

// Join an existing open match by its id.
export interface JoinConfig {
  matchId: string; // matchId as a decimal string (bigint-safe for keys + args)
  name: string;
  faction: number;
}

// Host a brand-new multiplayer match.
export interface CreateMatchConfig {
  name: string;
  faction: number;
  preset: string; // map preset id
}
