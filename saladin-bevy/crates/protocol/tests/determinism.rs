use bevy_app::prelude::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, Faction, Fx, GatherState, ResourceType, START_FOOD,
    START_GOLD, START_STONE, START_WOOD, Stance, Stockpile, UnitKind, V2, ZERO, is_passable,
    unit_def,
};
use saladin_protocol::*;

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app
}

fn spawn_unit(app: &mut App, id: u64, pos: V2, target: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind: UnitKind::Peasant,
            target,
            has_target: true,
            speed: Fx::lit("2.5"),
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: 30,
            attack_target: 0,
            attack_cooldown: ZERO,
            stance: Stance::Aggressive,
            morale: Fx::ONE,
            routing: false,
            home: pos,
            garrisoned_in: 0,
            path: vec![target],
            path_idx: 0,
        },
    ));
}

#[test]
fn two_worlds_simulate_identically() {
    let mut a = build();
    let mut b = build();
    for app in [&mut a, &mut b] {
        spawn_unit(app, 1, V2::new(Fx::lit("10"), Fx::lit("10")), V2::new(Fx::lit("30"), Fx::lit("20")));
        spawn_unit(app, 2, V2::new(Fx::lit("40"), Fx::lit("40")), V2::new(Fx::lit("12"), Fx::lit("44")));
    }
    for _ in 0..200 {
        step(a.world_mut());
        step(b.world_mut());
    }
    let ha = a.world().resource::<StateHash>().0;
    let hb = b.world().resource::<StateHash>().0;
    assert_eq!(ha, hb, "two identical worlds must hash the same after 200 ticks");
}

