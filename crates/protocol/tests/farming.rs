//! Farms: the only food that grows back. A field may only be sown on soil the
//! worldgen says is worth sowing, it plants a harvestable node that regrows at
//! a rate the soil sets, and razing the farm takes the crop with it.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, FARM_MIN_FERTILITY, FARM_STORE, Faction, Fx, ResourceType, Stockpile, V2,
    WORLD_SIZE, ZERO, building_def, compose_seed, fx, is_buildable_tile,
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
        Building { kind: BuildingKind::Keep, hp: def.max_hp, cooldown: ZERO, rally: pos },
    ));
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
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0 });
    step(app.world_mut());
    assert_eq!(farms(&mut app), 0, "nothing grows on barren ground");

    let (fx_, fy) = block(seed, true);
    spawn_keep(&mut app, 11, 1, center(fx_ - 5, fy - 5));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(fx_, fy), facing: 0 });
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
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0 });
    step(app.world_mut());

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
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0 });
    step(app.world_mut());
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
        cmd(app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0 });
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
