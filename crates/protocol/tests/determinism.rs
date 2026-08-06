use bevy_app::prelude::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, Faction, Fx, GatherState, ResourceType, START_FOOD,
    START_GOLD, START_STONE, START_WOOD, Stockpile, UnitKind, V2, WORLD_SIZE, ZERO, is_passable,
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
            target,
            has_target: true,
            speed: Fx::lit("2.5"),
            hp: 30,
            path: vec![target],
            ..Unit::new(UnitKind::Peasant, pos)
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
            speed: unit_def(UnitKind::Spearman).speed,
            hp: unit_def(UnitKind::Spearman).max_hp,
            ..Unit::new(UnitKind::Spearman, pos)
        },
    ));
}

#[test]
fn a_famine_costs_morale_and_men_but_never_kills() {
    // The old rule bled hp until soldiers dropped dead. Nothing in this genre
    // does that, and an army that cannot even walk away is a punishment rather
    // than a decision. Hunger now takes spirit, and then it takes the men
    // themselves, who desert. It never takes their lives.
    let mut app = build();
    spawn_player(&mut app, 7, 0); // an empty larder
    for i in 0..10 {
        spawn_soldier(&mut app, 100 + i, 7);
    }
    let full = unit_def(UnitKind::Spearman).max_hp;

    // past the grace, while the army is still standing
    for _ in 0..40 * 8 {
        step(app.world_mut());
    }
    {
        let world = app.world_mut();
        let mut uq = world.query::<&Unit>();
        let standing: Vec<(i32, saladin_sim::Fx)> =
            uq.iter(world).map(|u| (u.hp, u.morale)).collect();
        assert!(!standing.is_empty(), "the army cannot be gone this early");
        for (hp, _) in &standing {
            assert_eq!(*hp, full, "hunger must never cost a soldier hp (got {hp})");
        }
        assert!(
            standing.iter().any(|(_, m)| *m < saladin_sim::MORALE_MAX),
            "a starving army must lose morale"
        );
    }

    // a famine with no end does empty an army - by desertion, and every man who
    // is still there is unwounded
    for _ in 0..40 * 22 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut uq = world.query::<&Unit>();
    let left: Vec<i32> = uq.iter(world).map(|u| u.hp).collect();
    assert!(left.len() < 10, "a sustained famine must cost men");
    for hp in &left {
        assert_eq!(*hp, full, "a deserting army leaves no wounded behind (got {hp})");
    }
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
        Building::new(BuildingKind::Keep, 1500, keep_pos),
    ));
    // a tree east of the keep
    let tree_pos = V2::new(f(cx + 4) + h, f(cy + 1) + h);
    app.world_mut().spawn((
        GameId(20),
        MatchId(1),
        Pos { pos: tree_pos, facing: ZERO },
        ResourceNode::deposit(ResourceType::Wood, 120),
    ));
    // a peasant near the tree, assigned to gather it
    let pe_pos = V2::new(f(cx + 4) + h, f(cy + 4) + h);
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: pe_pos, facing: ZERO },
        Unit {
            speed: unit_def(UnitKind::Peasant).speed,
            gather_state: GatherState::ToResource,
            target_node: 20,
            hp: 30,
            ..Unit::new(UnitKind::Peasant, pe_pos)
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
            speed: unit_def(UnitKind::Spearman).speed,
            hp: unit_def(UnitKind::Spearman).max_hp,
            ..Unit::new(UnitKind::Spearman, pos)
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
    'outer: while ty < WORLD_SIZE - 20 {
        let mut tx = 20;
        while tx < WORLD_SIZE - 20 {
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

    // brain ticks (every 20) queue more peasants toward the economy target;
    // a peasant now takes its full training time to walk out of the keep
    for _ in 0..400 {
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

/// The desync detector was blind to the entire combat/order layer: mutating
/// stance, morale, routing, attack_target, garrisoned_in or home left the state
/// hash bit-identical, so a peer that fought differently only showed up ticks
/// later as drifted positions. Every field a system writes must move the hash.
#[test]
fn every_written_unit_field_moves_the_state_hash() {
    type Mutation = (&'static str, fn(&mut Unit));
    let cases: &[Mutation] = &[
        ("stance", |u| u.stance = saladin_sim::Stance::HoldGround),
        ("morale", |u| u.morale = Fx::lit("0.4")),
        ("routing", |u| u.routing = true),
        ("attack_target", |u| u.attack_target = 99),
        ("garrisoned_in", |u| u.garrisoned_in = 77),
        ("home", |u| u.home = V2::new(Fx::lit("3"), Fx::lit("4"))),
        ("heading", |u| u.heading = 5),
        ("order", |u| u.order = ORDER_ATTACK_MOVE),
        ("order_target", |u| u.order_target = V2::new(Fx::lit("9"), Fx::lit("9"))),
        ("anchor", |u| u.anchor = V2::new(Fx::lit("7"), Fx::lit("1"))),
        ("engage_slot", |u| u.engage_slot = 3),
        ("charge_cd", |u| u.charge_cd = 4),
        ("rally_cd", |u| u.rally_cd = 6),
        ("setup_timer", |u| u.setup_timer = Fx::lit("1.5")),
        ("ration", |u| u.ration = Fx::lit("0.5")),
        ("attack_cd", |u| u.attack_cd = 3),
        ("target_node", |u| u.target_node = 42),
        ("carrying", |u| u.carrying = 7),
        ("carry_type", |u| u.carry_type = ResourceType::Gold),
        ("harvest_timer", |u| u.harvest_timer = Fx::lit("0.3")),
        ("path", |u| u.path = vec![V2::new(Fx::lit("2"), Fx::lit("2"))]),
        ("path_idx", |u| {
            u.path = vec![V2::ZERO, V2::ZERO];
            u.path_idx = 1;
        }),
    ];
    for &(name, mutate) in cases {
        let (mut a, mut b) = (build(), build());
        for app in [&mut a, &mut b] {
            spawn_soldier(app, 1, 1);
        }
        {
            let world = b.world_mut();
            let mut q = world.query::<&mut Unit>();
            let mut u = q.iter_mut(world).next().expect("the soldier");
            mutate(&mut u);
        }
        // one base tick: combat/gather are sub-rate and do not run at tick 1,
        // so the hash reflects exactly the field under test
        step(a.world_mut());
        step(b.world_mut());
        assert_ne!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "mutating {name} left the state hash identical — a desync there is invisible"
        );
    }
}

/// A tower's reload is command-driven sim state too: two peers whose garrisons
/// fired on different ticks must not agree.
#[test]
fn a_buildings_reload_moves_the_state_hash() {
    let (mut a, mut b) = (build(), build());
    let pos = V2::new(Fx::lit("20"), Fx::lit("20"));
    for app in [&mut a, &mut b] {
        app.world_mut().spawn((
            GameId(1),
            Owner(1),
            MatchId(1),
            Pos { pos, facing: ZERO },
            Building::new(BuildingKind::Tower, 500, pos),
        ));
    }
    {
        let world = b.world_mut();
        let mut q = world.query::<&mut Building>();
        q.iter_mut(world).next().expect("the tower").cooldown = Fx::lit("0.4");
    }
    step(a.world_mut());
    step(b.world_mut());
    assert_ne!(
        a.world().resource::<StateHash>().0,
        b.world().resource::<StateHash>().0,
        "a tower's reload state was invisible to the desync detector"
    );
}

/// Two worlds fed the same army must agree every single tick through a real
/// battle — charge, wounds, rout and rally — not just at the end.
#[test]
fn a_forty_on_forty_battle_hashes_identically_every_tick() {
    let seed = 1u32;
    let (cx, cy) = find_land_block(seed);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    let kinds = [UnitKind::Spearman, UnitKind::Archer, UnitKind::Knight, UnitKind::Crossbowman];
    let (mut a, mut b) = (build(), build());
    for app in [&mut a, &mut b] {
        app.world_mut().insert_resource(WorldConfig { seed });
        let mut id = 1u64;
        for i in 0..40 {
            let kind = kinds[i % kinds.len()];
            let (dx, dy) = ((i % 8) as i32, (i / 8) as i32);
            spawn_kind(app, id, 1, kind, V2::new(f(cx + dx), f(cy + dy)));
            spawn_kind(app, id + 1, 2, kind, V2::new(f(cx + dx), f(cy + dy + 7)));
            id += 2;
        }
    }
    for i in 0..600 {
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "the two armies diverged at tick {i}"
        );
    }
    let world = a.world_mut();
    let mut q = world.query::<&Unit>();
    assert!(q.iter(world).count() < 80, "no one so much as died in 30 s of battle");
}

fn spawn_kind(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit::new(kind, pos),
    ));
}

