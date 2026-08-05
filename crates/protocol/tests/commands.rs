//! End-to-end tests for the full lockstep command surface: garrison, demolish,
//! market, walls, research, rally, attack, gather and pause — each driven
//! through `CommandQueue` exactly as the netcode would.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile, Tech,
    UnitKind, V2, ZERO, building_def, fx, has_tech, is_passable, unit_def, upgrade_def,
};

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn find_land_block(seed: u32) -> (i32, i32) {
    for cy in 16..128 {
        for cx in 16..128 {
            if (0..6).all(|dx| (0..6).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no 6x6 land block found");
}

fn spawn_player(app: &mut App, id: u64, stock: Stockpile) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock,
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: id as u8,
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

fn spawn_unit(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    let def = unit_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: def.max_hp,
            attack_target: 0,
            attack_cooldown: ZERO,
            stance: Stance::Aggressive,
            morale: Fx::ONE,
            routing: false,
            home: pos,
            garrisoned_in: 0,
            job_site: 0,
            path: vec![],
            path_idx: 0,
        },
    ));
}

/// A unit's health, or None once it is dead — the difference matters when the
/// point of the test is that something got shot.
fn hp_of(app: &mut App, id: u64) -> Option<i32> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.hp)
}

fn unit_by_id(app: &mut App, id: u64) -> Unit {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone()).expect("unit exists")
}

fn player_stock(app: &mut App, id: u64) -> Stockpile {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == id).map(|p| p.stock).expect("player exists")
}

fn rich() -> Stockpile {
    Stockpile { wood: 1000, stone: 1000, food: 1000, gold: 1000 }
}

/// Base ticks to work a whole order through a production queue.
fn train_ticks(kind: UnitKind) -> usize {
    (unit_def(kind).train_time.to_num::<i64>() as usize + 1) * 20
}

/// Step until `done`, up to `max` base ticks. Returns whether it happened —
/// construction is labour now, so a test that wants a finished building has to
/// give it hands and time.
fn run_until(app: &mut App, max: usize, mut done: impl FnMut(&mut App) -> bool) -> bool {
    for _ in 0..max {
        step(app.world_mut());
        if done(app) {
            return true;
        }
    }
    false
}

fn kind_count(app: &mut App, kind: BuildingKind) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).filter(|b| b.kind == kind).count()
}

fn all_complete(app: &mut App, kind: BuildingKind) -> bool {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    let mut any = false;
    for b in q.iter(world).filter(|b| b.kind == kind) {
        any = true;
        if !b.complete() {
            return false;
        }
    }
    any
}

#[test]
fn garrison_and_ungarrison_cycle() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    spawn_unit(&mut app, 20, 1, UnitKind::Archer, V2::new(f(cx + 4), f(cy + 1)));

    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 20, building: 10 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 20).garrisoned_in, 10, "archer sheltered in the keep");

    cmd(&mut app, PlayerCommand::Ungarrison { player_id: 1, building: 10 });
    step(app.world_mut());
    let u = unit_by_id(&mut app, 20);
    assert_eq!(u.garrisoned_in, 0, "archer back on the field");
}

#[test]
fn cavalry_cannot_garrison() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    spawn_unit(&mut app, 20, 1, UnitKind::Knight, V2::new(f(cx + 4), f(cy + 1)));

    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 20, building: 10 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 20).garrisoned_in, 0, "cavalry must stay outside");
}

#[test]
fn demolish_refunds_half_and_ejects() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, Stockpile { wood: 0, stone: 0, food: 0, gold: 0 });
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    spawn_building(&mut app, 11, 1, BuildingKind::Tower, V2::new(f(cx + 4), f(cy + 4)));
    spawn_unit(&mut app, 20, 1, UnitKind::Archer, V2::new(f(cx + 4), f(cy + 1)));

    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 20, building: 11 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 20).garrisoned_in, 11);

    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: 11 });
    step(app.world_mut());

    let world = app.world_mut();
    let mut bq = world.query::<&Building>();
    assert!(bq.iter(world).all(|b| b.kind != BuildingKind::Tower), "tower razed");
    let u = unit_by_id(&mut app, 20);
    assert_eq!(u.garrisoned_in, 0, "occupant survived the demolish");
    let cost = building_def(BuildingKind::Tower).cost;
    let s = player_stock(&mut app, 1);
    assert_eq!(s.wood, cost.wood / 2, "half the wood refunded");
    assert_eq!(s.stone, cost.stone / 2, "half the stone refunded");
}

