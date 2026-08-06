//! FIELD LABOUR: the wheel a farmhand runs on his own — tend, reap, haul, tend.
//!
//! A rock's output is a function of how many hands you put on it. A field's is a
//! function of TIME AND CARE, and hands only set the pace and carry the result
//! home. That means a farm needs a CREW, not a click: the men who raised the plot
//! stay in it, tend the crop, cut it themselves when it comes in, walk the haul
//! to the nearest drop-off and go straight back to the furrows. The player's only
//! input is how many hands he leaves there — and taking them out is an explicit
//! order, never something the sim does behind his back.
//!
//! Kept apart from `farming.rs`, which owns the SEASON (growth, ripening,
//! lodging, re-sowing). This file owns the LABOUR crossing two systems:
//! `construction.rs` holds a hand in `Constructing`, `gather.rs` takes him out of
//! it when the crop is in and puts him back when the haul is banked. A bug in
//! that handoff strands peasants standing in a field forever, so every test here
//! watches for an idle man.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, FARM_REGEN_IDLE, Faction, Fx, GatherState, ResourceType, Stockpile, UnitKind, V2,
    WORLD_SIZE, ZERO, building_def, compose_seed, fx, is_buildable_tile, unit_def,
};

fn build_app(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"))
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

fn spawn_building(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

/// Hands, laid out on the clear ground `block` guarantees to the west of the
/// plot — never east of it, where the buildable window ends.
fn crew(app: &mut App, owner: u64, at: V2, n: u64, first: u64) -> Vec<u64> {
    let def = unit_def(UnitKind::Peasant);
    (0..n)
        .map(|i| {
            let id = first + i;
            let pos = V2::new(
                at.x - Fx::from_num(3 + (i % 3) as i32),
                at.y - Fx::from_num((i / 3) as i32),
            );
            app.world_mut().spawn((
                GameId(id),
                Owner(owner),
                MatchId(1),
                Pos { pos, facing: ZERO },
                Unit { speed: def.speed, hp: def.max_hp, ..Unit::new(UnitKind::Peasant, pos) },
            ));
            id
        })
        .collect()
}

/// A 2x2 farm block on soil worth sowing, with clear buildable room around it.
fn block(seed: u32) -> (i32, i32) {
    for cy in 12..WORLD_SIZE - 16 {
        for cx in 12..WORLD_SIZE - 16 {
            if !(-8..3).all(|dx| (-8..3).all(|dy| is_buildable_tile(seed, cx + dx, cy + dy))) {
                continue;
            }
            let c = center(cx, cy);
            if saladin_sim::soil_quality(seed, 2, c.x, c.y)
                > saladin_sim::FARM_MIN_FERTILITY + fx!("0.08")
            {
                return (cx, cy);
            }
        }
    }
    panic!("no fertile block on seed {seed}");
}

/// A standing farm on good soil, a drop-off at its edge, and the crew that
/// raised it still in the furrows. FARMING IS THE SHORT HAUL: this is the layout
/// the whole design is pitched at.
fn farmstead(seed: u32, hands: u64) -> (App, (i32, i32), Vec<u64>) {
    let (bx, by) = block(seed);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(bx - 5, by - 5));
    spawn_building(&mut app, 13, 1, BuildingKind::Storehouse, center(bx - 3, by + 2));
    let ids = crew(&mut app, 1, center(bx, by), hands, 20);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Farm,
        pos: center(bx, by),
        facing: 0,
        builders: ids.clone(),
    });
    let up = run_until(&mut app, 1200, |a| {
        let world = a.world_mut();
        let mut q = world.query::<&Building>();
        q.iter(world).any(|b| b.kind == BuildingKind::Farm && b.complete())
    });
    assert!(up.is_some(), "the crew never raised the farm");
    (app, (bx, by), ids)
}

fn run_until(app: &mut App, budget: u32, pred: impl Fn(&mut App) -> bool) -> Option<u32> {
    for t in 0..budget {
        step(app.world_mut());
        if pred(app) {
            return Some(t);
        }
    }
    None
}

fn farm_id(app: &mut App) -> u64 {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Building)>();
    q.iter(world).find(|(_, b)| b.kind == BuildingKind::Farm).map(|(g, _)| g.0).unwrap()
}

fn field_row(app: &mut App) -> (u64, ResourceNode, Crop) {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &ResourceNode, &FieldOf, Option<&Crop>)>();
    q.iter(world)
        .map(|(g, n, _, c)| (g.0, *n, c.copied().unwrap_or_default()))
        .next()
        .expect("no field on the map")
}

