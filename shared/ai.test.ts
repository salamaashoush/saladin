import { describe, it, expect } from 'vitest';
import {
  AiPhase,
  counterComposition,
  counterScore,
  nextPhase,
  nextBuild,
  foodCrisis,
  eatsFood,
  FIELD_UNITS,
  type PlannerState,
  type PlannerTuning,
  type UnitCensus,
} from './ai.ts';
import { plannerTuning, AI_PROFILES } from './defs.ts';
import { UnitKind, BuildingKind } from './enums.ts';
import { UNIT_DEFS } from './units.ts';

// ── fixtures ─────────────────────────────────────────────────────────────────

const owned = (...kinds: BuildingKind[]) => new Set<BuildingKind>(kinds);

// A full economy + barracks + stable + blacksmith + siege workshop, so the
// tech-tree gates inside the planner are all open unless a test narrows them.
const FULL_TECH = owned(
  BuildingKind.Keep,
  BuildingKind.Barracks,
  BuildingKind.Stable,
  BuildingKind.Blacksmith,
  BuildingKind.SiegeWorkshop
);

const tune = (over: Partial<PlannerTuning> = {}): PlannerTuning => ({
  ...plannerTuning(AI_PROFILES[2]), // start from Hard, then override
  ...over,
});

const state = (over: Partial<PlannerState> = {}): PlannerState => ({
  peasants: 10,
  pop: 12,
  cap: 30,
  food: 500,
  wood: 500,
  stone: 500,
  gold: 500,
  upkeep: 4,
  soldiers: 0,
  armyComposition: {},
  sieges: 0,
  towers: 0,
  owned: FULL_TECH,
  enemy: {},
  enemyHasWalls: false,
  threatNearHome: 0,
  ...over,
});

const heavy = (kind: number, n: number): UnitCensus => ({ [kind]: n });

const BARRACKS_UNITS = [
  UnitKind.Spearman,
  UnitKind.Archer,
  UnitKind.Crossbowman,
];

// ── counterScore ─────────────────────────────────────────────────────────────

describe('counterScore', () => {
  it('is zero for a non-combatant attacker (Peasant/Imam)', () => {
    expect(counterScore(UnitKind.Peasant, heavy(UnitKind.Spearman, 5))).toBe(0);
    expect(counterScore(UnitKind.Imam, heavy(UnitKind.Knight, 5))).toBe(0);
  });

  it('is zero against an empty enemy census', () => {
    expect(counterScore(UnitKind.Spearman, {})).toBe(0);
  });

  it('rates the spearman higher against mailed knights than the slashing knight', () => {
    // Knights are Mail; Slash glances mail (0.6) while the spearman has a bonus
    // vs Mail. The anti-mail unit should out-score the mirror.
    const enemy = heavy(UnitKind.Knight, 6);
    expect(counterScore(UnitKind.Spearman, enemy)).toBeGreaterThan(
      counterScore(UnitKind.Knight, enemy)
    );
  });

  it('rates cavalry highly against leather-armored archers', () => {
    const enemy = heavy(UnitKind.Archer, 6);
    // Mamluk (slash + bonus vs leather) beats the spearman vs archers.
    expect(counterScore(UnitKind.Mamluk, enemy)).toBeGreaterThan(
      counterScore(UnitKind.Spearman, enemy)
    );
  });
});

// ── counterComposition ───────────────────────────────────────────────────────

