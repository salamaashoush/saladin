//! AI brain behavior tests: research starts, scouting, threat recall plumbing.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, Faction, Stockpile, V2, ZERO, building_def,
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