/// Hands standing in the fields, as the SIM counts them — the number growth is
/// actually charged against.
fn tending(app: &mut App) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).filter(|b| b.kind == BuildingKind::Farm).map(|b| b.builders).sum()
}

fn stock_food(app: &mut App) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == 1).unwrap().stock.food
}

fn stock_wood(app: &mut App) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == 1).unwrap().stock.wood
}

fn unit_of(app: &mut App, id: u64) -> Unit {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone()).unwrap()
}

fn idle_hands(app: &mut App, ids: &[u64]) -> Vec<u64> {
    ids.iter()
        .copied()
        .filter(|id| unit_of(app, *id).gather_state == GatherState::Idle)
        .collect()
}

/// Put the season where a test needs it. Growing a real crop is the point of
/// other tests; here it is a preamble to the assertion.
fn pin_crop(app: &mut App, remaining: i32, ripe: bool) {
    let world = app.world_mut();
    let mut q = world.query::<(&mut ResourceNode, &mut Crop, &FieldOf)>();
    for (mut n, mut c, _) in q.iter_mut(world) {
        n.remaining = remaining;
        c.ripe = ripe;
        c.standing = 0;
    }
}

fn timber(app: &mut App, id: u64, at: V2) {
    app.world_mut().spawn((
        GameId(id),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        ResourceNode::deposit(ResourceType::Wood, 4000),
    ));
}

/// A hand doing nothing at all, whatever state he is calling it. `Idle` is the
/// only one anything in the sim looks for, so it is the only one that ever gets
/// caught — a man stood in `Harvesting` over a crop that is not in is just as
/// out of work and NOTHING will find him.
fn work_shy(app: &mut App, ids: &[u64], unripe: &[u64]) -> Vec<u64> {
    ids.iter()
        .copied()
        .filter(|id| {
            let u = unit_of(app, *id);
            match u.gather_state {
                GatherState::Idle => true,
                GatherState::Harvesting => u.job_site == 0 && unripe.contains(&u.target_node),
                _ => false,
            }
        })
        .collect()
}

fn unripe_fields(app: &mut App) -> Vec<u64> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &ResourceNode, &FieldOf, &Crop)>();
    q.iter(world)
        .filter(|(_, n, _, c)| !c.ripe || n.remaining <= 0)
        .map(|(g, _, _, _)| g.0)
        .collect()
}

/// THE round trip, and the riskiest code in the whole design: it crosses two
/// systems, and a bug in it parks peasants in a field forever — which reads as a
/// WORSE version of the complaint that started this work.
///
/// Three seasons off one plot, nobody ordered to do anything after the farm goes
/// up, and not one peasant standing idle at any tick of it.
#[test]
fn a_farm_crew_tends_reaps_hauls_and_goes_back_to_tending() {
    let seed = compose_seed(11, 1);
    let (mut app, _, hands) = farmstead(seed, 3);
    let cap = field_row(&mut app).1.cap;
    let before = stock_food(&mut app);

    // completion zeroes the crew count; the next construction pass re-counts the
    // hands that never left
    let settled = run_until(&mut app, 400, |a| tending(a) > 0);
    assert!(settled.is_some(), "the crew that raised the plot walked off it");

    let mut seasons = 0;
    let mut was_ripe = false;
    let mut reaped = false;
    let mut tended_again_after_a_harvest = false;
    let mut ran = 0u32;
    for t in 0..12_000 {
        step(app.world_mut());
        ran = t + 1;
        let stuck = idle_hands(&mut app, &hands);
        assert!(stuck.is_empty(), "peasants {stuck:?} were left standing idle at tick {t}");
        let (_, _, c) = field_row(&mut app);
        if c.ripe && !was_ripe {
            seasons += 1;
        }
        // the crop came in and went DOWN, with nobody sending anybody anywhere
        if was_ripe && !c.ripe {
            reaped = true;
        }
        was_ripe = c.ripe;
        // ...and the same crew went back into the furrows afterwards
        if reaped && tending(&mut app) > 0 {
            tended_again_after_a_harvest = true;
        }
        if seasons >= 3 && tended_again_after_a_harvest && stock_food(&mut app) >= before + cap * 2 {
            break;
        }
    }
    assert!(seasons >= 3, "one plot ran {seasons} seasons, not three");
    assert!(reaped, "the crew tended a crop it never cut");
    assert!(tended_again_after_a_harvest, "the crew reaped once and never went back to work");
    assert!(
        stock_food(&mut app) >= before + cap * 2,
        "three seasons brought home {} against a {cap} field",
        stock_food(&mut app) - before
    );
    // and the wheel is still turning, not parked at the end of it
    assert_eq!(idle_hands(&mut app, &hands).len(), 0);

    // THE NUMBER. Sustained food per 1000 ticks (50 s of game time) over the
    // whole run, the first growing season included and the three-hand crew never
    // touched: measured 117, floored well under it. A rate floor is the only
    // assertion that catches the wheel turning but turning BADLY — a handoff that
    // costs a hand a whole extra walk home passes every state assertion above.
    let per_1000_ticks = (stock_food(&mut app) - before) * 1000 / ran as i32;
    assert!(
        per_1000_ticks >= 90,
        "one plot, one crew: {per_1000_ticks} food per 1000 ticks over {ran} ticks"
    );
}

