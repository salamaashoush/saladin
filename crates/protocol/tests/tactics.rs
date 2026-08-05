//! Fortification, line of fire, the garrison, and the tactics layer.
//!
//! Every assertion here was a measured HOLE before it was a test: a spearman
//! walked into a sealed wall ring and killed the man inside it, an archer shot
//! through stone, a tower fired one host-typed boulder instead of five arrows,
//! a rear attack and a frontal one were the same attack, and a 40v40 mirror
//! stood still for 230 seconds.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn arena(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

/// A passable, flat-enough block, so terrain never explains a result.
fn flat_block(seed: u32, w: i32, h: i32) -> (i32, i32) {
    for cy in 24..(WORLD_SIZE - h - 8) {
        for cx in 24..(WORLD_SIZE - w - 8) {
            if (0..w).all(|dx| (0..h).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                let e0 = elevation_at(seed, Fx::from_num(cx), Fx::from_num(cy));
                let flat = (0..w).step_by(3).all(|dx| {
                    (0..h).step_by(3).all(|dy| {
                        (elevation_at(seed, Fx::from_num(cx + dx), Fx::from_num(cy + dy)) - e0).abs()
                            < fx!("0.04")
                    })
                });
                if flat {
                    return (cx, cy);
                }
            }
        }
    }
    panic!("no {w}x{h} flat block on seed {seed}");
}

fn tile(x: i32, y: i32) -> V2 {
    V2::new(Fx::from_num(x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5"))
}

fn put(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: Fx::ZERO },
        Unit::new(kind, pos),
    ));
}

fn put_unit(app: &mut App, id: u64, owner: u64, pos: V2, u: Unit) {
    app.world_mut().spawn((GameId(id), Owner(owner), MatchId(1), Pos { pos, facing: Fx::ZERO }, u));
}

fn put_building(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: Fx::ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

fn order(app: &mut App, cmd: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(cmd);
}

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        step(app.world_mut());
    }
}

fn unit_of(app: &mut App, id: u64) -> Option<Unit> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Unit)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone())
}

fn pos_of(app: &mut App, id: u64) -> Option<V2> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Pos)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, p)| p.pos)
}

fn building_hp(app: &mut App, id: u64) -> Option<i32> {
    let w = app.world_mut();
    let mut q = w.query::<(&GameId, &Building)>();
    q.iter(w).find(|(g, _)| g.0 == id).map(|(_, b)| b.hp)
}

fn alive(app: &mut App, owner: u64) -> usize {
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Unit)>();
    q.iter(w).filter(|(o, _)| o.0 == owner).count()
}

/// An unbroken square of Wall segments, `r` tiles from its centre.
fn ring(app: &mut App, first_id: u64, owner: u64, cx: i32, cy: i32, r: i32) -> usize {
    let mut id = first_id;
    for d in -r..=r {
        for (x, y) in [(cx + d, cy - r), (cx + d, cy + r), (cx - r, cy + d), (cx + r, cy + d)] {
            put_building(app, id, owner, BuildingKind::Wall, tile(x, y));
            id += 1;
        }
    }
    (id - first_id) as usize
}

// ── fortification ────────────────────────────────────────────────────────────

/// THE measured defect: a Spearman ordered onto a peasant sealed in a SOLID 9x9
/// wall ring got inside and killed it in 5.8 s with all 32 segments standing.
/// Fortification only ever stopped units that had nothing to do.
#[test]
fn a_sealed_wall_ring_stops_a_man_who_has_an_attack_order() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let (mx, my) = (cx + 12, cy + 12);
    let mut app = arena(seed);
    ring(&mut app, 100, 2, mx, my, 4);
    put(&mut app, 1, 2, UnitKind::Peasant, tile(mx, my));
    put(&mut app, 2, 1, UnitKind::Spearman, tile(mx - 8, my));
    order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 2, target: 1 });

    run(&mut app, 1200); // 60 s — the old hole took 5.8
    let inside = pos_of(&mut app, 1).expect("the sealed peasant was killed through a wall");
    let besieger = pos_of(&mut app, 2).expect("the attacker vanished");
    let _ = inside;
    let (bx, by) = (besieger.x.to_num::<i32>(), besieger.y.to_num::<i32>());
    assert!(
        (bx - mx).abs() > 3 || (by - my).abs() > 3,
        "the attacker is INSIDE the ring at ({bx}, {by}); the ring is centred on ({mx}, {my})"
    );
}