#[test]
fn keep_cannot_be_demolished() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));

    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: 10 });
    step(app.world_mut());
    let world = app.world_mut();
    let mut bq = world.query::<&Building>();
    assert_eq!(bq.iter(world).count(), 1, "the keep still stands");
}

#[test]
fn market_trade_sells_wood_for_gold() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, Stockpile { wood: 100, stone: 0, food: 0, gold: 0 });
    spawn_building(&mut app, 10, 1, BuildingKind::Market, V2::new(f(cx + 1), f(cy + 1)));

    cmd(&mut app, PlayerCommand::MarketTrade { player_id: 1, res: ResourceType::Wood, amount: 100 });
    step(app.world_mut());
    let s = player_stock(&mut app, 1);
    assert!(s.gold > 0, "sale minted gold, got {s:?}");
    assert!(s.wood < 100, "sale spent wood, got {s:?}");
}

#[test]
fn market_trade_requires_market() {
    let mut app = build();
    spawn_player(&mut app, 1, Stockpile { wood: 100, stone: 0, food: 0, gold: 0 });
    cmd(&mut app, PlayerCommand::MarketTrade { player_id: 1, res: ResourceType::Wood, amount: 100 });
    step(app.world_mut());
    let s = player_stock(&mut app, 1);
    assert_eq!(s.gold, 0, "no market, no trade");
    assert_eq!(s.wood, 100);
}

#[test]
fn place_wall_lays_a_line() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    spawn_player(&mut app, 1, rich());
    let tiles: Vec<(i32, i32)> = (0..4).map(|i| (cx + i, cy)).collect();
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles, builders: vec![] });
    step(app.world_mut());

    let world = app.world_mut();
    let mut bq = world.query::<&Building>();
    let walls = bq.iter(world).filter(|b| b.kind == BuildingKind::Wall).count();
    assert_eq!(walls, 4, "four wall tiles placed");
    let cost = building_def(BuildingKind::Wall).cost;
    let s = player_stock(&mut app, 1);
    assert_eq!(s.stone, 1000 - 4 * cost.stone, "paid for exactly four tiles");
}

#[test]
fn gate_and_tower_compose_into_a_wall_line() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    // a wall is masonry now: it is dragged, then RAISED, and a gate needs a
    // finished segment to slot into
    let crew: Vec<u64> = (0..3)
        .map(|i| {
            let id = 40 + i as u64;
            spawn_unit(&mut app, id, 1, UnitKind::Peasant, V2::new(f(cx + i), f(cy + 3)));
            id
        })
        .collect();
    let tiles: Vec<(i32, i32)> = (0..5).map(|i| (cx + i, cy)).collect();
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles, builders: crew });
    assert!(
        run_until(&mut app, 900, |app| all_complete(app, BuildingKind::Wall)),
        "the crew never finished the line"
    );
    assert_eq!(kind_count(&mut app, BuildingKind::Wall), 5, "five segments raised");
    let stone_after_walls = player_stock(&mut app, 1).stone;

    // a gate dropped onto the middle segment absorbs it (full refund) and
    // auto-orients to the X-run; a tower slots into another segment
    cmd(
        &mut app,
        PlayerCommand::Build {
            player_id: 1,
            kind: BuildingKind::Gatehouse,
            pos: V2::new(f(cx + 2), f(cy)),
            facing: 1, // deliberately wrong; the wall run must win
            builders: vec![],
        },
    );
    cmd(
        &mut app,
        PlayerCommand::Build {
            player_id: 1,
            kind: BuildingKind::Tower,
            pos: V2::new(f(cx + 4), f(cy)),
            facing: 0,
            builders: vec![],
        },
    );
    step(app.world_mut());

    let world = app.world_mut();
    let mut bq = world.query::<(&Building, &Pos)>();
    let walls = bq.iter(world).filter(|(b, _)| b.kind == BuildingKind::Wall).count();
    assert_eq!(walls, 3, "two segments absorbed");
    let gate_facing = bq
        .iter(world)
        .find(|(b, _)| b.kind == BuildingKind::Gatehouse)
        .map(|(_, p)| p.facing)
        .expect("gatehouse placed on the wall tile");
    assert_eq!(gate_facing, ZERO, "gate aligned to the X-run, not the bogus facing");
    assert_eq!(
        bq.iter(world).filter(|(b, _)| b.kind == BuildingKind::Tower).count(),
        1,
        "tower slotted into the line"
    );
    let wall_cost = building_def(BuildingKind::Wall).cost;
    let gate_cost = building_def(BuildingKind::Gatehouse).cost;
    let tower_cost = building_def(BuildingKind::Tower).cost;
    let s = player_stock(&mut app, 1);
    assert_eq!(
        s.stone,
        stone_after_walls - gate_cost.stone - tower_cost.stone + 2 * wall_cost.stone,
        "both absorbed segments refunded in full"
    );
}

