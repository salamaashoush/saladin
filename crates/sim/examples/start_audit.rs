//! Is every start's guaranteed resource actually REACHABLE from it? The fair
//! start guarantee counts nodes inside a radius; a walker can only work the ones
//! in its own terrain region.
//!
//! cargo run --release -p saladin-sim --example start_audit [slots] [seeds]

use saladin_sim::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let slots: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(2);
    let seeds: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(40);

    println!("slots {slots}, {seeds} base seeds x 4 presets");
    let mut starved = 0;
    let mut total = 0;
    let mut long_haul = 0;
    let mut worst_wood_road = 0;
    const LONG_HAUL: i32 = 30;
    let only: Option<u32> = std::env::var("AUDIT_SEED").ok().and_then(|s| s.parse().ok());
    for base in only.map(|b| b..=b).unwrap_or(1..=seeds) {
        for preset in 0..4u8 {
            let seed = compose_seed(base, preset);
            let nodes = scatter_nodes(seed, &node_kinds());
            let extra = fair_start_nodes(seed, &nodes, slots, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
            let all: Vec<ScatteredNode> = nodes.into_iter().chain(extra).collect();
            let r2 = FAIR_RADIUS * FAIR_RADIUS;
            for slot in 0..slots {
                let start = start_point(seed, slot);
                let region = region_at(seed, start.x, start.y);
                let mut inradius = [0usize; 3];
                let mut inregion = [0usize; 3];
                for n in &all {
                    let dx = n.pos.x - start.x;
                    let dy = n.pos.y - start.y;
                    if dx * dx + dy * dy > r2 {
                        continue;
                    }
                    let i = match n.res_type {
                        ResourceType::Wood => 0,
                        ResourceType::Stone => 1,
                        ResourceType::Food => 2,
                        ResourceType::Gold => continue,
                    };
                    inradius[i] += 1;
                    if region_at(seed, n.pos.x, n.pos.y) == region {
                        inregion[i] += 1;
                    }
                }
                // Region-connected is not the same as CHEAP to reach. Measure
                // the actual road: a start whose only wood is a sixty-tile
                // round trip has a working economy on paper and none in play.
                let mut astar = AStar::default();
                let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty);
                let cost = |tx: i32, ty: i32| move_cost_at(seed, tx, ty);
                let mut road = [i32::MAX; 3];
                for n in &all {
                    let i = match n.res_type {
                        ResourceType::Wood => 0,
                        ResourceType::Stone => 1,
                        ResourceType::Food => 2,
                        ResourceType::Gold => continue,
                    };
                    let dx = n.pos.x - start.x;
                    let dy = n.pos.y - start.y;
                    if dx * dx + dy * dy > r2 || region_at(seed, n.pos.x, n.pos.y) != region {
                        continue;
                    }
                    let path = astar.find_path_costed(
                        &passable, &cost, start.x, start.y, n.pos.x, n.pos.y, MAX_EXPANSIONS,
                    );
                    if path.is_empty() {
                        continue;
                    }
                    let mut len = Fx::ZERO;
                    let mut at = start;
                    for w in &path {
                        len = len + dist(at, *w);
                        at = *w;
                    }
                    road[i] = road[i].min(len.to_num::<i32>());
                }
                if road[0] > worst_wood_road {
                    worst_wood_road = road[0];
                }
                if road[0] > LONG_HAUL && road[0] != i32::MAX {
                    long_haul += 1;
                    if long_haul <= 12 {
                        println!(
                            "  base {base} preset {preset} slot {slot} at ({},{}): nearest wood is \
                             {} tiles BY ROAD (straight line {})",
                            start.x.to_num::<i32>(), start.y.to_num::<i32>(), road[0],
                            all.iter().filter(|n| n.res_type == ResourceType::Wood
                                && region_at(seed, n.pos.x, n.pos.y) == region)
                                .map(|n| dist(start, n.pos).to_num::<i32>())
                                .filter(|d| *d <= FAIR_RADIUS.to_num::<i32>())
                                .min().unwrap_or(-1)
                        );
                    }
                }
                total += 1;
                let want = [FAIR_MIN_WOOD, FAIR_MIN_STONE, FAIR_MIN_FOOD];
                let short: Vec<&str> = ["wood", "stone", "food"]
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| inregion[*i] < want[*i])
                    .map(|(_, s)| *s)
                    .collect();
                if only.is_some() {
                    println!(
                        "  base {base} preset {preset} slot {slot} at ({},{}): in-radius {inradius:?} in-region {inregion:?}",
                        start.x.to_num::<i32>(), start.y.to_num::<i32>()
                    );
                }
                if !short.is_empty() {
                    starved += 1;
                    if starved <= 25 {
                        println!(
                            "  base {base} preset {preset} slot {slot} at ({},{}): SHORT {short:?} \
                             in-radius {inradius:?} in-region {inregion:?}",
                            start.x.to_num::<i32>(),
                            start.y.to_num::<i32>()
                        );
                    }
                }
            }
        }
    }
    println!("starts short of a REACHABLE guaranteed resource: {starved} / {total}");
    println!("starts whose nearest wood is over {LONG_HAUL} tiles BY ROAD: {long_haul} / {total} (worst {worst_wood_road})");
}
