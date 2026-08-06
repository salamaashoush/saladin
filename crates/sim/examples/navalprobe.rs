//! Dev tool: the WATER audit. What is out there, what the sea movement domain
//! unlocks, and what guarantees a connected match.
//!
//! `cargo run --release -p saladin-sim --example navalprobe -- [seeds] [mode]`
//!
//!   --sea      is the sea ONE body? worst seed, not the mean
//!   --starts   every start point, diffable: the proof a seating change did not
//!              move the mainland presets
//!   --reach    what share of the map's nodes a start can work on foot vs by sea
//!   --grids    what the sea grids cost against a world build
//!   --fish     where the fishery rules actually land
//!   (none)     the full audit; add --nodes for the node census, --per-seed for rows

use saladin_sim::*;
use std::collections::BTreeMap;

const N: usize = WORLD_SIZE as usize;
const D4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn flood(mask: &[bool]) -> (Vec<i32>, Vec<u32>) {
    let mut label = vec![-1i32; N * N];
    let mut sizes: Vec<u32> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    for s in 0..N * N {
        if !mask[s] || label[s] >= 0 {
            continue;
        }
        let id = sizes.len();
        sizes.push(0);
        label[s] = id as i32;
        stack.push(s as u32);
        while let Some(i) = stack.pop() {
            let i = i as usize;
            sizes[id] += 1;
            let (tx, ty) = ((i % N) as i32, (i / N) as i32);
            for (dx, dy) in D4 {
                let (nx, ny) = (tx + dx, ty + dy);
                if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                    continue;
                }
                let j = ny as usize * N + nx as usize;
                if mask[j] && label[j] < 0 {
                    label[j] = id as i32;
                    stack.push(j as u32);
                }
            }
        }
    }
    (label, sizes)
}

/// 4-BFS distance in tiles from every `src` tile, walking only through `allow`.
fn bfs_dist(src: &[bool], allow: &[bool]) -> Vec<i32> {
    let mut d = vec![i32::MAX; N * N];
    let mut q = std::collections::VecDeque::new();
    for i in 0..N * N {
        if src[i] {
            d[i] = 0;
            q.push_back(i as u32);
        }
    }
    while let Some(i) = q.pop_front() {
        let i = i as usize;
        let (tx, ty) = ((i % N) as i32, (i / N) as i32);
        for (dx, dy) in D4 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                continue;
            }
            let j = ny as usize * N + nx as usize;
            if allow[j] && d[j] == i32::MAX {
                d[j] = d[i] + 1;
                q.push_back(j as u32);
            }
        }
    }
    d
}

fn pct(a: u64, b: u64) -> f64 {
    if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 }
}

fn quant(v: &mut Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[(((v.len() - 1) as f64) * q).round() as usize]
}

#[derive(Default, Clone)]
struct Acc {
    n: f64,
    deep: f64,
    shallow: f64,
    river: f64,
    lake: f64,
    marsh: f64,
    ford: f64,
    land: f64,
    land_regions: f64,
    land_regions_big: f64,
    dom_share: f64,
    dom_share_min: f64,
    outside_dom: f64,
    sea_bodies: f64,
    sea_main_share: f64,
    water_bodies: f64,
    lakes_sea_linked: f64,
    lakes_total: f64,
    rivers_sea_linked: f64,
    river_bodies: f64,
    shallow_bodies: f64,
    shallow_d1: f64,
    shallow_d2: f64,
    shallow_d3plus: f64,
    shallow_p50: f64,
    shallow_p90: f64,
    shallow_max: f64,
    coast_land: f64,
    dock_sites: f64,
    dock_sites_dom: f64,
    islands_sea_touch: f64,
    islands_landable: f64,
    naval_land_share: f64,
    starts_regions: f64,
    start_min_gap: f64,
    nodes: f64,
    nodes_reach: f64,
    nodes_naval: f64,
    node_food_unreach: f64,
    // shelf structure
    shallow_orphan: f64,
    sea_d1: f64,
    sea_d3: f64,
    sea_d6: f64,
    sea_d12: f64,
    sea_far: f64,
    shallow_at_d1: f64,
    shallow_at_d6: f64,
    shallow_at_d12: f64,
    coast_deep_only: f64,
    sea_body2: f64,
    // straits + starts
    strait_min: f64,
    strait_p50: f64,
    strait_max: f64,
    strait_n: f64,
    keep_to_sea_min: f64,
    keep_to_sea_mean: f64,
    keep_to_sea_max: f64,
    keeps_coastal: f64,
    keeps_ai_shore: f64,
    // fisheries
    fish_nodes: f64,
    fish_lake: f64,
    fish_river: f64,
    fish_shallow: f64,
    fish_deep: f64,
    fish_stranded: f64,
    water_per_fish: f64,
    start_drag_mean: f64,
    start_drag_max: f64,
    start_gap_mean: f64,
    start_gap_p25: f64,
    top_regions: [f64; 5],
    islands_start_sized: f64,
}

macro_rules! avg {
    ($a:expr, $f:ident) => {
        $a.$f / $a.n
    };
}

