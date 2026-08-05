//! Role fields, not hardcoded kinds.
//!
//! Fourteen building kinds used to produce six behaviours, because every one of
//! those behaviours was a `kind == X` branch buried in a system. `morale_radius`
//! and `defeat_on_death` are the two that live in combat; proving them here
//! proves the pattern, because the Mosque and the Keep run the SAME code path
//! and differ only by a row in `BUILDING_DEFS`.

use bevy_app::prelude::*;
use bevy_ecs::prelude::Entity;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, GatherState, MORALE_MIN, ResourceType, Stance, Stockpile, UnitKind,
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

fn spawn_shaken(app: &mut App, id: u64, owner: u64, pos: V2) {
    let def = unit_def(UnitKind::Spearman);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind: UnitKind::Spearman,
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
            stance: Stance::Defensive,
            morale: MORALE_MIN,
            routing: true,
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

fn morale_of(app: &mut App, id: u64) -> Fx {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).expect("unit").1.morale
}

/// Two identical broken spearmen, far from their keep. One stands on ground a
/// Mosque holds. `morale_radius` is a def field, so the faith structure steadies
/// men with the exact code the Keep uses.
#[test]
fn a_mosque_steadies_the_ground_it_holds() {
    let (cx, cy) = open_block();
    // far enough from the keep that the keep's own aura cannot reach
    let rally = center(cx + 30, cy + 30);

    let run = |with_mosque: bool| -> Fx {
        let mut app = build_app();
        spawn_player(&mut app, 1);
        spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
        if with_mosque {
            spawn_building(&mut app, 11, 1, BuildingKind::Mosque, rally);
        }
        spawn_shaken(&mut app, 20, 1, V2::new(rally.x + Fx::from_num(2), rally.y));
        for _ in 0..80 {
            step(app.world_mut());
        }
        morale_of(&mut app, 20)
    };

    let alone = run(false);
    let held = run(true);
    assert!(held > alone, "the mosque steadied nobody ({alone} alone vs {held} held)");
}

/// The match ends when the CAPITAL falls, and which building that is comes from
/// `defeat_on_death`. Losing anything else is a setback, not a defeat.
#[test]
fn only_the_capital_ends_the_match() {
    assert!(
        building_def(BuildingKind::Keep).defeat_on_death,
        "the keep must be the building whose loss ends the match"
    );
    for kind in
        [BuildingKind::Barracks, BuildingKind::Tower, BuildingKind::Mosque, BuildingKind::Storehouse]
    {
        assert!(
            !building_def(kind).defeat_on_death,
            "{kind:?} must not end the match when it falls"
        );
    }

    let (cx, cy) = open_block();
    let mut app = build_app();
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx - 4, cy - 4));
    spawn_building(&mut app, 11, 1, BuildingKind::Barracks, center(cx + 4, cy + 4));

    // the barracks burns down the way combat burns one down
    {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &GameId)>();
        let e = q.iter(world).find(|(_, g)| g.0 == 11).map(|(e, _)| e).unwrap();
        world.despawn(e);
    }
    for _ in 0..10 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    assert!(!q.iter(world).next().unwrap().defeated, "losing a barracks lost the match");
}

/// A ram should find stone works hard and timber halls soft. `siege_resist` is a
/// per-building multiplier precisely so this can be true WITHOUT touching the
/// armor cells the damage matrix shares with units.
#[test]
fn stone_works_outlast_timber_halls_against_a_ram() {
    let hits = |kind: BuildingKind| -> i32 {
        let def = building_def(kind);
        let ram = unit_def(UnitKind::Ram);
        let atk = saladin_sim::Attacker {
            attack: Fx::from_num(ram.attack),
            damage_type: ram.damage_type,
            bonus_vs_armor: ram.bonus_vs_armor,
        };
        let dmg = saladin_sim::building_damage(&atk, def).max(1);
        (def.max_hp + dmg - 1) / dmg
    };
    let keep = hits(BuildingKind::Keep);
    let workshop = hits(BuildingKind::SiegeWorkshop);
    let barracks = hits(BuildingKind::Barracks);
    let watchtower = hits(BuildingKind::Watchtower);

    assert!(
        keep > workshop,
        "the capital ({keep} hits) must not be softer than the siege shed ({workshop})"
    );
    assert!(
        watchtower > barracks,
        "a stone tower ({watchtower}) must outlast a timber hall ({barracks})"
    );
    assert!(keep > watchtower, "the keep ({keep}) must be the hardest thing on the map");
}
