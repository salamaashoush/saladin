//! Why the field-labour budget is breached: prints every tick where the bot's
//! farmhand share exceeds two thirds, with the peasant count either side, so a
//! transient between brain ticks can be told from a real overshoot.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{AiDifficulty, BuildingKind, Faction, UnitKind, compose_seed, operational};

fn main() {
    let seed: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(32676);
    let ticks: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4000);

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: compose_seed(seed, 0) });
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1,
        host: 1,
        difficulty: AiDifficulty::Hard,
        faction: Faction::Ayyubid,
        match_id: 1,
    });

    let mut prev = (0i32, 0i32);
    for t in 0..ticks {
        step(app.world_mut());
        let world = app.world_mut();
        let fields: Vec<u64> = {
            let mut q = world.query::<&FieldOf>();
            q.iter(world).map(|f| f.0).collect()
        };
        let farms = {
            let mut q = world.query::<(&Owner, &Building)>();
            q.iter(world)
                .filter(|(o, b)| o.0 == 1 && b.kind == BuildingKind::Farm && operational(b.state))
                .count()
        };
        let (hands, peasants) = {
            let mut q = world.query::<(&Owner, &Unit)>();
            let (mut hands, mut peasants) = (0, 0);
            for (o, u) in q.iter(world) {
                if o.0 != 1 || u.kind != UnitKind::Peasant {
                    continue;
                }
                peasants += 1;
                if fields.contains(&u.job_site) {
                    hands += 1;
                }
            }
            (hands, peasants)
        };
        if hands * 3 > peasants * 2 {
            println!(
                "t={t} BREACH hands={hands} peasants={peasants} farms={farms} \
                 (prev hands={} peasants={}) brain_tick={}",
                prev.0,
                prev.1,
                t % 20
            );
        }
        prev = (hands, peasants);
    }
}
