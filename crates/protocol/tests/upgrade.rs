//! Tower -> Watchtower: the game's one upgrade, and the reason it is an upgrade
//! and not a second purchase.
//!
//! The entity keeps its GameId, its owner, its garrison and its rally flag, and
//! it goes on SHOOTING the whole time it is being raised. A re-buy would give
//! you none of that: you would tear down the picket you already fought for and
//! start again on the same ground.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildState, BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile, UnitKind,
    V2, ZERO, building_def, compose_seed, is_buildable_tile, unit_def,
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

fn spawn_unit(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2, stance: Stance) {
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
            stance,
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

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + saladin_sim::fx!("0.5"), Fx::from_num(ty) + saladin_sim::fx!("0.5"))
}

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

fn building(app: &mut App, id: u64) -> Option<Building> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Building)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, b)| *b)
}

fn owner_of(app: &mut App, id: u64) -> Option<u64> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Owner)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, o)| o.0)
}

fn garrison_count(app: &mut App, id: u64) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<&Unit>();
    q.iter(world).filter(|u| u.garrisoned_in == id).count()
}

/// Everything the tower was, it still is - including the ground it holds.
#[test]
fn a_tower_becomes_a_watchtower_in_place() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    spawn_building(&mut app, 20, 1, BuildingKind::Tower, at);
    spawn_unit(&mut app, 30, 1, UnitKind::Archer, at, Stance::Defensive);

    let rally = V2::new(at.x + Fx::from_num(4), at.y);
    cmd(&mut app, PlayerCommand::SetRally { player_id: 1, building: 20, target: rally });
    cmd(&mut app, PlayerCommand::Garrison { player_id: 1, unit: 30, building: 20 });
    step(app.world_mut());
    assert_eq!(garrison_count(&mut app, 20), 1, "the archer never took the parapet");

    cmd(&mut app, PlayerCommand::UpgradeBuilding { player_id: 1, building: 20 });
    step(app.world_mut());

    let rising = building(&mut app, 20).expect("the tower kept its GameId");
    assert_eq!(rising.state, BuildState::Upgrading);
    assert_eq!(rising.kind, BuildingKind::Tower, "it is a Tower until the work is done");
    assert_eq!(rising.target_kind, BuildingKind::Watchtower);
    assert_eq!(garrison_count(&mut app, 20), 1, "the upgrade evicted the garrison");

    // one peasant raises it
    spawn_unit(&mut app, 40, 1, UnitKind::Peasant, V2::new(at.x + Fx::ONE, at.y), Stance::Defensive);
    cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: 40, building: 20 });
    for _ in 0..2000 {
        step(app.world_mut());
        if building(&mut app, 20).is_some_and(|b| b.kind == BuildingKind::Watchtower) {
            break;
        }
    }

    let done = building(&mut app, 20).expect("the watchtower kept the tower's GameId");
    assert_eq!(done.kind, BuildingKind::Watchtower, "the upgrade never finished");
    assert_eq!(done.state, BuildState::Complete);
    assert_eq!(done.hp, building_def(BuildingKind::Watchtower).max_hp, "it rose short of full");
    assert_eq!(owner_of(&mut app, 20), Some(1), "the upgrade changed hands");
    assert_eq!(done.rally, rally, "the rally flag was dropped");
    assert_eq!(garrison_count(&mut app, 20), 1, "the garrison did not survive the upgrade");
}

