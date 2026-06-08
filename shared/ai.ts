// Pure strategic AI planner shared by the module (authority) and tests. No
// SpacetimeDB / Three deps — holdings + a census in, decisions out — so it runs
// deterministically server-side and byte-for-byte in vitest. The ai_brain system
// reducer gathers a snapshot of the bot's world each tick, calls these planners,
// and executes the verdict through the SAME owner-parameterized helpers a human's
// reducers use (trainFrom / placeFor). No cheats, no special powers.

import { UnitKind, BuildingKind, ArmorClass } from './enums.ts';
import { UNIT_DEFS } from './units.ts';
import { BUILDING_DEFS } from './buildings_defs.ts';
import { effectiveDamage } from './combat.ts';

// Strategic phases a bot cycles through. Boot opens the game (peasants up); the
// rest are re-derived every decision tick from holdings/resources/threat, so a
// bot under attack snaps to Defend and a bot with a full economy + tech rolls
// into Siege/Assault. Numeric so it stores in a u8 column.
export const AiPhase = {
  Boot: 0, // opening: rush peasants to a working economy
  Economy: 1, // grow peasants / pop / resource coverage
  Expand: 2, // economy is healthy — add houses & support buildings
  Military: 3, // tech tree up, muster a mixed army
  Tech: 4, // push the building tech tree (Stable→Blacksmith→SiegeWorkshop)
  Siege: 5, // army formed, add siege to crack walls/keeps
  Assault: 6, // enough force on hand — march on the enemy
  Defend: 7, // under threat near home — towers + hold the army back
} as const;
export type AiPhase = (typeof AiPhase)[keyof typeof AiPhase];

export const AI_PHASE_LABELS: Record<AiPhase, string> = {
  [AiPhase.Boot]: 'Boot',
  [AiPhase.Economy]: 'Economy',
  [AiPhase.Expand]: 'Expand',
  [AiPhase.Military]: 'Military',
  [AiPhase.Tech]: 'Tech',
  [AiPhase.Siege]: 'Siege',
  [AiPhase.Assault]: 'Assault',
  [AiPhase.Defend]: 'Defend',
};

// A tally of enemy units by UnitKind. Sparse — only kinds the bot has actually
// seen appear. Keyed by the numeric UnitKind.
export type UnitCensus = Record<number, number>;

// The planner's view of one bot. Everything the pure functions need; the system
// reducer fills it from a single per-tick scan of the bot's holdings.
export interface PlannerState {
  // economy
  peasants: number;
  pop: number; // current population (units owned)
  cap: number; // population capacity from buildings
  food: number;
  wood: number;
  stone: number;
  gold: number;
  upkeep: number; // food eaten per economy tick by the current army
  // military
  soldiers: number; // combat units on hand (attack > 0)
  armyComposition: UnitCensus; // own combat units by kind (+ Imams)
  sieges: number;
  towers: number; // defensive towers already standing
  // holdings
  owned: ReadonlySet<BuildingKind>; // building kinds the bot already has
  // intel
  enemy: UnitCensus; // census of all visible enemy units by kind
  enemyHasWalls: boolean; // enemy fields Wall / Gatehouse — needs siege/anti-stone
  threatNearHome: number; // enemy combatants close to the bot's keep
}

// Tuning knobs the planner reads, supplied by the AiProfile per difficulty.
// Difficulty is decision QUALITY + cadence, never a resource/vision handicap.
export interface PlannerTuning {
  peasantTarget: number; // economy size before pushing military
  armyTarget: number; // soldiers to build toward
  coreArmy: number; // standing army to keep WHILE teching (defensive core)
  popBuffer: number; // free pop headroom to keep before building a House
  foodFloorMult: number; // bias to food while food <= upkeep * this
  woodBuffer: number; // wood reserve kept before optional (tower) spends
  maxTowers: number; // defensive towers near the keep
  wantsCavalry: boolean; // teches a Stable and fields cavalry
  wantsSiege: boolean; // teches Blacksmith→SiegeWorkshop and fields siege
  siegeTarget: number; // siege engines to build toward (0 = never)
  imamTarget: number; // support Imams to keep (0 = never)
  defendThreat: number; // enemy combatants near home that trigger Defend
  foodFloor: number; // food balance below which no MORE upkeep is added (crisis)
  reservePeasants: number; // extra gatherers to add during a food crisis
}

