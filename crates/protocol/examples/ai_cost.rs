//! What one brain tick costs. Six Hard bots on a real map, timed — the number
//! to check before and after any change to the placement rules, because
//! `place_near` ring-probes a whole perimeter every decision window.
//!
//! cargo run --release -p saladin-protocol --example ai_cost -- [ticks] [bots] [seed]

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{AiDifficulty, Faction, compose_seed};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let ticks: usize = a.first().and_then(|s| s.parse().ok()).unwrap_or(8000);
    let bots: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    let base: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: compose_seed(base, 0) });
    scatter_world_nodes(app.world_mut(), 1);
    for i in 0..bots {
        app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
            player_id: 1000 + i,
            host: 1000,
            difficulty: AiDifficulty::Hard,
            faction: if i % 2 == 0 { Faction::Ayyubid } else { Faction::Crusader },
            match_id: 1,
        });
    }

    let t0 = std::time::Instant::now();
    for _ in 0..ticks {
        step(app.world_mut());
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let world = app.world_mut();
    let units = world.query::<&Unit>().iter(world).count();
    let buildings = world.query::<&Building>().iter(world).count();
    println!(
        "{bots} Hard bots x {ticks} ticks on seed {base}: {:.3} ms/tick ({:.2} s total), \
         {units} units, {buildings} buildings, hash {:#x}",
        ms / ticks as f64,
        ms / 1000.0,
        world.resource::<StateHash>().0
    );
}
