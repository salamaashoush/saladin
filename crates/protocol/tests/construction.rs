//! Construction, repair and production: the lifecycle a building never had.
//!
//! A Build order founds a SITE — paid for, frail and inert. Peasants walk to it
//! and raise it; more hands finish sooner but never linearly; a raider can burn
//! a half-built hall because its health is the labour banked so far. Repair runs
//! the same loop backwards through the stockpile, and a production queue means
//! five orders take five orders' worth of time.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildState, BuildingKind, Faction, Fx, MAX_BUILDERS,
    Stockpile, UnitKind, V2, ZERO, building_def, compose_seed, is_buildable_tile, unit_def,
};

fn seed() -> u32 {
    compose_seed(11, 1)
}

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: seed() });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 9000, stone: 9000, food: 9000, gold: 9000 },
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
    let def = building_def(BuildingKind::Keep);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(BuildingKind::Keep, def.max_hp, pos),
    ));
}

fn peasants(app: &mut App, owner: u64, at: V2, n: u64, first: u64) -> Vec<u64> {
    let def = unit_def(UnitKind::Peasant);
    (0..n)
        .map(|i| {
            let id = first + i;
            let pos = V2::new(at.x + Fx::from_num(2 + i as i32), at.y + Fx::from_num(2));
            app.world_mut().spawn((
                GameId(id),
                Owner(owner),
                MatchId(1),
                Pos { pos, facing: ZERO },
                Unit {
                    speed: def.speed,
                    hp: def.max_hp,
                    ..Unit::new(UnitKind::Peasant, pos)
                },
            ));
            id
        })
        .collect()
}

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + saladin_sim::fx!("0.5"), Fx::from_num(ty) + saladin_sim::fx!("0.5"))
}

/// A wide patch of buildable ground with room for a keep and a site beside it.
fn open_block() -> (i32, i32) {
    let s = seed();
    for cy in 16..saladin_sim::WORLD_SIZE - 20 {
        for cx in 16..saladin_sim::WORLD_SIZE - 20 {
            if (-6..8).all(|dx| (-6..8).all(|dy| is_buildable_tile(s, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no open block");
}

fn find_building(app: &mut App, kind: BuildingKind) -> Option<(u64, Building)> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Building)>();
    q.iter(world).find(|(_, b)| b.kind == kind).map(|(g, b)| (g.0, *b))
}

fn stock(app: &mut App) -> Stockpile {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).next().unwrap().stock
}

/// Site a Barracks with `n` hands and return the tick it completed on, or None.
fn ticks_to_raise(n: u64, limit: u32) -> Option<u32> {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    let hands = peasants(&mut app, 1, at, n, 100);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Barracks,
        pos: at,
        facing: 0,
        builders: hands,
    });
    for t in 0..limit {
        step(app.world_mut());
        if find_building(&mut app, BuildingKind::Barracks).is_some_and(|(_, b)| b.complete()) {
            return Some(t);
        }
    }
    None
}

/// The defining property of the whole redesign: paying for a building is not
/// the same as having one.
#[test]
fn a_site_with_no_hands_never_rises() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Barracks,
        pos: at,
        facing: 0,
        builders: vec![],
    });
    step(app.world_mut());

    let (_, b) = find_building(&mut app, BuildingKind::Barracks).expect("the order founded a site");
    assert_eq!(b.state, BuildState::Site, "a Build must found a site, not a building");
    let def = building_def(BuildingKind::Barracks);
    assert!(b.hp < def.max_hp, "a foundation must be frail ({} of {})", b.hp, def.max_hp);

    for _ in 0..600 {
        step(app.world_mut());
    }
    let (_, b) = find_building(&mut app, BuildingKind::Barracks).expect("still standing");
    assert_eq!(b.state, BuildState::Site, "an unmanned site raised itself");
    assert_eq!(b.work, Fx::ZERO, "labour appeared out of nowhere");
}

/// More hands finish sooner, and the curve bends: eight peasants are worth far
/// less than eight times one, or the only build order is "send everyone".
#[test]
fn hands_speed_the_work_with_diminishing_returns() {
    let one = ticks_to_raise(1, 3000).expect("one peasant must eventually finish a barracks");
    let three = ticks_to_raise(3, 3000).expect("three peasants must finish a barracks");
    assert!(three < one, "three hands ({three}) were no faster than one ({one})");

    let eight = ticks_to_raise(MAX_BUILDERS as u64, 3000).expect("a full crew must finish");
    assert!(eight <= three, "eight hands ({eight}) were slower than three ({three})");
    assert!(
        eight * 8 > one,
        "eight hands ({eight}) were EIGHT times one ({one}) - the curve is linear"
    );
}

