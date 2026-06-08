// Data-driven Blacksmith research. Pure: numbers/defs in, numbers/defs out — no
// SpacetimeDB or Three deps — so the authoritative module, the client UI and the
// tests all fold the SAME deltas. Bonuses are NEVER baked onto unit rows; they
// are DERIVED on read from the owner's completed-tech bitmask, so a researched
// upgrade applies to every CURRENT and FUTURE unit automatically.

import { BuildingKind, ArmorClass } from "./enums.ts";
import type { UnitDef } from "./units.ts";
import { UNIT_DEFS } from "./units.ts";
import type { BuildingDef } from "./buildings_defs.ts";
import { BUILDING_DEFS } from "./buildings_defs.ts";
import type { ResourceCost, Stockpile } from "./economy.ts";
import { canAfford } from "./economy.ts";
import { hasPrereq } from "./tech.ts";

// Each Tech is a BIT POSITION in the owner's u64 techMask (0..63). Stored as u8
// in the `research` table; the mask packs all completed techs into one number so
// combat math reads a single column. New techs append (never renumber — the bit
// index is persisted in player.techMask).
export const Tech = {
  ArmorMail: 0, // foot/missile troops: +1 armor tier (cap Mail) — survive arrows
  ArmorPlate: 1, // melee troops: +hp, harder to cut down
  FletchedArrows: 2, // archers/crossbows: +attack
  SharpenedBlades: 3, // melee attackers: +attack
  Masonry: 4, // structures: +hp / +armor tier (cap Stone)
  Conscription: 5, // ALL combatants: +hp — deeper ranks
} as const;
export type Tech = (typeof Tech)[keyof typeof Tech];

export const ALL_TECHS: Tech[] = [
  Tech.ArmorMail,
  Tech.ArmorPlate,
  Tech.FletchedArrows,
  Tech.SharpenedBlades,
  Tech.Masonry,
  Tech.Conscription,
];

// Additive deltas folded onto a base def. Every field is optional and ADDITIVE
// (armorClass/armorTier bump the tier index, clamped). Absent = no change.
export interface UnitDelta {
  attack?: number; // +flat damage per hit
  maxHp?: number; // +flat hit points
  range?: number; // +reach in world units
  armorTier?: number; // +armor class tiers (clamped to Mail for troops)
}

export interface BuildingDelta {
  maxHp?: number;
  armorTier?: number; // clamped to Stone
}

// Which units a unit-targeting tech touches. A pure predicate over the base def
// (read of UNIT_DEFS), so "applies to every ranged unit" stays data-driven and a
// new ranged unit auto-inherits the bonus with zero tech-table edits.
export type UnitPredicate = (def: UnitDef) => boolean;

const isCombatant = (d: UnitDef): boolean => d.attack > 0;
const isRanged = (d: UnitDef): boolean => !!d.ranged;
const isMelee = (d: UnitDef): boolean =>
  d.attack > 0 && !d.ranged && d.range <= 2;

export interface UpgradeDef {
  label: string;
  icon: string; // emoji shown in the Blacksmith research UI
  cost: ResourceCost;
  researchTime: number; // seconds of progress to complete
  requires?: BuildingKind; // extra building prereq beyond owning a Blacksmith
  appliesTo: UnitPredicate; // which units the unit-delta folds onto
  delta: UnitDelta; // additive bonus applied to every matching unit
  buildingDelta?: BuildingDelta; // optional structural bonus (Masonry)
  appliesToBuildings?: boolean; // when set, buildingDelta folds onto ALL structures
}

// The data. Tuning lives here only — systems read effectiveUnitDef and never see
// a literal bonus. Mail armor is the foot-troop survival pick; plate/sharpened
// split the melee line; fletching is the missile pick; masonry hardens the base;
// conscription is the across-the-board hp tech that always helps.
export const UPGRADE_DEFS: Record<Tech, UpgradeDef> = {
  [Tech.ArmorMail]: {
    label: "Mail Armor",
    icon: "🥼",
    cost: { wood: 60, gold: 40 },
    researchTime: 30,
    appliesTo: (d) => isCombatant(d) && !d.prefersBuildings, // troops, not siege
    delta: { armorTier: 1 },
  },
  [Tech.ArmorPlate]: {
    label: "Plate Barding",
    icon: "🛡️",
    cost: { wood: 40, stone: 30, gold: 60 },
    researchTime: 45,
    requires: BuildingKind.Stable,
    appliesTo: (d) => isMelee(d),
    delta: { maxHp: 25 },
  },
  [Tech.FletchedArrows]: {
    label: "Fletched Arrows",
    icon: "🏹",
    cost: { wood: 50, gold: 30 },
    researchTime: 30,
    appliesTo: (d) => isRanged(d),
    delta: { attack: 3 },
  },
  [Tech.SharpenedBlades]: {
    label: "Sharpened Blades",
    icon: "⚔️",
    cost: { wood: 50, gold: 30 },
    researchTime: 30,
    appliesTo: (d) => isMelee(d),
    delta: { attack: 3 },
  },
  [Tech.Masonry]: {
    label: "Masonry",
    icon: "🧱",
    cost: { wood: 40, stone: 80 },
    researchTime: 40,
    appliesTo: () => false, // structures only — no unit effect
    delta: {},
    appliesToBuildings: true,
    buildingDelta: { maxHp: 150, armorTier: 1 },
  },
  [Tech.Conscription]: {
    label: "Conscription",
    icon: "🪖",
    cost: { food: 60, gold: 50 },
    researchTime: 50,
    requires: BuildingKind.Barracks,
    appliesTo: (d) => isCombatant(d),
    delta: { maxHp: 15 },
  },
};

// ── bitmask helpers ───────────────────────────────────────────────────────────
// The mask is a u64 (bigint). One bit per Tech; bit index == the Tech value.

