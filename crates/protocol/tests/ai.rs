//! AI brain behavior tests: research starts, scouting, threat recall plumbing,
//! market trading, garrison defense, and cross-world bot determinism.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile,
    UnitKind, V2, ZERO, building_def, unit_def,
};

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    scatter_world_nodes(app.world_mut(), 1);
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

#[test]
fn bot_with_blacksmith_starts_research() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Normal,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());

    // hand the bot a blacksmith + a full warchest so research is affordable now
    let keep_pos = {
        let world = app.world_mut();
        let mut q = world.query::<(&Pos, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Keep).map(|(p, _)| p.pos).unwrap()
    };
    let smith_pos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(4), keep_pos.y);
    let def = building_def(BuildingKind::Blacksmith);
    app.world_mut().spawn((
        GameId(5000),
        Owner(1000),
        MatchId(1),
        Pos { pos: smith_pos, facing: ZERO },
        Building { kind: BuildingKind::Blacksmith, hp: def.max_hp, cooldown: ZERO, rally: smith_pos },
    ));
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 2000, stone: 2000, food: 2000, gold: 2000 };
        }
    }

    // a few brain windows (brain runs every 20 base ticks)
    for _ in 0..200 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut rq = world.query::<&Research>();
    assert!(rq.iter(world).count() >= 1, "Normal bot should start a Blacksmith tech");
}

#[test]
fn hard_bot_sends_a_scout() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::Join { player_id: 1, name: "Saladin".into(), faction: Faction::Ayyubid, match_id: 1 },
    );
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Hard,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    for _ in 0..40 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut bq = world.query::<&Bot>();
    let scout = bq.iter(world).next().unwrap().scout_id;
    assert_ne!(scout, 0, "Hard bot should dispatch a scout toward the enemy keep");
}

fn keep_pos_of(app: &mut App, owner: u64) -> V2 {
    let world = app.world_mut();
    let mut q = world.query::<(&Pos, &Owner, &Building)>();
    q.iter(world)
        .find(|(_, o, b)| o.0 == owner && b.kind == BuildingKind::Keep)
        .map(|(p, _, _)| p.pos)
        .unwrap()
}

fn spawn_unit_row(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2, stance: Stance) {
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind,
            target: pos,
            has_target: false,
            speed: unit_def(kind).speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: unit_def(kind).max_hp,
            attack_target: 0,
            attack_cooldown: ZERO,
            stance,
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
fn bot_sells_glut_at_its_market_for_gold() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Normal,
            faction: Faction::Crusader,
            match_id: 1,
        },
    );
    step(app.world_mut());

    // hand the bot a Market and a deep wood glut with an empty purse
    let keep_pos = keep_pos_of(&mut app, 1000);
    let mpos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(5), keep_pos.y);
    let def = building_def(BuildingKind::Market);
    app.world_mut().spawn((
        GameId(6000),
        Owner(1000),
        MatchId(1),
        Pos { pos: mpos, facing: ZERO },
        Building { kind: BuildingKind::Market, hp: def.max_hp, cooldown: ZERO, rally: mpos },
    ));
    {
        let world = app.world_mut();
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            p.stock = Stockpile { wood: 2000, stone: 50, food: 800, gold: 0 };
        }
    }

    let mut earned = false;
    for _ in 0..20 {
        for _ in 0..20 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        if q.iter(world).any(|p| p.player_id == 1000 && p.stock.gold > 0) {
            earned = true;
            break;
        }
    }
    assert!(earned, "a gold-poor bot with a market and a wood glut must sell for gold");
}

#[test]
fn bot_garrisons_shooters_under_threat_and_releases_after() {
    let mut app = build();
    cmd(
        &mut app,
        PlayerCommand::Join { player_id: 1, name: "Foe".into(), faction: Faction::Crusader, match_id: 1 },
    );
    cmd(
        &mut app,
        PlayerCommand::AddAi {
            player_id: 1000,
            host: 1,
            difficulty: AiDifficulty::Normal,
            faction: Faction::Ayyubid,
            match_id: 1,
        },
    );
    step(app.world_mut());

    let keep_pos = keep_pos_of(&mut app, 1000);
    // the bot's archers stand at home
    for i in 0..3 {
        let pos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(3 + i), keep_pos.y);
        spawn_unit_row(&mut app, 7000 + i as u64, 1000, UnitKind::Archer, pos, Stance::Defensive);
    }
    // enemy knights camp inside the threat radius but outside aggro/keep fire
    for i in 0..4 {
        let pos = V2::new(keep_pos.x + saladin_sim::Fx::from_num(15), keep_pos.y + saladin_sim::Fx::from_num(i));
        spawn_unit_row(&mut app, 8000 + i as u64, 1, UnitKind::Knight, pos, Stance::HoldGround);
    }

    for _ in 0..200 {
        step(app.world_mut());
    }
    {
        let world = app.world_mut();
        let mut q = world.query::<(&Owner, &Unit)>();
        let sheltered = q
            .iter(world)
            .filter(|(o, u)| o.0 == 1000 && u.kind == UnitKind::Archer && u.garrisoned_in != 0)
            .count();
        assert!(sheltered > 0, "a defending bot should garrison its shooters");
    }

    // threat clears -> the bot empties its shelters
    {
        let world = app.world_mut();
        let mut q = world.query::<(bevy_ecs::entity::Entity, &Owner, &Unit)>();
        let knights: Vec<bevy_ecs::entity::Entity> =
            q.iter(world).filter(|(_, o, u)| o.0 == 1 && u.kind == UnitKind::Knight).map(|(e, _, _)| e).collect();
        for e in knights {
            world.despawn(e);
        }
    }
    for _ in 0..100 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<(&Owner, &Unit)>();
    let still_in = q.iter(world).filter(|(o, u)| o.0 == 1000 && u.garrisoned_in != 0).count();
    assert_eq!(still_in, 0, "all-clear should ungarrison every shelter");
}

#[test]
fn dueling_hard_bots_stay_in_lockstep() {
    let run = || {
        let mut app = build();
        for (id, faction) in [(1000, Faction::Ayyubid), (1001, Faction::Crusader)] {
            cmd(
                &mut app,
                PlayerCommand::AddAi { player_id: id, host: 1, difficulty: AiDifficulty::Hard, faction, match_id: 1 },
            );
        }
        app
    };
    let mut a = run();
    let mut b = run();
    for i in 0..600 {
        step(a.world_mut());
        step(b.world_mut());
        if i % 100 == 0 {
            let ha = a.world().resource::<StateHash>().0;
            let hb = b.world().resource::<StateHash>().0;
            assert_eq!(ha, hb, "bot worlds diverged at tick {i}");
        }
    }
    let ha = a.world().resource::<StateHash>().0;
    let hb = b.world().resource::<StateHash>().0;
    assert_eq!(ha, hb, "bot worlds diverged after 600 ticks");
}
