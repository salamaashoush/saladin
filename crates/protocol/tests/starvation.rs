//! THE BAGGAGE TRAIN — supply as a question of WHERE, not of how many.
//!
//! Two rules were tried here and both failed the same way. `bill = men *
//! FOOD_PER_UNIT` with an all-or-nothing failure executed armies for being one
//! loaf short. The same bill at a quarter of the rate stopped mattering at all:
//! measured, ten soldiers drawing 1.25 food/s against an 1868 stockpile. A flat
//! per-head drain on a stock has no band between crushing and irrelevant.
//!
//! What is asserted here is the model that replaced it. A garrison in reach of
//! its own stores draws NOTHING — that is the headline, and everything else
//! follows from it. Every tile past the supply radius costs more; a column that
//! outruns its stores is rationed proportionally, tires, loses heart and finally
//! walks away; a herd under its feet buys it a march; and A FORWARD STORE ENDS
//! THE FAMINE, which is the decision the whole model exists to offer.
//!
//! Army SIZE is limited by the pop cap and by what a soldier costs to raise
//! (three quarters of it bread). Supply limits DEPTH AND DURATION. Those are
//! different questions and this file only tests the second one.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, FIELD_RATION, Faction, Fx, MAX_STRAIN, MORALE_MAX, ROUT_THRESHOLD, ResourceType,
    SUPPLY_RADIUS, Stance, Stockpile, UnitKind, V2, ZERO, building_def, is_passable, strain,
    unit_def,
};

const SEED: u32 = 1;
/// Economy runs every 40 ticks; one tick past it is the sampling point.
const ECON: u32 = 40;

/// Home, and a march that is unambiguously past the end of the supply line —
/// far enough that every man in the column is at `MAX_STRAIN`.
const HOME: (&str, &str) = ("60", "60");
const AFIELD_X: &str = "260";

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: SEED });
    app
}

fn spawn_player(app: &mut App, id: u64, food: i32) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 0, stone: 0, food, gold: 0 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
}

fn spawn_store(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, building_def(kind).max_hp, pos),
    ));
}

fn spawn_keep(app: &mut App, id: u64, owner: u64, pos: V2) {
    spawn_store(app, id, owner, BuildingKind::Keep, pos);
}

fn spawn_unit(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    let def = unit_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            carry_type: ResourceType::Food,
            hp: def.max_hp,
            stance: Stance::HoldGround,
            ..Unit::new(kind, pos)
        },
    ));
}

/// A company standing in a row, ids `base..base+n`.
fn spawn_company(app: &mut App, base: u64, owner: u64, kind: UnitKind, at: V2, n: u64) {
    for i in 0..n {
        let p = V2::new(at.x + Fx::from_num(i as i32), at.y);
        spawn_unit(app, base + i, owner, kind, p);
    }
}

fn set_food(app: &mut App, owner: u64, food: i32) {
    let world = app.world_mut();
    let mut q = world.query::<&mut Player>();
    for mut p in q.iter_mut(world) {
        if p.player_id == owner {
            p.stock.food = food;
        }
    }
}

fn units_of(app: &mut App, owner: u64) -> Vec<Unit> {
    let world = app.world_mut();
    let mut q = world.query::<(&Owner, &Unit)>();
    q.iter(world).filter(|(o, _)| o.0 == owner).map(|(_, u)| u.clone()).collect()
}

fn alive(app: &mut App, owner: u64) -> usize {
    units_of(app, owner).len()
}

fn lost(app: &App, owner: u64) -> u32 {
    app.world().resource::<MatchStats>().0.get(&owner).map(|s| s.lost).unwrap_or(0)
}

fn food_of(app: &mut App, owner: u64) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == owner).map(|p| p.stock.food).unwrap_or(0)
}

fn hunger_of(app: &mut App, owner: u64) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == owner).map(|p| p.hunger).unwrap_or(0)
}

fn run(app: &mut App, ticks: u32) {
    for _ in 0..ticks {
        step(app.world_mut());
    }
}

/// Step `ticks`, holding `owner`'s larder at `food` so the ration under test is
/// the one being asserted rather than whatever is left of the opening stock.
fn run_at(app: &mut App, ticks: u32, owner: u64, food: i32) {
    for _ in 0..ticks {
        set_food(app, owner, food);
        step(app.world_mut());
    }
}