#[test]
fn gate_does_not_compose_with_enemy_walls() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    spawn_building(&mut app, 10, 2, BuildingKind::Wall, V2::new(f(cx), f(cy)));

    cmd(
        &mut app,
        PlayerCommand::Build {
            player_id: 1,
            kind: BuildingKind::Gatehouse,
            pos: V2::new(f(cx), f(cy)),
            facing: 0,
            builders: vec![],
        },
    );
    step(app.world_mut());

    let world = app.world_mut();
    let mut bq = world.query::<&Building>();
    assert_eq!(bq.iter(world).filter(|b| b.kind == BuildingKind::Gatehouse).count(), 0);
    assert_eq!(bq.iter(world).filter(|b| b.kind == BuildingKind::Wall).count(), 1);
}

#[test]
fn research_completes_and_flips_tech_mask() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Blacksmith, V2::new(f(cx + 1), f(cy + 1)));

    let tech = Tech::SharpenedBlades;
    cmd(&mut app, PlayerCommand::StartResearch { player_id: 1, building: 10, tech: tech as u8 });
    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut rq = world.query::<&Research>();
        assert_eq!(rq.iter(world).count(), 1, "research row inserted");
    }
    let s = player_stock(&mut app, 1);
    let cost = upgrade_def(tech).cost;
    assert_eq!(s.gold, 1000 - cost.gold, "research paid up front");

    // research ticks every 20 base ticks; run long enough to finish
    let secs = upgrade_def(tech).research_time.to_num::<i64>() as u64;
    for _ in 0..(secs + 2) * 20 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut pq = world.query::<&Player>();
    let mask = pq.iter(world).next().unwrap().tech_mask;
    assert!(has_tech(mask, tech), "tech bit set after completion");
}

#[test]
fn research_requires_blacksmith() {
    let mut app = build();
    spawn_player(&mut app, 1, rich());
    cmd(&mut app, PlayerCommand::StartResearch { player_id: 1, building: 10, tech: Tech::SharpenedBlades as u8 });
    step(app.world_mut());
    let world = app.world_mut();
    let mut rq = world.query::<&Research>();
    assert_eq!(rq.iter(world).count(), 0, "no blacksmith, no research");
}

#[test]
fn rally_point_sends_trained_units_marching() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));

    let rally = V2::new(f(cx + 5), f(cy + 5));
    cmd(&mut app, PlayerCommand::SetRally { player_id: 1, building: 10, target: rally });
    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    let out = run_until(&mut app, train_ticks(UnitKind::Peasant) + 20, |app| {
        let world = app.world_mut();
        let mut q = world.query::<&Unit>();
        q.iter(world).count() > 0
    });
    assert!(out, "the order never left the queue");

    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    let trained = q.iter(world).map(|(_, u)| u).next().expect("a unit trained");
    assert!(trained.has_target, "fresh unit marches to the rally point");
}

#[test]
fn attack_command_locks_target() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    spawn_unit(&mut app, 20, 1, UnitKind::Spearman, V2::new(f(cx + 1), f(cy + 1)));
    spawn_unit(&mut app, 21, 2, UnitKind::Spearman, V2::new(f(cx + 4), f(cy + 4)));

    cmd(&mut app, PlayerCommand::Attack { player_id: 1, unit: 20, target: 21 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 20).attack_target, 21);

    // own units are not attackable
    cmd(&mut app, PlayerCommand::Attack { player_id: 2, unit: 21, target: 21 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 21).attack_target, 0, "cannot attack yourself");
}