/// `--sea <n>`: is the sea ONE body? The whole ferry design rests on it, so the
/// number that matters is the WORST seed, not the mean.
fn sea_unity(count: u32) {
    println!("\n== main water body share of all salt water ==");
    for preset in 0..MAP_PRESETS.len() as u8 {
        let (mut worst, mut worst_seed, mut sum, mut n) = (100.0f64, 0u32, 0.0f64, 0.0f64);
        let mut worst_second = 0u32;
        let mut min_main = u32::MAX;
        for b in 1..=count {
            let seed = compose_seed(b, preset);
            let g = worldgrid::world_grid(seed);
            let bodies = water_region_grid(seed);
            let main = main_water_body(seed);
            let (mut sea, mut in_main) = (0u64, 0u64);
            let mut sizes: BTreeMap<u16, u32> = BTreeMap::new();
            for i in 0..g.biome.len() {
                if bodies[i] != u16::MAX {
                    *sizes.entry(bodies[i]).or_default() += 1;
                }
                if biome_sailable(g.biome[i]) {
                    sea += 1;
                    if bodies[i] == main {
                        in_main += 1;
                    }
                }
            }
            let share = pct(in_main, sea);
            sum += share;
            n += 1.0;
            min_main = min_main.min(sizes.get(&main).copied().unwrap_or(0));
            if share < worst {
                let mut v: Vec<u32> = sizes.values().copied().collect();
                v.sort_unstable_by(|a, c| c.cmp(a));
                worst_second = v.get(1).copied().unwrap_or(0);
                worst = share;
                worst_seed = b;
            }
        }
        println!(
            "  preset {preset} ({:<12}) mean {:>6.2}%  WORST {:>6.2}% (seed {worst_seed}, 2nd body {worst_second} tiles)  smallest main ocean {min_main} tiles",
            MAP_PRESETS[preset as usize].label,
            sum / n,
            worst
        );
    }
}

/// `--starts <n>`: every start point, every preset. Diff two runs of this to
/// PROVE the mainland presets did not move when the seating rule changed.
fn start_dump(count: u32) {
    for preset in 0..MAP_PRESETS.len() as u8 {
        for b in 1..=count {
            let seed = compose_seed(b, preset);
            let mut line = format!("p{preset} s{b:<6}");
            for slot in 0..8 {
                let s = start_point(seed, slot);
                line.push_str(&format!(
                    " ({:>3},{:>3})r{:<4}",
                    s.x.to_num::<i32>(),
                    s.y.to_num::<i32>(),
                    region_at(seed, s.x, s.y)
                ));
            }
            println!("{line}");
        }
    }
}

/// `--reach <n>`: THE headline number. What share of the map's resource nodes
/// can a start ever work — on foot today, and by land-or-sea with a ferry.
fn reach_audit(count: u32) {
    println!("\n== resource nodes reachable from a start ==");
    for preset in 0..MAP_PRESETS.len() as u8 {
        let (mut land_sum, mut sea_sum, mut n) = (0.0f64, 0.0f64, 0.0f64);
        let (mut land_worst, mut worst_seed) = (100.0f64, 0u32);
        let mut regions_used = 0.0f64;
        for b in 1..=count {
            let seed = compose_seed(b, preset);
            let rg = region_grid(seed);
            let wb = water_region_grid(seed);
            let nodes = scatter_nodes(seed, &node_kinds());
            let extra = fair_start_nodes(seed, &nodes, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
            let all: Vec<_> = nodes.iter().chain(extra.iter()).collect();
            // which land regions does a hull in this start's water body touch?
            let mut regs = std::collections::BTreeSet::new();
            for slot in 0..8 {
                let s = start_point(seed, slot);
                regs.insert(region_at(seed, s.x, s.y));
                let home = region_at(seed, s.x, s.y);
                let body = shore_body(seed, home, rg, wb);
                let (mut on_foot, mut by_sea) = (0u64, 0u64);
                for nd in &all {
                    let (tx, ty) = (nd.pos.x.to_num::<i32>(), nd.pos.y.to_num::<i32>());
                    let mut foot = false;
                    let mut sea = false;
                    for dy in -1..=1i32 {
                        for dx in -1..=1i32 {
                            let (nx, ny) = (tx + dx, ty + dy);
                            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                                continue;
                            }
                            let j = ny as usize * N + nx as usize;
                            foot |= rg[j] == home;
                            // a boat can work a water node in its own body, or
                            // land on any shore that body touches
                            sea |= body.is_some() && wb[j] == body.unwrap();
                            sea |= rg[j] != u16::MAX
                                && body.is_some()
                                && touches(rg[j], body.unwrap(), rg, wb);
                        }
                    }
                    if foot {
                        on_foot += 1;
                    }
                    if foot || sea {
                        by_sea += 1;
                    }
                }
                let lp = pct(on_foot, all.len() as u64);
                land_sum += lp;
                sea_sum += pct(by_sea, all.len() as u64);
                n += 1.0;
                if lp < land_worst {
                    land_worst = lp;
                    worst_seed = b;
                }
            }
            regions_used += regs.len() as f64;
        }
        println!(
            "  preset {preset} ({:<12}) on foot {:>5.1}% (worst start {:>5.1}%, seed {worst_seed})  ->  land-or-sea {:>5.1}%   distinct start regions {:>4.2}",
            MAP_PRESETS[preset as usize].label,
            land_sum / n,
            land_worst,
            sea_sum / n,
            regions_used / count as f64
        );
    }
}

/// The water body a land region's shore sits on (the biggest one it touches).
fn shore_body(_seed: u32, region: u16, rg: &[u16], wb: &[u16]) -> Option<u16> {
    let mut best: BTreeMap<u16, u32> = BTreeMap::new();
    for i in 0..N * N {
        if rg[i] != region {
            continue;
        }
        let (tx, ty) = ((i % N) as i32, (i / N) as i32);
        for (dx, dy) in D4 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                continue;
            }
            let w = wb[ny as usize * N + nx as usize];
            if w != u16::MAX {
                *best.entry(w).or_default() += 1;
            }
        }
    }
    best.into_iter().max_by_key(|e| e.1).map(|e| e.0)
}