fn fx(s: &str) -> Fx {
    Fx::lit(s)
}

fn home() -> V2 {
    V2::new(fx(HOME.0), fx(HOME.1))
}

fn afield() -> V2 {
    V2::new(fx(AFIELD_X), fx(HOME.1))
}

/// A town with its keep and a company beside it, in supply.
fn garrison_town(food: i32, n: u64) -> App {
    let mut app = build();
    spawn_player(&mut app, 1, food);
    spawn_keep(&mut app, 10, 1, home());
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), n);
    app
}

/// A keep at home and a column at the far end of the map.
fn column_in_the_field(food: i32, n: u64) -> App {
    let mut app = build();
    spawn_player(&mut app, 1, food);
    spawn_keep(&mut app, 10, 1, home());
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, afield(), n);
    app
}

/// THE HEADLINE. A garrison beside its own keep, with an EMPTY LARDER, for
/// eighty seconds. It is not "cheap to hold an army at home", it is free: the
/// ground it stands on is already yours and already feeding it.
///
/// This test replaces `a_one_unit_shortfall_costs_one_unit_of_rations` as it
/// applied at home, which asserted the old guarantee that a garrison one loaf
/// short takes a proportional bite. It takes no bite at all now — that is the
/// whole point of the rework and the reason the flat per-head tax is gone.
#[test]
fn a_garrison_beside_its_keep_costs_nothing_at_all() {
    let mut app = garrison_town(0, 20);
    run(&mut app, 40 * ECON);
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 20, "a garrison starved with a keep at its back");
    assert_eq!(lost(&app, 1), 0);
    for u in &us {
        assert_eq!(u.ration, saladin_sim::FULL_RATION, "a garrison drew rations");
        assert_eq!(u.hp, unit_def(UnitKind::Spearman).max_hp);
        assert_eq!(u.morale, MORALE_MAX, "a fed man keeps his heart");
        assert_eq!(u.attack_cd, 0, "a fed man is rested");
    }
    assert_eq!(food_of(&mut app, 1), 0, "an empty larder cannot go further down");
    assert_eq!(hunger_of(&mut app, 1), 0, "a garrison is never a famine");

    // and with a full larder it is untouched: nothing is drawn, not a little
    let mut rich = garrison_town(500, 20);
    run(&mut rich, 40 * ECON);
    assert_eq!(food_of(&mut rich, 1), 500, "the garrison ate the stores");
}

/// ONE larder, ONE player, two bodies of men. The column at the end of the road
/// bills the whole thing and the garrison at the gate bills none of it — which
/// is also the half of a siege that costs the BESIEGER something.
///
/// This replaces `an_army_far_from_its_stores_degrades_faster_than_one_at_home`:
/// the old guarantee was that the column degrades FASTER. The new one is that
/// the garrison does not degrade at all.
#[test]
fn the_column_pays_the_whole_bill_and_the_garrison_pays_none_of_it() {
    let max_hp = unit_def(UnitKind::Spearman).max_hp;
    let mut app = build();
    spawn_player(&mut app, 1, 0);
    spawn_keep(&mut app, 10, 1, home());
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), 10);
    spawn_company(&mut app, 200, 1, UnitKind::Spearman, afield(), 10);

    run_at(&mut app, 12 * ECON, 1, 0);
    let left = units_of(&mut app, 1);
    let garrison: Vec<&Unit> = left.iter().filter(|u| u.home.x < fx("100")).collect();
    let column: Vec<&Unit> = left.iter().filter(|u| u.home.x >= fx("100")).collect();

    assert_eq!(garrison.len(), 10, "the garrison paid for the column's road");
    for u in &garrison {
        assert_eq!(u.ration, saladin_sim::FULL_RATION);
        assert_eq!(u.hp, max_hp);
        assert_eq!(u.morale, MORALE_MAX, "a man at home lost heart over a distant famine");
        assert_eq!(u.attack_cd, 0);
    }
    assert!(column.len() < 10, "the column at the end of the line cost nothing");
    for u in &column {
        assert_eq!(u.ration, Fx::ZERO, "the column had stores it could not have");
        assert!(u.morale <= ROUT_THRESHOLD, "morale {} in the field", u.morale);
        assert_eq!(u.hp, max_hp, "hunger must never cost a man his hp");
    }
    assert!(lost(&app, 1) > 0, "nobody left the column");
}

