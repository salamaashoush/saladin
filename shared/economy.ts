// Pure resource-economy contract shared by the authoritative module and the
// client. No SpacetimeDB or Three imports — just numbers in, numbers out, so it
// runs identically on the server (deterministic) and in tests.

import { ResourceType } from './enums.ts';
import {
  FOOD_PER_UNIT,
  STARVE_DPS,
  ECONOMY_DT,
  MARKET_RATE,
} from './constants.ts';

export interface ResourceCost {
  wood?: number;
  stone?: number;
  food?: number;
  gold?: number;
}

// A stockpile is anything carrying the four balances — the player row qualifies.
export interface Stockpile {
  wood: number;
  stone: number;
  food: number;
  gold: number;
}

export type ResourceField = 'wood' | 'stone' | 'food' | 'gold';

const RESOURCE_FIELDS: Record<ResourceType, ResourceField> = {
  [ResourceType.Wood]: 'wood',
  [ResourceType.Stone]: 'stone',
  [ResourceType.Food]: 'food',
  [ResourceType.Gold]: 'gold',
};

export function resourceField(resType: ResourceType): ResourceField {
  return RESOURCE_FIELDS[resType] ?? 'wood';
}

export function canAfford(p: Stockpile, cost: ResourceCost): boolean {
  return (
    p.wood >= (cost.wood ?? 0) &&
    p.stone >= (cost.stone ?? 0) &&
    p.food >= (cost.food ?? 0) &&
    p.gold >= (cost.gold ?? 0)
  );
}

// Returns the four new balances after spending `cost`. Does not mutate `p`;
// callers stamp the result onto the row. Floored at zero so an over-spend can
// never push a balance negative.
export function payCost(p: Stockpile, cost: ResourceCost): Stockpile {
  return {
    wood: Math.max(0, p.wood - (cost.wood ?? 0)),
    stone: Math.max(0, p.stone - (cost.stone ?? 0)),
    food: Math.max(0, p.food - (cost.food ?? 0)),
    gold: Math.max(0, p.gold - (cost.gold ?? 0)),
  };
}

// Returns the four new balances after refunding `frac` of `cost` (e.g. 0.5 on
// demolish). Fractions are floored per-resource so refunds stay integral.
export function refundCost(
  p: Stockpile,
  cost: ResourceCost,
  frac: number
): Stockpile {
  return {
    wood: p.wood + Math.floor((cost.wood ?? 0) * frac),
    stone: p.stone + Math.floor((cost.stone ?? 0) * frac),
    food: p.food + Math.floor((cost.food ?? 0) * frac),
    gold: p.gold + Math.floor((cost.gold ?? 0) * frac),
  };
}

// Returns the four new balances after adding `amt` of one resource type.
export function addResource(
  p: Stockpile,
  resType: ResourceType,
  amt: number
): Stockpile {
  const field = resourceField(resType);
  return { ...p, [field]: p[field] + amt };
}

// ── upkeep / starvation ───────────────────────────────────────────────────────

export interface UpkeepResult {
  food: number; // food balance after the tick (floored at 0)
  starving: boolean; // true when the unit count outpaces food this tick
  hpDrain: number; // hp each owned unit loses this tick when starving (0 otherwise)
}

// One economy tick of food upkeep for a player. Every owned unit eats
// FOOD_PER_UNIT; when the bill exceeds the stockpile the player starves and each
// unit bleeds STARVE_DPS over the tick. Pure: numbers in, numbers out, so the
// module and the test agree byte-for-byte. `dt` defaults to the economy tick.
export function applyUpkeep(
  food: number,
  unitCount: number,
  dt: number = ECONOMY_DT
): UpkeepResult {
  const bill = unitCount * FOOD_PER_UNIT;
  const starving = bill > food;
  const newFood = Math.max(0, food - bill);
  const hpDrain = starving ? Math.round(STARVE_DPS * dt) : 0;
  return { food: newFood, starving, hpDrain };
}

// ── market ────────────────────────────────────────────────────────────────────

export type Tradeable = 'wood' | 'stone';

export interface TradeResult {
  ok: boolean;
  spent: number; // units of the input resource consumed
  gold: number; // gold minted
}

// Sell `amount` of wood or stone for gold at MARKET_RATE input:1 gold. Rounds the
// sale DOWN to whole lots so a player can never mint a fractional or free coin,
// and refuses a sale it can't fully cover. Pure helper shared by the reducer.
export function marketSale(
  balance: number,
  amount: number
): TradeResult {
  if (amount <= 0 || balance <= 0) return { ok: false, spent: 0, gold: 0 };
  const affordable = Math.min(amount, balance);
  const gold = Math.floor(affordable / MARKET_RATE);
  if (gold <= 0) return { ok: false, spent: 0, gold: 0 };
  const spent = gold * MARKET_RATE;
  return { ok: true, spent, gold };
}
