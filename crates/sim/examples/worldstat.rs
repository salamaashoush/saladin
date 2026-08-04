//! Dev tool: biome / climate histogram across many seeds — the tuning dial for
//! worldgen diversity. Usage:
//! `cargo run -p saladin-sim --example worldstat -- [seeds] [preset|all] [--per-seed]`

use saladin_sim::*;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seeds: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(24);
    let per_seed = args.iter().any(|a| a == "--per-seed");
    let presets: Vec<u8> = match args.get(1).map(|s| s.as_str()) {
        Some("all") | None => (0..MAP_PRESETS.len() as u8).collect(),
        Some(p) if p.parse::<u8>().is_ok() => vec![p.parse().unwrap()],
        _ => (0..MAP_PRESETS.len() as u8).collect(),
    };
    let n = WORLD_SIZE as usize;

    for preset in presets {
        let mut totals: BTreeMap<&'static str, u64> = BTreeMap::new();
        let mut land_total = 0u64;
        let mut worst_dominance = 0.0f64;
        let mut mean_dominance = 0.0f64;
        let mut mean_kinds = 0.0f64;
        println!("\n=== preset {preset} ({}) over {seeds} seeds ===", MAP_PRESETS[preset as usize].label);
        for s in 0..seeds {
            let seed = compose_seed(1000 + s * 7919, preset);
            let g = worldgrid::world_grid(seed);
            let mut per: BTreeMap<&'static str, u64> = BTreeMap::new();
            let mut land = 0u64;
            let (mut t_sum, mut p_sum, mut f_sum) = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..n * n {
                let b = g.biome[i];
                let label = biome_def(b).label;
                *totals.entry(label).or_default() += 1;
                if biome_passable(b) {
                    *per.entry(label).or_default() += 1;
                    land += 1;
                    t_sum += g.temp[i].to_num::<f64>();
                    p_sum += g.moisture[i].to_num::<f64>();
                    f_sum += g.fertility[i].to_num::<f64>();
                }
            }
            land_total += land;
            let ln = land.max(1) as f64;
            let top = per.values().copied().max().unwrap_or(0) as f64 / ln;
            worst_dominance = worst_dominance.max(top);
            mean_dominance += top;
            mean_kinds += per.values().filter(|&&v| v as f64 / ln > 0.03).count() as f64;
            if per_seed {
                let mut ranked: Vec<_> = per.iter().collect();
                ranked.sort_by_key(|e| std::cmp::Reverse(*e.1));
                let top3: Vec<String> = ranked
                    .iter()
                    .take(3)
                    .map(|e| format!("{} {:.0}%", e.0, *e.1 as f64 * 100.0 / ln))
                    .collect();
                println!(
                    "  seed {:>7} {:<18} land {:>4.1}%  T {:.2} P {:.2} F {:.2}  {}",
                    seed_base(seed),
                    g.climate.label,
                    land as f64 * 100.0 / (n * n) as f64,
                    t_sum / ln,
                    p_sum / ln,
                    f_sum / ln,
                    top3.join(", ")
                );
            }
        }
        let all: u64 = totals.values().sum();
        for (label, count) in &totals {
            let pct = *count as f64 * 100.0 / all as f64;
            if pct >= 0.05 {
                println!("  {label:<14} {pct:>6.2}%  {}", "#".repeat((pct * 0.8) as usize));
            }
        }
        println!(
            "  land {:.1}% | top-land-biome share mean {:.0}% worst {:.0}% | biomes>3% mean {:.1}",
            land_total as f64 * 100.0 / all as f64,
            mean_dominance / seeds as f64 * 100.0,
            worst_dominance * 100.0,
            mean_kinds / seeds as f64,
        );
    }
}