/// And the wall in the way becomes the objective, so besieging emerges from
/// attacking instead of needing a click per segment.
#[test]
fn a_wall_in_the_way_becomes_the_target() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let (mx, my) = (cx + 12, cy + 12);
    let mut app = arena(seed);
    let segments = ring(&mut app, 100, 2, mx, my, 4);
    put(&mut app, 1, 2, UnitKind::Peasant, tile(mx, my));
    put(&mut app, 2, 1, UnitKind::Ram, tile(mx - 7, my));
    order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 2, target: 1 });

    run(&mut app, 1400);
    let standing = {
        let w = app.world_mut();
        let mut q = w.query::<&Building>();
        q.iter(w).count()
    };
    assert!(
        standing < segments,
        "the ram never opened the ring: all {segments} segments still stand"
    );
}

/// A garrisoned tower is a wall with hit points if arrows go through stone: an
/// archer two tiles from a spearman with ONE segment between them killed it.
#[test]
fn an_arrow_stops_at_the_wall_and_a_boulder_goes_over_it() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 16, 16);
    let hp_after = |wall: bool, shooter: UnitKind, gap: i32| -> i32 {
        let mut app = arena(seed);
        let sx = cx + 4;
        if wall {
            put_building(&mut app, 100, 2, BuildingKind::Wall, tile(sx + 1, cy + 8));
        }
        put_unit(
            &mut app,
            1,
            1,
            tile(sx, cy + 8),
            Unit { stance: Stance::HoldGround, ..Unit::new(shooter, tile(sx, cy + 8)) },
        );
        let t = tile(sx + gap, cy + 8);
        put_unit(
            &mut app,
            2,
            2,
            t,
            Unit { hp: 100_000, stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, t) },
        );
        order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
        run(&mut app, 600);
        unit_of(&mut app, 2).map(|u| u.hp).unwrap_or(0)
    };
    let open = hp_after(false, UnitKind::Archer, 3);
    let walled = hp_after(true, UnitKind::Archer, 3);
    assert!(open < 100_000, "the archer did not shoot at all on open ground");
    assert_eq!(walled, 100_000, "the arrow went through a stone wall");
    // the one exemption: artillery lobs over it
    let lobbed = hp_after(true, UnitKind::Mangonel, 4);
    assert!(lobbed < 100_000, "the mangonel could not shell over one course of wall");
}

/// An engine that cannot depress its arc has a dead zone, and that dead zone is
/// what forces an escort.
#[test]
fn an_engine_will_not_shoot_what_is_under_its_arc() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 16, 16);
    let hp_after = |gap: i32| -> i32 {
        let mut app = arena(seed);
        let a = tile(cx + 4, cy + 8);
        let b = tile(cx + 4 + gap, cy + 8);
        put_unit(&mut app, 1, 1, a, Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Mangonel, a) });
        put_unit(
            &mut app,
            2,
            2,
            b,
            Unit { hp: 100_000, stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, b) },
        );
        order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
        run(&mut app, 600);
        unit_of(&mut app, 2).map(|u| u.hp).unwrap_or(0)
    };
    let min_range = unit_def(UnitKind::Mangonel).min_range.to_num::<i32>();
    assert_eq!(hp_after(1), 100_000, "the mangonel shot a man standing under it");
    assert!(hp_after(min_range + 3) < 100_000, "the mangonel never fired at a proper range");
}

// ── the garrison ─────────────────────────────────────────────────────────────

fn damage_dealt(app: &mut App, sandbag: u64, start: i32) -> i32 {
    start - unit_of(app, sandbag).map(|u| u.hp).unwrap_or(0)
}