/// The strand test. Six hands on ONE field, which is far more than it needs, for
/// long enough to cover several whole seasons: a hand with nothing to cut has to
/// have something to do, every single tick.
#[test]
fn no_hand_is_ever_left_standing_in_a_field() {
    let seed = compose_seed(11, 1);
    let (mut app, _, raised) = farmstead(seed, 3);
    let field = field_row(&mut app).0;
    let extra = crew(&mut app, 1, center(0, 0), 3, 60);
    // spawned beside the plot, ordered onto a crop that is not in yet: the order
    // has to MEAN something
    {
        let (bx, by) = block(seed);
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Pos)>();
        for (i, id) in extra.iter().enumerate() {
            for (g, mut p) in q.iter_mut(world) {
                if g.0 == *id {
                    p.pos = center(bx - 4, by - 2 - i as i32);
                }
            }
        }
    }
    for &u in &extra {
        cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: u, node: field });
    }
    let all: Vec<u64> = raised.iter().chain(extra.iter()).copied().collect();

    for t in 0..3000 {
        step(app.world_mut());
        let stuck = idle_hands(&mut app, &all);
        assert!(stuck.is_empty(), "peasants {stuck:?} had nothing to do at tick {t}");
    }
    assert!(stock_food(&mut app) > 9000, "six hands on a field brought nothing home");
}

/// A click that does nothing is worse than a wrong one. Ordering a peasant onto
/// a crop that is not in yet used to be a dead end: he walked over and stood
/// there. Now it is the tend order, which is the same committed-builder state
/// `Repair` produces — no new command verb.
#[test]
fn an_order_on_a_growing_field_puts_a_peasant_to_work() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), raised) = farmstead(seed, 3);
    // clear the plot so the measurement is about the ONE hand under test
    for &u in &raised {
        cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: u, target: center(bx - 7, by) });
    }
    step(app.world_mut());
    let (field, node, crop) = field_row(&mut app);
    assert!(!crop.ripe && node.remaining < node.cap, "the field is already in - test is blind");
    let farm = farm_id(&mut app);

    let hand = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    let before = stock_food(&mut app);
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: hand, node: field });
    step(app.world_mut());

    let u = unit_of(&mut app, hand);
    assert_eq!(u.gather_state, GatherState::Constructing, "the order was a dead end");
    assert_eq!(u.job_site, farm, "the hand was not put on the farm");

    let working = run_until(&mut app, 400, |a| tending(a) == 1);
    assert!(working.is_some(), "the hand never reached the furrows");
    assert_eq!(stock_food(&mut app), before, "he cut a crop that was not in");
    assert_eq!(unit_of(&mut app, hand).carrying, 0, "he walked off with the seedlings");

    // and when the season does come in, the same hand cuts it with no second order
    let cut = run_until(&mut app, 12_000, |a| stock_food(a) > before);
    assert!(cut.is_some(), "the tending hand never became the reaping hand");
}

/// Ownership. `Repair` is an order you may only give on your own masonry, and
/// tending is the same order — an enemy's growing crop is his to lose, not yours
/// to nurse.
#[test]
fn an_enemy_crop_is_never_tended_by_the_man_who_wants_to_cut_it() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), _) = farmstead(seed, 3);
    spawn_player(&mut app, 2);
    let settled = run_until(&mut app, 400, |a| tending(a) == 3);
    assert!(settled.is_some(), "the crew never all reached the furrows");
    let (field, _, crop) = field_row(&mut app);
    assert!(!crop.ripe, "the field is already in - test is blind");
    let mine = tending(&mut app);

    let raider = crew(&mut app, 2, center(bx, by), 1, 70)[0];
    cmd(&mut app, PlayerCommand::Gather { player_id: 2, unit: raider, node: field });
    step(app.world_mut());
    let u = unit_of(&mut app, raider);
    assert_eq!(u.gather_state, GatherState::ToResource, "an enemy field is a target, not a job");
    assert_eq!(u.job_site, 0, "an enemy's crop hired one of his rival's peasants");
    for _ in 0..200 {
        step(app.world_mut());
    }
    assert_eq!(tending(&mut app), mine, "the enemy's man joined the farm's crew");
}