/// The road prices itself BY THE TILE. A raid over the fence is nearly free, a
/// march is real, a siege at the far end of the map is the full rate — and the
/// ramp is continuous, because a cliff at the supply line would be the
/// all-or-nothing rule wearing a distance check.
#[test]
fn the_deeper_the_march_the_dearer_it_is() {
    let spent = |x: &str| -> i32 {
        let mut app = build();
        spawn_player(&mut app, 1, 5000);
        spawn_keep(&mut app, 10, 1, home());
        spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx(x), fx("60")), 10);
        run(&mut app, 10 * ECON);
        5000 - food_of(&mut app, 1)
    };
    let at_home = spent("70"); // inside the radius
    let over_the_fence = spent("100"); // ~6 tiles past it
    let a_march = spent("160");
    let a_siege = spent("300"); // capped at MAX_STRAIN

    assert_eq!(at_home, 0, "a garrison spent {at_home} food");
    assert!(over_the_fence > 0, "one step past the line is free");
    assert!(over_the_fence < a_march, "{over_the_fence} vs {a_march}");
    assert!(a_march < a_siege, "{a_march} vs {a_siege}");
    // the far end of the map is dear, not infinite: ten men at MAX_STRAIN over
    // ten economy ticks, and the cap is what stops a corner costing a fortune
    let cap = (FIELD_RATION * MAX_STRAIN * Fx::from_num(10) * Fx::from_num(10)).to_num::<i32>();
    assert!(a_siege >= cap - 10 && a_siege <= cap + 10, "{a_siege} vs the capped {cap}");
}

/// THE COUNTERPLAY, AND THE REASON THE MODEL IS A DECISION. A column starving
/// 200 tiles out is one Storehouse away from being in supply — a forward store
/// is what a besieging camp IS, and it is a building the defender can sortie
/// against. Without this the model would only be a punishment.
#[test]
fn a_forward_store_ends_the_famine() {
    let mut app = column_in_the_field(0, 10);
    run_at(&mut app, 3 * ECON, 1, 0);
    for u in units_of(&mut app, 1) {
        assert_eq!(u.ration, Fx::ZERO, "the column was meant to be starving");
    }

    // plant the camp store right beside them
    spawn_store(&mut app, 20, 1, BuildingKind::Storehouse, afield());
    run_at(&mut app, 3 * ECON, 1, 0);
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 10, "the store arrived and men still walked out");
    for u in &us {
        assert_eq!(u.ration, saladin_sim::FULL_RATION, "the camp store fed nobody");
        assert_eq!(u.attack_cd, 0, "a supplied man is rested");
    }
    assert_eq!(hunger_of(&mut app, 1), 0, "the famine clock kept running under a store");
    // and it costs nothing to hold there now, with the larder empty
    assert_eq!(food_of(&mut app, 1), 0);
}

/// A shortfall of one man's rations costs one man's rations. The rule this
/// replaced was `bill > food`: one loaf short and every soldier starved at once.
#[test]
fn a_one_unit_shortfall_costs_one_unit_of_rations() {
    // 20 men at MAX_STRAIN bill 6 a tick, so 5 is one man's share short
    let mut app = column_in_the_field(5, 20);
    run(&mut app, ECON + 1);
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 20, "one loaf short must not kill anybody");
    for u in &us {
        assert!(u.ration > fx("0.75") && u.ration < fx("1"), "ration was {}", u.ration);
        assert_eq!(u.hp, unit_def(UnitKind::Spearman).max_hp, "short commons never cost hp");
        assert!(u.morale > fx("0.8"), "morale was {} on five sixths rations", u.morale);
    }
    assert_eq!(food_of(&mut app, 1), 0, "the larder is emptied, not overdrawn");
    assert_eq!(hunger_of(&mut app, 1), 0, "five sixths rations is not a famine");
}