describe('counterComposition', () => {
  it('answers a knight-heavy enemy with an anti-mail unit (spear/crossbow)', () => {
    const pick = counterComposition(heavy(UnitKind.Knight, 8), FULL_TECH, {
      wantsSiege: true,
    });
    expect([UnitKind.Spearman, UnitKind.Crossbowman]).toContain(pick);
    // whatever it picks must carry an explicit mail bonus or punch mail well
    const def = UNIT_DEFS[pick as UnitKind];
    expect(def.bonusVsArmor?.[2 /* Mail */] ?? 0).toBeGreaterThan(1);
  });

  it('answers a Mamluk (mail) heavy enemy with an anti-mail unit too', () => {
    const pick = counterComposition(heavy(UnitKind.Mamluk, 8), FULL_TECH, {
      wantsSiege: true,
    });
    expect([UnitKind.Spearman, UnitKind.Crossbowman]).toContain(pick);
  });

  it('answers an archer-heavy enemy with fast cavalry', () => {
    const pick = counterComposition(heavy(UnitKind.Archer, 8), FULL_TECH, {
      wantsSiege: true,
    });
    expect([UnitKind.Knight, UnitKind.Mamluk, UnitKind.HorseArcher]).toContain(
      pick
    );
  });

  it('answers an infantry (spearman) heavy enemy with a counter, not a mirror', () => {
    const pick = counterComposition(heavy(UnitKind.Spearman, 8), FULL_TECH, {
      wantsSiege: true,
    });
    // Spearmen are leather — slash/cav punish them; never a pure-siege pick.
    expect(UNIT_DEFS[pick as UnitKind].prefersBuildings).toBeFalsy();
    expect(pick).not.toBe(UnitKind.Peasant);
  });

  it('picks siege when the enemy has walls and siege is allowed', () => {
    const pick = counterComposition(heavy(UnitKind.Spearman, 4), FULL_TECH, {
      wantsSiege: true,
      enemyHasWalls: true,
    });
    expect([UnitKind.Mangonel, UnitKind.Ram]).toContain(pick);
  });

  it('does NOT pick siege for walls when siege is disabled', () => {
    const pick = counterComposition(heavy(UnitKind.Spearman, 4), FULL_TECH, {
      wantsSiege: false,
      enemyHasWalls: true,
    });
    expect([UnitKind.Mangonel, UnitKind.Ram]).not.toContain(pick);
  });

  it('opens with reliable infantry when there is no intel', () => {
    const pick = counterComposition({}, owned(BuildingKind.Barracks));
    expect([UnitKind.Spearman, UnitKind.Archer]).toContain(pick);
  });

  it('only ever recommends a unit the bot can actually train', () => {
    // Barracks only: must not propose cavalry/siege.
    const justBarracks = owned(BuildingKind.Barracks);
    for (const enemyKind of FIELD_UNITS) {
      const pick = counterComposition(heavy(enemyKind, 6), justBarracks, {
        wantsSiege: true,
        enemyHasWalls: true,
      });
      expect(BARRACKS_UNITS).toContain(pick);
    }
  });

  it('is deterministic — same census yields the same pick every call', () => {
    const c = heavy(UnitKind.Knight, 5);
    const a = counterComposition(c, FULL_TECH, { wantsSiege: true });
    const b = counterComposition(c, FULL_TECH, { wantsSiege: true });
    expect(a).toBe(b);
  });
});

// ── nextPhase ────────────────────────────────────────────────────────────────

