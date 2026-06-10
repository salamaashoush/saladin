//! End-to-end tests for the full lockstep command surface: garrison, demolish,
//! market, walls, research, rally, attack, gather and pause — each driven
//! through `CommandQueue` exactly as the netcode would.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile, Tech,
    UnitKind, V2, ZERO, building_def, has_tech, is_passable, unit_def, upgrade_def,
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
        Building { kind, hp: def.max_hp, cooldown: ZERO, rally: pos },
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
            path: vec![],
            path_idx: 0,
        },
    ));
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
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles });
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
    step(app.world_mut());

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
        ResourceNode { res_type: ResourceType::Wood, remaining: 100 },
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
