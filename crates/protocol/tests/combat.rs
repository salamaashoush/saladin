//! The combat regression floor. Until now combat state was touched only
//! incidentally (roles.rs, stats.rs and a determinism test walking two
//! peasants), which is exactly why a 20% cadence error went unnoticed by a
//! fully green suite.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    ArmorClass, Attacker, COMBAT_DT, Fx, UnitKind, V2, ZERO, effective_damage, is_passable,
    unit_def,
};

fn build(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn find_land_block(seed: u32) -> (i32, i32) {
    for cy in 16..128 {
        for cx in 16..128 {
            if (0..8).all(|dx| (0..8).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no 8x8 land block found");
}

fn tile(x: i32, y: i32) -> V2 {
    V2::new(Fx::from_num(x) + saladin_sim::fx!("0.5"), Fx::from_num(y) + saladin_sim::fx!("0.5"))
}

/// Combat ticks the sim waits between blows, derived the way `combat.rs` does.
fn cadence_ticks(rate: Fx) -> i32 {
    ((rate + saladin_sim::fx!("0.1")) / COMBAT_DT).to_num::<i32>().max(1)
}

/// The cooldown was an `Fx` decremented by COMBAT_DT, so every rate was rounded
/// UP to the next whole combat tick and a declared 1.0 s cadence really fired
/// every 1.2 s. Rounding to the nearest tick makes a rate that IS a multiple of
/// COMBAT_DT exact, and caps everything else at half a tick.
#[test]
fn no_unit_is_more_than_half_a_combat_tick_off_its_declared_rate() {
    let half = COMBAT_DT / Fx::from_num(2);
    // COMBAT_DT is 0.2 in I32F32, which is not exactly 0.2 — a rate that DOES
    // land on the grid still misses by a few bits
    let epsilon = saladin_sim::fx!("0.01");
    let mut off_grid = Vec::new();
    for &k in UnitKind::ALL {
        let d = unit_def(k);
        if d.attack <= 0 {
            continue;
        }
        let real = Fx::from_num(cadence_ticks(d.attack_rate)) * COMBAT_DT;
        let err = (real - d.attack_rate).abs();
        assert!(
            err <= half,
            "{} fires every {real} s against a declared {} s",
            d.label,
            d.attack_rate
        );
        if err > epsilon {
            off_grid.push(d.label);
        }
    }
    // A rate that is not a multiple of COMBAT_DT can never be delivered exactly.
    // The Knight (1.10 s) and the Horse Archer (1.30 s) were the two the engine
    // could only ever lie about; the roster retune put every rate on the grid,
    // and `UnitDef.attack_ticks` now states the cadence outright.
    assert!(
        off_grid.is_empty(),
        "these rates cannot be delivered by a 200 ms combat tick: {off_grid:?}"
    );
}

/// The declared rate has to be what the loop actually delivers, measured by
/// counting real blows in a real world rather than by re-deriving the formula.
#[test]
fn a_spearman_strikes_at_its_declared_rate() {
    let seed = 1u32;
    let (cx, cy) = find_land_block(seed);
    let mut app = build(seed);
    let a = tile(cx + 2, cy + 2);
    let b = tile(cx + 3, cy + 2);
    app.world_mut().spawn((
        GameId(1),
        Owner(1),
        MatchId(1),
        Pos { pos: a, facing: ZERO },
        Unit::new(UnitKind::Spearman, a),
    ));
    // a sandbag that never hits back: a fight the attacker can lose morale in
    // measures the rout, not the cadence. It looks AT its attacker (heading 8
    // is -X) so every blow lands frontally — otherwise this measures
    // `REAR_MULT` as well as the cadence and reads 50% fast.
    app.world_mut().spawn((
        GameId(2),
        Owner(2),
        MatchId(1),
        Pos { pos: b, facing: ZERO },
        Unit { hp: 1_000_000, heading: 8, ..Unit::new(UnitKind::Peasant, b) },
    ));

    const SECONDS: usize = 20;
    for _ in 0..SECONDS * 20 {
        step(app.world_mut());
    }
    let d = unit_def(UnitKind::Spearman);
    let per_blow = effective_damage(
        &Attacker {
            attack: Fx::from_num(d.attack),
            damage_type: d.damage_type,
            bonus_vs_armor: d.bonus_vs_armor,
        },
        ArmorClass::Unarmored,
    );
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    let hp = q.iter(world).find(|(g, _)| g.0 == 2).map(|(_, u)| u.hp).expect("the sandbag");
    let blows = (1_000_000 - hp) / per_blow;
    let expected = SECONDS as f64 / d.attack_rate.to_num::<f64>();
    assert!(
        (blows as f64 - expected).abs() <= 1.5,
        "a spearman landed {blows} blows in {SECONDS} s; its declared 1.0 s rate wants {expected:.0}"
    );
}

/// Every fighter's cadence, measured the same way. A retune that silently
/// changes how often a unit swings should show up here, not in a play session.
#[test]
fn every_fighter_delivers_the_blows_its_rate_promises() {
    let seed = 1u32;
    let (cx, cy) = find_land_block(seed);
    for &k in UnitKind::ALL {
        let d = unit_def(k);
        if d.attack <= 0 || d.aggro_range <= Fx::ZERO {
            continue;
        }
        let mut app = build(seed);
        let a = tile(cx + 2, cy + 2);
        // an engine that cannot depress its arc has a DEAD ZONE: stand the
        // sandbag outside it, or this measures the min_range rule, not a rate
        let gap = if d.min_range > Fx::ZERO { d.min_range.ceil().to_num::<i32>() + 1 } else { 1 };
        let b = tile(cx + 2 + gap, cy + 2);
        app.world_mut().spawn((
            GameId(1),
            Owner(1),
            MatchId(1),
            Pos { pos: a, facing: ZERO },
            Unit::new(k, a),
        ));
        app.world_mut().spawn((
            GameId(2),
            Owner(2),
            MatchId(1),
            Pos { pos: b, facing: ZERO },
            Unit { hp: 10_000_000, heading: 8, ..Unit::new(UnitKind::Peasant, b) },
        ));
        const SECONDS: usize = 30;
        for _ in 0..SECONDS * 20 {
            step(app.world_mut());
        }
        let per_blow = effective_damage(
            &Attacker {
                attack: Fx::from_num(d.attack),
                damage_type: d.damage_type,
                bonus_vs_armor: d.bonus_vs_armor,
            },
            ArmorClass::Unarmored,
        );
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        let hp = q.iter(world).find(|(g, _)| g.0 == 2).map(|(_, u)| u.hp).expect("the sandbag");
        let blows = (10_000_000 - hp) / per_blow;
        let real = Fx::from_num(cadence_ticks(d.attack_rate)) * COMBAT_DT;
        let expected = SECONDS as f64 / real.to_num::<f64>();
        assert!(
            blows > 0,
            "{} never landed a blow on a target one tile away",
            d.label
        );
        assert!(
            (blows as f64 - expected).abs() <= 2.0,
            "{} landed {blows} blows in {SECONDS} s; its {real} s cadence wants {expected:.0}",
            d.label
        );
    }
}

/// A cooldown that survives a round trip through the field: two identical
/// duels, one started a tick later, must not drift apart in blow count.
#[test]
fn the_cooldown_is_a_whole_number_of_combat_ticks() {
    let seed = 1u32;
    let (cx, cy) = find_land_block(seed);
    let mut counts = Vec::new();
    for offset in 0..4 {
        let mut app = build(seed);
        for _ in 0..offset {
            step(app.world_mut());
        }
        let a = tile(cx + 2, cy + 2);
        let b = tile(cx + 3, cy + 2);
        app.world_mut().spawn((
            GameId(1),
            Owner(1),
            MatchId(1),
            Pos { pos: a, facing: ZERO },
            Unit::new(UnitKind::Spearman, a),
        ));
        app.world_mut().spawn((
            GameId(2),
            Owner(2),
            MatchId(1),
            Pos { pos: b, facing: ZERO },
            Unit { hp: 1_000_000, ..Unit::new(UnitKind::Peasant, b) },
        ));
        for _ in 0..400 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        let hp = q.iter(world).find(|(g, _)| g.0 == 2).map(|(_, u)| u.hp).expect("the sandbag");
        counts.push(1_000_000 - hp);
    }
    let lo = counts.iter().min().unwrap();
    let hi = counts.iter().max().unwrap();
    assert!(
        hi - lo <= 12,
        "the same duel started on different ticks did {lo}..{hi} damage — the cadence depends on tick phase"
    );
}