/// Nine tenths fed, forever. Without a floor under the consequences a permanent
/// small shortfall is a slow execution — the death spiral this rework exists to
/// remove.
#[test]
fn a_column_at_nine_tenths_rations_never_dies() {
    // 20 men at MAX_STRAIN bill 6; 5 is 83%, 6 is full
    let mut app = column_in_the_field(6, 20);
    run_at(&mut app, 40 * ECON, 1, 5);
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 20, "a small shortfall killed men over 80 seconds");
    assert_eq!(lost(&app, 1), 0);
    for u in &us {
        assert_eq!(u.hp, unit_def(UnitKind::Spearman).max_hp, "tired troops, not dying ones");
        assert!(u.ration > fx("0.8"), "ration {}", u.ration);
        assert!(u.morale > fx("0.8"), "morale {}", u.morale);
    }
    assert_eq!(hunger_of(&mut app, 1), 0, "five sixths is short, not a famine");
}

/// The grace and the ramp are the one good part of the old model and they are
/// kept: spirits break first, and bodies never.
#[test]
fn hunger_tires_before_it_kills() {
    let max_hp = unit_def(UnitKind::Spearman).max_hp;
    let mut app = column_in_the_field(1, 10);

    // through the grace: demoralized, slowed, bodies intact
    run_at(&mut app, 5 * ECON, 1, 1);
    for u in units_of(&mut app, 1) {
        assert_eq!(u.hp, max_hp, "hunger must never cost a man his hp");
        assert!(u.morale < MORALE_MAX, "hunger must bite morale immediately");
        assert!(u.attack_cd > 0, "hungry men swing slower");
    }
    assert_eq!(alive(&mut app, 1), 10);

    // past it, bodies are still intact and it is the ranks that thin
    run_at(&mut app, 6 * ECON, 1, 1);
    let us = units_of(&mut app, 1);
    assert!(us.iter().all(|u| u.hp == max_hp), "hunger must never cost a man his hp");
}

#[test]
fn feeding_resets_the_starvation_spiral() {
    let mut app = column_in_the_field(1, 10);
    run_at(&mut app, 12 * ECON, 1, 1);
    let max_hp = unit_def(UnitKind::Spearman).max_hp;
    let starved = units_of(&mut app, 1);
    assert!(starved.iter().all(|u| u.hp == max_hp), "hunger must never cost hp");
    assert!(starved.iter().all(|u| u.morale < MORALE_MAX), "a famine must bite morale");

    run_at(&mut app, 10 * ECON, 1, 500);
    let after = units_of(&mut app, 1);
    assert!(after.iter().all(|u| u.hp == max_hp), "a fed man is a whole man");
    assert_eq!(hunger_of(&mut app, 1), 0, "hunger counter must reset when fed");
    for u in &after {
        assert_eq!(u.ration, saladin_sim::FULL_RATION);
        assert_eq!(u.attack_cd, 0, "a fed man is rested");
    }
}

/// An empty larder BLEEDS a column. The old rule executed it: every man's morale
/// drained at once and every man's hp followed. Here the men with the least heart
/// go first, a few a night, and the professionals hold longest.
#[test]
fn an_empty_larder_bleeds_men_instead_of_executing_the_army() {
    let mut app = build();
    spawn_player(&mut app, 1, 0);
    spawn_keep(&mut app, 10, 1, home());
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, afield(), 16);
    spawn_company(&mut app, 200, 1, UnitKind::Sergeant, V2::new(fx(AFIELD_X), fx("64")), 8);

    // the grace: hearts gone, ranks still full
    run_at(&mut app, 5 * ECON, 1, 0);
    assert_eq!(alive(&mut app, 1), 24, "nobody walks out on the first evening");
    for u in units_of(&mut app, 1) {
        assert!(u.morale <= ROUT_THRESHOLD, "morale {} on nothing at all", u.morale);
    }

    // then it bleeds: a trickle, never the whole host in one tick
    let cap = 24 / 8 + 1;
    let mut prev = 24usize;
    let mut samples = 0;
    for _ in 0..12 {
        run_at(&mut app, ECON, 1, 0);
        let n = alive(&mut app, 1);
        assert!(prev - n <= cap, "{} men vanished in one economy tick", prev - n);
        if n < prev {
            samples += 1;
        }
        prev = n;
    }
    assert!(samples >= 3, "the army emptied in {samples} ticks, not a bleed");
    assert!(prev < 24, "an empty larder cost nothing at all");
    assert!(lost(&app, 1) > 0, "deserters must be tallied as losses");

    // discipline is the answer to hunger: the levy walks before the professionals
    let left = units_of(&mut app, 1);
    let levy = left.iter().filter(|u| u.kind == UnitKind::Spearman).count();
    let pros = left.iter().filter(|u| u.kind == UnitKind::Sergeant).count();
    assert!(pros * 16 >= levy * 8, "sergeants ({pros}/8) left before spearmen ({levy}/16)");
}

