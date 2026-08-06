//! Passability + reachability around one node id, for the AI-economy stall hunt.
//! cargo run --release -p saladin-protocol --example stall_map -- <node_id>

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(3058);
    let secs: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(500);
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
    for _ in 0..secs * 20 {
        step(app.world_mut());
    }
    let w = app.world_mut();

    let node = {
        let mut q = w.query::<(&GameId, &Pos, &ResourceNode)>();
        q.iter(w).find(|(g, ..)| g.0 == want).map(|(_, p, n)| (p.pos, n.res_type, n.remaining))
    };
    let Some((npos, rt, rem)) = node else {
        println!("node {want} is gone");
        return;
    };
    let peas: Vec<(u64, V2)> = {
        let mut q = w.query::<(&GameId, &Owner, &Pos, &Unit)>();
        let mut v: Vec<_> = q
            .iter(w)
            .filter(|(_, o, _, u)| o.0 == 1 && u.kind == UnitKind::Peasant && u.target_node == want)
            .map(|(g, _, p, _)| (g.0, p.pos))
            .collect();
        v.sort_by_key(|x| x.0);
        v
    };
    let (nx, ny) = (npos.x.to_num::<i32>(), npos.y.to_num::<i32>());
    println!(
        "node {want} {rt:?} rem={rem} at ({:.3},{:.3}) tile ({nx},{ny}) passable={} region={}",
        npos.x.to_num::<f32>(),
        npos.y.to_num::<f32>(),
        is_passable(seed, nx, ny),
        region_at(seed, npos.x, npos.y),
    );
    println!("{} peasants targeting it", peas.len());

    let pass = |x: i32, y: i32| is_passable(seed, x, y);
    let r = 12;
    println!("\npassability, '*' = node tile, digits = peasant, region id per tile below:");
    for y in (ny - r)..=(ny + r) {
        let mut row = String::new();
        for x in (nx - r)..=(nx + r) {
            let here = peas.iter().position(|(_, p)| {
                p.x.to_num::<i32>() == x && p.y.to_num::<i32>() == y
            });
            let c = if x == nx && y == ny {
                '*'
            } else if let Some(i) = here {
                char::from_digit(i as u32 % 10, 10).unwrap()
            } else if pass(x, y) {
                '.'
            } else {
                '#'
            };
            row.push(c);
        }
        println!("  y={y:3} {row}");
    }

    // which tiles are within harvest reach of the node at all?
    let reach = harvest_reach(0);
    println!("\ntiles whose CENTRE is within harvest reach ({:.2}) of the node:", reach.to_num::<f32>());
    let mut any = false;
    for y in (ny - 2)..=(ny + 2) {
        for x in (nx - 2)..=(nx + 2) {
            let c = V2::new(Fx::from_num(x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5"));
            if dist(c, npos) <= reach {
                println!(
                    "  ({x},{y}) centre d={:.3} passable={}",
                    dist(c, npos).to_num::<f32>(),
                    pass(x, y)
                );
                any = true;
            }
        }
    }
    if !any {
        println!("  NONE — no tile centre is within reach of this node at all");
    }
    // the closest any point of a passable tile can get
    println!("\nclosest approach from each passable tile in the 5x5 (tile centre distance):");
    let mut best: Vec<(f32, i32, i32)> = Vec::new();
    for y in (ny - 3)..=(ny + 3) {
        for x in (nx - 3)..=(nx + 3) {
            if !pass(x, y) {
                continue;
            }
            let c = V2::new(Fx::from_num(x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5"));
            best.push((dist(c, npos).to_num::<f32>(), x, y));
        }
    }
    best.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (d, x, y) in best.iter().take(6) {
        println!("  ({x},{y}) d={d:.3}");
    }

    // capped vs uncapped reachable flood from each stalled peasant
    println!("\nnearest_reachable_passable_grid, capped(1024) vs uncapped:");
    for (id, p) in &peas {
        let mut flood = Flood::new();
        let capped = nearest_reachable_passable_grid(&mut flood, &pass, *p, npos, 1024).unwrap().at;
        let full =
            nearest_reachable_passable_grid(&mut flood, &pass, *p, npos, 384 * 384).unwrap().at;
        println!(
            "  u{id} at({:.2},{:.2}) d={:.2}  capped->({:.1},{:.1}) d={:.2}   uncapped->({:.1},{:.1}) d={:.2}",
            p.x.to_num::<f32>(),
            p.y.to_num::<f32>(),
            dist(*p, npos).to_num::<f32>(),
            capped.x.to_num::<f32>(),
            capped.y.to_num::<f32>(),
            dist(capped, npos).to_num::<f32>(),
            full.x.to_num::<f32>(),
            full.y.to_num::<f32>(),
            dist(full, npos).to_num::<f32>(),
        );
    }
}