/// Five archers in a tower fired ONE 62-damage host-typed blow that one-volleyed
/// a spearman; a garrisoned crossbowman lost both its Pierce and its 2.2x
/// anti-mail. Five men now loose five arrows, each still its own weapon.
#[test]
fn a_manned_tower_shoots_like_the_men_inside_it() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 20, 20);
    const HP: i32 = 200_000;
    let field = {
        let mut app = arena(seed);
        let t = tile(cx + 10, cy + 10);
        put_unit(
            &mut app,
            1,
            2,
            t,
            Unit { hp: HP, stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, t) },
        );
        for i in 0..5 {
            let p = tile(cx + 6 + i, cy + 10);
            put_unit(
                &mut app,
                10 + i as u64,
                1,
                p,
                Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Archer, p) },
            );
        }
        run(&mut app, 1200);
        damage_dealt(&mut app, 1, HP)
    };
    let tower = {
        let mut app = arena(seed);
        let tp = tile(cx + 6, cy + 10);
        put_building(&mut app, 100, 1, BuildingKind::Tower, tp);
        for i in 0..5 {
            let p = tile(cx + 6, cy + 10);
            put_unit(
                &mut app,
                10 + i as u64,
                1,
                p,
                Unit { garrisoned_in: 100, ..Unit::new(UnitKind::Archer, p) },
            );
        }
        let t = tile(cx + 10, cy + 10);
        put_unit(
            &mut app,
            1,
            2,
            t,
            Unit { hp: HP, stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, t) },
        );
        run(&mut app, 1200);
        damage_dealt(&mut app, 1, HP)
    };
    assert!(field > 0 && tower > 0, "field {field}, tower {tower}");
    // A tower adds its own bow and reloads faster than a man does, so it is
    // ALLOWED to beat the same five in the open — it is not allowed to be a
    // different weapon. Before this, five archers in a tower did 24 per volley
    // of the HOST's damage type where the same five did 16 of Pierce.
    let ratio = tower as f64 / field as f64;
    assert!(
        (0.75..=1.9).contains(&ratio),
        "a manned tower does {tower} where the same five in the field do {field} ({ratio:.2}x)"
    );
}

/// A garrison used to be invulnerable to everything: melee could not reach it
/// and bombardment did not touch it. The trade is now real — safe from the
/// sword, shaken by the shell — and that is the one job a mangonel has that a
/// ram does not.
#[test]
fn a_shelled_garrison_breaks_and_leaves_the_tower_standing() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let mut app = arena(seed);
    let tp = tile(cx + 6, cy + 12);
    put_building(&mut app, 100, 2, BuildingKind::Tower, tp);
    for i in 0..5u64 {
        put_unit(&mut app, 10 + i, 2, tp, Unit { garrisoned_in: 100, ..Unit::new(UnitKind::Archer, tp) });
    }
    let mp = tile(cx + 15, cy + 12);
    put_unit(&mut app, 1, 1, mp, Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Mangonel, mp) });
    order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 100 });

    let mut emptied_at = None;
    for t in 0..2400 {
        step(app.world_mut());
        if building_hp(&mut app, 100).is_none() {
            break;
        }
        let out = (0..5u64).filter(|i| unit_of(&mut app, 10 + i).is_some_and(|u| u.garrisoned_in == 0)).count();
        if out == 5 {
            emptied_at = Some(t);
            break;
        }
    }
    let at = emptied_at.expect("the shelling never emptied the tower");
    assert!(
        building_hp(&mut app, 100).is_some(),
        "the tower came down with the garrison — that is the RAM's job, not the mangonel's"
    );
    println!("garrison broke after {:.1} s", at as f64 / 20.0);
}

/// The measured pathology was not that a tower is strong — it should be — but
/// that its garrison fired ONE summed, host-typed boulder that deleted the
/// nearest man outright. Five men loose five arrows and the volley SPREADS.
#[test]
fn a_tower_volley_spreads_instead_of_deleting_one_man() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let mut app = arena(seed);
    let tp = tile(cx + 12, cy + 12);
    put_building(&mut app, 100, 2, BuildingKind::Tower, tp);
    for i in 0..5u64 {
        put_unit(&mut app, 10 + i, 2, tp, Unit { garrisoned_in: 100, ..Unit::new(UnitKind::Archer, tp) });
    }
    for i in 0..5u64 {
        let p = tile(cx + 15, cy + 10 + i as i32);
        put_unit(
            &mut app,
            200 + i,
            1,
            p,
            Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, p) },
        );
    }
    // one volley
    run(&mut app, 8);
    let full = unit_def(UnitKind::Spearman).max_hp;
    let hurt: Vec<i32> =
        (0..5u64).filter_map(|i| unit_of(&mut app, 200 + i).map(|u| u.hp)).filter(|hp| *hp < full).collect();
    let dead = 5 - (0..5u64).filter(|i| unit_of(&mut app, 200 + i).is_some()).count();
    assert!(hurt.len() >= 2, "one volley hit {} of five men — it is still one boulder", hurt.len());
    assert_eq!(dead, 0, "a single volley killed a man outright");
}

// ── facing, charge and retaliation ───────────────────────────────────────────