/// Desertion removes ENTITIES, so if it were order-dependent it would desync the
/// lockstep on the exact tick an army broke. Two worlds, hashes compared every
/// single tick.
#[test]
fn desertion_is_deterministic_across_two_worlds() {
    let mut worlds = [build(), build()];
    for app in worlds.iter_mut() {
        spawn_player(app, 1, 0);
        spawn_keep(app, 10, 1, home());
        spawn_company(app, 100, 1, UnitKind::Spearman, afield(), 16);
        spawn_company(app, 200, 1, UnitKind::Sergeant, V2::new(fx(AFIELD_X), fx("64")), 8);
    }
    for t in 0..(16 * ECON) {
        let [a, b] = &mut worlds;
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "hashes diverged at tick {t}"
        );
    }
    let [a, b] = &mut worlds;
    let ids = |app: &mut App| -> Vec<u64> {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        let mut v: Vec<u64> = q.iter(world).map(|(g, _)| g.0).collect();
        v.sort_unstable();
        v
    };
    let sa = ids(a);
    assert!(sa.len() < 24, "nobody deserted, so nothing was proved");
    assert_eq!(sa, ids(b), "different men walked out of two identical worlds");
}

/// An army in the field has something to do besides starve. Foraging is thin and
/// it strips the herd, so it buys a march and never a war.
#[test]
fn a_column_in_the_field_lives_off_a_wild_herd() {
    // a patch of dry ground well away from either keep
    let mut spot = None;
    for x in 150..190 {
        for y in 150..190 {
            if is_passable(SEED, x, y) && is_passable(SEED, x + 1, y) {
                spot = Some((x, y));
                break;
            }
        }
        if spot.is_some() {
            break;
        }
    }
    let (hx, hy) = spot.expect("dry ground");
    let herd = V2::new(Fx::from_num(hx) + fx("0.5"), Fx::from_num(hy) + fx("0.5"));

    let mut app = build();
    spawn_player(&mut app, 1, 0);
    spawn_player(&mut app, 2, 0);
    spawn_keep(&mut app, 10, 1, home());
    spawn_keep(&mut app, 11, 2, V2::new(fx("300"), fx("300")));
    app.world_mut().spawn((
        GameId(50),
        MatchId(1),
        Pos { pos: herd, facing: ZERO },
        ResourceNode::deposit(ResourceType::Food, 120),
    ));
    // player 1's column camps ON the herd, player 2's a long way from anything
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, herd, 4);
    spawn_company(&mut app, 200, 2, UnitKind::Spearman, V2::new(fx("100"), fx("100")), 4);

    run(&mut app, 3 * ECON);
    for u in units_of(&mut app, 1) {
        assert_eq!(u.ration, saladin_sim::FULL_RATION, "the herd fed nobody");
        assert_eq!(u.morale, MORALE_MAX);
    }
    for u in units_of(&mut app, 2) {
        assert_eq!(u.ration, Fx::ZERO, "a column with no herd and no stores eats air");
    }
    let left = {
        let world = app.world_mut();
        let mut q = world.query::<&ResourceNode>();
        q.iter(world).next().unwrap().remaining
    };
    assert!(left < 120, "the herd was not touched");
    assert!(left > 0);

    // and it runs out: forage buys a march, not a war
    run(&mut app, 60 * ECON);
    let end = {
        let world = app.world_mut();
        let mut q = world.query::<&ResourceNode>();
        q.iter(world).next().map(|n| n.remaining).unwrap_or(0)
    };
    assert_eq!(end, 0, "a 120-head herd fed four men forever");
}