// Food crisis: the larder is at/under the floor while an army eats from it. Per
// the economy rule only combat units (and the Imam) draw upkeep, so during a
// crisis the planner must stop adding ANY food-eater — let the gatherers catch
// up — and instead pour effort into food (more peasants). Peasants themselves do
// not eat, so growing them is always safe and directly fixes the shortfall.
export function foodCrisis(s: PlannerState, tune: PlannerTuning): boolean {
  return s.upkeep > 0 && s.food <= tune.foodFloor;
}

// A trained kind that draws food upkeep — combat units eat, and the Imam eats as
// a fielded unit. Peasants and buildings do not. Used to gate spends in a crisis.
export function eatsFood(kind: number): boolean {
  return UNIT_DEFS[kind as UnitKind]?.attack > 0 || kind === UnitKind.Imam;
}

// ── unit-kind helpers ────────────────────────────────────────────────────────

// Combat units a bot can field, in rough tech order. Peasants/Imams excluded —
// the planner trains those on their own tracks. counterComposition scores over
// this set so it only ever recommends something trainable.
export const FIELD_UNITS: number[] = [
  UnitKind.Spearman,
  UnitKind.Archer,
  UnitKind.Crossbowman,
  UnitKind.Knight,
  UnitKind.HorseArcher,
  UnitKind.Mamluk,
  UnitKind.Mangonel,
  UnitKind.Ram,
];

// Building that must exist before a unit kind can be trained (its own training
// hall). Used so the planner only proposes units it can actually build.
function trainerFor(kind: number): BuildingKind {
  for (const k of [
    BuildingKind.Keep,
    BuildingKind.Barracks,
    BuildingKind.Stable,
    BuildingKind.SiegeWorkshop,
  ] as BuildingKind[]) {
    if (BUILDING_DEFS[k].trains.includes(kind)) return k;
  }
  return BuildingKind.Barracks;
}

// True when the bot owns the trainer AND any extra tech prereq for `kind`.
function canTrain(kind: number, owned: ReadonlySet<BuildingKind>): boolean {
  if (!owned.has(trainerFor(kind))) return false;
  const req = UNIT_DEFS[kind as UnitKind].requires;
  return req === undefined || owned.has(req);
}

// ── counter-composition ──────────────────────────────────────────────────────

// Score how well attacker kind `a` answers the enemy mix: sum, over each enemy
// kind, of (expected damage per second a deals to that kind) weighted by how many
// of that kind the enemy has. Uses the SAME effectiveDamage matrix the live
// combat loop uses, so the bot's counters track real battlefield outcomes. DPS,
// not per-hit, so a slow heavy hitter isn't overvalued vs a fast skirmisher.
export function counterScore(a: number, enemy: UnitCensus): number {
  const adef = UNIT_DEFS[a as UnitKind];
  if (!adef || adef.attack <= 0 || adef.attackRate <= 0) return 0;
  let score = 0;
  let total = 0;
  for (const key of Object.keys(enemy)) {
    const ek = Number(key);
    const n = enemy[ek];
    if (!n || n <= 0) continue;
    const edef = UNIT_DEFS[ek as UnitKind];
    if (!edef) continue;
    total += n;
    const dmg = effectiveDamage(
      { attack: adef.attack, damageType: adef.damageType, bonusVsArmor: adef.bonusVsArmor },
      edef.armorClass as ArmorClass
    );
    score += (dmg / adef.attackRate) * n;
  }
  return total === 0 ? 0 : score / total;
}

// Given the enemy unit tally, return the best trainable unit kind to add next.
// Falls back to a sensible default when the enemy is unseen (no intel): the bot
// opens with the cheapest reliable infantry it can build. Pure & deterministic —
// ties break toward the lower UnitKind so the same census always yields the same
// pick (no ctx.random here; the brain handles any RNG tie-breaks).
export function counterComposition(
  enemy: UnitCensus,
  owned: ReadonlySet<BuildingKind> = new Set([BuildingKind.Barracks]),
  opts: { wantsSiege?: boolean; enemyHasWalls?: boolean } = {}
): number {
  const trainable = FIELD_UNITS.filter((k) => canTrain(k, owned));
  if (trainable.length === 0) return UnitKind.Spearman;

  // Walls/gatehouses on the field: prefer the siege answer (it cracks Stone, the
  // only thing that does) when the bot can build it and is allowed to.
  if (opts.enemyHasWalls && opts.wantsSiege) {
    if (trainable.includes(UnitKind.Mangonel)) return UnitKind.Mangonel;
    if (trainable.includes(UnitKind.Ram)) return UnitKind.Ram;
  }

  const enemyTotal = Object.values(enemy).reduce((s, n) => s + (n > 0 ? n : 0), 0);
  // No intel yet — open with the cheapest reliable trainable infantry.
  if (enemyTotal === 0) {
    if (trainable.includes(UnitKind.Spearman)) return UnitKind.Spearman;
    if (trainable.includes(UnitKind.Archer)) return UnitKind.Archer;
    return trainable[0];
  }

  // Score every trainable unit against the enemy mix; pick the best DPS answer.
  // Exclude pure siege from the general counter (siege is for structures, gated
  // above) so it doesn't get picked just for being a big number vs soft units.
  let best = trainable[0];
  let bestScore = -Infinity;
  for (const k of trainable) {
    if (UNIT_DEFS[k as UnitKind].prefersBuildings) continue;
    const s = counterScore(k, enemy);
    if (s > bestScore + 1e-9) {
      bestScore = s;
      best = k;
    }
  }
  return best;
}