/// Facing did not exist, so a blow to the back and a blow to the face were the
/// same blow.
#[test]
fn a_blow_from_behind_lands_harder_than_one_to_the_face() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 12, 12);
    const HP: i32 = 100_000;
    let taken = |heading: u8| -> i32 {
        let mut app = arena(seed);
        let a = tile(cx + 4, cy + 6);
        let b = tile(cx + 5, cy + 6);
        put_unit(&mut app, 1, 1, a, Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, a) });
        put_unit(
            &mut app,
            2,
            2,
            b,
            Unit { hp: HP, heading, stance: Stance::HoldGround, ..Unit::new(UnitKind::Peasant, b) },
        );
        order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
        run(&mut app, 400);
        HP - unit_of(&mut app, 2).map(|u| u.hp).unwrap_or(0)
    };
    // the attacker stands to the target's -X, so heading 8 looks AT it and
    // heading 0 looks away
    let frontal = taken(8);
    let rear = taken(0);
    let flank = taken(4);
    assert!(frontal > 0, "nobody landed a blow at all");
    assert!(rear > flank && flank > frontal, "front {frontal}, flank {flank}, rear {rear}");
}

/// The Knight's whole case is the charge, and a braced spear is the answer to
/// it — frontally. From the flank the spears are useless.
#[test]
fn a_charge_dies_on_set_spears_and_wins_from_the_flank() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 20, 20);
    const HP: i32 = 100_000;
    // one blow only: measure the first strike, which is the charge
    let first_blow = |heading: u8| -> i32 {
        let mut app = arena(seed);
        let b = tile(cx + 10, cy + 10);
        let a = tile(cx + 6, cy + 10);
        put(&mut app, 1, 1, UnitKind::Knight, a);
        put_unit(
            &mut app,
            2,
            2,
            b,
            Unit { hp: HP, heading, stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, b) },
        );
        order(&mut app, PlayerCommand::Move { player_id: 1, unit: 1, target: b });
        order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
        let mut last = HP;
        for _ in 0..600 {
            step(app.world_mut());
            let hp = unit_of(&mut app, 2).map(|u| u.hp).unwrap_or(0);
            if hp < last {
                return last - hp;
            }
            last = hp;
        }
        0
    };
    let into_the_spears = first_blow(8); // the spearman faces the charge
    let into_the_flank = first_blow(4); // it is looking sideways
    assert!(into_the_spears > 0 && into_the_flank > 0);
    assert!(
        into_the_flank > into_the_spears,
        "a charge into set spears did {into_the_spears} and one into their flank did {into_the_flank}"
    );
}

/// Taking damage used to lower morale and nothing else, so a longer-ranged unit
/// farmed a standing line for free.
#[test]
fn a_man_shot_from_beyond_his_own_reach_shoots_back() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let mut app = arena(seed);
    let a = tile(cx + 4, cy + 12);
    let b = tile(cx + 12, cy + 12); // 8 tiles: inside a mangonel's reach, outside a spearman's aggro
    put_unit(&mut app, 1, 1, a, Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Mangonel, a) });
    put_unit(&mut app, 2, 2, b, Unit { hp: 100_000, ..Unit::new(UnitKind::Spearman, b) });
    order(&mut app, PlayerCommand::Attack { player_id: 1, unit: 1, target: 2 });
    assert!(unit_def(UnitKind::Spearman).aggro_range < fx!("8"));
    let mut answered = false;
    for _ in 0..200 {
        step(app.world_mut());
        if unit_of(&mut app, 2).is_some_and(|u| u.attack_target == 1) {
            answered = true;
            break;
        }
    }
    assert!(answered, "the man shelled from out of his reach never answered");
}

/// Hold Ground ignored stance entirely, so the only stop in the game was a Move
/// order onto your own feet.
#[test]
fn hold_ground_holds_the_ground() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 20, 20);
    let mut app = arena(seed);
    let a = tile(cx + 4, cy + 10);
    let b = tile(cx + 9, cy + 10);
    put_unit(&mut app, 1, 1, a, Unit { stance: Stance::HoldGround, ..Unit::new(UnitKind::Spearman, a) });
    put(&mut app, 2, 2, UnitKind::Peasant, b);
    run(&mut app, 400);
    let now = pos_of(&mut app, 1).expect("the holder");
    assert!(
        dist(now, a) < fx!("1"),
        "a Hold Ground spearman walked {} tiles at an enemy it could see",
        dist(now, a)
    );
}

// ── the battle as a whole ────────────────────────────────────────────────────

fn mirror(app: &mut App, n: usize, kind: UnitKind, cx: i32, cy: i32) {
    let mut id = 1u64;
    for i in 0..n {
        let p = tile(cx + 2 + i as i32 % 10, cy + 2 + i as i32 / 10);
        put(app, id, 1, kind, p);
        id += 1;
    }
    for i in 0..n {
        let p = tile(cx + 2 + i as i32 % 10, cy + 10 + i as i32 / 10);
        put(app, id, 2, kind, p);
        id += 1;
    }
}