/// The other half of the labour decision: those hands are wood, stone, a wall or
/// an army everywhere else, and taking one back has to be a single explicit
/// order that STICKS. Nothing in the tend/reap wheel may quietly re-hire him.
#[test]
fn an_explicit_move_takes_a_hand_out_of_the_fields_for_good() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), hands) = farmstead(seed, 3);
    let settled = run_until(&mut app, 400, |a| tending(a) == 3);
    assert!(settled.is_some(), "the crew never all reached the furrows");

    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: hands[0], target: center(bx - 7, by) });
    step(app.world_mut());
    assert_eq!(unit_of(&mut app, hands[0]).job_site, 0, "the order did not release him");

    // a whole season, harvest included: he must never drift back in. `builders`
    // is written by the NEXT construction pass, so the count is given a few ticks
    // to catch up with the order; the release itself is immediate.
    for t in 0..2000 {
        step(app.world_mut());
        assert_eq!(
            unit_of(&mut app, hands[0]).job_site,
            0,
            "the field re-hired a man the player took back, at tick {t}"
        );
        assert!(
            t < 8 || tending(&mut app) <= 2,
            "the farm counted a hand it does not have at tick {t}"
        );
    }
}

/// A reaper who is not the field's own crew has NO plot to fall back on, and a
/// crop can go from ripe to stubble while he is still walking to it — somebody
/// else cut it, or it lodged away under him. He must take other work.
///
/// Standing over a crop that is not in is doing nothing, but it is doing nothing
/// in `Harvesting`, and `Idle` is the only state anything in this sim looks for:
/// not the auto-gather balancer, not `construction`, not one existing test.
/// Measured on the hard bot before this was fixed: 2.6% / 4.9% / 7.8% of every
/// peasant-tick on seeds 48514 / 20250 / 1234, seven men at once, one of them
/// stood in the furrows for 125 SECONDS.
#[test]
fn a_reaper_whose_crop_was_cut_from_under_him_finds_other_work() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), raised) = farmstead(seed, 3);
    // the plot is left to the rain, so the season cannot come round again inside
    // the measuring window and rescue the assertion
    for &u in &raised {
        cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: u, target: center(bx - 7, by - 4) });
    }
    let hand = crew(&mut app, 1, center(bx - 4, by + 2), 1, 40)[0];
    timber(&mut app, 500, center(bx - 6, by + 4));

    let cap = field_row(&mut app).1.cap;
    pin_crop(&mut app, cap, true);
    let field = field_row(&mut app).0;
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: hand, node: field });
    let cutting = run_until(&mut app, 400, |a| {
        unit_of(a, hand).gather_state == GatherState::Harvesting
    });
    assert!(cutting.is_some(), "the reaper never reached the crop");
    assert_eq!(unit_of(&mut app, hand).job_site, 0, "he was hired by the farm - test is blind");

    // and now the crop is gone out from under him, exactly as the crew stripping
    // it or a lodged season leaves it
    pin_crop(&mut app, 4, false);
    let stubble = unripe_fields(&mut app);
    assert_eq!(stubble, vec![field]);

    let moved_on = run_until(&mut app, 600, |a| work_shy(a, &[hand], &stubble).is_empty());
    assert!(
        moved_on.is_some(),
        "the reaper stood over the stubble for 30 seconds: {:?}",
        unit_of(&mut app, hand).gather_state
    );
    // he took real work, not another lap of the same field
    let u = unit_of(&mut app, hand);
    assert_ne!(u.target_node, field, "he went straight back to the crop that is not in");
    assert_eq!(u.job_site, 0, "the farm hired a man the player never gave it");

    // ...and he keeps working, for longer than the season he walked out on
    for t in 0..2000 {
        step(app.world_mut());
        let stubble = unripe_fields(&mut app);
        let shy = work_shy(&mut app, &[hand], &stubble);
        assert!(shy.is_empty(), "the reaper had nothing to do at tick {t}");
    }
    assert!(stock_wood(&mut app) > 9000, "he found work that brought nothing home");
}