/// Health is authoritative and additive: work adds it, damage subtracts it. A
/// site under fire needs no special case, and progress is never un-done.
#[test]
fn damage_takes_health_from_a_site_but_never_its_progress() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    let hands = peasants(&mut app, 1, at, 2, 100);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Barracks,
        pos: at,
        facing: 0,
        builders: hands,
    });
    for _ in 0..120 {
        step(app.world_mut());
    }
    let (id, before) = find_building(&mut app, BuildingKind::Barracks).unwrap();
    assert_eq!(before.state, BuildState::Site);
    assert!(before.work > Fx::ZERO, "the crew banked no labour at all");

    // a raider's blow, applied straight to the row the way combat applies it
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Building)>();
        let (_, mut b) = q.iter_mut(world).find(|(g, _)| g.0 == id).unwrap();
        b.hp -= 40;
    }
    let hurt = find_building(&mut app, BuildingKind::Barracks).unwrap().1;
    step(app.world_mut());
    let after = find_building(&mut app, BuildingKind::Barracks).unwrap().1;

    assert!(after.work >= hurt.work, "damage rolled back construction progress");
    assert!(after.hp < before.hp + 40, "the blow did not land");
}

/// A mis-click costs you the labour, not the levy. Cancelling hands back what
/// has not been built yet.
#[test]
fn cancelling_a_site_refunds_the_unspent_remainder() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    let before = stock(&mut app);
    let hands = peasants(&mut app, 1, at, 2, 100);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Barracks,
        pos: at,
        facing: 0,
        builders: hands,
    });
    step(app.world_mut());
    let cost = building_def(BuildingKind::Barracks).cost;
    assert_eq!(stock(&mut app).wood, before.wood - cost.wood, "the site was not paid for");

    // run until the job is roughly a third done, then abandon it
    let id = loop {
        step(app.world_mut());
        let (id, b) = find_building(&mut app, BuildingKind::Barracks).expect("site alive");
        assert_eq!(b.state, BuildState::Site, "the site finished before the test could cancel");
        if b.work > saladin_sim::fx!("0.3") {
            break id;
        }
    };
    let work = find_building(&mut app, BuildingKind::Barracks).unwrap().1.work;
    cmd(&mut app, PlayerCommand::CancelSite { player_id: 1, building: id });
    step(app.world_mut());

    assert!(find_building(&mut app, BuildingKind::Barracks).is_none(), "the site outlived cancel");
    let back = stock(&mut app).wood - (before.wood - cost.wood);
    let expected = saladin_sim::cancel_refund(&cost, work).wood;
    assert_eq!(back, expected, "cancel refunded {back} of {}, expected {expected}", cost.wood);
    assert!(back < cost.wood, "cancelling was free - the sunk labour cost nothing");
    assert!(back > 0, "cancelling refunded nothing at all");

    // the crew is released, not left standing at a hole that no longer exists
    let world = app.world_mut();
    let mut q = world.query::<&Unit>();
    assert!(
        q.iter(world).all(|u| u.job_site != id),
        "a builder is still assigned to the cancelled site"
    );
}

/// Damage was permanent for the whole match. It is not any more, and the mend
/// is charged against the stockpile so a battered wall is a real bill.
#[test]
fn a_damaged_building_is_mended_and_the_owner_pays_for_it() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    let hands = peasants(&mut app, 1, at, 3, 100);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Barracks,
        pos: at,
        facing: 0,
        builders: hands.clone(),
    });
    let id = loop {
        step(app.world_mut());
        let (id, b) = find_building(&mut app, BuildingKind::Barracks).expect("site alive");
        if b.complete() {
            break id;
        }
    };
    let max_hp = building_def(BuildingKind::Barracks).max_hp;
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Building)>();
        let (_, mut b) = q.iter_mut(world).find(|(g, _)| g.0 == id).unwrap();
        b.hp = max_hp / 4;
    }
    let purse = stock(&mut app);
    for u in &hands {
        cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: *u, building: id });
    }
    for _ in 0..600 {
        step(app.world_mut());
        if find_building(&mut app, BuildingKind::Barracks).unwrap().1.hp >= max_hp {
            break;
        }
    }
    let healed = find_building(&mut app, BuildingKind::Barracks).unwrap().1;
    assert_eq!(healed.hp, max_hp, "the mend never reached full health");
    assert_eq!(healed.state, BuildState::Complete, "mending must not re-open the site");
    let after = stock(&mut app);
    assert!(after.wood < purse.wood, "the repair was free");
    let cost = building_def(BuildingKind::Barracks).cost;
    assert!(
        purse.wood - after.wood <= cost.wood,
        "mending cost more than building anew ({} vs {})",
        purse.wood - after.wood,
        cost.wood
    );
}

