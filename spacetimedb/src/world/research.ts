import { BUILDING_DEFS } from '../../../shared/defs.ts';
import {
  BuildingKind,
  type BuildingKind as BuildingKindT,
} from '../../../shared/enums.ts';
import {
  Tech,
  UPGRADE_DEFS,
  hasTech,
  type Tech as TechT,
} from '../../../shared/research.ts';
import { hasPrereq } from '../../../shared/tech.ts';
import { canAfford, payCost } from '../../../shared/economy.ts';
import { ownedBuildingKinds } from './placement.ts';

// True if `owner` has a Blacksmith building (the research host).
export function hasBlacksmith(ctx: any, owner: any): boolean {
  for (const b of [...ctx.db.building.iter()])
    if (b.owner.equals(owner) && b.kind === BuildingKind.Blacksmith) return true;
  return false;
}

// Owner-parameterized "begin researching a tech at the Blacksmith". The human's
// reducer authorizes via ctx.sender then delegates here; the AI brain calls it
// directly with the bot identity. Returns null on success or an error string.
// Authority (who owns the building) lives in the caller — never here.
//
// Validates: the building is a Blacksmith, the tech's extra prereq is met, the
// tech is not already done (bit set in techMask) nor already in progress, and the
// player can afford it. On success it pays the cost and inserts a research row.
export function startResearchFor(
  ctx: any,
  owner: any,
  b: any,
  tech: number
): string | null {
  if (!b || b.kind !== BuildingKind.Blacksmith)
    return 'research must start at a Blacksmith';
  const up = UPGRADE_DEFS[tech as TechT];
  if (!up) return 'unknown tech';

  const p = ctx.db.player.identity.find(owner);
  if (!p) return 'not in game';

  if (hasTech(p.techMask, tech as TechT)) return 'already researched';

  // Extra building prereq (e.g. Plate needs a Stable). Owning a Blacksmith is
  // implied by holding `b`; this gates the rest of the tree.
  if (up.requires !== undefined && !hasPrereq(ownedBuildingKinds(ctx, owner), up))
    return `requires ${BUILDING_DEFS[up.requires as BuildingKindT].label}`;

  // Already queued/completing for this owner — one row per tech in flight.
  for (const r of [...ctx.db.research.owner.filter(owner)])
    if (r.tech === tech) return 'already in progress';

  if (!canAfford(p, up.cost)) return 'not enough resources';

  ctx.db.player.identity.update({ ...p, ...payCost(p, up.cost) });
  ctx.db.research.insert({
    researchId: 0n,
    owner,
    tech,
    progress: 0,
    done: false,
  });
  return null;
}

// Techs the owner has neither completed nor begun — the AI picks its next research
// from this set (intersected with its profile priority list). Deterministic order.
export function availableTechs(ctx: any, owner: any, mask: bigint): TechT[] {
  const inFlight = new Set<number>();
  for (const r of [...ctx.db.research.owner.filter(owner)]) inFlight.add(r.tech);
  const out: TechT[] = [];
  for (const t of [
    Tech.ArmorMail,
    Tech.SharpenedBlades,
    Tech.FletchedArrows,
    Tech.ArmorPlate,
    Tech.Conscription,
    Tech.Masonry,
  ]) {
    if (hasTech(mask, t)) continue;
    if (inFlight.has(t)) continue;
    out.push(t);
  }
  return out;
}