/// The same hole, reached the way the game actually reaches it: several hands
/// sent to ONE ripe field, which they strip long before the last of them can get
/// a sickle into it. Whoever arrives late is standing over stubble.
#[test]
fn a_crowd_sent_to_one_ripe_field_does_not_stand_over_the_stubble() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), raised) = farmstead(seed, 3);
    timber(&mut app, 500, center(bx - 8, by + 4));
    let mob = crew(&mut app, 1, center(bx - 5, by + 3), 6, 60);
    pin_crop(&mut app, 24, true); // three carries for six men

    let field = field_row(&mut app).0;
    for &u in &mob {
        cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: u, node: field });
    }
    let all: Vec<u64> = raised.iter().chain(mob.iter()).copied().collect();
    let mut worst = 0u32;
    let mut streak: std::collections::HashMap<u64, u32> = Default::default();
    for t in 0..3000 {
        step(app.world_mut());
        let stubble = unripe_fields(&mut app);
        for id in work_shy(&mut app, &all, &stubble) {
            let e = streak.entry(id).or_insert(0);
            *e += 1;
            worst = worst.max(*e);
        }
        for &id in &all {
            if !work_shy(&mut app, &[id], &stubble).contains(&id) {
                streak.insert(id, 0);
            }
        }
        assert!(worst < 40, "a hand was left with nothing to do for {worst} ticks by tick {t}");
    }
    assert!(stock_food(&mut app) > 9000, "six men on a ripe field brought nothing home");
}

/// THE LANDMINE. `construction` skips its whole pass when there is nothing to
/// build — and a standing farm always wants hands, so the skip cannot be keyed
/// on that alone. If it fires while a farm still carries a stale `builders`
/// count, the crop grows forever on labour nobody is doing, and two peers can
/// disagree about when it stops.
#[test]
fn a_farm_whose_crew_walked_away_does_not_grow_on_phantom_hands() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), hands) = farmstead(seed, 3);
    let settled = run_until(&mut app, 400, |a| tending(a) == 3);
    assert!(settled.is_some(), "the crew never all reached the furrows");

    for &u in &hands {
        cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: u, target: center(bx - 7, by) });
    }
    let emptied = run_until(&mut app, 400, |a| tending(a) == 0);
    assert!(emptied.is_some(), "the farm kept a crew of {} that had left", tending(&mut app));

    // an empty town: no hammers out, nothing hurt, nothing queued. Whatever the
    // construction loop does or skips, the field may only creep in on the rain.
    let start = field_row(&mut app).1.remaining;
    for _ in 0..401 {
        step(app.world_mut());
    }
    let (_, n, c) = field_row(&mut app);
    assert!(!c.ripe && n.remaining < n.cap, "the window hit the cap and stopped measuring");
    assert_eq!(
        n.remaining - start,
        FARM_REGEN_IDLE * 10,
        "an unworked field grew on phantom hands ({start} -> {})",
        n.remaining
    );
    assert_eq!(tending(&mut app), 0);
}

/// A wide clear window with room for a ROW of plots on soil worth sowing, kept
/// apart from `block` so the layout of every other test in this file stays put.
fn wide_block(seed: u32) -> (i32, i32) {
    for cy in 16..WORLD_SIZE - 20 {
        for cx in 16..WORLD_SIZE - 20 {
            if !(-13..3).all(|dx: i32| {
                (-8..3).all(|dy: i32| is_buildable_tile(seed, cx + dx, cy + dy))
            }) {
                continue;
            }
            let rich = |x: i32| {
                saladin_sim::soil_quality(seed, 2, center(x, cy).x, center(x, cy).y)
                    > saladin_sim::FARM_MIN_FERTILITY + fx!("0.06")
            };
            if rich(cx) && rich(cx - 4) && rich(cx - 8) {
                return (cx, cy);
            }
        }
    }
    panic!("no fertile row on seed {seed}");
}

