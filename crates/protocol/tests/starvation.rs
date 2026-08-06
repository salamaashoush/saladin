//! Supply, which is what hunger should have been.
//!
//! The rule these tests replaced was `starving = bill > food`: ONE loaf short
//! and every soldier on the map starved at the same instant, at the same rate,
//! with no rationing, no supply line, no foraging and no way to answer it. What
//! is asserted here is the difference: a one-man shortfall costs one man's
//! rations, an army at nine tenths is tired rather than dying, a column at the
//! far end of a supply line pays for the road, men in the field live off the
//! land, and the ones who leave are individuals with no heart left rather than
//! the whole host being executed together.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, MORALE_MAX, ROUT_THRESHOLD, ResourceType, Stance, Stockpile,
    UnitKind, V2, ZERO,
    building_def, is_passable, unit_def,
};

const SEED: u32 = 1;
/// Economy runs every 40 ticks; one tick past it is the sampling point.
const ECON: u32 = 40;

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

fn spawn_keep(app: &mut App, id: u64, owner: u64, pos: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(BuildingKind::Keep, building_def(BuildingKind::Keep).max_hp, pos),
    ));
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

/// A town with its keep and a company beside it, in supply.
fn garrison_town(food: i32, n: u64) -> App {
    let mut app = build();
    spawn_player(&mut app, 1, food);
    spawn_keep(&mut app, 10, 1, V2::new(fx("60"), fx("60")));
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), n);
    app
}

/// THE COMPLAINT, IN ONE TEST. One loaf short used to starve the whole company.
/// Now it costs every man an equal share of the shortfall and nothing else.
/// Twenty men bill five loaves, so four is one short.
#[test]
fn a_one_unit_shortfall_costs_one_unit_of_rations() {
    let mut app = garrison_town(4, 20);
    for _ in 0..ECON + 1 {
        step(app.world_mut());
    }
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 20, "one loaf short must not kill anybody");
    for u in &us {
        assert!(u.ration > fx("0.75") && u.ration < fx("1"), "ration was {}", u.ration);
        assert_eq!(u.hp, unit_def(UnitKind::Spearman).max_hp, "short commons never cost hp");
        assert!(u.morale > fx("0.8"), "morale was {} on four fifths rations", u.morale);
    }
    assert_eq!(food_of(&mut app, 1), 0, "the larder is emptied, not overdrawn");
    assert_eq!(hunger_of(&mut app, 1), 0, "four fifths rations is not a famine");

    // and the contrast: the SAME company with nothing at all
    let mut empty = garrison_town(0, 20);
    for _ in 0..ECON + 1 {
        step(empty.world_mut());
    }
    for u in units_of(&mut empty, 1) {
        assert_eq!(u.ration, Fx::ZERO);
    }
}

/// Nine tenths fed, forever. `apply_supply` floors its attrition at 1 hp, so
/// without a "tired, not dying" threshold a permanent 10% shortfall is a slow
/// execution — the very death spiral this rework exists to remove.
#[test]
fn an_army_at_nine_tenths_rations_never_dies() {
    let mut app = garrison_town(18, 20);
    run_at(&mut app, 40 * ECON, 1, 18);
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 20, "a 10% shortfall killed men over 80 seconds");
    assert_eq!(lost(&app, 1), 0);
    for u in &us {
        assert_eq!(u.hp, unit_def(UnitKind::Spearman).max_hp, "tired troops, not dying ones");
        assert!(u.ration > fx("0.85"), "ration {}", u.ration);
        assert!(u.morale > fx("0.85"), "morale {}", u.morale);
    }
    assert_eq!(hunger_of(&mut app, 1), 0, "nine tenths is short, not a famine");
}

#[test]
fn an_army_beside_its_keep_never_deserts_while_it_is_fed() {
    let mut app = garrison_town(40, 20);
    run_at(&mut app, 40 * ECON, 1, 40);
    assert_eq!(alive(&mut app, 1), 20);
    assert_eq!(lost(&app, 1), 0);
    for u in units_of(&mut app, 1) {
        assert_eq!(u.ration, saladin_sim::FULL_RATION);
        assert_eq!(u.morale, MORALE_MAX);
        assert_eq!(u.attack_cd, 0, "a fed man is not tired");
    }
}

