// Pure morale math — no SpacetimeDB/Three deps so it is unit-testable and shared
// by the module (authority) and tests. Morale is a 0..1 scalar per combat unit:
// it sinks when the unit takes damage and recovers when it is not being hit,
// faster among allies and near a keep or an Imam's aura. When morale falls below
// ROUT_THRESHOLD the unit routs (flees, stops attacking) until it rallies back
// above the higher RALLY_THRESHOLD — the gap between the two is hysteresis so a
// unit doesn't flicker in and out of routing on the boundary. Deterministic.

export const MORALE_MAX = 1;
export const MORALE_MIN = 0;

// Routing band. Below ROUT a fresh (non-routing) unit breaks; a routing unit
// only stops fleeing once it climbs back above the higher RALLY threshold.
export const ROUT_THRESHOLD = 0.25;
export const RALLY_THRESHOLD = 0.5;

// How hard a hit dents morale: the drop scales with the fraction of max hp lost
// in the blow, times this weight. A blow that takes a tenth of max hp costs
// ~0.1 * MORALE_HIT_WEIGHT morale.
export const MORALE_HIT_WEIGHT = 1.5;

// Passive recovery per second when not being hit (lonely, in the open).
export const MORALE_RECOVER_BASE = 0.05;
// Extra recovery per nearby ally per second, capped by MORALE_ALLY_CAP allies so
// a huge blob doesn't recover instantly.
export const MORALE_RECOVER_PER_ALLY = 0.02;
export const MORALE_ALLY_CAP = 6;
// Flat recovery bonus per second when standing near an own keep or an Imam aura.
export const MORALE_RECOVER_SUPPORT = 0.12;

// New morale after taking a hit that removed `dmgFrac` (0..1) of the unit's max
// hp. Monotonic non-increasing in dmgFrac; clamped to [MIN, MAX].
export function moraleAfterHit(morale: number, dmgFrac: number): number {
  const drop = Math.max(0, dmgFrac) * MORALE_HIT_WEIGHT;
  return clamp(morale - drop);
}

// New morale after `dt` seconds of not being hit. Recovery is the base rate plus
// a per-ally term (capped) plus a flat support bonus when near a keep/Imam.
export function moraleRecover(
  morale: number,
  dt: number,
  nearAllies: number,
  nearKeepOrImam: boolean
): number {
  const allies = Math.max(0, Math.min(MORALE_ALLY_CAP, nearAllies));
  const rate =
    MORALE_RECOVER_BASE +
    allies * MORALE_RECOVER_PER_ALLY +
    (nearKeepOrImam ? MORALE_RECOVER_SUPPORT : 0);
  return clamp(morale + rate * Math.max(0, dt));
}

// A non-routing unit breaks when morale sinks below ROUT_THRESHOLD.
export function shouldRout(morale: number): boolean {
  return morale < ROUT_THRESHOLD;
}

// A routing unit rallies (resumes fighting) once morale climbs above the higher
// RALLY_THRESHOLD — the hysteresis gap stops boundary flicker.
export function hasRallied(morale: number): boolean {
  return morale > RALLY_THRESHOLD;
}

// Resolve the routing flag with hysteresis from the previous frame's flag and
// the current morale: a unit that was routing keeps routing until it rallies; a
// unit that wasn't starts routing only once it drops below ROUT. Pure so both
// the module and tests derive routing identically with no extra column needed.
export function isRouting(wasRouting: boolean, morale: number): boolean {
  if (wasRouting) return !hasRallied(morale);
  return shouldRout(morale);
}

function clamp(v: number): number {
  return Math.max(MORALE_MIN, Math.min(MORALE_MAX, v));
}
