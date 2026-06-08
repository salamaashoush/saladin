import { describe, it, expect } from 'vitest';
import {
  canAfford,
  payCost,
  refundCost,
  addResource,
  resourceField,
  UNIT_DEFS,
  BUILDING_DEFS,
  ResourceType,
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
