//! Part 4: how much room does a base actually have? Buildable-ground density
//! around real keep sites, and how far the nearest farm plot is.

use saladin_sim::*;

fn main() {
    println!("{:<10} {:>6} {:>8} {:>8} {:>8} {:>10} {:>10}", "seed/preset", "keep", "build%12", "build%20", "build%28", "farm@", "shore@");
    for preset in 0..4u8 {
        for base in [48514u32, 7, 991, 30303] {
            let seed = compose_seed(base, preset);
            let kfp = building_def(BuildingKind::Keep).footprint;
            let site = find_keep_site(seed, 0, kfp);
            let (kx, ky) = (site.x.to_num::<i32>(), site.y.to_num::<i32>());
            let half = fx!("0.5");
            let mut pct = [0f32; 3];
            for (i, r) in [12i32, 20, 28].iter().enumerate() {
                let (mut ok, mut tot) = (0, 0);
                for dy in -r..=*r {
                    for dx in -r..=*r {
                        if dx * dx + dy * dy > r * r {
                            continue;
                        }
                        tot += 1;
                        let (tx, ty) = (kx + dx, ky + dy);
                        let x = Fx::from_num(tx) + half;
                        let y = Fx::from_num(ty) + half;
                        if is_buildable_tile(seed, tx, ty) && slope_at(seed, x, y) <= BUILD_SLOPE_MAX {
                            ok += 1;
                        }
                    }
                }
                pct[i] = ok as f32 / tot as f32 * 100.0;
            }
            // nearest legal Farm (min_fertility) and nearest shore (Fishing Hut)
            let mut farm_at = -1i32;
            let mut shore_at = -1i32;
            'r: for r in 1..60i32 {
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dy.abs()) != r {
                            continue;
                        }
                        let x = Fx::from_num(kx + dx) + half;
                        let y = Fx::from_num(ky + dy) + half;
                        if farm_at < 0
                            && check_place(seed, BuildingKind::Farm, x, y, |_, _| false, |_, _| true, &[]).is_ok()
                        {
                            farm_at = r;
                        }
                        if shore_at < 0
                            && check_place(seed, BuildingKind::FishingHut, x, y, |_, _| false, |_, _| true, &[]).is_ok()
                        {
                            shore_at = r;
                        }
                        if farm_at >= 0 && shore_at >= 0 {
                            break 'r;
                        }
                    }
                }
            }
            println!("{:<10} {:>6} {:>7.0}% {:>7.0}% {:>7.0}% {:>10} {:>10}",
                format!("{base}/{preset}"), format!("{kx},{ky}"), pct[0], pct[1], pct[2],
                if farm_at < 0 { "NONE<60".into() } else { format!("{farm_at}t") },
                if shore_at < 0 { "NONE<60".into() } else { format!("{shore_at}t") });
        }
    }
    println!("\nTOWN_RADIUS = {} tiles, keep footprint {}", TOWN_RADIUS.to_num::<i32>(), building_def(BuildingKind::Keep).footprint);
    let area = std::f32::consts::PI * TOWN_RADIUS.to_num::<f32>().powi(2);
    println!("a single keep licenses {:.0} tiles of ground; a 2x2 building eats 4.", area);
    println!("houses needed for pop 200: {} (each 2x2 = 4 tiles, {} wood)",
        (200 - 8) / 6 + 1, ((200 - 8) / 6 + 1) * 40);
}