/// Five clicks used to produce five units in ONE tick: nothing to watch,
/// nothing to cancel and no pacing lever anywhere.
#[test]
fn a_production_queue_takes_time_and_hands_back_a_cancelled_order() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    let keep_at = center(cx - 4, cy - 4);
    spawn_keep(&mut app, 10, 1, keep_at);

    let peasant_cost = unit_def(UnitKind::Peasant).cost;
    let before = stock(&mut app);
    for _ in 0..5 {
        cmd(&mut app, PlayerCommand::TrainAt {
            player_id: 1,
            building: 10,
            kind: UnitKind::Peasant,
        });
    }
    step(app.world_mut());

    let world = app.world_mut();
    let mut uq = world.query::<&Unit>();
    assert_eq!(uq.iter(world).count(), 0, "five orders produced units in the same tick");
    let (_, b) = find_building(&mut app, BuildingKind::Keep).unwrap();
    assert_eq!(b.queue_len, 5, "the orders did not reach the queue");
    assert_eq!(
        stock(&mut app).food,
        before.food - peasant_cost.food * 5,
        "the queue must be paid for at ENQUEUE or a five-deep queue walks the pop cap"
    );

    // drop the last order: it comes back in full
    cmd(&mut app, PlayerCommand::CancelTrain { player_id: 1, building: 10 });
    step(app.world_mut());
    let (_, b) = find_building(&mut app, BuildingKind::Keep).unwrap();
    assert_eq!(b.queue_len, 4, "cancel did not shorten the queue");
    assert_eq!(
        stock(&mut app).food,
        before.food - peasant_cost.food * 4,
        "a cancelled order was not refunded"
    );

    for _ in 0..1200 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<&Unit>();
        if q.iter(world).count() >= 4 {
            break;
        }
    }
    let world = app.world_mut();
    let mut q = world.query::<&Unit>();
    assert_eq!(q.iter(world).count(), 4, "the queue never drained");
    let (_, b) = find_building(&mut app, BuildingKind::Keep).unwrap();
    assert_eq!(b.queue_len, 0);
    assert_eq!(b.train_work, Fx::ZERO, "a drained queue left training progress banked");
}

/// The whole lifecycle under the desync detector: two worlds found the same
/// site, crew it, finish it and queue from it, and their hashes never part.
#[test]
fn construction_keeps_two_worlds_in_lockstep() {
    let (cx, cy) = open_block();
    let at = center(cx + 2, cy + 2);
    let mut worlds: Vec<App> = (0..2)
        .map(|_| {
            let mut app = build_app();
            spawn_player(&mut app, 1);
            spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
            let hands = peasants(&mut app, 1, at, 3, 100);
            cmd(&mut app, PlayerCommand::Build {
                player_id: 1,
                kind: BuildingKind::Barracks,
                pos: at,
                facing: 0,
                builders: hands,
            });
            app
        })
        .collect();

    let mut queued = false;
    for t in 0..700 {
        for app in &mut worlds {
            step(app.world_mut());
        }
        let a = worlds[0].world().resource::<StateHash>().0;
        let b = worlds[1].world().resource::<StateHash>().0;
        assert_eq!(a, b, "construction desynced the two worlds at tick {t}");

        if !queued && find_building(&mut worlds[0], BuildingKind::Barracks).unwrap().1.complete() {
            queued = true;
            for app in &mut worlds {
                cmd(app, PlayerCommand::TrainAt {
                    player_id: 1,
                    building: 10,
                    kind: UnitKind::Peasant,
                });
            }
        }
    }
    assert!(queued, "the barracks never finished, so the test proved nothing");
}