// ── phase machine ────────────────────────────────────────────────────────────

// Transition from the current phase given the live state. Phase is advisory —
// nextBuild re-derives the concrete action — but it drives cadence (e.g. Assault
// gating in the brain) and is surfaced for debugging/telemetry. Threat always
// wins: a bot with foes at the gate drops everything and Defends.
export function nextPhase(s: PlannerState, tune: PlannerTuning): AiPhase {
  if (s.threatNearHome >= tune.defendThreat) return AiPhase.Defend;

  const hasBarracks = s.owned.has(BuildingKind.Barracks);
  const economyReady = s.peasants >= tune.peasantTarget;
  const techComplete =
    (!tune.wantsCavalry || s.owned.has(BuildingKind.Stable)) &&
    (!tune.wantsSiege ||
      (s.owned.has(BuildingKind.Blacksmith) &&
        s.owned.has(BuildingKind.SiegeWorkshop)));

  // Opening: no military building yet and economy still ramping.
  if (!hasBarracks && !economyReady) return AiPhase.Boot;
  // Economy still short of target — keep growing peasants.
  if (!economyReady) return AiPhase.Economy;
  // Economy is set but no military building stands yet — pivoting from pure
  // economy into the war machine (first Barracks + pop/support buildings).
  if (!hasBarracks) return AiPhase.Expand;
  // Economy + a Barracks, but the tech tree isn't fully up yet.
  if (!techComplete) return AiPhase.Tech;
  // Army mustered to target and (if it teches siege) has siege — go on offense.
  if (s.soldiers >= tune.armyTarget) {
    if (tune.wantsSiege && s.sieges < tune.siegeTarget) return AiPhase.Siege;
    return AiPhase.Assault;
  }
  // Tech is up, still building the army.
  return AiPhase.Military;
}

// ── adaptive build order ─────────────────────────────────────────────────────

export interface BuildDecision {
  kind: number; // UnitKind when isUnit, else BuildingKind
  isUnit: boolean;
  trainer?: BuildingKind; // which building should train it (units only)
}

const HOUSE: BuildDecision = { kind: BuildingKind.House, isUnit: false };

