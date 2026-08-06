//! Do units end up standing on impassable tiles, or inside passable tiles they
//! cannot legally walk out of? Both would explain gatherers frozen next to a
//! node they can never reach.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

/// A passable tile with no legal exit (4 orthogonal neighbours blocked and all
/// four diagonals refused by A*'s corner rule).
fn is_pocket(seed: u32, x: i32, y: i32) -> bool {
    if !is_passable(seed, x, y) {
        return false;
    }
    let o = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    if o.iter().any(|(dx, dy)| is_passable(seed, x + dx, y + dy)) {
        return false;
    }
    // orthogonals all blocked -> every diagonal is corner-refused too
    true
}

fn main() {
    let secs: u32 =
        std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(500);
    let base: u32 = std::env::var("PROBE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(48514);
    let seed = compose_seed(base, 0);

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1,
        host: 1,
        difficulty: AiDifficulty::Hard,
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());

    let mut on_wall: std::collections::BTreeMap<u64, u32> = Default::default();
    let mut in_pocket: std::collections::BTreeMap<u64, u32> = Default::default();
    let mut first_wall: Option<(u64, u32, i32, i32)> = None;
    let mut first_pocket: Option<(u64, u32, i32, i32)> = None;

    for t in 0..secs * 20 {
        step(app.world_mut());
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &Owner, &Pos, &Unit)>();
        for (g, o, p, u) in q.iter(w) {
            if o.0 != 1 || u.kind != UnitKind::Peasant {
                continue;
            }
            let (x, y) = (p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>());
            if !is_passable(seed, x, y) {
                *on_wall.entry(g.0).or_default() += 1;
                first_wall.get_or_insert((g.0, t, x, y));
            } else if is_pocket(seed, x, y) {
                *in_pocket.entry(g.0).or_default() += 1;
                first_pocket.get_or_insert((g.0, t, x, y));
            }
        }
    }

    println!("ticks each peasant spent STANDING ON AN IMPASSABLE TILE:");
    if on_wall.is_empty() {
        println!("  none");
    }
    for (id, n) in &on_wall {
        println!("  u{id}: {n} ticks");
    }
    if let Some((id, t, x, y)) = first_wall {
        println!("  first: u{id} at tick {t} on tile ({x},{y})");
    }
    println!("\nticks each peasant spent INSIDE A ONE-TILE POCKET (no legal exit):");
    if in_pocket.is_empty() {
        println!("  none");
    }
    for (id, n) in &in_pocket {
        println!("  u{id}: {n} ticks");
    }
    if let Some((id, t, x, y)) = first_pocket {
        println!("  first: u{id} at tick {t} on tile ({x},{y})");
    }

    // is (130,152) on this map such a pocket?
    println!("\n(130,152) passable={} pocket={}", is_passable(seed, 130, 152), is_pocket(seed, 130, 152));
    let mut n_pockets = 0;
    for y in 0..WORLD_SIZE {
        for x in 0..WORLD_SIZE {
            if is_pocket(seed, x, y) {
                n_pockets += 1;
            }
        }
    }
    println!("one-tile pockets on this map: {n_pockets}");
}