/// A deep push prices itself. ONE larder, ONE player, ONE tick: the garrison at
/// the gate tightens its belt while the column 120 tiles out is on nothing —
/// because the far band is fed LAST and pays a carter's premium for the road.
/// This is the positioning decision the flat poll tax never offered, and it is
/// also the half of a siege that costs the BESIEGER something.
#[test]
fn an_army_far_from_its_stores_degrades_faster_than_one_at_home() {
    let max_hp = unit_def(UnitKind::Spearman).max_hp;
    let mut app = build();
    spawn_player(&mut app, 1, 8);
    spawn_keep(&mut app, 10, 1, V2::new(fx("60"), fx("60")));
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), 10);
    spawn_company(&mut app, 200, 1, UnitKind::Spearman, V2::new(fx("160"), fx("160")), 10);

    run_at(&mut app, 12 * ECON, 1, 2);
    let left = units_of(&mut app, 1);
    let home: Vec<&Unit> = left.iter().filter(|u| u.home.x < fx("100")).collect();
    let away: Vec<&Unit> = left.iter().filter(|u| u.home.x >= fx("100")).collect();

    assert_eq!(home.len(), 10, "the garrison paid for the column's road");
    for u in &home {
        assert!(u.ration > fx("0.75") && u.ration < fx("1"), "ration {}", u.ration);
        assert_eq!(u.hp, max_hp, "eight tenths of a ration must not waste a garrison");
        assert!(u.morale < MORALE_MAX, "short commons are still short");
        assert!(u.attack_cd > 0, "hungry men swing as fast as fed ones");
    }
    assert!(away.len() < 10, "the column at the end of the line cost nothing");
    for u in &away {
        assert_eq!(u.ration, Fx::ZERO, "the column was fed before the garrison");
        assert!(u.morale <= ROUT_THRESHOLD, "morale {} in the field", u.morale);
        assert!(u.hp < max_hp, "a column on nothing at all never wasted");
    }
    assert!(lost(&app, 1) > 0, "nobody left the column");
}

/// The grace and the ramp are the one good part of the old model and they are
/// kept: spirits break first, bodies only under a real famine, and a third of a
/// ration is grim enough to waste men without emptying the ranks.
#[test]
fn hunger_tires_before_it_kills() {
    let max_hp = unit_def(UnitKind::Spearman).max_hp;
    let mut app = garrison_town(1, 10);

    // through the grace: demoralized, slowed, bodies intact
    run_at(&mut app, 5 * ECON, 1, 1);
    for u in units_of(&mut app, 1) {
        assert_eq!(u.hp, max_hp, "hunger must never cost a man his hp");
        assert!(u.morale < MORALE_MAX, "hunger must bite morale immediately");
        assert!(u.attack_cd > 0, "hungry men swing slower");
    }
    assert_eq!(alive(&mut app, 1), 10);

    // past it: the ramp bites, and nobody has walked out yet
    run_at(&mut app, 4 * ECON, 1, 1);
    let us = units_of(&mut app, 1);
    assert_eq!(us.len(), 10, "a third of a ration must not empty the ranks");
    assert_eq!(lost(&app, 1), 0, "a third of a ration is not desertion");
    assert!(us.iter().all(|u| u.hp == max_hp), "hunger must never cost a man his hp");
}

#[test]
fn feeding_resets_the_starvation_spiral() {
    let mut app = garrison_town(1, 10);
    run_at(&mut app, 12 * ECON, 1, 1);
    let max_hp = unit_def(UnitKind::Spearman).max_hp;
    let starved = units_of(&mut app, 1);
    assert!(starved.iter().all(|u| u.hp == max_hp), "hunger must never cost hp");
    assert!(starved.iter().all(|u| u.morale < MORALE_MAX), "a famine must bite morale");

    run_at(&mut app, 10 * ECON, 1, 500);
    let after = units_of(&mut app, 1);
    assert!(after.iter().all(|u| u.hp == max_hp), "a fed man is a whole man");
    assert_eq!(hunger_of(&mut app, 1), 0, "hunger counter must reset when fed");
    for u in units_of(&mut app, 1) {
        assert_eq!(u.ration, saladin_sim::FULL_RATION);
        assert_eq!(u.attack_cd, 0, "a fed man is rested");
    }
}

