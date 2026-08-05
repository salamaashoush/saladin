//! Dev tool: biome / climate histogram across many seeds — the tuning dial for
//! worldgen diversity. Usage:
//! `cargo run -p saladin-sim --example worldstat -- [seeds] [preset|all] [--per-seed] [--seeds a,b,c]`

use saladin_sim::*;
use std::collections::{BTreeMap, BTreeSet};

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(24);
    let per_seed = args.iter().any(|a| a == "--per-seed");
    let explicit: Option<Vec<u32>> = args
        .iter()
        .position(|a| a == "--seeds")
        .and_then(|i| args.get(i + 1))
        .map(|list| list.split(',').filter_map(|s| s.trim().parse().ok()).collect());
    // a fixed arithmetic series only ever rolled 6 of the 8 archetypes; the
    // hash spread covers all of them at 12 seeds
    let bases: Vec<u32> = explicit
        .unwrap_or_else(|| (0..count).map(|s| 1000 + (s.wrapping_mul(2654435761) % 100000)).collect());
    let presets: Vec<u8> = match args.get(1).map(|s| s.as_str()) {
        Some(p) if p.parse::<u8>().is_ok() => vec![p.parse().unwrap()],
        _ => (0..MAP_PRESETS.len() as u8).collect(),
    };
    let n = WORLD_SIZE as usize;
    let mut archetypes: BTreeSet<&'static str> = BTreeSet::new();

    for preset in presets {
        let mut totals: BTreeMap<&'static str, u64> = BTreeMap::new();
        let mut land_total = 0u64;
        let mut worst_dominance = 0.0f64;
        let mut mean_dominance = 0.0f64;
        let mut mean_kinds = 0.0f64;
        let mut heights: Vec<f64> = Vec::new();
        let mut slopes: Vec<f64> = Vec::new();
        let mut high_country = 0u64;
        println!(
            "\n=== preset {preset} ({}) over {} seeds ===",
            MAP_PRESETS[preset as usize].label,
            bases.len()
        );
        for &b in &bases {
            let seed = compose_seed(b, preset);
            let g = worldgrid::world_grid(seed);
            archetypes.insert(g.climate.label);
            let mut per: BTreeMap<&'static str, u64> = BTreeMap::new();
            let mut land = 0u64;
            let (mut t_sum, mut p_sum, mut f_sum) = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..n * n {
                let bi = g.biome[i];
                let label = biome_def(bi).label;
                *totals.entry(label).or_default() += 1;
                if matches!(bi, Biome::Mountain | Biome::Snow | Biome::Alpine | Biome::Cliff | Biome::Hills) {
                    high_country += 1;
                }
                if biome_passable(bi) {
                    *per.entry(label).or_default() += 1;
                    land += 1;
                    t_sum += g.temp[i].to_num::<f64>();
                    p_sum += g.moisture[i].to_num::<f64>();
                    f_sum += g.fertility[i].to_num::<f64>();
                    heights.push(g.tile_h[i].to_num::<f64>());
                    slopes.push(g.slope[i].to_num::<f64>());
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
        heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  land {:.1}% | top-land-biome share mean {:.0}% worst {:.0}% | biomes>3% mean {:.1}",
            land_total as f64 * 100.0 / all as f64,
            mean_dominance / bases.len() as f64 * 100.0,
            worst_dominance * 100.0,
            mean_kinds / bases.len() as f64,
        );
        println!(
            "  tile_h p10 {:.3} p50 {:.3} p90 {:.3} p99 {:.3} | high country {:.1}% of land",
            quantile(&heights, 0.10),
            quantile(&heights, 0.50),
            quantile(&heights, 0.90),
            quantile(&heights, 0.99),
            high_country as f64 * 100.0 / land_total.max(1) as f64,
        );
        slopes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  land slope p50 {:.2} p75 {:.2} p90 {:.2} p97 {:.2} p99 {:.2} max {:.2}",
            quantile(&slopes, 0.50),
            quantile(&slopes, 0.75),
            quantile(&slopes, 0.90),
            quantile(&slopes, 0.97),
            quantile(&slopes, 0.99),
            slopes.last().copied().unwrap_or(0.0),
        );
    }
    let labels: Vec<&str> = archetypes.iter().copied().collect();
    println!("\narchetypes covered ({}/{}): {}", labels.len(), CLIMATES.len(), labels.join(", "));
}