#[test]
fn gather_command_targets_node() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_unit(&mut app, 20, 1, UnitKind::Peasant, V2::new(f(cx + 1), f(cy + 1)));
    app.world_mut().spawn((
        GameId(30),
        MatchId(1),
        Pos { pos: V2::new(f(cx + 4), f(cy + 4)), facing: ZERO },
        ResourceNode::deposit(ResourceType::Wood, 100),
    ));

    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: 20, node: 30 });
    step(app.world_mut());
    let u = unit_by_id(&mut app, 20);
    assert_eq!(u.gather_state, GatherState::ToResource);
    assert_eq!(u.target_node, 30);

    // soldiers cannot gather
    spawn_unit(&mut app, 21, 1, UnitKind::Spearman, V2::new(f(cx + 2), f(cy + 1)));
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: 21, node: 30 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 21).gather_state, GatherState::Idle);
}

#[test]
fn pause_freezes_movement_until_resume() {
    let mut app = build();
    scatter_world_nodes(app.world_mut(), 1);
    cmd(
        &mut app,
        PlayerCommand::Join { player_id: 1, name: "Saladin".into(), faction: Faction::Ayyubid, match_id: 1 },
    );
    step(app.world_mut());

    // order a peasant somewhere, then pause before it moves
    let (uid, from) = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Pos, &Unit)>();
        let (g, p, _) = q.iter(world).next().expect("a peasant");
        (g.0, p.pos)
    };
    let target = V2::new(from.x + Fx::from_num(8), from.y);
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: uid, target });
    cmd(&mut app, PlayerCommand::Pause { player_id: 1 });
    for _ in 0..40 {
        step(app.world_mut());
    }
    let frozen = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Pos)>();
        q.iter(world).find(|(g, _)| g.0 == uid).map(|(_, p)| p.pos).unwrap()
    };
    assert_eq!(frozen, from, "paused match: nobody moves");

    cmd(&mut app, PlayerCommand::Resume { player_id: 1 });
    for _ in 0..40 {
        step(app.world_mut());
    }
    let moved = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Pos)>();
        q.iter(world).find(|(g, _)| g.0 == uid).map(|(_, p)| p.pos).unwrap()
    };
    assert_ne!(moved, from, "resumed match: the order plays out");
}

#[test]
fn auto_gather_puts_idle_peasants_to_work() {
    let mut app = build();
    scatter_world_nodes(app.world_mut(), 1);
    cmd(
        &mut app,
        PlayerCommand::Join { player_id: 1, name: "Saladin".into(), faction: Faction::Ayyubid, match_id: 1 },
    );
    step(app.world_mut());

    // idle every peasant, then auto-gather them back to work
    let ids: Vec<u64> = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).map(|(g, _)| g.0).collect()
    };
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Unit>();
        for mut u in q.iter_mut(world) {
            u.gather_state = GatherState::Idle;
            u.target_node = 0;
        }
    }
    cmd(&mut app, PlayerCommand::AutoGather { player_id: 1 });
    step(app.world_mut());
    let world = app.world_mut();
    let mut q = world.query::<&Unit>();
    let working = q.iter(world).filter(|u| u.gather_state == GatherState::ToResource).count();
    assert_eq!(working, ids.len(), "every idle peasant sent to a node");
}

#[test]
fn garrisoned_archers_let_walls_fire() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    // a gatehouse can host a garrison but has no fire of its own
    spawn_building(&mut app, 10, 1, BuildingKind::Gatehouse, V2::new(f(cx + 1), f(cy + 1)));
    spawn_unit(&mut app, 20, 1, UnitKind::Archer, V2::new(f(cx + 2), f(cy + 1)));
    // an enemy within archer range of the gatehouse
    spawn_unit(&mut app, 30, 2, UnitKind::Peasant, V2::new(f(cx + 4), f(cy + 1)));

    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 20, building: 10 });
    // run combat ticks: the manned gatehouse should wound the peasant
    for _ in 0..40 {
        step(app.world_mut());
    }
    let hp = unit_by_id(&mut app, 30).hp;
    assert!(hp < unit_def(UnitKind::Peasant).max_hp, "manned gatehouse fires, peasant hp {hp}");
}