/// An empty larder BLEEDS an army. The old rule executed it: every man's morale
/// drained at once and every man's hp followed. Here the men with the least
/// heart go first, a few a night, and the professionals hold longest.
#[test]
fn an_empty_larder_bleeds_men_instead_of_executing_the_army() {
    let mut app = build();
    spawn_player(&mut app, 1, 0);
    spawn_keep(&mut app, 10, 1, V2::new(fx("60"), fx("60")));
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), 16);
    spawn_company(&mut app, 200, 1, UnitKind::Sergeant, V2::new(fx("66"), fx("64")), 8);

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
    assert!(
        pros * 16 >= levy * 8,
        "sergeants ({pros}/8) left before spearmen ({levy}/16)"
    );
}

/// Desertion removes ENTITIES, so if it were order-dependent it would desync the
/// lockstep on the exact tick an army broke. Two worlds, hashes compared every
/// single tick.
#[test]
fn desertion_is_deterministic_across_two_worlds() {
    let mut worlds = [build(), build()];
    for app in worlds.iter_mut() {
        spawn_player(app, 1, 0);
        spawn_keep(app, 10, 1, V2::new(fx("60"), fx("60")));
        spawn_company(app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), 16);
        spawn_company(app, 200, 1, UnitKind::Sergeant, V2::new(fx("66"), fx("64")), 8);
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

/// An army in the field has something to do besides die. Foraging is thin and it
/// strips the herd, so it buys a march and never a war.
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
    spawn_keep(&mut app, 10, 1, V2::new(fx("60"), fx("60")));
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

    for _ in 0..3 * ECON {
        step(app.world_mut());
    }
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
    for _ in 0..30 * ECON {
        step(app.world_mut());
    }
    let end = {
        let world = app.world_mut();
        let mut q = world.query::<&ResourceNode>();
        q.iter(world).next().map(|n| n.remaining).unwrap_or(0)
    };
    assert_eq!(end, 0, "a 120-head herd fed four men forever");
}

/// The muster roll is a ROLE question. A peasant beside a starving company keeps
/// its heart, its health and its place — and arming one later must not silently
/// put it on the roll.
#[test]
fn only_soldiers_draw_rations() {
    let mut app = build();
    spawn_player(&mut app, 1, 0);
    spawn_keep(&mut app, 10, 1, V2::new(fx("60"), fx("60")));
    spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), 8);
    spawn_company(&mut app, 300, 1, UnitKind::Peasant, V2::new(fx("66"), fx("64")), 6);
    spawn_unit(&mut app, 400, 1, UnitKind::Imam, V2::new(fx("70"), fx("64")));

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

/// TIRED TROOPS BEFORE DEAD ONES. Half rations sits exactly on the attrition
/// threshold: no wasting, no desertion, no rout — the only thing hunger has done
/// is slow the arm. Measured as damage dealt to a target that never fights back,
/// so nothing but the cadence can explain the gap.
#[test]
fn hungry_men_swing_slower() {
    const DUMMY_HP: i32 = 200_000;
    let damage = |food: i32| -> i32 {
        let mut app = build();
        spawn_player(&mut app, 1, food);
        spawn_player(&mut app, 2, 0);
        // the keep is a drop-off (so the company is in supply) but well out of
        // its own bow range, or the tower does the killing instead of the men
        spawn_keep(&mut app, 10, 1, V2::new(fx("40"), fx("60")));
        spawn_company(&mut app, 100, 1, UnitKind::Spearman, V2::new(fx("66"), fx("60")), 8);
        // a target that cannot answer: peasants never take a swing and never
        // draw rations, so only the attackers' cadence is under test
        let dummy = V2::new(fx("66"), fx("61"));
        app.world_mut().spawn((
            GameId(500),
            Owner(2),
            MatchId(1),
            Pos { pos: dummy, facing: ZERO },
            Unit { hp: DUMMY_HP, stance: Stance::HoldGround, ..Unit::new(UnitKind::Peasant, dummy) },
        ));
        run_at(&mut app, 15 * ECON, 1, food);
        let hp = units_of(&mut app, 2).first().map(|u| u.hp).unwrap_or(0);
        assert_eq!(units_of(&mut app, 1).len(), 8, "the attackers were meant to survive");
        DUMMY_HP - hp
    };
    // eight men bill two loaves, so one loaf is exactly half rations
    let fed = damage(100);
    let half = damage(1);
    assert!(fed > 0 && half > 0, "nobody swung at all: {fed} vs {half}");
    assert!(half < fed, "half-fed men swung as often as fed ones ({half} vs {fed})");
    let ratio = half as f64 / fed as f64;
    assert!(ratio > 0.6 && ratio < 0.95, "cadence ratio {ratio:.3} (want ~0.8)");
}
