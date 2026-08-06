//! Dev tool: where the resource scatter actually lands, against the terrain it
//! reads. Usage: `cargo run --release -p saladin-sim --example resprobe -- [seeds] [preset|all]`

use saladin_sim::*;
use std::collections::BTreeMap;

fn q(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

fn kind_of(n: &ScatteredNode, seed: u32) -> &'static str {
    match (n.res_type, n.yield_) {
        (ResourceType::Wood, _) => "timber",
        (ResourceType::Stone, y) if y == STONE_YIELD => "quarry",
        (ResourceType::Stone, _) => "stone-lode",
        (ResourceType::Gold, y) if y == GOLD_YIELD => "vein",
        (ResourceType::Gold, y) if y < GOLD_YIELD => "placer",
        (ResourceType::Gold, _) => "gold-lode",
        (ResourceType::Food, _) => {
            if is_sailable(seed, n.pos.x.to_num::<i32>(), n.pos.y.to_num::<i32>()) { "fishery" } else { "herds" }
        }
    }
}

#[derive(Default)]
struct Tally {
    n: usize,
    slope: Vec<f64>,
    ore: Vec<f64>,
    fert: Vec<f64>,
    height: Vec<f64>,
}

fn share_slopes(nodes: &[ScatteredNode], seed: u32, kind: &str) -> Vec<f64> {
    nodes
        .iter()
        .filter(|nd| kind_of(nd, seed) == kind)
        .map(|nd| slope_at(seed, nd.pos.x, nd.pos.y).to_num::<f64>())
        .collect()
}