/// The shortest open-water hop between the first two seatable islands of an
/// Archipelago seed. Pure function of the seed, so both worlds get the same one.
fn strait(seed: u32) -> (V2, V2) {
    let starts = saladin_sim::start_regions(seed);
    let land = saladin_sim::region_grid(seed);
    let sea = saladin_sim::water_region_grid(seed);
    let ocean = saladin_sim::main_water_body(seed);
    let coast = |region: u16| -> Vec<V2> {
        let mut out = Vec::new();
        for ty in 1..WORLD_SIZE - 1 {
            for tx in 1..WORLD_SIZE - 1 {
                if sea[(ty * WORLD_SIZE + tx) as usize] != ocean {
                    continue;
                }
                let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| land[((ty + dy) * WORLD_SIZE + tx + dx) as usize] == region);
                if touches {
                    out.push(V2::new(Fx::from_num(tx) + Fx::lit("0.5"), Fx::from_num(ty) + Fx::lit("0.5")));
                }
            }
        }
        out
    };
    let (ca, cb) = (coast(starts[0]), coast(starts[1]));
    let mut best: Option<(Fx, V2, V2)> = None;
    for (i, pa) in ca.iter().enumerate() {
        if !i.is_multiple_of(3) {
            continue;
        }
        for (j, pb) in cb.iter().enumerate() {
            if !j.is_multiple_of(3) {
                continue;
            }
            let d = saladin_sim::dist2(*pa, *pb);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, *pa, *pb));
            }
        }
    }
    let (_, pa, pb) = best.expect("two coasts on an archipelago");
    (pa, pb)
}

