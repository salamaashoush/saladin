// Pure resource-economy contract shared by the authoritative module and the
// client. No SpacetimeDB or Three imports — just numbers in, numbers out, so it
// runs identically on the server (deterministic) and in tests.

import { ResourceType } from './enums.ts';

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