/// The whole point of upgrading rather than re-buying: the picket never stops
/// covering the ground while it is being raised.
#[test]
fn a_rising_tower_keeps_firing() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    spawn_building(&mut app, 20, 1, BuildingKind::Tower, at);

    // a raider inside the tower's reach
    let raider_at = V2::new(at.x + Fx::from_num(3), at.y);
    spawn_unit(&mut app, 50, 2, UnitKind::Spearman, raider_at, Stance::Defensive);

    cmd(&mut app, PlayerCommand::UpgradeBuilding { player_id: 1, building: 20 });
    step(app.world_mut());
    assert_eq!(building(&mut app, 20).unwrap().state, BuildState::Upgrading);

    let hp_before = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).find(|(g, _)| g.0 == 50).unwrap().1.hp
    };
    for _ in 0..60 {
        step(app.world_mut());
    }
    let after = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).find(|(g, _)| g.0 == 50).map(|(_, u)| u.hp)
    };
    assert_eq!(building(&mut app, 20).unwrap().state, BuildState::Upgrading, "it finished too soon");
    match after {
        None => {} // shot dead while the tower was rising - the strongest form of the claim
        Some(hp) => assert!(hp < hp_before, "a rising tower stopped shooting ({hp_before} -> {hp})"),
    }
}

/// A foundation has no parapet. A SITE must not shoot, or planting one next to
/// an enemy is a free instant turret.
#[test]
fn a_tower_site_does_not_fire() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
    let at = center(cx + 2, cy + 2);
    let def = building_def(BuildingKind::Tower);
    app.world_mut().spawn((
        GameId(20),
        Owner(1),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        Building::site(BuildingKind::Tower, def.max_hp, at),
    ));
    let raider_at = V2::new(at.x + Fx::from_num(3), at.y);
    spawn_unit(&mut app, 50, 2, UnitKind::Spearman, raider_at, Stance::Defensive);

    let hp_before = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Unit)>();
        q.iter(world).find(|(g, _)| g.0 == 50).unwrap().1.hp
    };
    for _ in 0..60 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    let hp = q.iter(world).find(|(g, _)| g.0 == 50).expect("the raider lived").1.hp;
    assert_eq!(hp, hp_before, "a foundation shot at somebody");
}

/// The upgrade is sim state that mutates a building's KIND in place, which is
/// exactly the divergence a hash folding only hp could never see.
#[test]
fn an_upgrade_keeps_two_worlds_in_lockstep() {
    let (cx, cy) = open_block();
    let at = center(cx + 2, cy + 2);
    let mut worlds: Vec<App> = (0..2)
        .map(|_| {
            let mut app = build_app();
            spawn_player(&mut app, 1);
            spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
            spawn_building(&mut app, 20, 1, BuildingKind::Tower, at);
            spawn_unit(
                &mut app,
                40,
                1,
                UnitKind::Peasant,
                V2::new(at.x + Fx::ONE, at.y),
                Stance::Defensive,
            );
            cmd(&mut app, PlayerCommand::UpgradeBuilding { player_id: 1, building: 20 });
            cmd(&mut app, PlayerCommand::Repair { player_id: 1, unit: 40, building: 20 });
            app
        })
        .collect();

    let mut finished = false;
    for t in 0..2000 {
        for app in &mut worlds {
            step(app.world_mut());
        }
        let a = worlds[0].world().resource::<StateHash>().0;
        let b = worlds[1].world().resource::<StateHash>().0;
        assert_eq!(a, b, "the upgrade desynced the two worlds at tick {t}");
        if building(&mut worlds[0], 20).is_some_and(|b| b.kind == BuildingKind::Watchtower) {
            finished = true;
        }
    }
    assert!(finished, "the upgrade never completed, so lockstep across it proved nothing");
}

/// A Watchtower is EARNED. It is off the build bar, and the command layer
/// refuses it too - one rule set, not a UI convention.
#[test]
fn a_watchtower_cannot_be_bought() {
    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
    spawn_building(&mut app, 20, 1, BuildingKind::Tower, center(cx + 5, cy + 5));
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Watchtower,
        pos: center(cx + 2, cy + 2),
        facing: 0,
        builders: vec![],
    });
    step(app.world_mut());
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    assert_eq!(
        q.iter(world).filter(|b| b.kind == BuildingKind::Watchtower).count(),
        0,
        "a watchtower was bought outright"
    );
}
