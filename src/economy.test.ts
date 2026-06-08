import { describe, it, expect } from 'vitest';
import {
  canAfford,
  payCost,
  refundCost,
  addResource,
  resourceField,
  applyUpkeep,
  marketSale,
  UNIT_DEFS,
  BUILDING_DEFS,
  ResourceType,
  FOOD_PER_UNIT,
  STARVE_DPS,
  ECONOMY_DT,
  MARKET_RATE,
  type ResourceCost,
  type Stockpile,
} from '../shared/index.ts';

const stock = (
  wood = 0,
  stone = 0,
  food = 0,
  gold = 0
): Stockpile => ({ wood, stone, food, gold });

describe('canAfford', () => {
  it('passes when every named resource is covered', () => {
    expect(canAfford(stock(50, 30, 10, 5), { wood: 50, stone: 30 })).toBe(true);
  });

  it('treats an empty cost as always affordable', () => {
    expect(canAfford(stock(0, 0, 0, 0), {})).toBe(true);
  });

  it('passes when the balance exactly equals the cost', () => {
    expect(canAfford(stock(20), { wood: 20 })).toBe(true);
  });

  it('fails when wood is short', () => {
    expect(canAfford(stock(10), { wood: 20 })).toBe(false);
  });

  it('fails when any single resource is short even if others suffice', () => {
    expect(canAfford(stock(100, 0, 100, 100), { wood: 1, stone: 1 })).toBe(false);
    expect(canAfford(stock(100, 100, 0, 100), { food: 5 })).toBe(false);
    expect(canAfford(stock(100, 100, 100, 0), { gold: 1 })).toBe(false);
  });
});

describe('payCost', () => {
  it('subtracts each named resource and leaves others untouched', () => {
    expect(payCost(stock(50, 30, 10, 5), { wood: 20, gold: 5 })).toEqual(
      stock(30, 30, 10, 0)
    );
  });

  it('does not mutate the input stockpile', () => {
    const p = stock(50, 50, 50, 50);
    payCost(p, { wood: 10 });
    expect(p).toEqual(stock(50, 50, 50, 50));
  });

  it('floors at zero rather than going negative', () => {
    expect(payCost(stock(5), { wood: 20 })).toEqual(stock(0));
  });
});

describe('refundCost', () => {
  it('adds back the given fraction, floored per resource', () => {
    expect(refundCost(stock(0), { wood: 41 }, 0.5)).toEqual(stock(20));
  });

  it('refunds across multiple resources', () => {
    expect(refundCost(stock(10, 10, 0, 0), { wood: 40, stone: 20 }, 0.5)).toEqual(
      stock(30, 20, 0, 0)
    );
  });

  it('refunds nothing for an empty cost', () => {
    expect(refundCost(stock(7, 7, 7, 7), {}, 1)).toEqual(stock(7, 7, 7, 7));
  });
});

describe('addResource', () => {
  it('adds to the field selected by resource type', () => {
    expect(addResource(stock(10), ResourceType.Wood, 8)).toEqual(stock(18));
    expect(addResource(stock(0, 0, 0, 0), ResourceType.Stone, 3)).toEqual(
      stock(0, 3, 0, 0)
    );
    expect(addResource(stock(0, 0, 0, 0), ResourceType.Food, 5)).toEqual(
      stock(0, 0, 5, 0)
    );
    expect(addResource(stock(0, 0, 0, 0), ResourceType.Gold, 2)).toEqual(
      stock(0, 0, 0, 2)
    );
  });

  it('does not mutate the input stockpile', () => {
    const p = stock(1, 2, 3, 4);
    addResource(p, ResourceType.Wood, 100);
    expect(p).toEqual(stock(1, 2, 3, 4));
  });
});

describe('resourceField', () => {
  it('maps every resource type to its balance field', () => {
    expect(resourceField(ResourceType.Wood)).toBe('wood');
    expect(resourceField(ResourceType.Stone)).toBe('stone');
    expect(resourceField(ResourceType.Food)).toBe('food');
    expect(resourceField(ResourceType.Gold)).toBe('gold');
  });
});