fn spawn_player(app: &mut App, id: u64, food: i32) {
    app.world_mut().spawn((
        GameId(id),
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

fn spawn_soldier(app: &mut App, id: u64, owner: u64) {
    let pos = V2::new(Fx::lit("20"), Fx::lit("20"));
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind: UnitKind::Spearman,
            target: pos,
            has_target: false,
            speed: unit_def(UnitKind::Spearman).speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: unit_def(UnitKind::Spearman).max_hp,
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

#[test]
fn starvation_drains_soldiers() {
    let mut app = build();
    spawn_player(&mut app, 7, 5); // only 5 food
    for i in 0..10 {
        spawn_soldier(&mut app, 100 + i, 7); // 10 eaters, bill 10 > 5
    }
    // economy runs at tick 40 (every 40 base ticks)
    for _ in 0..40 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    // player starved to 0 food
    let mut pq = world.query::<&Player>();
    assert_eq!(pq.iter(world).next().unwrap().stock.food, 0);
    // each soldier bled hp (70 -> 62, drain round(4*2)=8)
    let mut uq = world.query::<&Unit>();
    let hp = uq.iter(world).next().unwrap().hp;
    assert_eq!(hp, 62);
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

#[test]
fn peasant_harvests_tree_and_banks_at_keep() {
    let mut app = build();
    let seed = 1u32;
    app.world_mut().insert_resource(WorldConfig { seed });
    let (cx, cy) = find_land_block(seed);
    let f = |n: i32| Fx::from_num(n);
    let h = Fx::lit("0.5");

    spawn_player(&mut app, 1, 100);

    // keep (3×3) anchored in the block
    let keep_pos = V2::new(f(cx + 1) + h, f(cy + 1) + h);
    app.world_mut().spawn((
        GameId(10),
        Owner(1),
        MatchId(1),
        Pos { pos: keep_pos, facing: ZERO },
        Building { kind: BuildingKind::Keep, hp: 1500, cooldown: ZERO, rally: keep_pos },
    ));
    // a tree east of the keep
    let tree_pos = V2::new(f(cx + 4) + h, f(cy + 1) + h);
    app.world_mut().spawn((
        GameId(20),
        MatchId(1),
        Pos { pos: tree_pos, facing: ZERO },
        ResourceNode { res_type: ResourceType::Wood, remaining: 120 },
    ));
    // a peasant near the tree, assigned to gather it
    let pe_pos = V2::new(f(cx + 4) + h, f(cy + 4) + h);
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: pe_pos, facing: ZERO },
        Unit {
            kind: UnitKind::Peasant,
            target: pe_pos,
            has_target: false,
            speed: unit_def(UnitKind::Peasant).speed,
            gather_state: GatherState::ToResource,
            target_node: 20,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: 30,
            attack_target: 0,
            attack_cooldown: ZERO,
            stance: Stance::Aggressive,
            morale: Fx::ONE,
            routing: false,
            home: pe_pos,
            garrisoned_in: 0,
            path: vec![],
            path_idx: 0,
        },
    ));

    for _ in 0..400 {
        step(app.world_mut());
    }

    let world = app.world_mut();
    let mut pq = world.query::<&Player>();
    let wood = pq.iter(world).next().unwrap().stock.wood;
    assert!(wood >= 8, "peasant should bank at least one wood load (8), got {wood}");
}

fn spawn_combatant(app: &mut App, id: u64, owner: u64, pos: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind: UnitKind::Spearman,
            target: pos,
            has_target: false,
            speed: unit_def(UnitKind::Spearman).speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: unit_def(UnitKind::Spearman).max_hp,
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

#[test]
fn combat_scales_to_hundreds_of_units() {
    let mut app = build();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    let seed = 1;
    let f = |n: i32| Fx::from_num(n);
    let h = Fx::lit("0.5");

    // place enemy pairs across the map's land; the spatial grid keeps each unit's
    // work bounded to its cell block, so this stays fast despite the count
    let mut id = 1u64;
    let mut placed = 0;
    let mut ty = 20;
    'outer: while ty < 130 {
        let mut tx = 20;
        while tx < 130 {
            if is_passable(seed, tx, ty) && is_passable(seed, tx + 1, ty) {
                spawn_combatant(&mut app, id, 1, V2::new(f(tx) + h, f(ty) + h));
                spawn_combatant(&mut app, id + 1, 2, V2::new(f(tx + 1) + h, f(ty) + h));
                id += 2;
                placed += 2;
                if placed >= 400 {
                    break 'outer;
                }
            }
            tx += 5;
        }
        ty += 5;
    }
    assert!(placed >= 200, "expected a big battle, placed only {placed}");
    let before = placed;

    // 50 combat ticks (200 base ticks) — completes quickly thanks to the grid
    for _ in 0..200 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut uq = world.query::<&Unit>();
    let after = uq.iter(world).count();
    assert!(after < before, "combat at scale should produce casualties: {after}/{before} remain");
}

#[test]
fn adjacent_enemies_fight() {
    let mut app = build();
    let seed = 1u32;
    app.world_mut().insert_resource(WorldConfig { seed });
    let (cx, cy) = find_land_block(seed);
    let f = |n: i32| Fx::from_num(n);
    let h = Fx::lit("0.5");
    // one tile apart — within a spearman's reach (1.2)
    spawn_combatant(&mut app, 1, 1, V2::new(f(cx + 2) + h, f(cy + 2) + h));
    spawn_combatant(&mut app, 2, 2, V2::new(f(cx + 3) + h, f(cy + 2) + h));

    for _ in 0..60 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut uq = world.query::<&Unit>();
    let hps: Vec<i32> = uq.iter(world).map(|u| u.hp).collect();
    let full = unit_def(UnitKind::Spearman).max_hp;
    assert!(hps.iter().any(|&hp| hp < full), "at least one spearman should be wounded, got {hps:?}");
}

#[test]
fn join_command_founds_base_and_economy_runs() {
    let mut app = build();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Join {
        player_id: 1,
        name: "Saladin".into(),
        faction: Faction::Ayyubid,
        match_id: 1,
    });

    // first tick applies the Join: keep + 5 peasants + player
    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut pq = world.query::<&Player>();
        assert_eq!(pq.iter(world).count(), 1, "one player founded");
        let mut bq = world.query::<&Building>();
        assert_eq!(bq.iter(world).count(), 1, "a keep");
        let mut uq = world.query::<&Unit>();
        assert_eq!(uq.iter(world).count(), 5, "five starting peasants");
    }

    // run the economy a while; peasants gather and bank
    for _ in 0..800 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut pq = world.query::<&Player>();
    let s = pq.iter(world).next().unwrap().stock;
    let gained = s.wood > START_WOOD || s.food > START_FOOD || s.stone > START_STONE || s.gold > START_GOLD;
    assert!(gained, "peasants should bank resources, got {s:?}");
}

#[test]
fn ai_bot_founds_base_and_trains() {
    let mut app = build();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1000,
        host: 1,
        difficulty: AiDifficulty::Easy,
        faction: Faction::Crusader,
        match_id: 1,
    });

    step(app.world_mut());
    {
        let world = app.world_mut();
        let mut bq = world.query::<&Bot>();
        assert_eq!(bq.iter(world).count(), 1, "bot driver attached");
        let mut uq = world.query::<&Unit>();
        assert_eq!(uq.iter(world).count(), 5, "bot starts with 5 peasants");
    }

    // brain ticks (every 20) train more peasants toward the economy target
    for _ in 0..80 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut uq = world.query::<&Unit>();
    let n = uq.iter(world).count();
    assert!(n >= 6, "AI should have trained at least one extra peasant, got {n}");
}

#[test]
fn units_reach_their_target() {
    let mut app = build();
    let target = V2::new(Fx::lit("30"), Fx::lit("20"));
    spawn_unit(&mut app, 1, V2::new(Fx::lit("10"), Fx::lit("10")), target);
    for _ in 0..400 {
        step(app.world_mut());
    }
    // after enough ticks the peasant has arrived and cleared its target
    let world = app.world_mut();
    let mut q = world.query::<(&Pos, &Unit)>();
    let (pos, unit) = q.iter(world).next().unwrap();
    assert!(!unit.has_target, "unit should have arrived");
    assert_eq!(pos.pos, target, "arrived unit snaps to target");
}