describe('nextPhase', () => {
  it('opens in Boot with no military building and a thin economy', () => {
    const s = state({ peasants: 2, owned: owned(BuildingKind.Keep) });
    expect(nextPhase(s, tune())).toBe(AiPhase.Boot);
  });

  it('is Economy once a barracks exists but peasants are below target', () => {
    const s = state({
      peasants: 4,
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks),
    });
    expect(nextPhase(s, tune({ peasantTarget: 10 }))).toBe(AiPhase.Economy);
  });

  it('is Expand when the economy is ready but no Barracks stands yet', () => {
    const s = state({ peasants: 10, owned: owned(BuildingKind.Keep) });
    expect(nextPhase(s, tune({ peasantTarget: 10 }))).toBe(AiPhase.Expand);
  });

  it('is Tech when the economy is set but the tree is incomplete', () => {
    const s = state({
      peasants: 10,
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks), // no stable/siege
    });
    expect(nextPhase(s, tune({ peasantTarget: 10 }))).toBe(AiPhase.Tech);
  });

  it('is Military when tech is up but the army is still small', () => {
    const s = state({ peasants: 10, soldiers: 2 });
    expect(nextPhase(s, tune({ peasantTarget: 10, armyTarget: 14 }))).toBe(
      AiPhase.Military
    );
  });

  it('is Siege when the army is at target but siege is not yet fielded', () => {
    const s = state({ peasants: 10, soldiers: 14, sieges: 0 });
    expect(
      nextPhase(s, tune({ peasantTarget: 10, armyTarget: 14, siegeTarget: 2 }))
    ).toBe(AiPhase.Siege);
  });

  it('is Assault when the army and siege are both ready', () => {
    const s = state({ peasants: 10, soldiers: 14, sieges: 2 });
    expect(
      nextPhase(s, tune({ peasantTarget: 10, armyTarget: 14, siegeTarget: 2 }))
    ).toBe(AiPhase.Assault);
  });

  it('drops everything to Defend when foes are at the gate', () => {
    const s = state({
      peasants: 10,
      soldiers: 14,
      sieges: 2,
      threatNearHome: 5,
    });
    expect(nextPhase(s, tune({ defendThreat: 3 }))).toBe(AiPhase.Defend);
  });

  it('an Easy bot (no cavalry/siege) reaches Assault without teching them', () => {
    const easy = tune({
      ...plannerTuning(AI_PROFILES[0]),
    });
    const s = state({
      peasants: 6,
      soldiers: 6,
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks),
    });
    expect(nextPhase(s, easy)).toBe(AiPhase.Assault);
  });
});

// ── nextBuild ────────────────────────────────────────────────────────────────

describe('nextBuild — economy first', () => {
  it('builds peasants up to target before anything else', () => {
    const s = state({ peasants: 3, owned: owned(BuildingKind.Keep) });
    const plan = nextBuild(s, tune({ peasantTarget: 10 }))!;
    expect(plan).toMatchObject({ kind: UnitKind.Peasant, isUnit: true });
  });

  it('builds a House when pop headroom drops to the buffer', () => {
    const s = state({ peasants: 10, pop: 28, cap: 30 });
    const plan = nextBuild(s, tune({ peasantTarget: 10, popBuffer: 3 }))!;
    expect(plan).toMatchObject({ kind: BuildingKind.House, isUnit: false });
  });
});