/// The muster roll is a ROLE question. A peasant beside a starving column keeps
/// its heart, its health and its place — and arming one later must not silently
/// put it on the roll.
#[test]
fn only_soldiers_draw_rations() {
    let mut app = build();
    spawn_player(&mut app, 1, 0);
    spawn_keep(&mut app, 10, 1, home());
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, afield(), 8);
    spawn_company(&mut app, 300, 1, UnitKind::Peasant, V2::new(fx(AFIELD_X), fx("64")), 6);
    spawn_unit(&mut app, 400, 1, UnitKind::Imam, V2::new(fx("270"), fx("64")));

    run_at(&mut app, 20 * ECON, 1, 0);
    let left = units_of(&mut app, 1);
    assert_eq!(
        left.iter().filter(|u| u.kind == UnitKind::Peasant).count(),
        6,
        "peasants starved on a soldier's ration"
    );
    assert_eq!(left.iter().filter(|u| u.kind == UnitKind::Imam).count(), 1);
    for u in left.iter().filter(|u| u.kind != UnitKind::Spearman) {
        assert_eq!(u.hp, unit_def(u.kind).max_hp);
        assert_eq!(u.ration, saladin_sim::FULL_RATION, "{:?} drew rations", u.kind);
    }
    assert!(
        left.iter().filter(|u| u.kind == UnitKind::Spearman).count() < 8,
        "the soldiers were not affected at all"
    );
}

/// TIRED TROOPS BEFORE DEAD ONES. Half rations in the field: no wasting, no
/// desertion, no rout — the only thing hunger has done is slow the arm. Measured
/// as damage dealt to a target that never fights back, so nothing but the
/// cadence can explain the gap.
#[test]
fn hungry_men_swing_slower() {
    const DUMMY_HP: i32 = 200_000;
    // eight men at MAX_STRAIN bill 2.4 a tick, so 1 food is a bit under half
    let damage = |food: i32| -> i32 {
        let mut app = build();
        spawn_player(&mut app, 1, food);
        spawn_player(&mut app, 2, 0);
        spawn_keep(&mut app, 10, 1, home());
        spawn_company(&mut app, 100, 1, UnitKind::Spearman, afield(), 8);
        // a target that cannot answer: peasants never take a swing and never
        // draw rations, so only the attackers' cadence is under test
        let dummy = V2::new(fx(AFIELD_X), fx("61"));
        app.world_mut().spawn((
            GameId(500),
            Owner(2),
            MatchId(1),
            Pos { pos: dummy, facing: ZERO },
            Unit { hp: DUMMY_HP, stance: Stance::HoldGround, ..Unit::new(UnitKind::Peasant, dummy) },
        ));
        run_at(&mut app, 4 * ECON, 1, food);
        let hp = units_of(&mut app, 2).first().map(|u| u.hp).unwrap_or(0);
        assert_eq!(units_of(&mut app, 1).len(), 8, "the attackers were meant to survive");
        DUMMY_HP - hp
    };
    let fed = damage(100);
    let half = damage(1);
    assert!(fed > 0 && half > 0, "nobody swung at all: {fed} vs {half}");
    assert!(half < fed, "half-fed men swung as often as fed ones ({half} vs {fed})");
    let ratio = half as f64 / fed as f64;
    assert!(ratio > 0.5 && ratio < 0.95, "cadence ratio {ratio:.3} (want ~0.8)");
}

/// The sim's own rule, read back off a real world: a man inside the radius is at
/// zero strain and a man past it is not. Every consumer — the economy tick, the
/// bot's war chest, the HUD readout — goes through this one function.
#[test]
fn strain_is_measured_from_the_nearest_store() {
    assert_eq!(strain(SUPPLY_RADIUS), Fx::ZERO);
    assert_eq!(strain(SUPPLY_RADIUS - Fx::ONE), Fx::ZERO);
    assert!(strain(SUPPLY_RADIUS + Fx::ONE) > Fx::ZERO);
    // a second store closer to the column is the one that counts
    let mut app = column_in_the_field(1000, 10);
    run(&mut app, 2 * ECON);
    let spent_far = 1000 - food_of(&mut app, 1);
    assert!(spent_far > 0);
    spawn_store(&mut app, 20, 1, BuildingKind::Storehouse, afield());
    let before = food_of(&mut app, 1);
    run(&mut app, 2 * ECON);
    assert_eq!(food_of(&mut app, 1), before, "the nearer store was ignored");
}
