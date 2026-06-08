// Elevation as a gameplay layer, distinct from render height. `elevation` is the
// normalized terrain height in [0,1] at a tile, derived from the same seeded
// field the module and client already agree on. High ground is a tactical
// advantage: a ranged attacker (archer/tower) shooting downhill reaches and sees
// farther. All pure + deterministic.
import { sampleTerrain } from './terrain.ts';

// Normalized 0..1 elevation at (seed, x, y). Clamped — the raw fbm field can
// briefly exceed [0,1] after the radial falloff, so callers get a stable range.
export function elevation(seed: number, x: number, y: number): number {
  const h = sampleTerrain(seed, x, y).height;
  return h < 0 ? 0 : h > 1 ? 1 : h;
}

// Largest elevation delta that still grants a bonus; beyond this it saturates.
export const ELEV_BONUS_SPAN = 0.25;
// Maximum range/vision multiplier when fully uphill of the target.
export const ELEV_BONUS_MAX = 0.25;

// Range/vision multiplier for an attacker on `attackerElev` firing at a target on
// `targetElev`, both in [0,1]. 1.0 = no change. Higher ground returns >1 (up to
// 1 + ELEV_BONUS_MAX); shooting uphill returns <1 (down to 1 - ELEV_BONUS_MAX).
// Linear in the elevation delta, clamped at ±ELEV_BONUS_SPAN. Pure.
export function elevationRangeBonus(
  attackerElev: number,
  targetElev: number
): number {
  const delta = attackerElev - targetElev;
  const clamped = Math.max(-ELEV_BONUS_SPAN, Math.min(ELEV_BONUS_SPAN, delta));
  return 1 + (clamped / ELEV_BONUS_SPAN) * ELEV_BONUS_MAX;
}