#[test]
fn market_buys_food_with_gold_at_a_spread() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, Stockpile { wood: 0, stone: 0, food: 0, gold: 100 });
    spawn_building(&mut app, 10, 1, BuildingKind::Market, V2::new(f(cx + 1), f(cy + 1)));

    cmd(&mut app, PlayerCommand::MarketBuy { player_id: 1, res: ResourceType::Food, amount: 20 });
    step(app.world_mut());
    let s = player_stock(&mut app, 1);
    assert_eq!(s.food, 20, "bought the full lot, got {s:?}");
    assert_eq!(s.gold, 100 - 20 * saladin_sim::MARKET_BUY_RATE, "paid the spread, got {s:?}");
    // round trip is lossy by design: sell it straight back
    cmd(&mut app, PlayerCommand::MarketTrade { player_id: 1, res: ResourceType::Food, amount: 20 });
    step(app.world_mut());
    let s2 = player_stock(&mut app, 1);
    assert!(s2.gold < 100, "the merchant's cut makes buy/sell loops a loss, got {s2:?}");
}

#[test]
fn market_buy_requires_market_and_gold() {
    let mut app = build();
    spawn_player(&mut app, 1, Stockpile { wood: 0, stone: 0, food: 0, gold: 100 });
    cmd(&mut app, PlayerCommand::MarketBuy { player_id: 1, res: ResourceType::Food, amount: 20 });
    step(app.world_mut());
    assert_eq!(player_stock(&mut app, 1).food, 0, "no market, no purchase");
}

/// Which of two Barracks trains is sim state, so it cannot be raw ECS
/// iteration order: a live world spawned 21 before 20 trains from 21, and the
/// SAME world after a save/restore trains from 20, because restore re-spawns
/// sorted by id. Lowest GameId wins, in both worlds.
#[test]
fn two_barracks_always_train_from_the_lower_game_id() {
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + fx!("0.5");
    let high = V2::new(f(cx + 1), f(cy + 1));
    let low = V2::new(f(cx + 12), f(cy + 12));

    let trained_near_low = |restore_first: bool| -> bool {
        let mut app = build();
        spawn_player(&mut app, 1, rich());
        // insertion order is the OPPOSITE of id order
        spawn_building(&mut app, 21, 1, BuildingKind::Barracks, high);
        spawn_building(&mut app, 20, 1, BuildingKind::Barracks, low);
        spawn_building(&mut app, 30, 1, BuildingKind::House, V2::new(f(cx + 5), f(cy + 1)));
        if restore_first {
            let snap = save::snapshot(app.world_mut());
            let bytes = save::to_bytes(&snap);
            save::restore(app.world_mut(), save::from_bytes(&bytes).expect("save round trips"));
        }
        cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Spearman });
        for _ in 0..600 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<(&Pos, &Unit)>();
        let at = q
            .iter(world)
            .find(|(_, u)| u.kind == UnitKind::Spearman)
            .map(|(p, _)| p.pos)
            .expect("a spearman trained");
        saladin_sim::dist(at, low) < saladin_sim::dist(at, high)
    };

    assert!(trained_near_low(false), "live world trained from the higher id");
    assert!(trained_near_low(true), "restored world trained from the higher id");
}

/// A ram levelling a Siege Workshop faster than a Keep is the shape defence has
/// to have. `siege_resist` is what puts it there, and only the LIVE combat loop
/// proves it is read: the same ram, the same tick count, two structures.
#[test]
fn a_ram_takes_far_longer_to_crack_stone_than_timber() {
    let (cx, cy) = find_land_block(1);
    let hp_after = |kind: BuildingKind| -> (i32, i32) {
        let mut app = build();
        spawn_player(&mut app, 1, Stockpile::default());
        spawn_player(&mut app, 2, Stockpile::default());
        let at = V2::new(Fx::from_num(cx) + fx!("0.5"), Fx::from_num(cy) + fx!("0.5"));
        spawn_building(&mut app, 10, 1, kind, at);
        let from = V2::new(at.x + fx!("1.2"), at.y);
        spawn_unit(&mut app, 20, 2, UnitKind::Ram, from);
        cmd(&mut app, PlayerCommand::Attack { player_id: 2, unit: 20, target: 10 });
        for _ in 0..60 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        let hp = q.iter(world).find(|(g, _)| g.0 == 10).map(|(_, b)| b.hp).unwrap_or(0);
        (hp, building_def(kind).max_hp)
    };

    let (keep_hp, keep_max) = hp_after(BuildingKind::Keep);
    let (shop_hp, shop_max) = hp_after(BuildingKind::SiegeWorkshop);
    let lost = |hp: i32, max: i32| max - hp;
    assert!(lost(keep_hp, keep_max) > 0, "the ram never reached the keep");
    assert!(
        lost(shop_hp, shop_max) > lost(keep_hp, keep_max),
        "a timber hall must fall faster than the keep ({} vs {})",
        lost(shop_hp, shop_max),
        lost(keep_hp, keep_max)
    );
}