/// `state_hash` folded only a building's hp and demanded a `Pos`, so every
/// Player row was invisible: adding 12345 wood left the checksum bit-identical.
/// That is the detector blind to payments, refunds and repair charges - which is
/// most of what a construction system does.
#[test]
fn the_desync_detector_sees_the_stockpile_and_the_tech_tree() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    step(app.world_mut());
    let base = app.world().resource::<StateHash>().0;

    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        q.iter_mut(world).next().unwrap().stock.wood += 12345;
    }
    step(app.world_mut());
    let after_wood = app.world().resource::<StateHash>().0;
    assert_ne!(base, after_wood, "12345 wood did not move the state hash");

    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        q.iter_mut(world).next().unwrap().tech_mask = 0xff;
    }
    step(app.world_mut());
    assert_ne!(after_wood, app.world().resource::<StateHash>().0, "the tech tree is not hashed");

    // and the lifecycle itself: a site and a finished hall must never collide
    let hp = {
        let world = app.world_mut();
        let mut q = world.query::<&mut Building>();
        let mut b = q.iter_mut(world).next().unwrap();
        b.state = BuildState::Site;
        b.hp
    };
    step(app.world_mut());
    let as_site = app.world().resource::<StateHash>().0;
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Building>();
        let mut b = q.iter_mut(world).next().unwrap();
        b.state = BuildState::Complete;
        b.hp = hp;
    }
    step(app.world_mut());
    let complete = app.world().resource::<StateHash>().0;
    assert_ne!(as_site, complete, "build state is not hashed");

    // the rally flag is command-driven sim state - it decides where every unit
    // this hall trains walks to, so a peer that missed the order diverges in
    // unit positions with the checksum still agreeing
    cmd(&mut app, PlayerCommand::SetRally {
        player_id: 1,
        building: 10,
        target: center(cx + 6, cy + 6),
    });
    step(app.world_mut());
    let rallied = app.world().resource::<StateHash>().0;
    assert_ne!(complete, rallied, "the rally flag is not hashed");

    // and what an upgrade is turning INTO
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Building>();
        q.iter_mut(world).next().unwrap().target_kind = BuildingKind::Watchtower;
    }
    step(app.world_mut());
    assert_ne!(rallied, app.world().resource::<StateHash>().0, "the upgrade target is not hashed");
}

/// Masonry thickens the walls you ALREADY have. Max hp is DERIVED from the tech
/// mask, so without a retro-apply pass the tech raises every ceiling and lays no
/// stone: every building you own would simply start reading as damaged, which is
/// the opposite of what the research table promises.
#[test]
fn a_structural_tech_hardens_the_walls_already_standing() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let smith_at = center(cx + 2, cy + 2);
    let sdef = building_def(BuildingKind::Blacksmith);
    app.world_mut().spawn((
        GameId(11),
        Owner(1),
        MatchId(1),
        Pos { pos: smith_at, facing: ZERO },
        Building::new(BuildingKind::Blacksmith, sdef.max_hp, smith_at),
    ));
    // a site of the same kind, to prove the pass does NOT heal a foundation
    let site_at = center(cx + 6, cy + 2);
    app.world_mut().spawn((
        GameId(12),
        Owner(1),
        MatchId(1),
        Pos { pos: site_at, facing: ZERO },
        Building::site(BuildingKind::Blacksmith, sdef.max_hp, site_at),
    ));

    let before_keep = building_by_id(&mut app, 10).unwrap().hp;
    let before_site = building_by_id(&mut app, 12).unwrap().hp;
    let masonry = saladin_sim::Tech::Masonry as u8;
    let delta = saladin_sim::building_hp_delta(
        0,
        saladin_sim::set_tech(0, saladin_sim::Tech::Masonry),
        BuildingKind::Keep,
    );
    assert!(delta > 0, "Masonry must raise a keep's ceiling for this test to mean anything");

    cmd(&mut app, PlayerCommand::StartResearch { player_id: 1, building: 11, tech: masonry });
    for _ in 0..4000 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<&Research>();
        if q.iter(world).any(|r| r.done) {
            break;
        }
    }
    let world = app.world_mut();
    let mut q = world.query::<&Research>();
    assert!(q.iter(world).any(|r| r.done), "Masonry never finished");

    let after_keep = building_by_id(&mut app, 10).unwrap();
    assert_eq!(
        after_keep.hp,
        before_keep + delta,
        "Masonry raised the ceiling but laid no stone - the keep now reads as damaged"
    );
    assert_eq!(
        after_keep.hp,
        saladin_sim::effective_building_def(BuildingKind::Keep, after_keep_mask(&mut app)).max_hp,
        "a hardened keep must stand at FULL health, not below its new ceiling"
    );
    assert_eq!(
        building_by_id(&mut app, 12).unwrap().hp,
        before_site,
        "a foundation gained stone nobody had laid yet"
    );
}

