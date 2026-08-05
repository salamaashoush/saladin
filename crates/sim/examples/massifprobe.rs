//! Dev tool: per-seed feature counts worldstat's percentage histogram rounds
//! away (Wadi, shore kinds), plus a search for seeds whose slot-0 start sits
//! under a massif — the ones worth screenshotting.

use saladin_sim::*;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "shore".into());
    if mode == "start" {
        let preset: u8 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
        let mut best: Vec<(u32, u32)> = Vec::new();
        for base in 1..400u32 {
            let seed = compose_seed(base, preset);
            let g = worldgrid::world_grid(seed);
            let s = start_point(seed, 0);
            let (sx, sy) = (s.x.to_num::<i32>(), s.y.to_num::<i32>());
            let mut high = 0u32;
            for dy in -45..=45i32 {
                for dx in -45..=45i32 {
                    let (tx, ty) = (sx + dx, sy + dy);
                    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
                        continue;
                    }
                    let i = (ty * WORLD_SIZE + tx) as usize;
                    if matches!(g.biome[i], Biome::Mountain | Biome::Snow | Biome::Cliff | Biome::Alpine) {
                        high += 1;
                    }
                }
            }
            best.push((base, high));
        }
        best.sort_by_key(|e| std::cmp::Reverse(e.1));
        for (b, h) in best.iter().take(12) {
            println!("base {b:>4} preset {preset}: {h} high tiles near slot 0");
        }
        return;
    }

    println!("{:<8} {:<18} {:>6} {:>6} {:>6} {:>6} {:>6}", "seed", "climate", "wadi", "sand", "dune", "green", "cliff");
    for base in [1000u32, 36761, 5226, 40987, 9452, 77917, 13678, 82143, 17904, 86369] {
        for preset in [0u8, 2] {
            let seed = compose_seed(base, preset);
            let g = worldgrid::world_grid(seed);
            let (mut wadi, mut sand, mut dune, mut cliff) = (0u32, 0u32, 0u32, 0u32);
            let mut green = 0u32;
            for i in 0..g.biome.len() {
                match g.biome[i] {
                    Biome::Wadi => wadi += 1,
                    Biome::Sand => sand += 1,
                    Biome::Dunes => dune += 1,
                    Biome::Cliff => cliff += 1,
                    _ => {}
                }
                // shore tiles that stayed green: land within the beach band
                if g.tile_h[i] < fx!("0.40")
                    && g.tile_h[i] >= fx!("0.38")
                    && matches!(g.biome[i], Biome::Grassland | Biome::Marsh | Biome::Savanna | Biome::Oasis)
                {
                    green += 1;
                }
            }
            println!(
                "{:<8} {:<18} {wadi:>6} {sand:>6} {dune:>6} {green:>6} {cliff:>6}",
                format!("{base}/{preset}"),
                g.climate.label
            );
        }
    }
}