/// A hole in the ground is not a building. Until a peasant finishes it, a site
/// grants no population, no prerequisite, no trade, no research, no garrison
/// and no fire — the whole point of `operational()` being one choke point.
#[test]
fn a_site_unlocks_nothing_until_it_is_finished() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    spawn_building(&mut app, 11, 1, BuildingKind::Barracks, V2::new(f(cx + 4), f(cy + 1)));

    for (kind, x, y) in [
        (BuildingKind::Market, cx + 1, cy + 4),
        (BuildingKind::Blacksmith, cx + 4, cy + 4),
        (BuildingKind::Tower, cx + 1, cy + 7),
    ] {
        cmd(&mut app, PlayerCommand::Build {
            player_id: 1,
            kind,
            pos: V2::new(f(x), f(y)),
            facing: 0,
            builders: vec![],
        });
    }
    step(app.world_mut());

    // every one of them is a real, frail, inert target
    {
        let world = app.world_mut();
        let mut q = world.query::<&Building>();
        for b in q.iter(world).filter(|b| b.kind != BuildingKind::Keep && b.kind != BuildingKind::Barracks) {
            let max = building_def(b.kind).max_hp;
            assert!(!b.complete(), "{:?} stood up on its own", b.kind);
            assert_eq!(b.hp, saladin_sim::site_start_hp(max), "{:?} site hp", b.kind);
            assert_eq!(b.work, ZERO, "{:?} site work", b.kind);
        }
        assert_eq!(q.iter(world).filter(|b| b.kind == BuildingKind::Tower).count(), 1);
    }

    // no trade from a market that is a pile of stone
    let before = player_stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::MarketTrade { player_id: 1, res: ResourceType::Wood, amount: 100 });
    step(app.world_mut());
    assert_eq!(player_stock(&mut app, 1).gold, before.gold, "a market site traded");

    // no research from a forge that is a foundation
    let smith = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Blacksmith).map(|(g, _)| g.0).unwrap()
    };
    cmd(&mut app, PlayerCommand::StartResearch {
        player_id: 1,
        building: smith,
        tech: Tech::SharpenedBlades as u8,
    });
    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut q = world.query::<&Research>();
        assert_eq!(q.iter(world).count(), 0, "a blacksmith site started research");
    }

    // no prerequisite: a Stable needs a STANDING forge
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Stable,
        pos: V2::new(f(cx + 7), f(cy + 7)),
        facing: 0,
        builders: vec![],
    });
    step(app.world_mut());
    assert_eq!(kind_count(&mut app, BuildingKind::Stable), 0, "a site unlocked the tech tree");

    // no garrison: a foundation has no parapet to shelter under
    let tower = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Tower).map(|(g, _)| g.0).unwrap()
    };
    spawn_unit(&mut app, 20, 1, UnitKind::Archer, V2::new(f(cx + 2), f(cy + 7)));
    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 20, building: tower });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 20).garrisoned_in, 0, "an archer sheltered in a foundation");
}

/// And no fire: an unfinished tower has no bows on it. The site is the ONLY
/// structure on this map, so it is the only thing that could shoot.
#[test]
fn an_unfinished_tower_shoots_nobody() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Tower,
        pos: V2::new(f(cx + 4), f(cy + 4)),
        facing: 0,
        builders: vec![],
    });
    step(app.world_mut());
    spawn_unit(&mut app, 30, 2, UnitKind::Peasant, V2::new(f(cx + 5), f(cy + 4)));
    for _ in 0..60 {
        step(app.world_mut());
    }
    assert_eq!(
        hp_of(&mut app, 30),
        Some(unit_def(UnitKind::Peasant).max_hp),
        "an unfinished tower fired"
    );
}

