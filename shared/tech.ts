// Pure tech-tree gate shared by the module (authority) and the client (UI dim).
// A building or unit def with a `requires` field needs the player to already own
// that prerequisite building before it can be placed/trained. No deps beyond the
// def shapes, so it runs identically server-side and in tests.

import type { BuildingKind } from './enums.ts';

export interface Gated {
  requires?: BuildingKind;
}

// True when `def` has no prerequisite, or the prerequisite kind is in `owned`.
export function hasPrereq(owned: ReadonlySet<BuildingKind>, def: Gated): boolean {
  if (def.requires === undefined) return true;
  return owned.has(def.requires);
}