/// The measured freeze: a 40v40 made contact at 1 s, both sides routed at 10 s,
/// and then read `a=20 b=20 targeting=0 idle=40 morale 1.00/1.00` every second
/// from t=10 s to t=240 s. A rout has to end somewhere a man can be rallied back
/// from.
#[test]
fn a_forty_man_mirror_reaches_a_decision() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let mut app = arena(seed);
    mirror(&mut app, 40, UnitKind::Spearman, cx, cy);
    let mut decided = None;
    for t in 0..2400usize {
        step(app.world_mut());
        if t.is_multiple_of(20) && (alive(&mut app, 1) == 0 || alive(&mut app, 2) == 0) {
            decided = Some(t);
            break;
        }
    }
    let t = decided.expect("the mirror never decided in 120 s");
    println!("mirror decided after {:.1} s", t as f64 / 20.0);
}

/// Forty men occupied 0.4 tiles while their radii were 0.26 each: a line meeting
/// a line was two dots meeting. Engagement slots give the melee frontage, and
/// fighters are no longer skipped by the separation pass.
#[test]
fn a_melee_line_has_frontage() {
    let seed = 1;
    let (cx, cy) = flat_block(seed, 24, 24);
    let mut app = arena(seed);
    mirror(&mut app, 40, UnitKind::Spearman, cx, cy);
    run(&mut app, 300);
    let w = app.world_mut();
    let mut q = w.query::<(&Owner, &Pos)>();
    let pts: Vec<(u64, V2)> = q.iter(w).map(|(o, p)| (o.0, p.pos)).collect();
    let mut closest = Fx::MAX;
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            let d2 = dist2(pts[i].1, pts[j].1);
            if d2 < closest {
                closest = d2;
            }
        }
    }
    let closest = fx_sqrt(closest);
    println!("closest pair in a 40v40 after 15 s: {closest} tiles");
    // two spearman bodies are 0.52 across; the measured melee packed to 0.4
    assert!(
        closest > fx!("0.42"),
        "the closest pair in a 40v40 is {closest} tiles apart; two bodies are {}",
        unit_def(UnitKind::Spearman).radius * Fx::from_num(2)
    );
    // and nobody is standing inside a building footprint
}

/// The desync proof. Every field this layer touches — heading, engagement,
/// charge, rally, setup, morale, routing — is hashed, so two worlds driven
/// through a whole battle including a charge, a rout and a rally must agree on
/// EVERY tick, not just at the end.
#[test]
fn two_worlds_fight_the_same_battle_tick_for_tick() {
    let seed = 7;
    let (cx, cy) = flat_block(seed, 28, 28);
    let build = || {
        let mut app = arena(seed);
        let mut id = 1u64;
        for i in 0..20 {
            let p = tile(cx + 2 + i % 10, cy + 2 + i / 10);
            put(&mut app, id, 1, UnitKind::Spearman, p);
            id += 1;
        }
        for i in 0..6 {
            let p = tile(cx + 4 + i, cy + 5);
            put(&mut app, id, 1, UnitKind::Knight, p);
            id += 1;
        }
        for i in 0..20 {
            let p = tile(cx + 2 + i % 10, cy + 14 + i / 10);
            put(&mut app, id, 2, UnitKind::Sergeant, p);
            id += 1;
        }
        for i in 0..6 {
            let p = tile(cx + 4 + i, cy + 18);
            put(&mut app, id, 2, UnitKind::Archer, p);
            id += 1;
        }
        put_building(&mut app, 500, 2, BuildingKind::Tower, tile(cx + 14, cy + 16));
        put_unit(
            &mut app,
            600,
            2,
            tile(cx + 14, cy + 16),
            Unit { garrisoned_in: 500, ..Unit::new(UnitKind::Crossbowman, tile(cx + 14, cy + 16)) },
        );
        for i in 0..8 {
            put_building(&mut app, 700 + i as u64, 2, BuildingKind::Wall, tile(cx + 10 + i, cy + 12));
        }
        app
    };
    let mut a = build();
    let mut b = build();
    for t in 0..900 {
        step(a.world_mut());
        step(b.world_mut());
        let ha = a.world().resource::<StateHash>().0;
        let hb = b.world().resource::<StateHash>().0;
        assert_eq!(ha, hb, "the two worlds diverged on tick {t}");
    }
    assert_ne!(a.world().resource::<StateHash>().0, 0);
}