/// Population arrives with the roof, not with the order.
#[test]
fn a_house_only_houses_once_it_stands() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    // the keep's 8 population, all of it spoken for
    let crew: Vec<u64> = (0..8)
        .map(|i| {
            let id = 40 + i as u64;
            spawn_unit(&mut app, id, 1, UnitKind::Peasant, V2::new(f(cx + 3), f(cy + 3 + i)));
            id
        })
        .collect();

    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::House,
        pos: V2::new(f(cx + 1), f(cy + 4)),
        facing: 0,
        builders: crew,
    });
    step(app.world_mut());
    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut q = world.query::<&Building>();
        let keep = q.iter(world).find(|b| b.kind == BuildingKind::Keep).unwrap();
        assert_eq!(keep.queue_len, 0, "a house site housed a peasant");
    }

    assert!(
        run_until(&mut app, 900, |app| all_complete(app, BuildingKind::House)),
        "the crew never raised the house"
    );
    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    step(app.world_mut());
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    let keep = q.iter(world).find(|b| b.kind == BuildingKind::Keep).unwrap();
    assert_eq!(keep.queue_len, 1, "a finished house housed nobody");
}

/// Razing pays for what is standing, never for what burned: a shell must be
/// worth less than a pristine building, and an untouched site costs nothing.
#[test]
fn a_refund_is_worth_what_is_left_standing() {
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    let razed = |hp: i32| -> Stockpile {
        let mut app = build();
        spawn_player(&mut app, 1, Stockpile::default());
        spawn_building(&mut app, 10, 1, BuildingKind::House, V2::new(f(cx + 1), f(cy + 1)));
        {
            let world = app.world_mut();
            let mut q = world.query::<&mut Building>();
            for mut b in q.iter_mut(world) {
                b.hp = hp;
            }
        }
        cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: 10 });
        step(app.world_mut());
        player_stock(&mut app, 1)
    };
    let full = building_def(BuildingKind::House).max_hp;
    let pristine = razed(full);
    let shell = razed(1);
    assert!(pristine.wood > 0, "a demolished house paid nothing back");
    assert!(
        shell.wood < pristine.wood && shell.food <= pristine.food,
        "a burnt-out shell refunded as much as a whole house ({shell:?} vs {pristine:?})"
    );

    // an untouched site hands back everything the order cost
    let mut app = build();
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    let before = player_stock(&mut app, 1);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::House,
        pos: V2::new(f(cx + 1), f(cy + 4)),
        facing: 0,
        builders: vec![],
    });
    step(app.world_mut());
    let sited = player_stock(&mut app, 1);
    assert!(sited.wood < before.wood, "the site was free");
    let house = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::House).map(|(g, _)| g.0).unwrap()
    };
    cmd(&mut app, PlayerCommand::CancelSite { player_id: 1, building: house });
    step(app.world_mut());
    assert_eq!(player_stock(&mut app, 1), before, "cancelling an untouched site cost something");
}

/// A gate is a door in YOUR line, not a breach in it. One tile walled in on
/// every side but the gate: the owner walks in, the enemy does not.
#[test]
fn a_gatehouse_is_a_door_for_its_owner_only() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    let (tx, ty) = (cx + 2, cy + 2);
    let mut id = 10;
    for dx in -1..=1i32 {
        for dy in -1..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let kind =
                if (dx, dy) == (1, 0) { BuildingKind::Gatehouse } else { BuildingKind::Wall };
            spawn_building(&mut app, id, 1, kind, V2::new(f(tx + dx), f(ty + dy)));
            id += 1;
        }
    }

    let inside = V2::new(f(tx), f(ty));
    spawn_unit(&mut app, 50, 1, UnitKind::Spearman, V2::new(f(cx + 5), f(cy + 2)));
    spawn_unit(&mut app, 51, 2, UnitKind::Spearman, V2::new(f(cx + 5), f(cy + 3)));
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 50, target: inside });
    cmd(&mut app, PlayerCommand::Move { player_id: 2, unit: 51, target: inside });
    step(app.world_mut());

    assert!(unit_by_id(&mut app, 50).has_target, "the owner cannot use his own gate");
    assert!(!unit_by_id(&mut app, 51).has_target, "the enemy walked straight through the gate");
}