/// Cargo is `garrisoned_in` pointed at a host that MOVES, which nothing in this
/// sim had before. Every field the ferry touches — the passengers' `Pos`, the
/// hull's path, the drowning pass — is hashed, so two worlds handed the same
/// crossing have to agree on every tick of it.
#[test]
fn a_crossing_hashes_the_same_on_two_worlds() {
    let seed = saladin_sim::compose_seed(7, 3);
    let (from, to) = strait(seed);
    let beach =
        saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    let shore = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), to.x, to.y);

    let mut a = build();
    let mut b = build();
    for app in [&mut a, &mut b] {
        app.world_mut().insert_resource(WorldConfig { seed });
        app.world_mut().spawn((
            GameId(500),
            MatchId(1),
            Player {
                player_id: 1,
                name: "P".into(),
                faction: Faction::Ayyubid,
                stock: Stockpile { wood: 500, stone: 500, food: 500, gold: 500 },
                color: 0,
                online: true,
                keep: 0,
                defeated: false,
                slot: 0,
                tech_mask: 0,
                hunger: 0,
            },
        ));
        spawn_kind(app, 1, 1, UnitKind::Barge, from);
        for i in 0..6u64 {
            spawn_kind(app, 10 + i, 1, UnitKind::Spearman, beach);
        }
        app.world_mut()
            .resource_mut::<CommandQueue>()
            .0
            .push(PlayerCommand::Embark { player_id: 1, units: (10..16).collect(), boat: 1 });
    }

    let mut sailed = false;
    for t in 0..900 {
        if !sailed && t == 4 {
            for app in [&mut a, &mut b] {
                app.world_mut()
                    .resource_mut::<CommandQueue>()
                    .0
                    .push(PlayerCommand::Move { player_id: 1, unit: 1, target: to });
            }
            sailed = true;
        }
        if t == 860 {
            for app in [&mut a, &mut b] {
                app.world_mut()
                    .resource_mut::<CommandQueue>()
                    .0
                    .push(PlayerCommand::Disembark { player_id: 1, boat: 1, target: shore });
            }
        }
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "the crossing diverged at tick {t}"
        );
    }
    let world = a.world_mut();
    let mut q = world.query::<&Unit>();
    assert_eq!(
        q.iter(world).filter(|u| u.garrisoned_in == 0).count(),
        7,
        "the party never got off the boat"
    );
}