// The single best macro action to take next, given holdings + intel + tuning.
// One action per call (the brain runs it once per decision window). Order of
// concern: don't starve → keep pop headroom → grow the economy to target → bring
// the building tech tree up in order → field a countering army → siege → optional
// defense. Every military branch is gated by the tech tree (won't propose a unit
// whose trainer/prereq the bot lacks, won't skip Blacksmith before SiegeWorkshop).
export function nextBuild(s: PlannerState, tune: PlannerTuning): BuildDecision | null {
  const has = (k: BuildingKind) => s.owned.has(k);
  const popHeadroom = s.cap - s.pop;
  const popFull = popHeadroom <= 0;

  // 0) Food crisis: the army is out-eating the larder. STOP adding food-eaters
  //    (every branch below that trains a combat unit / Imam is skipped while this
  //    holds) and instead add gatherers — peasants don't eat, so more of them
  //    directly closes the gap. A Granary near food helps bank it. If we can't
  //    grow gatherers (pop full), build a House so we can. This breaks the
  //    starve-spiral where a bot trains an army it cannot feed.
  if (foodCrisis(s, tune)) {
    if (s.peasants < tune.peasantTarget + tune.reservePeasants && !popFull) {
      return { kind: UnitKind.Peasant, isUnit: true, trainer: BuildingKind.Keep };
    }
    if (popFull) return HOUSE;
    if (has(BuildingKind.Keep) && !has(BuildingKind.Granary)) {
      return { kind: BuildingKind.Granary, isUnit: false };
    }
    return null; // hold — wait for food to recover before adding upkeep
  }

  // 1) Economy: peasants up to target while there's room. Peasants are the
  //    engine — keep building them through the early phases before military.
  if (s.peasants < tune.peasantTarget && !popFull) {
    return { kind: UnitKind.Peasant, isUnit: true, trainer: BuildingKind.Keep };
  }

  // 2) Pop headroom: a House when we're about to cap out (so training never
  //    stalls). Checked after the economy-peasant push so we don't over-house.
  if (popHeadroom <= tune.popBuffer) return HOUSE;

  // 3) Tech tree, strictly in order. Each gate is the shared `requires` chain:
  //    Barracks (free) → Stable / Blacksmith (need Barracks) → SiegeWorkshop
  //    (needs Blacksmith). Never propose a step whose prereq is missing. The
  //    Barracks always comes first — the bot must not be defenceless.
  if (!has(BuildingKind.Barracks)) {
    return { kind: BuildingKind.Barracks, isUnit: false };
  }

  // 3a) Defensive core: keep a small standing army WHILE teching the rest of the
  //     tree, so a bot saving for the (expensive) Stable/Blacksmith/SiegeWorkshop
  //     is never left army-less. Once the core is met, teching resumes and the
  //     full army is built afterward (steps 7+). Threatened bots prioritise this
  //     core over teching.
  const techComplete =
    (!tune.wantsCavalry || has(BuildingKind.Stable)) &&
    (!tune.wantsSiege ||
      (has(BuildingKind.Blacksmith) && has(BuildingKind.SiegeWorkshop)));
  if (!techComplete && s.soldiers < tune.coreArmy && !popFull) {
    const kind = counterComposition(s.enemy, s.owned, {
      wantsSiege: tune.wantsSiege,
      enemyHasWalls: s.enemyHasWalls,
    });
    return { kind, isUnit: true, trainer: trainerFor(kind) };
  }

  if (tune.wantsCavalry && !has(BuildingKind.Stable)) {
    return { kind: BuildingKind.Stable, isUnit: false };
  }
  if (tune.wantsSiege && !has(BuildingKind.Blacksmith)) {
    return { kind: BuildingKind.Blacksmith, isUnit: false };
  }
  if (
    tune.wantsSiege &&
    has(BuildingKind.Blacksmith) &&
    !has(BuildingKind.SiegeWorkshop)
  ) {
    return { kind: BuildingKind.SiegeWorkshop, isUnit: false };
  }

  // 4) Defense: under threat near home, throw up towers up to the cap.
  if (s.threatNearHome >= tune.defendThreat && countTowersBelowCap(s, tune)) {
    return { kind: BuildingKind.Tower, isUnit: false };
  }

  if (popFull) return HOUSE; // army wants to grow but pop is capped

  // 5) Support: fold in an Imam once an army is forming, to steady morale.
  if (
    tune.imamTarget > 0 &&
    s.soldiers >= 2 &&
    countOwnKind(s.armyComposition, UnitKind.Imam) < tune.imamTarget
  ) {
    return { kind: UnitKind.Imam, isUnit: true, trainer: BuildingKind.Keep };
  }

  // 6) Siege: once the workshop is up and the army has a core, build toward the
  //    siege target — especially if the enemy walls up.
  if (
    tune.wantsSiege &&
    has(BuildingKind.SiegeWorkshop) &&
    s.sieges < tune.siegeTarget &&
    (s.soldiers >= 2 || s.enemyHasWalls)
  ) {
    const siege =
      s.enemyHasWalls && canTrain(UnitKind.Mangonel, s.owned)
        ? UnitKind.Mangonel
        : canTrain(UnitKind.Ram, s.owned)
          ? UnitKind.Ram
          : UnitKind.Mangonel;
    return { kind: siege, isUnit: true, trainer: BuildingKind.SiegeWorkshop };
  }

  // 7) Army: field the best counter to the enemy mix, up to the army target.
  if (s.soldiers < tune.armyTarget) {
    const kind = counterComposition(s.enemy, s.owned, {
      wantsSiege: tune.wantsSiege,
      enemyHasWalls: s.enemyHasWalls,
    });
    return { kind, isUnit: true, trainer: trainerFor(kind) };
  }

  // 8) Otherwise top up defensive towers with spare wood (the brain enforces the
  //    wood buffer before spending).
  if (countTowersBelowCap(s, tune)) {
    return { kind: BuildingKind.Tower, isUnit: false };
  }

  return null;
}

function countTowersBelowCap(s: PlannerState, tune: PlannerTuning): boolean {
  return s.towers < tune.maxTowers;
}

export function countOwnKind(census: UnitCensus, kind: number): number {
  return census[kind] ?? 0;
}
