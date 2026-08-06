//! How big does the reachable-region flood have to be before a gatherer at
//! (131.5,151.5) on seed 48514 can see the food node at (129.02,153.01)?

use saladin_sim::*;

fn main() {
    let seed = compose_seed(48514, 0);
    let pass = |x: i32, y: i32| is_passable(seed, x, y);
    let node = V2::new(fx!("129.022"), fx!("153.008"));
    let reach = harvest_reach(0);

    for &from in &[
        V2::new(fx!("131.5"), fx!("151.5")),
        V2::new(fx!("132.5"), fx!("150.5")),
        V2::new(fx!("130.5"), fx!("152.5")),
    ] {
        println!(
            "\nfrom ({:.1},{:.1}) d={:.2} reach={:.2}",
            from.x.to_num::<f32>(),
            from.y.to_num::<f32>(),
            dist(from, node).to_num::<f32>(),
            reach.to_num::<f32>()
        );
        let mut flood = Flood::new();
        for cap in [256usize, 512, 1024, 2048, 4096, 8192, 16384, 32768, 384 * 384] {
            let r = nearest_reachable_passable_grid(&mut flood, &pass, from, node, cap).unwrap();
            let d = dist(r.at, node);
            println!(
                "  cap {cap:>6} -> ({:.1},{:.1}) d={:.2} truncated={}{}",
                r.at.x.to_num::<f32>(),
                r.at.y.to_num::<f32>(),
                d.to_num::<f32>(),
                r.truncated,
                if d <= reach { "   <= IN HARVEST RANGE" } else { "" }
            );
        }
        if let Some(a) = approach_tile(seed, &pass, from, node, 4) {
            println!(
                "  approach_tile(region) -> ({:.1},{:.1}) d={:.2}",
                a.x.to_num::<f32>(),
                a.y.to_num::<f32>(),
                dist(a, node).to_num::<f32>()
            );
        }
        // and the A* path length to the true approach tile
        let mut a = AStar::new();
        let cost = |x: i32, y: i32| move_cost_at(seed, x, y);
        let goal =
            nearest_reachable_passable_grid(&mut flood, &pass, from, node, 384 * 384).unwrap().at;
        let p = a.find_path_costed(&pass, &cost, from.x, from.y, goal.x, goal.y, MAX_EXPANSIONS);
        println!("  A* to the true approach tile: {} waypoints", p.len());
    }
}
