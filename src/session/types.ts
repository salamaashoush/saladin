// Shapes for the player-driven match actions, shared by the actions hook and the
// UI store (kept here to avoid a store↔hook import cycle).

export interface SkirmishConfig {
  name: string;
  faction: number;
  enemies: number[]; // one AiDifficulty per opponent
  seed: number; // 0 = let the server roll a fresh map seed
  preset: string; // map preset id (render flavor)
}

export interface JoinConfig {
  name: string;
  faction: number;
}
