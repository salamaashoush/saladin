//! Farms: the only food that grows back. A field may only be sown on soil the
//! worldgen says is worth sowing, it plants a harvestable node that regrows at
//! a rate the soil sets, and razing the farm takes the crop with it.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, FARM_MIN_FERTILITY, FARM_STORE, Faction, Fx, ResourceType,
    Stockpile, UnitKind, V2, WORLD_SIZE, ZERO, building_def, compose_seed, fx, is_buildable_tile,
    unit_def,
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

/// A building is LABOUR now: a Build order without hands founds a site and
/// nothing more, so every test that wants a standing farm hires a crew.
fn crew(app: &mut App, owner: u64, at: V2, n: u64, first: u64) -> Vec<u64> {
    let def = unit_def(UnitKind::Peasant);
    (0..n)
        .map(|i| {
            let id = first + i;
            let pos = V2::new(at.x + Fx::from_num(3 + i as i32), at.y);
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

/// Site a farm at `at` with a crew and run until it stands. Returns false if
/// the field was never ploughed.
fn raise_farm(app: &mut App, at: V2, first_id: u64) -> bool {
    let hands = crew(app, 1, at, 3, first_id);
    cmd(app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Farm,
        pos: at,
        facing: 0,
        builders: hands,
    });
    for _ in 0..900 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<&Building>();
        if q.iter(world).any(|b| b.kind == BuildingKind::Farm && b.complete()) {
            return true;
        }
    }
    false
}

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"))
}

/// A tile whose 2x2 farm footprint sits on soil that passes (`rich`) or fails
/// (`!rich`) the threshold, with clear buildable room beside it for the
/// anchoring keep. Measured with the SAME helper `check_place` uses.
fn block(seed: u32, rich: bool) -> (i32, i32) {
    for cy in 12..WORLD_SIZE - 16 {
        for cx in 12..WORLD_SIZE - 16 {
            if !(-8..3).all(|dx| (-8..3).all(|dy| is_buildable_tile(seed, cx + dx, cy + dy))) {
                continue;
            }
            let q = saladin_sim::soil_quality(seed, 2, center(cx, cy).x, center(cx, cy).y);
            if rich && q > FARM_MIN_FERTILITY + fx!("0.08") {
                return (cx, cy);
            }
            if !rich && q < FARM_MIN_FERTILITY - fx!("0.06") {
                return (cx, cy);
            }
        }
    }
    panic!("no {} block on seed {seed}", if rich { "fertile" } else { "barren" });
}

fn farms(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).filter(|b| b.kind == BuildingKind::Farm).count()
}

fn fields(app: &mut App) -> Vec<ResourceNode> {
    let world = app.world_mut();
    let mut q = world.query::<(&ResourceNode, &FieldOf)>();
    q.iter(world).map(|(n, _)| *n).collect()
}

#[test]
fn a_field_needs_soil_worth_sowing() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);

    let (bx, by) = block(seed, false);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(farms(&mut app), 0, "nothing grows on barren ground");

    let (fx_, fy) = block(seed, true);
    spawn_keep(&mut app, 11, 1, center(fx_ - 5, fy - 5));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(fx_, fy), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(farms(&mut app), 1, "good soil takes the plough");
}

#[test]
fn a_sown_field_is_harvestable_and_regrows() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");

    let sown = fields(&mut app);
    assert_eq!(sown.len(), 1, "sowing a farm plants exactly one field");
    assert_eq!(sown[0].res_type, ResourceType::Food);
    assert_eq!(sown[0].cap, FARM_STORE);
    assert!(sown[0].regen > 0, "a field that never regrows is just a slow node");

    // eat most of it, then let the economy tick run
    {
        let world = app.world_mut();
        let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
        for (mut n, _) in q.iter_mut(world) {
            n.remaining = 4;
        }
    }
    for _ in 0..41 {
        step(app.world_mut());
    }
    let after = fields(&mut app);
    assert!(after[0].remaining > 4, "the field did not grow back (still {})", after[0].remaining);
    assert!(after[0].remaining <= FARM_STORE, "a field grew past its capacity");
}

