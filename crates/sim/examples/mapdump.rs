//! Dev tool: dump a seed's biome map to a PNG-convertible PPM. Land the seating
//! rule will not put a player on is dimmed, and the eight starts are marked, so
//! an archipelago's islands and who is on which are visible at a glance.
//! Usage: `cargo run -p saladin-sim --example mapdump -- <base> <preset> [out.ppm]`

use saladin_sim::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let base: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    let preset: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let out = args.get(2).cloned().unwrap_or_else(|| "/tmp/map.ppm".into());
    let seed = compose_seed(base, preset);

    let main = dominant_region(seed);
    let legal = start_regions(seed);
    let regions = region_grid(seed);
    let n = WORLD_SIZE as usize;
    let mut img = vec![0u8; n * n * 3];
    let half = fx!("0.5");
    for ty in 0..n {
        for tx in 0..n {
            let s = sample_terrain(seed, Fx::from_num(tx as i32) + half, Fx::from_num(ty as i32) + half);
            let c = biome_def(s.biome).color;
            let i = (ty * n + tx) * 3;
            let (mut r, mut g, mut b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
            let region = regions[ty * n + tx];
            if region != u16::MAX && !legal.contains(&region) {
                // land nobody can be seated on: dim so the split jumps out
                r /= 2;
                g /= 2;
                b /= 2;
            }
            img[i] = r;
            img[i + 1] = g;
            img[i + 2] = b;
        }
    }
    for slot in 0..8 {
        let p = start_point(seed, slot);
        let (sx, sy) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
        for dy in -3..=3i32 {
            for dx in -3..=3i32 {
                if dx.abs().max(dy.abs()) < 2 {
                    continue;
                }
                let (tx, ty) = (sx + dx, sy + dy);
                if tx < 0 || ty < 0 || tx >= n as i32 || ty >= n as i32 {
                    continue;
                }
                let c = PLAYER_COLORS[slot];
                let i = (ty as usize * n + tx as usize) * 3;
                img[i] = (c >> 16) as u8;
                img[i + 1] = (c >> 8) as u8;
                img[i + 2] = c as u8;
            }
        }
    }
    let mut ppm = format!("P6\n{n} {n}\n255\n").into_bytes();
    ppm.extend_from_slice(&img);
    std::fs::write(&out, ppm).expect("write ppm");

    let mut pass = 0u32;
    let mut dom = 0u32;
    for &r in regions {
        if r != u16::MAX {
            pass += 1;
            if r == main {
                dom += 1;
            }
        }
    }
    let seated: std::collections::BTreeSet<u16> =
        (0..8).map(|s| region_at(seed, start_point(seed, s).x, start_point(seed, s).y)).collect();
    println!(
        "seed {base} preset {preset}: {pass} passable, dominant {dom} ({}%), {} legal start islands, {} of them seated, wrote {out}",
        dom * 100 / pass.max(1),
        legal.len(),
        seated.len()
    );
}
