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
  SquadRole,
  squadRole,
  targetForRole,
  raidQuota,
  mustered,
  shouldRecall,
  recallCount,
  type PlannerState,
  type PlannerTuning,
  type UnitCensus,
  type TacticalTuning,
  type AssaultIntel,
  type TacticalTarget,
  type ThreatState,
} from './ai.ts';
import { plannerTuning, tacticalTuning, AI_PROFILES } from './defs.ts';
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
    // Economy at target and army at target — Easy teches neither cavalry nor siege,
    // so a full economy + Barracks + army should go straight to Assault.
    const s = state({
      peasants: easy.peasantTarget,
      soldiers: easy.armyTarget,
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

// ── tactical: squad roles ─────────────────────────────────────────────────────

describe('squadRole', () => {
  it('classifies siege engines (prefersBuildings) as Siege', () => {
    expect(squadRole(UnitKind.Ram)).toBe(SquadRole.Siege);
    expect(squadRole(UnitKind.Mangonel)).toBe(SquadRole.Siege);
  });

  it('classifies light fast cavalry as Raiders', () => {
    // HorseArcher: speed 4.0, hp 60 — a textbook raider.
    expect(squadRole(UnitKind.HorseArcher)).toBe(SquadRole.Raider);
  });

  it('keeps heavy cavalry in the Main body even though they are fast', () => {
    // Knight (130hp) and Mamluk (150hp) are fast but too heavy to raid — they
    // belong in the line, so the assault never loses its punch.
    expect(squadRole(UnitKind.Knight)).toBe(SquadRole.Main);
    expect(squadRole(UnitKind.Mamluk)).toBe(SquadRole.Main);
  });

  it('classifies foot troops and support as Main', () => {
    expect(squadRole(UnitKind.Spearman)).toBe(SquadRole.Main);
    expect(squadRole(UnitKind.Archer)).toBe(SquadRole.Main);
    expect(squadRole(UnitKind.Crossbowman)).toBe(SquadRole.Main);
    expect(squadRole(UnitKind.Imam)).toBe(SquadRole.Main); // tags along with the line
  });
});

// ── tactical: target selection per role ───────────────────────────────────────

const tgt = (id: number, x: number, y: number): TacticalTarget => ({
  id: BigInt(id),
  x,
  y,
});

// A full picture: a keep at (100,100), a wall and a tower guarding it, and two
// enemy peasants gathering off to the side.
const intel = (over: Partial<AssaultIntel> = {}): AssaultIntel => {
  const keep = tgt(1, 100, 100);
  const wall = tgt(2, 90, 95);
  const tower = tgt(3, 95, 90);
  const gatherer1 = tgt(4, 20, 20);
  const gatherer2 = tgt(5, 30, 25);
  return {
    keep,
    defenses: [wall, tower, keep],
    buildings: [keep, wall, tower],
    gatherers: [gatherer1, gatherer2],
    ...over,
  };
};

describe('targetForRole', () => {
  it('sends the MAIN body at the enemy keep', () => {
    const t = targetForRole(SquadRole.Main, 0, 0, intel());
    expect(t?.id).toBe(intel().keep!.id);
  });

  it('sends SIEGE at the nearest defensive structure (wall/tower), not the keep', () => {
    // Standing near the wall, siege should pick the wall over the distant keep.
    const t = targetForRole(SquadRole.Siege, 89, 96, intel());
    expect([intel().defenses[0].id, intel().defenses[1].id]).toContain(t?.id);
  });

  it('SIEGE focuses a building even when no dedicated defenses stand', () => {
    const noDefenses = intel({ defenses: [] });
    const t = targetForRole(SquadRole.Siege, 0, 0, noDefenses);
    // falls back to the nearest building (which includes the keep)
    expect(noDefenses.buildings.map((b) => b.id)).toContain(t?.id);
  });

  it('sends RAIDERS at the nearest enemy gatherer (the economy)', () => {
    const t = targetForRole(SquadRole.Raider, 18, 18, intel());
    expect(t?.id).toBe(intel().gatherers[0].id); // the closer of the two
  });

  it('returns null for a raider when there are no gatherers (rejoin the body)', () => {
    const t = targetForRole(SquadRole.Raider, 0, 0, intel({ gatherers: [] }));
    expect(t).toBeNull();
  });

  it('MAIN falls back to other buildings once the keep is gone', () => {
    const noKeep = intel({ keep: null });
    const t = targetForRole(SquadRole.Main, 91, 94, noKeep);
    expect(noKeep.buildings.map((b) => b.id)).toContain(t?.id);
  });

  it('is deterministic — nearest wins by squared distance', () => {
    const a = targetForRole(SquadRole.Raider, 31, 26, intel());
    const b = targetForRole(SquadRole.Raider, 31, 26, intel());
    expect(a?.id).toBe(b?.id);
    expect(a?.id).toBe(intel().gatherers[1].id); // gatherer2 at (30,25) is nearer
  });
});

// ── tactical: raid quota ──────────────────────────────────────────────────────

describe('raidQuota', () => {
  it('peels off a fraction of the raiders, at least one when raiding', () => {
    expect(raidQuota(4, 0.34)).toBe(1); // floor(4*0.34)=1
    expect(raidQuota(8, 0.34)).toBe(2);
    expect(raidQuota(1, 0.34)).toBe(1); // at least one
  });

  it('never raids when the profile disables it or no raiders exist', () => {
    expect(raidQuota(8, 0)).toBe(0);
    expect(raidQuota(0, 0.5)).toBe(0);
  });

  it('never sends more raiders than exist', () => {
    expect(raidQuota(2, 1.0)).toBe(2);
  });
});

// ── tactical: muster-before-assault ───────────────────────────────────────────

describe('mustered', () => {
  it('waits until the army reaches waveSize before committing', () => {
    expect(mustered(3, 8)).toBe(false);
    expect(mustered(7, 8)).toBe(false);
    expect(mustered(8, 8)).toBe(true);
    expect(mustered(12, 8)).toBe(true);
  });
});

// ── tactical: defensive recall ────────────────────────────────────────────────

const tac = (over: Partial<TacticalTuning> = {}): TacticalTuning => ({
  ...tacticalTuning(AI_PROFILES[2]), // Hard baseline
  ...over,
});

const threat = (over: Partial<ThreatState> = {}): ThreatState => ({
  attackers: 0,
  fieldArmy: 0,
  homeArmy: 0,
  ...over,
});

describe('shouldRecall', () => {
  it('does not recall when no enemies are at home', () => {
    expect(
      shouldRecall(threat({ attackers: 0, fieldArmy: 10 }), tac({ defendThreat: 3 }))
    ).toBe(false);
  });

  it('does not recall when there is no field army to bring back', () => {
    expect(
      shouldRecall(threat({ attackers: 6, fieldArmy: 0, homeArmy: 2 }), tac())
    ).toBe(false);
  });

  it('recalls when attackers at home outweigh the home defenders', () => {
    expect(
      shouldRecall(
        threat({ attackers: 6, fieldArmy: 10, homeArmy: 1 }),
        tac({ defendThreat: 3, recallMargin: 0 })
      )
    ).toBe(true);
  });

  it('does NOT recall when the home garrison already covers the attack', () => {
    // 4 attackers but 4 already home (+ margin) — keep pressing the assault.
    expect(
      shouldRecall(
        threat({ attackers: 4, fieldArmy: 10, homeArmy: 4 }),
        tac({ defendThreat: 3, recallMargin: 1 })
      )
    ).toBe(false);
  });

  it('an Easy bot tolerates a bigger imbalance before recalling (bluntness)', () => {
    const easy = tacticalTuning(AI_PROFILES[0]);
    const hard = tacticalTuning(AI_PROFILES[2]);
    // a 4v3 raid: Hard (margin 0) recalls, Easy (margin 2) shrugs it off.
    const t = threat({ attackers: 4, fieldArmy: 10, homeArmy: 3 });
    expect(shouldRecall(t, hard)).toBe(true);
    expect(shouldRecall(t, easy)).toBe(false);
  });
});

describe('recallCount', () => {
  it('is zero when no recall is warranted', () => {
    expect(recallCount(threat({ attackers: 0, fieldArmy: 10 }), tac())).toBe(0);
  });

  it('brings back enough to match the attack, over what is already home', () => {
    // 6 attackers, 2 home → need 4 more; field army of 20 with fraction 0.6 caps
    // at 12, so 4 come back.
    const n = recallCount(
      threat({ attackers: 6, fieldArmy: 20, homeArmy: 2 }),
      tac({ defendThreat: 3, recallMargin: 0, recallFraction: 0.6 })
    );
    expect(n).toBe(4);
  });

  it('never pulls more than the recall fraction of the field army', () => {
    // huge attack but small field army: fraction 0.5 of 6 = 3 max.
    const n = recallCount(
      threat({ attackers: 99, fieldArmy: 6, homeArmy: 0 }),
      tac({ defendThreat: 3, recallMargin: 0, recallFraction: 0.5 })
    );
    expect(n).toBe(3);
  });

  it('recalls at least one when it recalls at all', () => {
    const n = recallCount(
      threat({ attackers: 4, fieldArmy: 1, homeArmy: 3 }),
      tac({ defendThreat: 3, recallMargin: 0, recallFraction: 0.6 })
    );
    expect(n).toBe(1);
  });
});

// ── tactical: difficulty shapes behaviour, not resources ──────────────────────

describe('tactical difficulty profiles', () => {
  it('Easy never raids or scouts; Hard does both', () => {
    const easy = tacticalTuning(AI_PROFILES[0]);
    const hard = tacticalTuning(AI_PROFILES[2]);
    expect(easy.raidFraction).toBe(0);
    expect(easy.scouts).toBe(false);
    expect(hard.raidFraction).toBeGreaterThan(0);
    expect(hard.scouts).toBe(true);
  });

  it('Hard reacts faster to a home threat than Easy', () => {
    expect(tacticalTuning(AI_PROFILES[2]).defendReactDelay).toBeLessThan(
      tacticalTuning(AI_PROFILES[0]).defendReactDelay
    );
  });

  it('Hard recalls a larger share of the field army than Easy', () => {
    expect(tacticalTuning(AI_PROFILES[2]).recallFraction).toBeGreaterThan(
      tacticalTuning(AI_PROFILES[0]).recallFraction
    );
  });
});
