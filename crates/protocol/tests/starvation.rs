//! Starvation escalates realistically: an empty larder demoralizes soldiers
//! during the grace period (no attrition), then ramps hp drain in afterwards;
//! feeding the army resets the spiral.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    Faction, Fx, GatherState, MORALE_MAX, ResourceType, Stance, Stockpile, UnitKind, V2, ZERO,
    unit_def,
};

fn build(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn spawn_player(app: &mut App, id: u64, food: i32) {
    app.world_mut().spawn((
        GameId(900 + id),
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

fn spawn_soldier(app: &mut App, id: u64, owner: u64, pos: V2) {
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
            carry_type: ResourceType::Food,
            harvest_timer: ZERO,
            hp: def.max_hp,
            attack_target: 0,
            attack_cooldown: ZERO,
            stance: Stance::HoldGround,
            morale: MORALE_MAX,
            routing: false,
            home: pos,
            garrisoned_in: 0,
            path: vec![],
            path_idx: 0,
        },
    ));
}

fn soldier(app: &mut App, id: u64) -> Unit {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone()).expect("unit")
}

#[test]
fn starvation_breaks_morale_before_bodies() {
    let mut app = build(1);
    spawn_player(&mut app, 1, 0);
    let pos = V2::new(Fx::from_num(60), Fx::from_num(60));
    spawn_soldier(&mut app, 30, 1, pos);
    let max_hp = unit_def(UnitKind::Spearman).max_hp;

    // two economy ticks into the grace: hungry + demoralized, body intact
    for _ in 0..85 {
        step(app.world_mut());
    }
    let u = soldier(&mut app, 30);
    assert_eq!(u.hp, max_hp, "no attrition during the grace period");
    assert!(u.morale < MORALE_MAX, "hunger must bite morale immediately");

    // deep famine: attrition ramped in, hp now falling
    for _ in 0..600 {
        step(app.world_mut());
    }
    let u = soldier(&mut app, 30);
    assert!(u.hp < max_hp, "prolonged famine must cost hp (got {}/{max_hp})", u.hp);
}

#[test]
fn feeding_resets_the_starvation_spiral() {
    let mut app = build(1);
    spawn_player(&mut app, 1, 0);
    let pos = V2::new(Fx::from_num(60), Fx::from_num(60));
    spawn_soldier(&mut app, 30, 1, pos);

    // run deep into famine, then restock the larder
    for _ in 0..400 {
        step(app.world_mut());
    }
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock.food = 500;
        }
    }
    let hp_at_refeed = soldier(&mut app, 30).hp;
    for _ in 0..200 {
        step(app.world_mut());
    }
    let u = soldier(&mut app, 30);
    assert_eq!(u.hp, hp_at_refeed, "fed soldiers must stop bleeding");
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    assert_eq!(q.iter(world).next().unwrap().hunger, 0, "hunger counter must reset when fed");
}