fn touches(region: u16, body: u16, rg: &[u16], wb: &[u16]) -> bool {
    for i in 0..N * N {
        if rg[i] != region {
            continue;
        }
        let (tx, ty) = ((i % N) as i32, (i / N) as i32);
        for (dx, dy) in D4 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                continue;
            }
            if wb[ny as usize * N + nx as usize] == body {
                return true;
            }
        }
    }
    false
}

/// `--grids`: what the two new cached grids cost against a world build.
fn grid_cost(count: u32) {
    use std::time::Instant;
    let (mut build, mut land, mut sea) = (0.0f64, 0.0f64, 0.0f64);
    for b in 1..=count {
        for preset in 0..4u8 {
            let seed = compose_seed(b, preset);
            let t = Instant::now();
            let _ = worldgrid::world_grid(seed);
            build += t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let _ = passable_grid(seed);
            let _ = region_grid(seed);
            land += t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let _ = sailable_grid(seed);
            let _ = water_region_grid(seed);
            let _ = main_water_body(seed);
            sea += t.elapsed().as_secs_f64() * 1000.0;
        }
    }
    let n = (count * 4) as f64;
    println!(
        "\n== per-world build cost ==\n  worldgrid {:>7.2} ms   land grids {:>6.2} ms   SEA grids {:>6.2} ms  ({:.2}% of the build)",
        build / n,
        land / n,
        sea / n,
        sea / build * 100.0
    );
    println!("  sea memory per seed: {} KiB", (N * N * (1 + 2)) / 1024);
}