/// `plots` farms in a row west of `bx`, `each` hands on every one of them, one
/// drop-off serving the lot.
fn farmrow(seed: u32, plots: i32, each: u64) -> (App, Vec<u64>) {
    let (bx, by) = wide_block(seed);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(bx - 4, by - 6));
    spawn_building(&mut app, 13, 1, BuildingKind::Storehouse, center(bx - 4, by + 2));
    let mut all = Vec::new();
    for i in 0..plots {
        let at = center(bx - 4 * i, by);
        let ids = crew(&mut app, 1, at, each, 20 + 10 * i as u64);
        cmd(&mut app, PlayerCommand::Build {
            player_id: 1,
            kind: BuildingKind::Farm,
            pos: at,
            facing: 0,
            builders: ids.clone(),
        });
        all.extend(ids);
        // one plot at a time: two sites founded on the same tick would have both
        // crews walk to whichever is nearer
        let up = run_until(&mut app, 1200, |a| {
            let world = a.world_mut();
            let mut q = world.query::<&Building>();
            q.iter(world).filter(|b| b.kind == BuildingKind::Farm && b.complete()).count()
                == (i + 1) as usize
        });
        assert!(up.is_some(), "plot {i} never went up");
    }
    (app, all)
}

/// THE LABOUR DECISION, and the reason it is a decision at all: labour has
/// DIMINISHING returns on one plot (`BUILDER_RATE`) and none of the hands can be
/// in two fields at once, so three men over three farms must beat three men
/// stacked on one. Without this the answer to "how many hands" is always "all of
/// them, on the nearest farm", and the mechanic collapses into a slider.
///
/// Measured over 44 seed x preset x crew combinations: one hand sustains
/// 1.27-1.40 food/s on a median field, three hands on ONE field only 2.91-3.31 —
/// so three separate plots (~3.99) beat one crowded one by a quarter.
#[test]
fn hands_spread_over_plots_beat_hands_stacked_on_one() {
    let seed = compose_seed(11, 1);
    let run = |plots: i32, each: u64| -> i32 {
        let (mut app, hands) = farmrow(seed, plots, each);
        let before = stock_food(&mut app);
        for t in 0..4000 {
            step(app.world_mut());
            let stuck = idle_hands(&mut app, &hands);
            assert!(stuck.is_empty(), "peasants {stuck:?} idle at tick {t} ({plots}x{each})");
        }
        stock_food(&mut app) - before
    };
    let spread = run(3, 1);
    let stacked = run(1, 3);
    assert!(
        spread > stacked,
        "three hands over three plots brought {spread}, stacked on one {stacked}"
    );
    // and the margin is the whole point, not a rounding win
    assert!(
        spread * 100 >= stacked * 110,
        "spreading three hands bought only {}% of the stacked haul",
        spread * 100 / stacked.max(1)
    );
}

/// Two worlds, the same script, hashes compared EVERY tick across a run that
/// tends, ripens, reaps, hauls, re-sows and takes a hand back mid-season. The
/// handoff writes `gather_state`, `job_site` and `Building.builders`, and all
/// three are hashed — a peer that hands off on a different tick is caught here
/// and not minutes later as drifted unit positions.
#[test]
fn the_labour_wheel_keeps_two_worlds_in_lockstep() {
    let seed = compose_seed(11, 1);
    let (bx, by) = block(seed);
    let mut worlds: Vec<App> = (0..2).map(|_| farmstead(seed, 4).0).collect();
    // a crowd with no plot, so the run also crosses the branch where a reaper
    // arrives to find the crop already cut and has to pick other work: whether
    // he gives up on tick N or tick N+1 is hashed state
    for app in &mut worlds {
        timber(app, 500, center(bx - 6, by + 4));
        crew(app, 1, center(bx - 4, by + 3), 4, 60);
    }

    for t in 0..2400u32 {
        // the same order, on the same tick, in both worlds - which is what
        // lockstep IS
        if t == 300 {
            for app in &mut worlds {
                cmd(app, PlayerCommand::Move {
                    player_id: 1,
                    unit: 21,
                    target: center(bx - 7, by),
                });
            }
        }
        if t == 900 {
            let field = field_row(&mut worlds[0]).0;
            for app in &mut worlds {
                cmd(app, PlayerCommand::Gather { player_id: 1, unit: 21, node: field });
            }
        }
        if t % 400 == 60 {
            let field = field_row(&mut worlds[0]).0;
            for app in &mut worlds {
                for u in 60..64 {
                    cmd(app, PlayerCommand::Gather { player_id: 1, unit: u, node: field });
                }
            }
        }
        for app in &mut worlds {
            step(app.world_mut());
        }
        let a = worlds[0].world().resource::<StateHash>().0;
        let b = worlds[1].world().resource::<StateHash>().0;
        assert_eq!(a, b, "the labour wheel desynced the two worlds at tick {t}");
    }
    assert!(stock_food(&mut worlds[0]) > 9000, "the run never reaped anything");
    assert!(tending(&mut worlds[0]) > 0, "the run ended with the fields abandoned");
}