fn building_by_id(app: &mut App, id: u64) -> Option<Building> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Building)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, b)| *b)
}

fn after_keep_mask(app: &mut App) -> u64 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).next().unwrap().tech_mask
}

/// The whole opening played through the command surface a human clicks, in one
/// world, in order: found a hall, watch a crew raise it, train out of it, burn
/// it, mend it, and have the SAME crew walk to the next foundation without a
/// second order. Every step is covered somewhere in isolation; nothing checks
/// that they compose, and the crew hand-off is what makes a wall drag work.
#[test]
fn a_town_is_founded_manned_burned_mended_and_moves_on() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx - 4, cy - 4));
    let hall = center(cx + 2, cy + 2);
    let crew = peasants(&mut app, 1, hall, 3, 100);

    // 1. a Build order is a FOUNDATION: paid for, frail, and it unlocks nothing
    let purse = stock(&mut app);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Barracks,
        pos: hall,
        facing: 0,
        builders: crew.clone(),
    });
    step(app.world_mut());
    let def = building_def(BuildingKind::Barracks);
    let (hall_id, b) = find_building(&mut app, BuildingKind::Barracks).expect("site founded");
    assert_eq!(b.state, BuildState::Site);
    assert_eq!(stock(&mut app).wood, purse.wood - def.cost.wood, "a site is paid for up front");
    cmd(&mut app, PlayerCommand::TrainAt {
        player_id: 1,
        building: hall_id,
        kind: UnitKind::Spearman,
    });
    step(app.world_mut());
    assert_eq!(
        find_building(&mut app, BuildingKind::Barracks).unwrap().1.queue_len,
        0,
        "a hole in the ground took a training order"
    );

    // 2. the crew raises it
    for _ in 0..2000 {
        step(app.world_mut());
        if find_building(&mut app, BuildingKind::Barracks).unwrap().1.complete() {
            break;
        }
    }
    let b = find_building(&mut app, BuildingKind::Barracks).unwrap().1;
    assert_eq!(b.state, BuildState::Complete, "three hands never finished the hall");
    assert_eq!(b.hp, def.max_hp, "a finished hall stands at full health");
    assert_eq!(b.builders, 0, "a finished hall still has a crew on the books");

    // 3. and now it produces, to the rally flag
    let rally = center(cx + 8, cy + 2);
    cmd(&mut app, PlayerCommand::SetRally { player_id: 1, building: hall_id, target: rally });
    cmd(&mut app, PlayerCommand::TrainAt {
        player_id: 1,
        building: hall_id,
        kind: UnitKind::Spearman,
    });
    step(app.world_mut());
    assert_eq!(find_building(&mut app, BuildingKind::Barracks).unwrap().1.queue_len, 1);
    let spearman = {
        let mut found = None;
        for _ in 0..1200 {
            step(app.world_mut());
            let world = app.world_mut();
            let mut q = world.query::<(&GameId, &Unit)>();
            if let Some((g, _)) = q.iter(world).find(|(_, u)| u.kind == UnitKind::Spearman) {
                found = Some(g.0);
                break;
            }
        }
        found.expect("the queued spearman never mustered")
    };
    let marching = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).find(|(g, _)| g.0 == spearman).map(|(_, u)| u.has_target).unwrap()
    };
    assert!(marching, "a trained unit ignored the rally flag");

    // 4. a raid takes it to a quarter health, and the same hands mend it
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Building)>();
        let (_, mut row) = q.iter_mut(world).find(|(g, _)| g.0 == hall_id).unwrap();
        row.hp = def.max_hp / 4;
    }
    let before_mend = stock(&mut app);
    for u in &crew {
        cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: *u, building: hall_id });
    }
    for _ in 0..900 {
        step(app.world_mut());
        if find_building(&mut app, BuildingKind::Barracks).unwrap().1.hp >= def.max_hp {
            break;
        }
    }
    assert_eq!(
        find_building(&mut app, BuildingKind::Barracks).unwrap().1.hp,
        def.max_hp,
        "the hall was never mended"
    );
    assert!(stock(&mut app).wood < before_mend.wood, "mending was free");

    // 5. a new foundation in reach takes the crew over on its own — no order.
    // This is what walks a dragged wall line segment by segment.
    let tower = center(cx + 6, cy + 6);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Tower,
        pos: tower,
        facing: 0,
        builders: vec![],
    });
    step(app.world_mut());
    let (tower_id, _) = find_building(&mut app, BuildingKind::Tower).expect("tower site founded");
    for _ in 0..2000 {
        step(app.world_mut());
        if find_building(&mut app, BuildingKind::Tower).unwrap().1.complete() {
            break;
        }
    }
    let t = find_building(&mut app, BuildingKind::Tower).unwrap().1;
    assert_eq!(
        t.state,
        BuildState::Complete,
        "the idle crew never walked to the next foundation in reach"
    );

    // 6. and the finished tower is RAISED into a watchtower, in place
    cmd(&mut app, PlayerCommand::UpgradeBuilding { player_id: 1, building: tower_id });
    step(app.world_mut());
    assert_eq!(building_by_id(&mut app, tower_id).unwrap().state, BuildState::Upgrading);
    for u in &crew {
        cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: *u, building: tower_id });
    }
    for _ in 0..2000 {
        step(app.world_mut());
        if building_by_id(&mut app, tower_id).unwrap().kind == BuildingKind::Watchtower {
            break;
        }
    }
    let w = building_by_id(&mut app, tower_id).expect("the tower kept its GameId");
    assert_eq!(w.kind, BuildingKind::Watchtower, "the upgrade never finished");
    assert_eq!(w.state, BuildState::Complete);
    assert_eq!(w.hp, building_def(BuildingKind::Watchtower).max_hp);
}