/// `--fish <n>`: where the fishery rule actually lands its nodes.
fn fish_audit(count: u32) {
    println!("\n== fishery placement ==");
    for preset in 0..MAP_PRESETS.len() as u8 {
        let (mut want, mut got, mut on_water, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut facing = [0.0f64; 4]; // lake, river, shoal, sea
        for b in 1..=count {
            let seed = compose_seed(b, preset);
            let mut rules = node_kinds();
            let keep: Vec<usize> = rules
                .iter()
                .enumerate()
                .filter(|(_, r)| r.res_type == ResourceType::Food && r.regen > 0)
                .map(|e| e.0)
                .collect();
            let mut w = 0;
            for (j, r) in rules.iter_mut().enumerate() {
                if keep.contains(&j) {
                    w += r.count;
                } else {
                    r.count = 0;
                }
            }
            let placed = scatter_nodes(seed, &rules);
            want += w as f64;
            got += placed.len() as f64;
            n += 1.0;
            for nd in &placed {
                let (tx, ty) = (nd.pos.x.to_num::<i32>(), nd.pos.y.to_num::<i32>());
                if is_sailable(seed, tx, ty) {
                    on_water += 1.0;
                }
                match water_class(sample_terrain(seed, nd.pos.x, nd.pos.y).biome) {
                    WaterClass::Fresh => facing[0] += 1.0,
                    WaterClass::Flowing => facing[1] += 1.0,
                    WaterClass::Shoal => facing[2] += 1.0,
                    WaterClass::Sea => facing[3] += 1.0,
                    WaterClass::None => {}
                }
            }
        }
        println!(
            "  preset {preset} ({:<12}) wanted {:>5.0}  placed {:>5.0} ({:>5.1}%)  on water {:>5.1}%  | lake {:>5.1} river {:>5.1} shallows {:>5.1} sea {:>5.1}",
            MAP_PRESETS[preset as usize].label,
            want / n,
            got / n,
            got / want.max(1.0) * 100.0,
            on_water / got.max(1.0) * 100.0,
            facing[0] / n,
            facing[1] / n,
            facing[2] / n,
            facing[3] / n
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(12);
    let per_seed = args.iter().any(|a| a == "--per-seed");
    let do_nodes = args.iter().any(|a| a == "--nodes");
    for (flag, f) in [
        ("--sea", sea_unity as fn(u32)),
        ("--starts", start_dump),
        ("--reach", reach_audit),
        ("--grids", grid_cost),
        ("--fish", fish_audit),
    ] {
        if args.iter().any(|a| a == flag) {
            f(count);
            return;
        }
    }
    let bases: Vec<u32> =
        (0..count).map(|s| 1000 + (s.wrapping_mul(2654435761) % 100000)).collect();
    if let Some(i) = args.iter().position(|a| a == "--rules") {
        let base: u32 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1000);
        let preset: u8 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(3);
        rule_census(base, preset);
        fish_gate(base, preset);
        return;
    }
    let presets: Vec<u8> = match args.get(1).map(|s| s.as_str()) {
        Some(p) if p.parse::<u8>().is_ok() => vec![p.parse().unwrap()],
        _ => (0..MAP_PRESETS.len() as u8).collect(),
    };

    for preset in presets {
        let mut a = Acc { dom_share_min: 1e9, ..Default::default() };
        println!(
            "\n=== preset {preset} ({}) over {} seeds, {N}x{N} ===",
            MAP_PRESETS[preset as usize].label,
            bases.len()
        );
        for &b in &bases {
            let seed = compose_seed(b, preset);
            let g = worldgrid::world_grid(seed);
            a.n += 1.0;

            let mut kinds: BTreeMap<&'static str, u64> = BTreeMap::new();
            let mut is_sea = vec![false; N * N];
            let mut is_water = vec![false; N * N];
            let mut is_shallow = vec![false; N * N];
            let mut is_river = vec![false; N * N];
            let mut is_lake = vec![false; N * N];
            let mut is_land_t = vec![false; N * N];
            for i in 0..N * N {
                let bi = g.biome[i];
                *kinds.entry(biome_def(bi).label).or_default() += 1;
                is_water[i] = biome_is_water(bi);
                is_sea[i] = matches!(bi, Biome::DeepWater | Biome::ShallowWater);
                is_shallow[i] = bi == Biome::ShallowWater;
                is_river[i] = bi == Biome::River;
                is_lake[i] = bi == Biome::Lake;
                is_land_t[i] = biome_passable(bi);
            }
            let tot = (N * N) as u64;
            let cnt = |l: &str| *kinds.get(l).unwrap_or(&0);
            a.deep += pct(cnt("Sea"), tot);
            a.shallow += pct(cnt("Shallows"), tot);
            a.river += pct(cnt("River"), tot);
            a.lake += pct(cnt("Lake"), tot);
            a.marsh += pct(cnt("Marsh"), tot);
            a.ford += pct(cnt("Ford"), tot);
            let land: u64 = is_land_t.iter().filter(|&&x| x).count() as u64;
            a.land += pct(land, tot);

            // ── land connectivity (the shipped region_grid) ────────────────
            let rg = region_grid(seed);
            let main = dominant_region(seed);
            let mut rsizes: BTreeMap<u16, u32> = BTreeMap::new();
            for &r in rg {
                if r != u16::MAX {
                    *rsizes.entry(r).or_default() += 1;
                }
            }
            let dom = *rsizes.get(&main).unwrap_or(&0) as u64;
            let dshare = pct(dom, land);
            a.land_regions += rsizes.len() as f64;
            a.land_regions_big += rsizes.values().filter(|&&v| v >= 200).count() as f64;
            a.dom_share += dshare;
            a.dom_share_min = a.dom_share_min.min(dshare);
            a.outside_dom += (land - dom) as f64;

            // ── water connectivity ─────────────────────────────────────────
            let (sea_lbl, sea_sz) = flood(&is_sea);
            let sea_main = sea_sz
                .iter()
                .enumerate()
                .max_by_key(|e| *e.1)
                .map(|(i, _)| i as i32)
                .unwrap_or(-1);
            let sea_total: u64 = sea_sz.iter().map(|&s| s as u64).sum();
            a.sea_bodies += sea_sz.iter().filter(|&&s| s >= 20).count() as f64;
            a.sea_main_share +=
                pct(sea_sz.get(sea_main.max(0) as usize).copied().unwrap_or(0) as u64, sea_total);
            let (_wl, wsz) = flood(&is_water);
            a.water_bodies += wsz.iter().filter(|&&s| s >= 20).count() as f64;

            // lakes / rivers: does the body touch the main sea?
            let touches_main_sea = |mask: &[bool]| -> (u32, u32) {
                let (lbl, sz) = flood(mask);
                let mut linked = vec![false; sz.len()];
                for i in 0..N * N {
                    if lbl[i] < 0 {
                        continue;
                    }
                    let (tx, ty) = ((i % N) as i32, (i / N) as i32);
                    for (dx, dy) in D4 {
                        let (nx, ny) = (tx + dx, ty + dy);
                        if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                            continue;
                        }
                        let j = ny as usize * N + nx as usize;
                        if sea_lbl[j] == sea_main {
                            linked[lbl[i] as usize] = true;
                        }
                    }
                }
                (
                    sz.iter().filter(|&&s| s >= 8).count() as u32,
                    sz.iter()
                        .enumerate()
                        .filter(|e| *e.1 >= 8 && linked[e.0])
                        .count() as u32,
                )
            };
            let (lk_tot, lk_link) = touches_main_sea(&is_lake);
            a.lakes_total += lk_tot as f64;
            a.lakes_sea_linked += lk_link as f64;
            let (rv_tot, rv_link) = touches_main_sea(&is_river);
            a.river_bodies += rv_tot as f64;
            a.rivers_sea_linked += rv_link as f64;

            // ── shallows: a navigation band, or a paint stripe? ────────────
            let land_src: Vec<bool> = is_land_t.clone();
            let allow_sh: Vec<bool> = (0..N * N).map(|i| is_shallow[i] || is_land_t[i]).collect();
            let d = bfs_dist(&land_src, &allow_sh);
            let mut widths: Vec<f64> = Vec::new();
            let (mut s1, mut s2, mut s3) = (0u64, 0u64, 0u64);
            for i in 0..N * N {
                if !is_shallow[i] {
                    continue;
                }
                let v = if d[i] == i32::MAX { 99 } else { d[i] };
                widths.push(v as f64);
                match v {
                    1 => s1 += 1,
                    2 => s2 += 1,
                    _ => s3 += 1,
                }
            }
            let shn = widths.len().max(1) as u64;
            a.shallow_d1 += pct(s1, shn);
            a.shallow_d2 += pct(s2, shn);
            a.shallow_d3plus += pct(s3, shn);
            a.shallow_max += widths.iter().cloned().fold(0.0, f64::max);
            a.shallow_p50 += quant(&mut widths, 0.5);
            a.shallow_p90 += quant(&mut widths, 0.9);
            let (_shl, shsz) = flood(&is_shallow);
            a.shallow_bodies += shsz.iter().filter(|&&s| s >= 20).count() as f64;
            a.shallow_orphan +=
                pct((0..N * N).filter(|&i| is_shallow[i] && d[i] == i32::MAX).count() as u64, shn);

            // ── the shelf: how the sea is banded by distance from shore ────
            let allow_sea: Vec<bool> = (0..N * N).map(|i| is_sea[i] || is_land_t[i]).collect();
            let dsea = bfs_dist(&land_src, &allow_sea);
            let (mut b1, mut b3, mut b6, mut b12, mut bfar) = (0u64, 0u64, 0u64, 0u64, 0u64);
            let (mut sh1, mut al1, mut sh6, mut al6, mut sh12, mut al12) =
                (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
            for i in 0..N * N {
                if !is_sea[i] {
                    continue;
                }
                let v = dsea[i];
                if v == i32::MAX {
                    bfar += 1;
                    continue;
                }
                match v {
                    1 => b1 += 1,
                    2..=3 => b3 += 1,
                    4..=6 => b6 += 1,
                    7..=12 => b12 += 1,
                    _ => bfar += 1,
                }
                if v <= 1 {
                    al1 += 1;
                    if is_shallow[i] {
                        sh1 += 1;
                    }
                }
                if (4..=6).contains(&v) {
                    al6 += 1;
                    if is_shallow[i] {
                        sh6 += 1;
                    }
                }
                if (7..=12).contains(&v) {
                    al12 += 1;
                    if is_shallow[i] {
                        sh12 += 1;
                    }
                }
            }
            let sean = sea_total.max(1);
            a.sea_d1 += pct(b1, sean);
            a.sea_d3 += pct(b3, sean);
            a.sea_d6 += pct(b6, sean);
            a.sea_d12 += pct(b12, sean);
            a.sea_far += pct(bfar, sean);
            a.shallow_at_d1 += pct(sh1, al1.max(1));
            a.shallow_at_d6 += pct(sh6, al6.max(1));
            a.shallow_at_d12 += pct(sh12, al12.max(1));
            let mut ssz: Vec<u32> = sea_sz.clone();
            ssz.sort_unstable_by(|x, y| y.cmp(x));
            a.sea_body2 += ssz.get(1).copied().unwrap_or(0) as f64;

            // ── shoreline + dock siting ────────────────────────────────────
            let mut coast = 0u64;
            let mut docks = 0u64;
            let mut docks_dom = 0u64;
            let mut deep_only = 0u64;
            let half = fx!("0.5");
            for ty in 0..N as i32 {
                for tx in 0..N as i32 {
                    let i = ty as usize * N + tx as usize;
                    if !is_land_t[i] {
                        continue;
                    }
                    let (mut wet, mut shal) = (false, false);
                    for (dx, dy) in D4 {
                        let (nx, ny) = (tx + dx, ty + dy);
                        if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                            continue;
                        }
                        let j = ny as usize * N + nx as usize;
                        wet |= is_sea[j];
                        shal |= is_shallow[j];
                    }
                    if !wet {
                        continue;
                    }
                    coast += 1;
                    if !shal {
                        deep_only += 1;
                    }
                    // what check_place would allow for a 1x1 requires_water hut
                    if is_buildable_tile(seed, tx, ty)
                        && slope_at(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half)
                            <= BUILD_SLOPE_MAX
                    {
                        docks += 1;
                        if rg[i] == main {
                            docks_dom += 1;
                        }
                    }
                }
            }
            a.coast_land += coast as f64;
            a.dock_sites += docks as f64;
            a.dock_sites_dom += docks_dom as f64;
            a.coast_deep_only += pct(deep_only, coast.max(1));

            // ── what a sea domain unlocks ──────────────────────────────────
            // A land region is reachable by boat if any of its tiles borders
            // the MAIN sea body (the one continuous ocean a hull can sail).
            let mut region_seatouch: BTreeMap<u16, bool> = BTreeMap::new();
            for i in 0..N * N {
                let r = rg[i];
                if r == u16::MAX {
                    continue;
                }
                let (tx, ty) = ((i % N) as i32, (i / N) as i32);
                let mut t = false;
                for (dx, dy) in D4 {
                    let (nx, ny) = (tx + dx, ty + dy);
                    if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                        continue;
                    }
                    t |= sea_lbl[ny as usize * N + nx as usize] == sea_main;
                }
                let e = region_seatouch.entry(r).or_insert(false);
                *e |= t;
            }
            let naval_land: u64 = rsizes
                .iter()
                .filter(|e| *region_seatouch.get(e.0).unwrap_or(&false))
                .map(|e| *e.1 as u64)
                .sum();
            a.naval_land_share += pct(naval_land.max(dom), land);
            a.islands_sea_touch += rsizes
                .iter()
                .filter(|e| *e.0 != main && *e.1 >= 50 && *region_seatouch.get(e.0).unwrap_or(&false))
                .count() as f64;
            a.islands_landable += rsizes
                .iter()
                .filter(|e| *e.0 != main && *e.1 >= 600 && *region_seatouch.get(e.0).unwrap_or(&false))
                .count() as f64;

            // ── straits: how far a hull must sail to reach the other islands ─
            // BFS over sea from the mainland shore; the distance at which each
            // other landmass is first touched is the crossing that boat has to
            // make.
            {
                let mut src = vec![false; N * N];
                for i in 0..N * N {
                    if rg[i] == main {
                        src[i] = true;
                    }
                }
                let allow: Vec<bool> = (0..N * N).map(|i| is_sea[i] || src[i]).collect();
                let dm = bfs_dist(&src, &allow);
                // nearest sea-distance from the mainland to every other region
                let mut best: BTreeMap<u16, i32> = BTreeMap::new();
                for i in 0..N * N {
                    let r = rg[i];
                    if r == u16::MAX || r == main {
                        continue;
                    }
                    let (tx, ty) = ((i % N) as i32, (i / N) as i32);
                    for (dx, dy) in D4 {
                        let (nx, ny) = (tx + dx, ty + dy);
                        if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                            continue;
                        }
                        let j = ny as usize * N + nx as usize;
                        if is_sea[j] && dm[j] != i32::MAX {
                            let e = best.entry(r).or_insert(i32::MAX);
                            *e = (*e).min(dm[j]);
                        }
                    }
                }
                let mut xs: Vec<f64> = best
                    .iter()
                    .filter(|e| rsizes.get(e.0).copied().unwrap_or(0) >= 600 && *e.1 != i32::MAX)
                    .map(|e| *e.1 as f64)
                    .collect();
                if !xs.is_empty() {
                    a.strait_n += xs.len() as f64;
                    a.strait_min += xs.iter().cloned().fold(f64::MAX, f64::min);
                    a.strait_max += xs.iter().cloned().fold(0.0, f64::max);
                    a.strait_p50 += quant(&mut xs, 0.5);
                }
            }

            // ── starts ─────────────────────────────────────────────────────
            let starts: Vec<V2> = (0..8).map(|s| start_point(seed, s)).collect();
            let regs: std::collections::BTreeSet<u16> =
                starts.iter().map(|s| region_at(seed, s.x, s.y)).collect();
            a.starts_regions += regs.len() as f64;
            let mut mingap = f64::MAX;
            for i in 0..starts.len() {
                for j in i + 1..starts.len() {
                    let dx = (starts[i].x - starts[j].x).to_num::<f64>();
                    let dy = (starts[i].y - starts[j].y).to_num::<f64>();
                    mingap = mingap.min((dx * dx + dy * dy).sqrt());
                }
            }
            a.start_min_gap += mingap;
            {
                // how far the dominant-region snap DRAGS each spawn anchor, and
                // how tightly the 8 starts end up packed
                let mut drag: Vec<f64> = Vec::new();
                for slot in 0..8 {
                    let c = spawn_corner(slot);
                    let dx = (starts[slot].x - c.x).to_num::<f64>();
                    let dy = (starts[slot].y - c.y).to_num::<f64>();
                    drag.push((dx * dx + dy * dy).sqrt());
                }
                let mut gaps: Vec<f64> = Vec::new();
                for i in 0..8 {
                    for j in i + 1..8 {
                        let dx = (starts[i].x - starts[j].x).to_num::<f64>();
                        let dy = (starts[i].y - starts[j].y).to_num::<f64>();
                        gaps.push((dx * dx + dy * dy).sqrt());
                    }
                }
                a.start_drag_mean += drag.iter().sum::<f64>() / 8.0;
                a.start_drag_max += drag.iter().cloned().fold(0.0, f64::max);
                a.start_gap_mean += gaps.iter().sum::<f64>() / gaps.len() as f64;
                a.start_gap_p25 += quant(&mut gaps, 0.25);
                let mut sizes: Vec<u32> = rsizes.values().copied().collect();
                sizes.sort_unstable_by(|x, y| y.cmp(x));
                for k in 0..5 {
                    a.top_regions[k] += sizes.get(k).copied().unwrap_or(0) as f64;
                }
                // an island big enough to hold a start's FAIR_RADIUS disc
                a.islands_start_sized += sizes.iter().filter(|&&v| v >= 1300).count() as f64;
            }

            // ── can a start even REACH the sea? (TOWN_RADIUS chain) ────────
            {
                let sea_src: Vec<bool> = is_sea.clone();
                let all: Vec<bool> = vec![true; N * N];
                let dshore = bfs_dist(&sea_src, &all);
                let mut ds: Vec<f64> = Vec::new();
                for slot in 0..8 {
                    let k = find_keep_site(seed, slot, 3);
                    let i = worldgrid::tile_index(k.x, k.y);
                    ds.push(dshore[i] as f64);
                }
                a.keep_to_sea_min += ds.iter().cloned().fold(f64::MAX, f64::min);
                a.keep_to_sea_max += ds.iter().cloned().fold(0.0, f64::max);
                a.keep_to_sea_mean += ds.iter().sum::<f64>() / 8.0;
                a.keeps_coastal +=
                    ds.iter().filter(|&&v| v <= TOWN_RADIUS.to_num::<f64>()).count() as f64;
                // the AI only looks in a 14-tile CHEBYSHEV box around its keep
                for slot in 0..8 {
                    let k = find_keep_site(seed, slot, 3);
                    let (kx, ky) = (k.x.to_num::<i32>(), k.y.to_num::<i32>());
                    let mut seen = false;
                    'box_: for dy in -14..=14i32 {
                        for dx in -14..=14i32 {
                            let (nx, ny) = (kx + dx, ky + dy);
                            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                                continue;
                            }
                            if is_sea[ny as usize * N + nx as usize] {
                                seen = true;
                                break 'box_;
                            }
                        }
                    }
                    if seen {
                        a.keeps_ai_shore += 1.0;
                    }
                }
            }

            // ── nodes: what is playable today vs what boats would open ─────
            let (mut nds, mut nreach, mut nnaval, mut nfood_un) = (0u64, 0u64, 0u64, 0u64);
            if do_nodes {
                let scattered = scatter_nodes(seed, &node_kinds());
                let extra =
                    fair_start_nodes(seed, &scattered, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
                for nd in scattered.iter().chain(extra.iter()) {
                    nds += 1;
                    // node_reachable semantics: the node tile or any 8-neighbour
                    // shares the walker's region
                    let tx = nd.pos.x.to_num::<i32>().clamp(0, N as i32 - 1);
                    let ty = nd.pos.y.to_num::<i32>().clamp(0, N as i32 - 1);
                    let mut regs_here: Vec<u16> = Vec::new();
                    for dy in -1..=1i32 {
                        for dx in -1..=1i32 {
                            let (nx, ny) = (tx + dx, ty + dy);
                            if nx < 0 || ny < 0 || nx >= N as i32 || ny >= N as i32 {
                                continue;
                            }
                            let r = rg[ny as usize * N + nx as usize];
                            if r != u16::MAX {
                                regs_here.push(r);
                            }
                        }
                    }
                    let on_main = regs_here.iter().any(|&r| r == main);
                    let on_naval =
                        regs_here.iter().any(|&r| *region_seatouch.get(&r).unwrap_or(&false));
                    if on_main {
                        nreach += 1;
                    }
                    if on_main || on_naval {
                        nnaval += 1;
                    }
                    if !on_main && nd.res_type == ResourceType::Food {
                        nfood_un += 1;
                    }
                    // a "fishery" is a food node the scatter accepted for its
                    // waterside: classify by the water it actually faces
                    if nd.res_type == ResourceType::Food && is_coastal(seed, nd.pos.x, nd.pos.y) {
                        a.fish_nodes += 1.0;
                        if !on_main {
                            a.fish_stranded += 1.0;
                        }
                        match node_site(seed, nd.pos.x, nd.pos.y).adjacent_water {
                            Some(Biome::Lake) => a.fish_lake += 1.0,
                            Some(Biome::River) => a.fish_river += 1.0,
                            Some(Biome::ShallowWater) => a.fish_shallow += 1.0,
                            Some(Biome::DeepWater) => a.fish_deep += 1.0,
                            _ => {}
                        }
                    }
                }
                a.water_per_fish += (tot - land) as f64;
                a.nodes += nds as f64;
                a.nodes_reach += nreach as f64;
                a.nodes_naval += nnaval as f64;
                a.node_food_unreach += nfood_un as f64;
            }

            if per_seed {
                println!(
                    "  seed {b:>6}: land {:>5.1}%  water {:>5.1}% (deep {:>5.1} sh {:>4.1} riv {:>4.1} lake {:>4.1})  \
regions {:>3} dom {:>5.1}%  sea bodies {:>2}  naval-land {:>5.1}%  docks {docks:>5}  nodes {nreach}/{nds} -> {nnaval}",
                    pct(land, tot),
                    100.0 - pct(land, tot),
                    pct(cnt("Sea"), tot),
                    pct(cnt("Shallows"), tot),
                    pct(cnt("River"), tot),
                    pct(cnt("Lake"), tot),
                    rsizes.len(),
                    dshare,
                    sea_sz.iter().filter(|&&s| s >= 20).count(),
                    pct(naval_land.max(dom), land),
                );
            }
        }

        println!("  WATER BUDGET  deep {:>5.2}%  shallows {:>5.2}%  river {:>4.2}%  lake {:>4.2}%  marsh {:>4.2}%  ford {:>4.2}%  | LAND {:>5.2}%",
            avg!(a, deep), avg!(a, shallow), avg!(a, river), avg!(a, lake), avg!(a, marsh), avg!(a, ford), avg!(a, land));
        println!("  LAND GRAPH    regions {:>5.1} (>=200 tiles: {:>4.1})  dominant {:>5.1}% of land (worst seed {:>5.1}%)  stranded {:>7.0} tiles/map",
            avg!(a, land_regions), avg!(a, land_regions_big), avg!(a, dom_share), a.dom_share_min, avg!(a, outside_dom));
        println!("  SEA GRAPH     sea bodies>=20 {:>4.1}  main body holds {:>5.1}% of all sea  | all-water bodies>=20 {:>5.1}",
            avg!(a, sea_bodies), avg!(a, sea_main_share), avg!(a, water_bodies));
        println!("  INLAND WATER  lakes>=8 {:>4.1} of which sea-linked {:>4.1}   river bodies>=8 {:>5.1} of which sea-linked {:>5.1}",
            avg!(a, lakes_total), avg!(a, lakes_sea_linked), avg!(a, river_bodies), avg!(a, rivers_sea_linked));
        println!("  SHALLOWS      band width from shore: p50 {:>4.1}  p90 {:>4.1}  max {:>5.1} tiles | 1 tile {:>4.1}%  2 {:>4.1}%  3+ {:>4.1}%  | strips>=20: {:>4.1}",
            avg!(a, shallow_p50), avg!(a, shallow_p90), avg!(a, shallow_max), avg!(a, shallow_d1), avg!(a, shallow_d2), avg!(a, shallow_d3plus), avg!(a, shallow_bodies));
        println!("  SHORELINE     coast land tiles {:>7.0}  dock-legal {:>7.0} ({:>4.1}% of coast)  on the mainland {:>7.0}",
            avg!(a, coast_land), avg!(a, dock_sites), avg!(a, dock_sites) * 100.0 / avg!(a, coast_land).max(1.0), avg!(a, dock_sites_dom));
        println!("  NAVAL UNLOCK  land reachable today {:>5.1}%  ->  by sea {:>5.1}%   islands>=50 {:>4.1}  base-sized islands>=600 {:>4.1}",
            avg!(a, dom_share), avg!(a, naval_land_share), avg!(a, islands_sea_touch), avg!(a, islands_landable));
        println!("  SHELF         sea by distance from shore: 1 {:>4.1}%  2-3 {:>4.1}%  4-6 {:>4.1}%  7-12 {:>4.1}%  13+ {:>4.1}%  | shallow share at d1 {:>4.1}%  d4-6 {:>4.1}%  d7-12 {:>4.1}%",
            avg!(a, sea_d1), avg!(a, sea_d3), avg!(a, sea_d6), avg!(a, sea_d12), avg!(a, sea_far),
            avg!(a, shallow_at_d1), avg!(a, shallow_at_d6), avg!(a, shallow_at_d12));
        println!("  COAST TYPE    coast with NO shallows (deep right at the beach) {:>5.1}%  | shallow banks orphaned from any shore {:>4.1}%  | 2nd sea body {:>7.0} tiles",
            avg!(a, coast_deep_only), avg!(a, shallow_orphan), avg!(a, sea_body2));
        println!("  STRAITS       crossings to base-sized islands: n {:>4.1}  nearest {:>5.1}  median {:>5.1}  farthest {:>5.1} tiles of open water",
            avg!(a, strait_n), avg!(a, strait_min), avg!(a, strait_p50), avg!(a, strait_max));
        println!("  STARTS        distinct start regions {:>4.2}  closest pair {:>5.1} tiles apart  | keep-to-sea min {:>5.1} mean {:>5.1} max {:>5.1}  keeps within TOWN_RADIUS of water {:>4.2}/8",
            avg!(a, starts_regions), avg!(a, start_min_gap),
            avg!(a, keep_to_sea_min), avg!(a, keep_to_sea_mean), avg!(a, keep_to_sea_max), avg!(a, keeps_coastal));
        println!("  AI SHORE SCAN keeps with sea inside the bot's 14-tile box {:>4.2}/8", avg!(a, keeps_ai_shore));
        println!("  SNAP COST     dominant-region drag on the spawn anchor: mean {:>6.1} max {:>6.1} tiles | start spacing mean {:>5.1} p25 {:>5.1}",
            avg!(a, start_drag_mean), avg!(a, start_drag_max), avg!(a, start_gap_mean), avg!(a, start_gap_p25));
        println!("  START ISLANDS land regions >=1300 tiles (one FAIR_RADIUS disc) {:>5.2} per map", avg!(a, islands_start_sized));
        println!("  LANDMASSES    5 biggest land regions (tiles): {:>6.0} {:>6.0} {:>6.0} {:>6.0} {:>6.0}",
            a.top_regions[0] / a.n, a.top_regions[1] / a.n, a.top_regions[2] / a.n, a.top_regions[3] / a.n, a.top_regions[4] / a.n);
        if do_nodes {
            println!("  NODES         placed {:>7.0}  reachable today {:>7.0} ({:>4.1}%)  with boats {:>7.0} ({:>4.1}%)  stranded FOOD nodes {:>6.0}",
                avg!(a, nodes), avg!(a, nodes_reach), avg!(a, nodes_reach) * 100.0 / avg!(a, nodes).max(1.0),
                avg!(a, nodes_naval), avg!(a, nodes_naval) * 100.0 / avg!(a, nodes).max(1.0), avg!(a, node_food_unreach));
            println!("  FISHERIES     coastal food nodes {:>6.1}  facing: lake {:>5.1}  river {:>5.1}  shallows {:>6.1}  open sea {:>6.1}  | stranded {:>5.1}  | water tiles per fishery {:>7.0}",
                avg!(a, fish_nodes), avg!(a, fish_lake), avg!(a, fish_river), avg!(a, fish_shallow),
                avg!(a, fish_deep), avg!(a, fish_stranded), avg!(a, water_per_fish) / avg!(a, fish_nodes).max(1.0));
        }
    }
}