/// Headroom check for `fair_start_nodes`: how much reachable ring each start
/// actually has to place its guaranteed minima on, across all 100 test worlds.
fn fair_headroom() {
    let mut worst = (usize::MAX, 0u32, 0u8, 0usize);
    let (mut ring_rocky, mut ring_all) = (0usize, 0usize);
    let mut short = 0usize;
    let mut natural = [0usize; 3];
    let mut hist = [0usize; 6];
    for base in 1..=25u32 {
        for preset in 0..4u8 {
            let seed = compose_seed(base, preset);
            let nodes = scatter_nodes(seed, &node_kinds());
            for slot in 0..8 {
                let start = start_point(seed, slot);
                let region = region_at(seed, start.x, start.y);
                let (sx, sy) = (start.x.to_num::<i32>(), start.y.to_num::<i32>());
                let r = FAIR_RADIUS.to_num::<i32>();
                let mut cells = 0usize;
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dy.abs()) < 6 {
                            continue;
                        }
                        let (tx, ty) = (sx + dx, sy + dy);
                        if !is_passable(seed, tx, ty) {
                            continue;
                        }
                        let p = V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"));
                        if region_at(seed, p.x, p.y) == region {
                            cells += 1;
                            let s = node_site(seed, p.x, p.y);
                            if s.slope >= fx!("0.18") || rock_density(s.biome) > Fx::ZERO {
                                ring_rocky += 1;
                            }
                            ring_all += 1;
                        }
                    }
                }
                if cells < worst.0 {
                    worst = (cells, base, preset, slot);
                }
                hist[(cells / 100).min(5)] += 1;
                let r2 = FAIR_RADIUS * FAIR_RADIUS;
                let mut have = [0usize; 3];
                for nd in &nodes {
                    let (dx, dy) = (nd.pos.x - start.x, nd.pos.y - start.y);
                    if dx * dx + dy * dy > r2 {
                        continue;
                    }
                    match nd.res_type {
                        ResourceType::Wood => have[0] += 1,
                        ResourceType::Stone => have[1] += 1,
                        ResourceType::Food => have[2] += 1,
                        ResourceType::Gold => {}
                    }
                }
                for (i, m) in [FAIR_MIN_WOOD, FAIR_MIN_STONE, FAIR_MIN_FOOD].iter().enumerate() {
                    if have[i] >= *m {
                        natural[i] += 1;
                    }
                }
                if have[0] < FAIR_MIN_WOOD || have[1] < FAIR_MIN_STONE || have[2] < FAIR_MIN_FOOD {
                    short += 1;
                }
            }
        }
    }
    let (mut topups, mut rocky) = (0usize, 0usize);
    for base in 1..=25u32 {
        for preset in 0..4u8 {
            let seed = compose_seed(base, preset);
            let nodes = scatter_nodes(seed, &node_kinds());
            for e in fair_start_nodes(seed, &nodes, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD) {
                if e.res_type != ResourceType::Stone {
                    continue;
                }
                topups += 1;
                let s = node_site(seed, e.pos.x, e.pos.y);
                if s.slope >= fx!("0.18") || rock_density(s.biome) > Fx::ZERO {
                    rocky += 1;
                }
            }
        }
    }
    println!(
        "  guaranteed stone on rocky ground: {rocky}/{topups} ({:.0}%)",
        rocky as f64 * 100.0 / topups.max(1) as f64
    );
    println!(
        "  rocky share of the reachable ring: {:.0}% (what an unfiltered top-up would hit)",
        ring_rocky as f64 * 100.0 / ring_all.max(1) as f64
    );
    println!("fair-start headroom over 100 worlds x 8 slots:");
    println!(
        "  tightest ring: {} in-region passable tiles at rings 6..20 (seed {} preset {} slot {})",
        worst.0, worst.1, worst.2, worst.3
    );
    println!("  ring size histogram (per 100 tiles): {hist:?}");
    println!(
        "  starts the raw scatter already satisfies: wood {}/800 stone {}/800 food {}/800 | {} need a top-up",
        natural[0], natural[1], natural[2], short
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--fair") {
        fair_headroom();
        return;
    }
    let count: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(8);
    let explicit: Option<Vec<u32>> = args
        .iter()
        .position(|a| a == "--seeds")
        .and_then(|i| args.get(i + 1))
        .map(|l| l.split(',').filter_map(|s| s.trim().parse().ok()).collect());
    let per_seed = args.iter().any(|a| a == "--per-seed");
    let bases: Vec<u32> = explicit
        .unwrap_or_else(|| (0..count).map(|s| 1000 + (s.wrapping_mul(2654435761) % 100000)).collect());
    let presets: Vec<u8> = match args.get(1).map(|s| s.as_str()) {
        Some(p) if p.parse::<u8>().is_ok() => vec![p.parse().unwrap()],
        _ => (0..MAP_PRESETS.len() as u8).collect(),
    };
    let n = WORLD_SIZE as usize;

    for preset in presets {
        println!("\n=== preset {preset} ({}) over {} seeds ===", MAP_PRESETS[preset as usize].label, bases.len());
        let mut land_slope: Vec<f64> = Vec::new();
        let mut per_kind: BTreeMap<&'static str, Tally> = BTreeMap::new();
        let mut fair_extra = 0usize;
        let mut biome_of_stone: BTreeMap<&'static str, usize> = BTreeMap::new();
        for &b in &bases {
            let seed = compose_seed(b, preset);
            let g = worldgrid::world_grid(seed);
            for i in 0..n * n {
                if biome_passable(g.biome[i]) {
                    land_slope.push(g.slope[i].to_num::<f64>());
                }
            }
            let nodes = scatter_nodes(seed, &node_kinds());
            let extra = fair_start_nodes(seed, &nodes, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
            fair_extra += extra.len();
            if per_seed {
                let mut ls: Vec<f64> = (0..n * n)
                    .filter(|&i| biome_passable(g.biome[i]))
                    .map(|i| g.slope[i].to_num::<f64>())
                    .collect();
                ls.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let (p75, lm) = (q(&ls, 0.75), mean(&ls));
                let share = |k: &str| {
                    let v: Vec<f64> = nodes
                        .iter()
                        .filter(|nd| kind_of(nd, seed) == k)
                        .map(|nd| slope_at(seed, nd.pos.x, nd.pos.y).to_num::<f64>())
                        .collect();
                    let above = v.iter().filter(|&&s| s > p75).count() as f64 * 100.0 / v.len().max(1) as f64;
                    (above, mean(&v) / lm)
                };
                let (hb, hr) = share("herds");
                let (qb, qr) = share("quarry");
                let (tb, tr) = share("timber");
                let host = |f: fn(Biome) -> Fx| {
                    let v: Vec<f64> = (0..n * n)
                        .filter(|&i| biome_passable(g.biome[i]) && f(g.biome[i]) > Fx::ZERO)
                        .map(|i| g.slope[i].to_num::<f64>())
                        .collect();
                    mean(&v)
                };
                let (graze, stand) = (host(game_density), host(tree_density));
                let band = |lo: f64, hi: f64| {
                    let tiles = (0..n * n)
                        .filter(|&i| {
                            let s = g.slope[i].to_num::<f64>();
                            biome_passable(g.biome[i])
                                && tree_density(g.biome[i]) > Fx::ZERO
                                && s >= lo
                                && s < hi
                        })
                        .count();
                    let trees = share_slopes(&nodes, seed, "timber")
                        .iter()
                        .filter(|&&s| s >= lo && s < hi)
                        .count();
                    (trees, tiles)
                };
                let (tg, cg) = band(0.0, 0.22);
                let (ts, cs) = band(0.22, 0.62);
                let (_, cx) = band(0.62, 99.0);
                let dg = tg as f64 / cg.max(1) as f64;
                let ds = ts as f64 / cs.max(1) as f64;
                println!(
                    "         timber per tile: gentle {dg:.3} taper {ds:.3} ratio {:.2} | tree-capable tiles over cutoff {cx}",
                    ds / dg.max(1e-9)
                );
                println!(
                    "  seed {b:>6} land_mean {lm:.3} p75 {p75:.3} | herds >p75 {hb:.0}% x{hr:.2} host x{:.2} | quarry >p75 {qb:.0}% x{qr:.2} | timber >p75 {tb:.0}% x{tr:.2} host x{:.2}",
                    mean(&share_slopes(&nodes, seed, "herds")) / graze,
                    mean(&share_slopes(&nodes, seed, "timber")) / stand,
                );
            }
            for nd in &nodes {
                let k = kind_of(nd, seed);
                let s = node_site(seed, nd.pos.x, nd.pos.y);
                let e = per_kind.entry(k).or_default();
                e.n += 1;
                e.slope.push(s.slope.to_num::<f64>());
                e.ore.push(s.ore.to_num::<f64>());
                e.fert.push(s.fertility.to_num::<f64>());
                e.height.push(s.height.to_num::<f64>());
                if k == "quarry" || k == "stone-lode" {
                    *biome_of_stone.entry(biome_def(s.biome).label).or_default() += 1;
                }
            }
        }
        land_slope.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  land slope p50 {:.3} p75 {:.3} p90 {:.3} p99 {:.3} | mean {:.3}",
            q(&land_slope, 0.5),
            q(&land_slope, 0.75),
            q(&land_slope, 0.9),
            q(&land_slope, 0.99),
            mean(&land_slope)
        );
        let steep = q(&land_slope, 0.75);
        println!(
            "  {:<11} {:>7} {:>8} {:>8} {:>8} {:>7} {:>7} {:>7}",
            "kind", "n/seed", "slope", ">p75", ">0.25", "ore", "fert", "h"
        );
        for (k, t) in &per_kind {
            let sl = &t.slope;
            let pct = |v: f64| sl.iter().filter(|&&s| s > v).count() as f64 * 100.0 / sl.len().max(1) as f64;
            println!(
                "  {:<11} {:>7.0} {:>8.3} {:>7.0}% {:>7.0}% {:>7.3} {:>7.3} {:>7.3}",
                k,
                t.n as f64 / bases.len() as f64,
                mean(sl),
                pct(steep),
                pct(0.25),
                mean(&t.ore),
                mean(&t.fert),
                mean(&t.height)
            );
        }
        println!("  fair-start top-ups {:.1}/seed", fair_extra as f64 / bases.len() as f64);
        let total: usize = biome_of_stone.values().sum();
        let mut ranked: Vec<_> = biome_of_stone.into_iter().collect();
        ranked.sort_by_key(|e| std::cmp::Reverse(e.1));
        let top: Vec<String> = ranked
            .iter()
            .take(6)
            .map(|(l, c)| format!("{l} {:.0}%", *c as f64 * 100.0 / total.max(1) as f64))
            .collect();
        println!("  stone sits on: {}", top.join(", "));
    }
}
