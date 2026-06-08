import { describe, it, expect } from 'vitest';
import {
  moraleAfterHit,
  moraleRecover,
  shouldRout,
  hasRallied,
  isRouting,
  ROUT_THRESHOLD,
  RALLY_THRESHOLD,
  MORALE_MAX,
  MORALE_MIN,
} from './morale.ts';

describe('moraleAfterHit', () => {
  it('drops morale by a fraction of the blow size and is monotonic', () => {
    const light = moraleAfterHit(1, 0.1);
    const heavy = moraleAfterHit(1, 0.4);
    expect(light).toBeLessThan(1);
    expect(heavy).toBeLessThan(light); // a bigger blow dents harder
  });

  it('a harmless (0-fraction) hit leaves morale unchanged', () => {
    expect(moraleAfterHit(0.8, 0)).toBe(0.8);
  });

  it('never falls below MORALE_MIN', () => {
    expect(moraleAfterHit(0.1, 1)).toBe(MORALE_MIN);
    expect(moraleAfterHit(0.5, 5)).toBe(MORALE_MIN);
  });

  it('is monotonic non-increasing across a sweep of blow sizes', () => {
    let prev = moraleAfterHit(1, 0);
    for (let f = 0.05; f <= 1; f += 0.05) {
      const next = moraleAfterHit(1, f);
      expect(next).toBeLessThanOrEqual(prev);
      prev = next;
    }
  });
});

describe('moraleRecover', () => {
  it('recovers some morale over time even alone in the open', () => {
    expect(moraleRecover(0.3, 1, 0, false)).toBeGreaterThan(0.3);
  });

  it('recovers faster with nearby allies', () => {
    const alone = moraleRecover(0.3, 1, 0, false);
    const withAllies = moraleRecover(0.3, 1, 4, false);
    expect(withAllies).toBeGreaterThan(alone);
  });

  it('recovers faster near a keep or Imam aura', () => {
    const noSupport = moraleRecover(0.3, 1, 0, false);
    const support = moraleRecover(0.3, 1, 0, true);
    expect(support).toBeGreaterThan(noSupport);
  });

  it('allies and support stack', () => {
    const both = moraleRecover(0.2, 1, 4, true);
    const alliesOnly = moraleRecover(0.2, 1, 4, false);
    expect(both).toBeGreaterThan(alliesOnly);
  });

  it('caps at MORALE_MAX', () => {
    expect(moraleRecover(0.99, 10, 6, true)).toBe(MORALE_MAX);
  });

  it('does nothing for non-positive dt', () => {
    expect(moraleRecover(0.4, 0, 6, true)).toBe(0.4);
  });
});

describe('shouldRout / hasRallied thresholds', () => {
  it('routs below ROUT_THRESHOLD, holds at or above it', () => {
    expect(shouldRout(ROUT_THRESHOLD - 0.01)).toBe(true);
    expect(shouldRout(ROUT_THRESHOLD)).toBe(false);
    expect(shouldRout(0.9)).toBe(false);
  });

  it('rallies only above RALLY_THRESHOLD', () => {
    expect(hasRallied(RALLY_THRESHOLD + 0.01)).toBe(true);
    expect(hasRallied(RALLY_THRESHOLD)).toBe(false);
    expect(hasRallied(0.1)).toBe(false);
  });

  it('the rally bar sits above the rout bar (hysteresis gap exists)', () => {
    expect(RALLY_THRESHOLD).toBeGreaterThan(ROUT_THRESHOLD);
  });
});

describe('isRouting (hysteresis latch)', () => {
  it('a steady unit starts routing only once it drops below ROUT', () => {
    expect(isRouting(false, 0.4)).toBe(false); // between rout and rally, still steady
    expect(isRouting(false, ROUT_THRESHOLD - 0.01)).toBe(true);
  });

  it('a routing unit keeps fleeing through the hysteresis band', () => {
    // Climbed out of the rout band but not yet past rally -> still routing.
    expect(isRouting(true, 0.4)).toBe(true);
    expect(isRouting(true, ROUT_THRESHOLD + 0.05)).toBe(true);
  });

  it('a routing unit only rallies once it climbs past RALLY', () => {
    expect(isRouting(true, RALLY_THRESHOLD + 0.01)).toBe(false);
  });

  it('does not flicker on the boundary: full sweep up keeps fleeing until rally', () => {
    let routing = isRouting(false, 0.1); // break
    expect(routing).toBe(true);
    // Slowly recover; should stay routing until strictly above RALLY.
    for (let m = 0.1; m <= RALLY_THRESHOLD; m += 0.05) {
      routing = isRouting(routing, m);
      expect(routing).toBe(true);
    }
    routing = isRouting(routing, RALLY_THRESHOLD + 0.05);
    expect(routing).toBe(false);
  });
});