/// `--rules <base> <preset>`: how many of each scatter rule's nodes the map can
/// actually place. A water-heavy seed starves the attempt budget, so the map
/// loses resources before a single one of them is stranded.
#[allow(dead_code)]
fn rule_census(base: u32, preset: u8) {
    let seed = compose_seed(base, preset);
    let names = ["timber", "quarry", "herds", "FISHERY", "vein", "placer", "ml-gold", "ml-stone"];
    println!("\n-- scatter budget, base {base} preset {preset} --");
    for (i, name) in names.iter().enumerate() {
        let mut rules = node_kinds();
        let want = rules[i].count;
        for (j, r) in rules.iter_mut().enumerate() {
            if j != i {
                r.count = 0;
            }
        }
        let got = scatter_nodes(seed, &rules).len();
        println!("  {name:<9} wanted {want:>5}  placed {got:>5}  ({:>5.1}%)", got as f64 * 100.0 / want as f64);
    }
}

/// The `coastal_only` gate vs the `fishery()` density it feeds. A lakeshore or
/// riverbank tile can never reach the branch written for it: `is_coastal` only
/// answers yes for a SEA neighbour.
#[allow(dead_code)]
fn fish_gate(base: u32, preset: u8) {
    let seed = compose_seed(base, preset);
    let half = fx!("0.5");
    let (mut lake_sh, mut river_sh, mut sea_sh, mut lake_and_sea, mut river_and_sea) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            let (x, y) = (Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            if !is_land(seed, x, y) {
                continue;
            }
            let mut nb = [false; 4]; // lake, river, shallow, deep
            for (dx, dy) in D4 {
                let b = sample_terrain(seed, x + Fx::from_num(dx), y + Fx::from_num(dy)).biome;
                match b {
                    Biome::Lake => nb[0] = true,
                    Biome::River => nb[1] = true,
                    Biome::ShallowWater => nb[2] = true,
                    Biome::DeepWater => nb[3] = true,
                    _ => {}
                }
            }
            let sea = nb[2] || nb[3];
            if nb[0] {
                lake_sh += 1;
                if sea {
                    lake_and_sea += 1;
                }
            }
            if nb[1] {
                river_sh += 1;
                if sea {
                    river_and_sea += 1;
                }
            }
            if sea {
                sea_sh += 1;
            }
        }
    }
    println!("  waterside land tiles: lakeshore {lake_sh}  riverbank {river_sh}  seashore {sea_sh}");
    println!("  reachable by the coastal_only gate: lakeshore {lake_and_sea}  riverbank {river_and_sea}  (everything else is filtered out before fishery() runs)");
}