#[test]
fn razing_a_farm_takes_its_crop() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");
    assert_eq!(fields(&mut app).len(), 1);

    let farm_id = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Farm).map(|(g, _)| g.0).unwrap()
    };
    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: farm_id });
    for _ in 0..5 {
        step(app.world_mut());
    }
    assert_eq!(farms(&mut app), 0);
    assert!(fields(&mut app).is_empty(), "the crop outlived the farm");
}

#[test]
fn farms_keep_two_worlds_in_lockstep() {
    let seed = compose_seed(11, 1);
    let (bx, by) = block(seed, true);
    let mut worlds: Vec<App> = (0..2)
        .map(|_| {
            let mut app = build_app(seed);
            spawn_player(&mut app, 1);
            spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
            app
        })
        .collect();
    for app in &mut worlds {
        cmd(app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0, builders: vec![] });
    }
    for _ in 0..90 {
        for app in &mut worlds {
            step(app.world_mut());
        }
        let a = worlds[0].world().resource::<StateHash>().0;
        let b = worlds[1].world().resource::<StateHash>().0;
        assert_eq!(a, b, "farm regrowth desynced the two worlds");
    }
}

/// The Granary's whole role: it stores nothing and instead WORKS the fields in
/// its reach. Two identical farms, one hubbed and one not.
#[test]
fn a_granary_makes_the_fields_around_it_grow_faster() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");
    assert_eq!(fields(&mut app).len(), 1);

    let drain = |app: &mut App| {
        let world = app.world_mut();
        let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
        for (mut n, _) in q.iter_mut(world) {
            n.remaining = 1;
        }
    };
    drain(&mut app);
    for _ in 0..41 {
        step(app.world_mut());
    }
    let bare = fields(&mut app)[0].remaining;

    // the same field, now inside a granary's reach
    spawn_building(&mut app, 12, 1, BuildingKind::Granary, center(bx + 3, by + 3));
    drain(&mut app);
    for _ in 0..41 {
        step(app.world_mut());
    }
    let hubbed = fields(&mut app)[0].remaining;
    assert!(hubbed > bare, "granary aura did nothing ({bare} -> {hubbed})");
}

/// The aura is a work bonus its OWNER pays for. A granary the enemy built next
/// to your fields tended them for free, because the regen loop matched on
/// geography alone and never asked whose crop it was.
#[test]
fn an_enemy_granary_does_not_tend_your_fields() {
    let seed = compose_seed(11, 1);
    let (bx, by) = block(seed, true);

    let run = |granary_owner: Option<u64>| -> i32 {
        let mut app = build_app(seed);
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
        assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");
        if let Some(o) = granary_owner {
            spawn_building(&mut app, 12, o, BuildingKind::Granary, center(bx + 3, by + 3));
        }
        {
            let world = app.world_mut();
            let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
            for (mut n, _) in q.iter_mut(world) {
                n.remaining = 1;
            }
        }
        for _ in 0..41 {
            step(app.world_mut());
        }
        fields(&mut app)[0].remaining
    };

    let bare = run(None);
    let mine = run(Some(1));
    let theirs = run(Some(2));
    assert!(mine > bare, "your own granary must tend your fields ({bare} -> {mine})");
    assert_eq!(theirs, bare, "an enemy granary tended your crop ({bare} -> {theirs})");
}

/// The whole point of a farm: a peasant walks to the crop, cuts it and carries
/// it home. The field node sits at the farm's own footprint CENTRE, which on an
/// even footprint is the corner shared by four tiles the building occupies — so
/// the nearest ground a harvester can stand on is over a tile away and a plain
/// HARVEST_RANGE test can never be satisfied. Every earlier farming test
/// asserted the field EXISTED and REGREW; none of them ever ate from it.
#[test]
fn a_peasant_can_actually_eat_from_a_farm() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");

    let field = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &FieldOf)>();
        q.iter(world).map(|(g, _)| g.0).next().unwrap()
    };
    let hand = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    let food = |app: &mut App| -> i32 {
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        q.iter(world).find(|p| p.player_id == 1).unwrap().stock.food
    };
    let before = food(&mut app);
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: hand, node: field });
    for _ in 0..600 {
        step(app.world_mut());
    }
    assert!(food(&mut app) > before, "600 ticks on a standing field and not one bushel came in");
}