describe('applyUpkeep', () => {
  it('eats FOOD_PER_UNIT per owned unit each tick', () => {
    const r = applyUpkeep(100, 10);
    expect(r.food).toBe(100 - 10 * FOOD_PER_UNIT);
    expect(r.starving).toBe(false);
    expect(r.hpDrain).toBe(0);
  });

  it('floors food at zero and flags starvation when the bill exceeds stock', () => {
    const r = applyUpkeep(3, 10); // bill 10 > 3 food
    expect(r.food).toBe(0);
    expect(r.starving).toBe(true);
    expect(r.hpDrain).toBe(Math.round(STARVE_DPS * ECONOMY_DT));
  });

  it('does not starve when food exactly covers the bill', () => {
    const bill = 8 * FOOD_PER_UNIT;
    const r = applyUpkeep(bill, 8);
    expect(r.food).toBe(0);
    expect(r.starving).toBe(false);
    expect(r.hpDrain).toBe(0);
  });

  it('an army with no food and units starves; no units never starves', () => {
    expect(applyUpkeep(0, 5).starving).toBe(true);
    expect(applyUpkeep(0, 0).starving).toBe(false);
    expect(applyUpkeep(0, 0).hpDrain).toBe(0);
  });

  it('hp drain scales with the supplied dt', () => {
    const r = applyUpkeep(0, 1, 2);
    expect(r.hpDrain).toBe(Math.round(STARVE_DPS * 2));
  });
});

describe('marketSale', () => {
  it('mints one gold per MARKET_RATE units sold', () => {
    const r = marketSale(100, 2 * MARKET_RATE);
    expect(r.ok).toBe(true);
    expect(r.gold).toBe(2);
    expect(r.spent).toBe(2 * MARKET_RATE);
  });

  it('rounds the sale down to whole lots — never a free or partial coin', () => {
    // Offer one more than a clean lot: the odd unit is left unsold.
    const r = marketSale(100, MARKET_RATE + 1);
    expect(r.gold).toBe(1);
    expect(r.spent).toBe(MARKET_RATE);
  });

  it('caps the sale at the available balance', () => {
    const r = marketSale(MARKET_RATE, 1000);
    expect(r.gold).toBe(1);
    expect(r.spent).toBe(MARKET_RATE);
  });

  it('refuses a sale below one lot', () => {
    expect(marketSale(MARKET_RATE - 1, 100)).toEqual({ ok: false, spent: 0, gold: 0 });
    expect(marketSale(100, 1)).toEqual({ ok: false, spent: 0, gold: 0 });
  });

  it('refuses a non-positive amount or empty balance', () => {
    expect(marketSale(0, 100).ok).toBe(false);
    expect(marketSale(100, 0).ok).toBe(false);
    expect(marketSale(100, -5).ok).toBe(false);
  });
});

describe('def costs are valid ResourceCost objects', () => {
  const KEYS: (keyof ResourceCost)[] = ['wood', 'stone', 'food', 'gold'];

  const assertValid = (label: string, cost: ResourceCost) => {
    expect(typeof cost, `${label} cost should be an object`).toBe('object');
    for (const k of Object.keys(cost) as (keyof ResourceCost)[]) {
      expect(KEYS, `${label} has unknown cost key "${k}"`).toContain(k);
      const v = cost[k];
      expect(typeof v, `${label}.${k} should be a number`).toBe('number');
      expect(v as number, `${label}.${k} should be non-negative`).toBeGreaterThanOrEqual(0);
      expect(Number.isFinite(v), `${label}.${k} should be finite`).toBe(true);
    }
  };

  it('every UNIT_DEFS cost is a valid ResourceCost', () => {
    for (const [kind, def] of Object.entries(UNIT_DEFS))
      assertValid(`unit ${kind} (${def.label})`, def.cost);
  });

  it('every BUILDING_DEFS cost is a valid ResourceCost', () => {
    for (const [kind, def] of Object.entries(BUILDING_DEFS))
      assertValid(`building ${kind} (${def.label})`, def.cost);
  });
});