describe('nextBuild — respects the tech tree', () => {
  it('builds a Barracks first when none exists', () => {
    const s = state({ peasants: 10, owned: owned(BuildingKind.Keep) });
    const plan = nextBuild(s, tune({ peasantTarget: 10 }))!;
    expect(plan).toMatchObject({ kind: BuildingKind.Barracks });
  });

  it('teches Stable (cavalry) after the Barracks (core army already met)', () => {
    const s = state({
      peasants: 10,
      soldiers: 5,
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks),
    });
    const plan = nextBuild(
      s,
      tune({
        peasantTarget: 10,
        coreArmy: 0,
        wantsCavalry: true,
        wantsSiege: false,
      })
    )!;
    expect(plan).toMatchObject({ kind: BuildingKind.Stable });
  });

  it('teches Blacksmith before the Siege Workshop (never skips a tier)', () => {
    const s = state({
      peasants: 10,
      soldiers: 5,
      owned: owned(
        BuildingKind.Keep,
        BuildingKind.Barracks,
        BuildingKind.Stable
      ),
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, coreArmy: 0, wantsSiege: true })
    )!;
    // must be the Blacksmith, NOT the SiegeWorkshop (its prereq isn't met)
    expect(plan.kind).toBe(BuildingKind.Blacksmith);
    expect(plan.kind).not.toBe(BuildingKind.SiegeWorkshop);
  });

  it('only goes for the Siege Workshop once the Blacksmith stands', () => {
    const s = state({
      peasants: 10,
      soldiers: 5,
      owned: owned(
        BuildingKind.Keep,
        BuildingKind.Barracks,
        BuildingKind.Stable,
        BuildingKind.Blacksmith
      ),
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, coreArmy: 0, wantsSiege: true })
    )!;
    expect(plan.kind).toBe(BuildingKind.SiegeWorkshop);
  });

  it('keeps a defensive core army while still teching the tree', () => {
    // Barracks up, Stable not yet, core army not met → trains a soldier (the
    // counter), NOT the next tech building, so the bot is never defenceless.
    const s = state({
      peasants: 10,
      soldiers: 1,
      enemy: heavy(UnitKind.Archer, 4),
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks),
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, coreArmy: 4, wantsCavalry: true, imamTarget: 0 })
    )!;
    expect(plan.isUnit).toBe(true);
    expect(plan.kind).not.toBe(BuildingKind.Stable);
  });

  it('resumes teching once the defensive core is met', () => {
    const s = state({
      peasants: 10,
      soldiers: 4, // core met
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks),
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, coreArmy: 4, wantsCavalry: true })
    )!;
    expect(plan.kind).toBe(BuildingKind.Stable);
  });

  it('an Easy bot never proposes a Stable, Blacksmith or Siege Workshop', () => {
    const easy = tune({ ...plannerTuning(AI_PROFILES[0]) });
    // walk the build order: it should go Barracks then straight to army.
    const s = state({
      peasants: 6,
      soldiers: 0,
      owned: owned(BuildingKind.Keep, BuildingKind.Barracks),
    });
    const plan = nextBuild(s, easy)!;
    expect([
      BuildingKind.Stable,
      BuildingKind.Blacksmith,
      BuildingKind.SiegeWorkshop,
    ]).not.toContain(plan.kind);
  });
});

describe('nextBuild — army & counters', () => {
  it('trains the counter to the enemy mix once tech is up', () => {
    const s = state({
      peasants: 10,
      soldiers: 4,
      enemy: heavy(UnitKind.Knight, 8),
    });
    // imam/siege targets at 0 so the planner falls through to the army branch.
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, armyTarget: 14, imamTarget: 0, siegeTarget: 0 })
    )!;
    expect(plan.isUnit).toBe(true);
    expect([UnitKind.Spearman, UnitKind.Crossbowman]).toContain(plan.kind);
    expect(plan.trainer).toBe(BuildingKind.Barracks);
  });

  it('fields a support Imam once a small army forms', () => {
    const s = state({
      peasants: 10,
      soldiers: 3,
      armyComposition: { [UnitKind.Spearman]: 3 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, imamTarget: 1, siegeTarget: 0 })
    )!;
    expect(plan).toMatchObject({
      kind: UnitKind.Imam,
      isUnit: true,
      trainer: BuildingKind.Keep,
    });
  });

  it('builds siege toward the target once an army core exists', () => {
    const s = state({
      peasants: 10,
      soldiers: 6,
      sieges: 0,
      armyComposition: { [UnitKind.Spearman]: 6, [UnitKind.Imam]: 1 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, wantsSiege: true, siegeTarget: 2, imamTarget: 1 })
    )!;
    expect(plan.isUnit).toBe(true);
    expect([UnitKind.Ram, UnitKind.Mangonel]).toContain(plan.kind);
    expect(plan.trainer).toBe(BuildingKind.SiegeWorkshop);
  });

  it('prefers a Mangonel over a Ram when the enemy walls up', () => {
    const s = state({
      peasants: 10,
      soldiers: 6,
      sieges: 0,
      enemyHasWalls: true,
      armyComposition: { [UnitKind.Spearman]: 6 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, wantsSiege: true, siegeTarget: 2, imamTarget: 0 })
    )!;
    expect(plan.kind).toBe(UnitKind.Mangonel);
  });
});