export function techBit(tech: Tech): bigint {
  return 1n << BigInt(tech);
}

export function hasTech(mask: bigint, tech: Tech): boolean {
  return (mask & techBit(tech)) !== 0n;
}

export function setTech(mask: bigint, tech: Tech): bigint {
  return mask | techBit(tech);
}

// Completed techs in a mask, in canonical (ascending bit) order. Deterministic.
export function techsInMask(mask: bigint): Tech[] {
  return ALL_TECHS.filter((t) => hasTech(mask, t));
}

// ── armor-tier clamping ───────────────────────────────────────────────────────
// Troops cap at Mail (heavy infantry plate is still "mail" class here); structures
// cap at Stone. Tiers never go below the base.

const clampTier = (tier: number, cap: ArmorClass): ArmorClass =>
  Math.max(ArmorClass.Unarmored, Math.min(tier, cap)) as ArmorClass;

// ── effective defs ────────────────────────────────────────────────────────────

// Fold the owner's completed techs (mask) into the base unit def as ADDITIVE
// deltas. Pure and deterministic: same (kind, mask) → byte-identical def, so the
// module (authority) and the client agree, and the bonus auto-applies to current
// AND future units of that kind. Returns a NEW object; never mutates UNIT_DEFS.
export function effectiveUnitDef(kind: number, mask: bigint): UnitDef {
  const base = UNIT_DEFS[kind as 0];
  if (!base || mask === 0n) return base;
  let attack = base.attack;
  let maxHp = base.maxHp;
  let range = base.range;
  let tier = base.armorClass as number;
  let changed = false;
  for (const tech of techsInMask(mask)) {
    const up = UPGRADE_DEFS[tech];
    if (!up.appliesTo(base)) continue;
    const d = up.delta;
    if (d.attack) attack += d.attack;
    if (d.maxHp) maxHp += d.maxHp;
    if (d.range) range += d.range;
    if (d.armorTier) tier += d.armorTier;
    changed = true;
  }
  if (!changed) return base;
  return {
    ...base,
    attack,
    maxHp,
    range,
    armorClass: clampTier(tier, ArmorClass.Mail),
  };
}

// Fold structural techs (Masonry) into a base building def. Same purity contract.
export function effectiveBuildingDef(kind: number, mask: bigint): BuildingDef {
  const base = BUILDING_DEFS[kind as 0];
  if (!base || mask === 0n) return base;
  let maxHp = base.maxHp;
  let tier = base.armorClass as number;
  let changed = false;
  for (const tech of techsInMask(mask)) {
    const up = UPGRADE_DEFS[tech];
    if (!up.appliesToBuildings || !up.buildingDelta) continue;
    const d = up.buildingDelta;
    if (d.maxHp) maxHp += d.maxHp;
    if (d.armorTier) tier += d.armorTier;
    changed = true;
  }
  if (!changed) return base;
  return { ...base, maxHp, armorClass: clampTier(tier, ArmorClass.Stone) };
}

// ── research panel (UI-facing, pure) ──────────────────────────────────────────
// One deterministic descriptor per tech for the Blacksmith research panel. The
// SAME function feeds the client UI and the tests; it never reads SpacetimeDB or
// Three, so the panel a player sees is computed from the exact rules the module
// enforces (afford via shared canAfford, prereq via shared hasPrereq, done via
// the techMask bit). Status precedence, top → bottom: done > in_progress >
// locked (missing prereq) > unaffordable > available.

export type ResearchStatus =
  | "done" // bit set in techMask — already owned
  | "in_progress" // a research row exists and is advancing
  | "locked" // an extra building prereq (e.g. Stable) is not owned
  | "unaffordable" // prereqs met but the stockpile is short
  | "available"; // ready to start now

export interface ResearchRowState {
  tech: Tech;
  label: string;
  icon: string;
  cost: ResourceCost;
  status: ResearchStatus;
  progress: number; // 0..1; 1 when done, the live fraction while in_progress, else 0
  lockNote?: string; // when locked: "Requires <Building>" for the tooltip
}

// Minimal shape of a research table row the panel needs. Decoupled from the
// generated binding so this stays a pure shared helper.
export interface ResearchProgressRow {
  tech: number;
  progress: number;
  done: boolean;
}

// Render order for the panel — same canonical order the AI/availableTechs uses so
// players and bots see the tree laid out identically. Ascending bit order.
const PANEL_ORDER: Tech[] = ALL_TECHS;

export function researchPanelState(
  mask: bigint,
  rows: readonly ResearchProgressRow[],
  stock: Stockpile,
  ownedBuildings: ReadonlySet<BuildingKind>,
): ResearchRowState[] {
  // Index the owner's in-flight/completed rows by tech for O(1) lookup.
  const byTech = new Map<number, ResearchProgressRow>();
  for (const r of rows) byTech.set(r.tech, r);

  return PANEL_ORDER.map((tech) => {
    const up = UPGRADE_DEFS[tech];
    const row = byTech.get(tech);
    const base = { tech, label: up.label, icon: up.icon, cost: up.cost };

    if (hasTech(mask, tech))
      return { ...base, status: "done" as const, progress: 1 };

    if (row && !row.done)
      return {
        ...base,
        status: "in_progress" as const,
        progress: Math.max(0, Math.min(1, row.progress)),
      };

    if (up.requires !== undefined && !hasPrereq(ownedBuildings, up))
      return {
        ...base,
        status: "locked" as const,
        progress: 0,
        lockNote: `Requires ${BUILDING_DEFS[up.requires as 0].label}`,
      };

    if (!canAfford(stock, up.cost))
      return { ...base, status: "unaffordable" as const, progress: 0 };

    return { ...base, status: "available" as const, progress: 0 };
  });
}