/// A Tower BECOMES a Watchtower: same GameId, same garrison, same rally, and
/// still firing the whole way up. That is what makes it an upgrade rather than
/// the re-buy it used to be.
#[test]
fn a_tower_is_raised_in_place_and_never_stops_firing() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_player(&mut app, 2, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, V2::new(f(cx + 1), f(cy + 1)));
    spawn_building(&mut app, 11, 1, BuildingKind::Tower, V2::new(f(cx + 4), f(cy + 4)));
    spawn_unit(&mut app, 20, 1, UnitKind::Archer, V2::new(f(cx + 5), f(cy + 4)));
    let crew: Vec<u64> = (0..3)
        .map(|i| {
            let id = 40 + i as u64;
            spawn_unit(&mut app, id, 1, UnitKind::Peasant, V2::new(f(cx + 2), f(cy + 4 + i)));
            id
        })
        .collect();

    let rally = V2::new(f(cx + 1), f(cy + 5));
    cmd(&mut app, PlayerCommand::SetRally { player_id: 1, building: 11, target: rally });
    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 20, building: 11 });
    step(app.world_mut());
    assert_eq!(unit_by_id(&mut app, 20).garrisoned_in, 11);

    cmd(&mut app, PlayerCommand::UpgradeBuilding { player_id: 1, building: 11 });
    for u in &crew {
        cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: *u, building: 11 });
    }
    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        let (_, b) = q.iter(world).find(|(g, _)| g.0 == 11).unwrap();
        assert_eq!(b.kind, BuildingKind::Tower, "the tower vanished mid-upgrade");
        assert_eq!(b.target_kind, BuildingKind::Watchtower);
        assert!(b.complete(), "an upgrading tower is still a building");
    }

    // an enemy walks up WHILE it rises and gets shot for it
    spawn_unit(&mut app, 30, 2, UnitKind::Peasant, V2::new(f(cx + 6), f(cy + 4)));
    for _ in 0..40 {
        step(app.world_mut());
    }
    assert!(
        hp_of(&mut app, 30).is_none_or(|hp| hp < unit_def(UnitKind::Peasant).max_hp),
        "the tower stopped firing while it was being raised"
    );

    assert!(
        run_until(&mut app, 900, |app| {
            let world = app.world_mut();
            let mut q = world.query::<(&GameId, &Building)>();
            q.iter(world).any(|(g, b)| g.0 == 11 && b.kind == BuildingKind::Watchtower)
        }),
        "the tower never became a watchtower"
    );
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Building)>();
    let (_, b) = q.iter(world).find(|(g, _)| g.0 == 11).expect("same GameId survives the upgrade");
    assert_eq!(b.kind, BuildingKind::Watchtower);
    assert_eq!(b.hp, building_def(BuildingKind::Watchtower).max_hp);
    assert_eq!(b.rally, rally, "the rally flag was dropped");
    assert_eq!(unit_by_id(&mut app, 20).garrisoned_in, 11, "the garrison was evicted");
}

/// Damage is recoverable because repair and construction are ONE loop.
#[test]
fn a_battered_hall_is_mended_by_the_same_hands_that_raised_it() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1, rich());
    spawn_building(&mut app, 10, 1, BuildingKind::Barracks, V2::new(f(cx + 2), f(cy + 2)));
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Building>();
        for mut b in q.iter_mut(world) {
            b.hp = 60;
        }
    }
    for i in 0..3u64 {
        let id = 40 + i;
        spawn_unit(&mut app, id, 1, UnitKind::Peasant, V2::new(f(cx + 5), f(cy + 2 + i as i32)));
        cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: id, building: 10 });
    }
    let before = player_stock(&mut app, 1);
    assert!(
        run_until(&mut app, 900, |app| {
            let world = app.world_mut();
            let mut q = world.query::<&Building>();
            q.iter(world).all(|b| b.hp == building_def(b.kind).max_hp)
        }),
        "the hall was never mended"
    );
    let after = player_stock(&mut app, 1);
    assert!(after.wood < before.wood, "repair was free");
    let cost = building_def(BuildingKind::Barracks).cost;
    assert!(before.wood - after.wood <= cost.wood, "mending cost more than building anew");
}