/// `approach_tile` answers "a passable tile beside the job" — NOT "one this
/// hand can get to". It picks the tile closest to the job and breaks ties on
/// the side the walker is already on, so a dead-end pocket on the near side
/// beats an open approach on the far one. `walk_to` then handed the A* an
/// unreachable target, got nothing back, and dropped the crew: the foundation
/// stood at zero work forever with its cost already paid.
///
/// Measured on seed 4 — a farm founded on a tongue of land sealed the two tiles
/// behind it, and three of five peasants went idle for the rest of the match.
#[test]
fn a_builder_routes_to_the_far_approach_when_the_near_one_is_a_dead_end() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(cx, cy + 4));

    // the site, with its west neighbour walled into a one-tile dead end
    let (sx, sy) = (cx, cy);
    for (i, (dx, dy)) in [(-2, 0), (-1, -1), (-1, 1)].into_iter().enumerate() {
        let at = center(sx + dx, sy + dy);
        let def = building_def(BuildingKind::Wall);
        app.world_mut().spawn((
            GameId(20 + i as u64),
            Owner(1),
            MatchId(1),
            Pos { pos: at, facing: ZERO },
            Building::new(BuildingKind::Wall, def.max_hp, at),
        ));
    }
    // the hand stands WEST of the seal, so the dead-end tile wins the tie-break
    let hand_at = center(sx - 3, sy);
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: hand_at, facing: ZERO },
        Unit::new(UnitKind::Peasant, hand_at),
    ));

    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Tower,
        pos: center(sx, sy),
        facing: 0,
        builders: vec![30],
    });
    for _ in 0..400 {
        step(app.world_mut());
    }

    let (tower_id, tower) =
        find_building(&mut app, BuildingKind::Tower).expect("the tower was founded");
    let hand = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).find(|(g, _)| g.0 == 30).map(|(_, u)| u.clone()).expect("the hand")
    };
    assert_eq!(hand.job_site, tower_id, "the hand was dropped instead of routed round");
    assert!(
        tower.work > Fx::ZERO || tower.state != BuildState::Site,
        "the crew never reached the site: work {}",
        tower.work
    );
}