describe('nextBuild — defense', () => {
  it('throws up a tower when threatened and below the tower cap', () => {
    const s = state({
      peasants: 10,
      soldiers: 5,
      towers: 0,
      threatNearHome: 5,
    });
    const plan = nextBuild(s, tune({ peasantTarget: 10, defendThreat: 3, maxTowers: 3 }))!;
    expect(plan).toMatchObject({ kind: BuildingKind.Tower, isUnit: false });
  });

  it('stops building towers once the cap is reached', () => {
    const s = state({
      peasants: 10,
      soldiers: 5,
      towers: 3,
      threatNearHome: 5,
      armyComposition: { [UnitKind.Spearman]: 5 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, defendThreat: 3, maxTowers: 3, imamTarget: 0, siegeTarget: 0 })
    );
    // not a tower — should fall through to army/other
    expect(plan?.kind).not.toBe(BuildingKind.Tower);
  });
});

describe('foodCrisis & eatsFood', () => {
  it('eatsFood is true for combat units and the Imam, false for peasants/buildings', () => {
    expect(eatsFood(UnitKind.Spearman)).toBe(true);
    expect(eatsFood(UnitKind.Knight)).toBe(true);
    expect(eatsFood(UnitKind.Imam)).toBe(true);
    expect(eatsFood(UnitKind.Peasant)).toBe(false);
  });

  it('is a crisis only when an army eats and food is at/under the floor', () => {
    const t = tune({ foodFloor: 20 });
    expect(foodCrisis(state({ upkeep: 5, food: 10 }), t)).toBe(true);
    expect(foodCrisis(state({ upkeep: 5, food: 200 }), t)).toBe(false);
    // no army → no crisis even at zero food
    expect(foodCrisis(state({ upkeep: 0, food: 0 }), t)).toBe(false);
  });
});

describe('nextBuild — food crisis breaks the starve-spiral', () => {
  it('adds gatherers (peasants) instead of more army when starving', () => {
    const s = state({
      peasants: 10,
      soldiers: 5,
      upkeep: 5,
      food: 5, // below floor
      armyComposition: { [UnitKind.Spearman]: 5 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, foodFloor: 20, reservePeasants: 4 })
    )!;
    expect(plan).toMatchObject({ kind: UnitKind.Peasant, isUnit: true });
  });

  it('never trains a food-eating unit while in a food crisis', () => {
    // reserve peasants already met + pop full → it must NOT fall through to army.
    const s = state({
      peasants: 14, // target 10 + reserve 4
      pop: 30,
      cap: 30, // pop full
      soldiers: 5,
      upkeep: 5,
      food: 0,
      armyComposition: { [UnitKind.Spearman]: 5 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, foodFloor: 20, reservePeasants: 4 })
    );
    // pop full in a crisis → a House (to make room for gatherers), never a soldier
    if (plan?.isUnit) expect(eatsFood(plan.kind)).toBe(false);
    expect(plan?.kind).toBe(BuildingKind.House);
  });

  it('resumes the normal build order once food recovers', () => {
    const s = state({
      peasants: 10,
      soldiers: 4,
      upkeep: 4,
      food: 300, // healthy
      enemy: heavy(UnitKind.Knight, 6),
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, foodFloor: 20, imamTarget: 0, siegeTarget: 0 })
    )!;
    expect(plan.isUnit).toBe(true);
    expect(eatsFood(plan.kind)).toBe(true); // back to building the army
  });
});

describe('nextBuild — terminal', () => {
  it('returns null when the army is full and towers are capped', () => {
    const s = state({
      peasants: 10,
      soldiers: 14,
      towers: 3,
      sieges: 2,
      armyComposition: { [UnitKind.Spearman]: 14, [UnitKind.Imam]: 1 },
    });
    const plan = nextBuild(
      s,
      tune({ peasantTarget: 10, armyTarget: 14, maxTowers: 3, siegeTarget: 2, imamTarget: 1 })
    );
    expect(plan).toBeNull();
  });
});
