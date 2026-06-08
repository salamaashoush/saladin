// Pure garrison math — no SpacetimeDB/Three deps so it is unit-testable and
// shared by the module (authority) and tests. Garrisoning posts a unit INSIDE a
// defensive structure: it leaves the field (safe from melee/fire) and, if it is
// a ranged shooter, lends its firepower to the host's auto-fire. Data drives
// who may garrison (canGarrison) and how many a structure holds (garrisonCap).

import type { UnitDef } from './units.ts';
import type { BuildingDef } from './buildings_defs.ts';

// A unit may garrison when its def opts in (garrisonable). Foot soldiers and
// missile troops shelter in walls/towers; cavalry and siege engines cannot —
// they are too large/mounted to man a parapet, so their defs leave the flag off.
export function canGarrison(def: UnitDef | undefined): boolean {
  return !!def && def.garrisonable === true;
}

// True if `host` can hold occupants at all (a positive garrison capacity).
export function canHostGarrison(host: BuildingDef | undefined): boolean {
  return !!host && (host.garrisonCap ?? 0) > 0;
}

// How many more units `host` can take given its current occupant count.
export function garrisonFreeSlots(host: BuildingDef | undefined, occupants: number): number {
  return Math.max(0, (host?.garrisonCap ?? 0) - Math.max(0, occupants));
}

// An occupant's contribution to its host's fire: only ranged shooters add
// firepower (their attack), and only those count toward the firing limit.
export interface GarrisonOccupant {
  attack: number; // base damage per shot (0 = non-shooter, lends no fire)
  ranged: boolean; // true if the unit fires from range (archer/crossbow/horse-archer)
}

// Extra fire damage garrisoned shooters add to one volley from `host`. Only the
// first `garrisonCap` ranged occupants man the firing slits (a wall holds more
// bodies than it has murder-holes), so a packed keep can't fire infinitely.
// Non-shooters still occupy the structure (protected) but add nothing here.
// Pure + deterministic: same inputs always sum identically across module/tests.
export function garrisonFirePower(
  occupants: ReadonlyArray<GarrisonOccupant>,
  host: BuildingDef | undefined
): number {
  const cap = host?.garrisonCap ?? 0;
  if (cap <= 0) return 0;
  let total = 0;
  let firing = 0;
  for (const o of occupants) {
    if (!o.ranged || o.attack <= 0) continue;
    if (firing >= cap) break;
    total += o.attack;
    firing++;
  }
  return total;
}
