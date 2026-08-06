//! Dev tool: what the FERTILITY field actually looks like on ground a farm can
//! be sown on, how far that ground is from fresh water, and how much of it a
//! player can reach from their start. Usage:
//! `cargo run --release -p saladin-sim --example soilprobe -- [seeds] [preset|all] [--climates] [--spatial]`

use saladin_sim::*;
use std::collections::BTreeMap;

const N: i32 = WORLD_SIZE;

fn q(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

fn half() -> Fx {
    fx!("0.5")
}

/// Tiles a farm could actually be sown on ignoring soil: buildable biome +
/// under the foundation slope cap over the whole 2x2 footprint.
fn farmable_mask(seed: u32) -> Vec<bool> {
    let mut m = vec![false; (N * N) as usize];
    for ty in 1..N - 1 {
        for tx in 1..N - 1 {
            let mut ok = true;
            for (dx, dy) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
                let (ax, ay) = (tx + dx, ty + dy);
                if !is_buildable_tile(seed, ax, ay)
                    || slope_at(seed, Fx::from_num(ax) + half(), Fx::from_num(ay) + half())
                        > BUILD_SLOPE_MAX
                {
                    ok = false;
                    break;
                }
            }
            m[(ty * N + tx) as usize] = ok;
        }
    }
    m
}

/// Chebyshev BFS distance in tiles from every tile to the nearest fresh water
/// (river/lake), and separately to any water at all.
fn dist_to(seed: u32, pred: impl Fn(Biome) -> bool) -> Vec<i32> {
    let g = saladin_sim::worldgrid::world_grid(seed);
    let mut d = vec![i32::MAX; (N * N) as usize];
    let mut q: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    for i in 0..(N * N) as usize {
        if pred(g.biome[i]) {
            d[i] = 0;
            q.push_back(i as u32);
        }
    }
    while let Some(i) = q.pop_front() {
        let i = i as usize;
        let (tx, ty) = ((i as i32) % N, (i as i32) / N);
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let (nx, ny) = (tx + dx, ty + dy);
                if nx < 0 || ny < 0 || nx >= N || ny >= N {
                    continue;
                }
                let j = (ny * N + nx) as usize;
                if d[j] > d[i] + 1 {
                    d[j] = d[i] + 1;
                    q.push_back(j as u32);
                }
            }
        }
    }
    d
}

#[derive(Default)]
struct Acc {
    n: usize,
    farmable: usize,
    pass_gate: usize,
    rich50: usize,
    rich65: usize,
    fert: Vec<f64>,
    hist: [usize; 20],
    // spatial: |f(x) - f(x+1)| over farmable pairs, vs the field's own spread
    neigh_diff: Vec<f64>,
    // distance from a farmable tile to fresh water
    fresh_d: Vec<f64>,
    // best soil within r tiles of the start, and how far you must walk for 0.5
    start_best: Vec<f64>,
    start_walk: Vec<f64>,
    start_gate_share: Vec<f64>,
    /// What `regen = (1 + soil*7) as i32` actually comes out as, per field.
    regen_hist: [usize; 9],
    /// (fertility, freshwater distance, precip, temp, slope) for correlation.
    corr: Vec<(f64, f64, f64, f64, f64)>,
    /// Connected patches of soil >= BELT_T among farmable ground.
    belts: Vec<f64>,
    belt_walk: Vec<f64>,
    /// Same as fresh_d but for the WIDER predicate (river/lake/marsh/oasis) and
    /// for any water at all (sea included) — which predicate irrigation uses
    /// decides whether the mechanic is alive on Archipelago.
    wide_d: Vec<f64>,
    any_d: Vec<f64>,
    /// The AI's own window: SHORE_SCAN = 14 tiles around the keep.
    start_any14: Vec<f64>,
    start_gate14: Vec<f64>,
}

/// Roughly "soil good enough for regen 3+" — 2/7.
const BELT_T: f64 = 0.2858;

fn probe(seed: u32, acc: &mut Acc, spatial: bool) {
    let g = saladin_sim::worldgrid::world_grid(seed);
    let farm = farmable_mask(seed);
    let fresh = dist_to(seed, biome_is_fresh_water);
    let wide = dist_to(seed, |b| {
        biome_is_fresh_water(b) || matches!(b, Biome::Marsh | Biome::Oasis)
    });
    let anyw = dist_to(seed, biome_is_water);
    for ty in 0..N {
        for tx in 0..N {
            let i = (ty * N + tx) as usize;
            acc.n += 1;
            if !farm[i] {
                continue;
            }
            // what the farm command actually reads: the 2x2 footprint mean
            let f = soil_quality(seed, 2, Fx::from_num(tx) + half(), Fx::from_num(ty) + half())
                .to_num::<f64>();
            acc.farmable += 1;
            acc.fert.push(f);
            acc.hist[((f * 20.0) as usize).min(19)] += 1;
            if f >= FARM_MIN_FERTILITY.to_num::<f64>() {
                acc.pass_gate += 1;
            }
            if f >= 0.5 {
                acc.rich50 += 1;
            }
            if f >= 0.65 {
                acc.rich65 += 1;
            }
            acc.fresh_d.push(fresh[i].min(64) as f64);
            acc.wide_d.push(wide[i].min(64) as f64);
            acc.any_d.push(anyw[i].min(64) as f64);
            // the integer the farm command actually stores
            let regen = ((1.0 + f * FARM_REGEN_MAX as f64) as i32).max(1).min(8) as usize;
            if f >= FARM_MIN_FERTILITY.to_num::<f64>() {
                acc.regen_hist[regen] += 1;
            }
            if acc.corr.len() < 400_000 {
                acc.corr.push((
                    f,
                    fresh[i].min(64) as f64,
                    g.moisture[i].to_num::<f64>(),
                    g.temp[i].to_num::<f64>(),
                    g.slope[i].to_num::<f64>(),
                ));
            }
            if spatial && tx + 1 < N && farm[i + 1] {
                let f2 = g.fertility[i + 1].to_num::<f64>();
                acc.neigh_diff.push((g.fertility[i].to_num::<f64>() - f2).abs());
            }
        }
    }

    // ── belts: is the good soil a place you go TO, or scattered pixels? ──────
    let mut comp = vec![u32::MAX; (N * N) as usize];
    let mut sizes: Vec<usize> = Vec::new();
    let mut centers: Vec<(f64, f64)> = Vec::new();
    for start in 0..(N * N) as usize {
        if comp[start] != u32::MAX
            || !farm[start]
            || g.fertility[start].to_num::<f64>() < BELT_T
        {
            continue;
        }
        let id = sizes.len() as u32;
        let mut stack = vec![start as u32];
        comp[start] = id;
        let mut n = 0usize;
        let (mut sx, mut sy) = (0f64, 0f64);
        while let Some(i) = stack.pop() {
            let i = i as usize;
            n += 1;
            sx += ((i as i32) % N) as f64;
            sy += ((i as i32) / N) as f64;
            let (tx, ty) = ((i as i32) % N, (i as i32) / N);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (tx + dx, ty + dy);
                if nx < 0 || ny < 0 || nx >= N || ny >= N {
                    continue;
                }
                let j = (ny * N + nx) as usize;
                if comp[j] == u32::MAX && farm[j] && g.fertility[j].to_num::<f64>() >= BELT_T {
                    comp[j] = id;
                    stack.push(j as u32);
                }
            }
        }
        sizes.push(n);
        centers.push((sx / n as f64, sy / n as f64));
    }
    let big: Vec<usize> = (0..sizes.len()).filter(|&i| sizes[i] >= 16).collect();
    acc.belts.push(big.len() as f64);

    // from each start slot: how good is the soil you are handed, and how far
    // is the nearest genuinely rich ground?
    for slot in 0..4 {
        let s = start_point(seed, slot);
        let (sx, sy) = (s.x.to_num::<i32>(), s.y.to_num::<i32>());
        let mut best = 0.0f64;
        let mut walk = -1.0f64;
        let (mut inr, mut gate) = (0usize, 0usize);
        let (mut in14, mut gate14) = (0usize, 0usize);
        for r in 0..80i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let (tx, ty) = (sx + dx, sy + dy);
                    if tx < 1 || ty < 1 || tx >= N - 1 || ty >= N - 1 {
                        continue;
                    }
                    let i = (ty * N + tx) as usize;
                    if !farm[i] {
                        continue;
                    }
                    let f = g.fertility[i].to_num::<f64>();
                    best = best.max(f);
                    if r <= 14 {
                        in14 += 1;
                        if f >= FARM_MIN_FERTILITY.to_num::<f64>() {
                            gate14 += 1;
                        }
                    }
                    if r <= 24 {
                        inr += 1;
                        if f >= FARM_MIN_FERTILITY.to_num::<f64>() {
                            gate += 1;
                        }
                    }
                    if walk < 0.0 && f >= 0.5 {
                        walk = r as f64;
                    }
                }
            }
        }
        // how far to a belt big enough to hold four fields
        let mut bw = f64::MAX;
        for &bi in &big {
            let (cx, cy) = centers[bi];
            let d = ((cx - sx as f64).abs()).max((cy - sy as f64).abs());
            bw = bw.min(d);
        }
        acc.belt_walk.push(if bw == f64::MAX { 999.0 } else { bw });
        acc.start_any14.push(if gate14 > 0 { 1.0 } else { 0.0 });
        acc.start_gate14.push(if in14 == 0 { 0.0 } else { gate14 as f64 / in14 as f64 });
        acc.start_best.push(best);
        acc.start_walk.push(if walk < 0.0 { 80.0 } else { walk });
        acc.start_gate_share.push(if inr == 0 { 0.0 } else { gate as f64 / inr as f64 });
    }
}

fn report(name: &str, acc: &mut Acc) {
    acc.fert.sort_by(|a, b| a.partial_cmp(b).unwrap());
    acc.fresh_d.sort_by(|a, b| a.partial_cmp(b).unwrap());
    acc.start_walk.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let fa = acc.farmable.max(1) as f64;
    println!("\n=== {name} ===");
    println!(
        "tiles {}  farmable(biome+slope) {} = {:.1}% of map",
        acc.n,
        acc.farmable,
        100.0 * acc.farmable as f64 / acc.n.max(1) as f64
    );
    println!(
        "fertility on farmable: mean {:.3}  p10 {:.3}  p50 {:.3}  p90 {:.3}  p99 {:.3}  max {:.3}",
        mean(&acc.fert),
        q(&acc.fert, 0.10),
        q(&acc.fert, 0.50),
        q(&acc.fert, 0.90),
        q(&acc.fert, 0.99),
        acc.fert.last().copied().unwrap_or(0.0)
    );
    println!(
        "clears gate 0.22: {:.1}%   >=0.50: {:.1}%   >=0.65: {:.1}%",
        100.0 * acc.pass_gate as f64 / fa,
        100.0 * acc.rich50 as f64 / fa,
        100.0 * acc.rich65 as f64 / fa
    );
    print!("hist(0.05 bins): ");
    for (i, c) in acc.hist.iter().enumerate() {
        if i % 4 == 0 {
            print!("| ");
        }
        print!("{:.1} ", 100.0 * *c as f64 / fa);
    }
    println!();
    println!(
        "dist to fresh water from farmable: p10 {:.0}  p50 {:.0}  p90 {:.0}  within 4 tiles {:.1}%  within 8 {:.1}%",
        q(&acc.fresh_d, 0.10),
        q(&acc.fresh_d, 0.50),
        q(&acc.fresh_d, 0.90),
        100.0 * acc.fresh_d.iter().filter(|&&d| d <= 4.0).count() as f64 / acc.fresh_d.len().max(1) as f64,
        100.0 * acc.fresh_d.iter().filter(|&&d| d <= 8.0).count() as f64 / acc.fresh_d.len().max(1) as f64,
    );
    let share = |v: &[f64], t: f64| 100.0 * v.iter().filter(|&&d| d <= t).count() as f64 / v.len().max(1) as f64;
    println!(
        "within 4 tiles of: fresh(river/lake) {:.1}%   +marsh/oasis {:.1}%   any water incl. sea {:.1}%",
        share(&acc.fresh_d, 4.0),
        share(&acc.wide_d, 4.0),
        share(&acc.any_d, 4.0)
    );
    if !acc.neigh_diff.is_empty() {
        let sd = {
            let m = mean(&acc.fert);
            (acc.fert.iter().map(|f| (f - m) * (f - m)).sum::<f64>() / acc.fert.len() as f64).sqrt()
        };
        println!(
            "spatial: sd {:.3}  mean |f(x)-f(x+1)| {:.4}  ratio {:.2} (small = smooth belts, ~1.4 = white noise)",
            sd,
            mean(&acc.neigh_diff),
            mean(&acc.neigh_diff) / sd.max(1e-9)
        );
    }
    let rg: usize = acc.regen_hist.iter().sum::<usize>().max(1);
    print!("regen the command STORES on gate-clearing land: ");
    for (r, c) in acc.regen_hist.iter().enumerate() {
        if *c > 0 {
            print!("{r}->{:.1}%  ", 100.0 * *c as f64 / rg as f64);
        }
    }
    println!("(FARM_REGEN_MAX is {FARM_REGEN_MAX})");
    if !acc.corr.is_empty() {
        let n = acc.corr.len() as f64;
        let mf = acc.corr.iter().map(|c| c.0).sum::<f64>() / n;
        let r = |sel: fn(&(f64, f64, f64, f64, f64)) -> f64| {
            let m2 = acc.corr.iter().map(sel).sum::<f64>() / n;
            let (mut num, mut d1, mut d2) = (0.0, 0.0, 0.0);
            for c in &acc.corr {
                let (a, b) = (c.0 - mf, sel(c) - m2);
                num += a * b;
                d1 += a * a;
                d2 += b * b;
            }
            num / (d1.sqrt() * d2.sqrt()).max(1e-9)
        };
        println!(
            "corr(fertility, .): freshwater-dist {:+.2}  precip {:+.2}  temp {:+.2}  slope {:+.2}",
            r(|c| c.1),
            r(|c| c.2),
            r(|c| c.3),
            r(|c| c.4)
        );
    }
    acc.belt_walk.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "belts (>=16 contiguous farmable tiles over {BELT_T:.2}): {:.1} per map;  start to nearest belt p50 {:.0} p90 {:.0} tiles",
        mean(&acc.belts),
        q(&acc.belt_walk, 0.5),
        q(&acc.belt_walk, 0.9)
    );
    println!(
        "AI window (r<=14, SHORE_SCAN): starts with ANY gate-clearing soil {:.1}%   mean share of that ring farmable+gated {:.1}%",
        100.0 * mean(&acc.start_any14),
        100.0 * mean(&acc.start_gate14)
    );
    println!(
        "start slots: best soil in reach mean {:.3}  share of farmable r<=24 clearing gate {:.1}%  walk to soil>=0.5: p50 {:.0} tiles p90 {:.0}",
        mean(&acc.start_best),
        100.0 * mean(&acc.start_gate_share),
        q(&acc.start_walk, 0.5),
        q(&acc.start_walk, 0.9),
    );
}

/// Three panels side by side: what the client's soil overlay paints (the exact
/// wgsl ramp), the integer regen the farm command would store, and the biome
/// context (water + unbuildable). PPM so there is no image dependency.
fn dump(seed: u32, path: &str) {
    let g = saladin_sim::worldgrid::world_grid(seed);
    let farm = farmable_mask(seed);
    let w = (N * 3 + 8) as usize;
    let h = N as usize;
    let mut px = vec![0u8; w * h * 3];
    let smoothstep = |a: f64, b: f64, x: f64| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let mix = |a: [f64; 3], b: [f64; 3], t: f64| {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };
    let put = |px: &mut Vec<u8>, x: usize, y: usize, c: [f64; 3]| {
        let o = (y * w + x) * 3;
        for k in 0..3 {
            px[o + k] = (c[k].clamp(0.0, 1.0) * 255.0) as u8;
        }
    };
    for ty in 0..h {
        for tx in 0..N as usize {
            let i = ty * N as usize + tx;
            let f = g.fertility[i].to_num::<f64>();
            let b = g.biome[i];
            // panel A: the client's overlay ramp, exactly as terrain.wgsl mixes it
            let barren = [0.32, 0.06, 0.05];
            let fair = [0.35, 0.30, 0.05];
            let rich = [0.06, 0.42, 0.10];
            let mut tint = mix(barren, fair, smoothstep(0.10, 0.30, f));
            tint = mix(tint, rich, smoothstep(0.30, 0.62, f));
            let a = if biome_is_water(b) { [0.15, 0.30, 0.45] } else { tint };
            put(&mut px, tx, ty, a);
            // panel B: the integer regen the command stores, or why it refuses
            let c = if biome_is_water(b) {
                [0.15, 0.30, 0.45]
            } else if !farm[i] {
                [0.20, 0.20, 0.20]
            } else if f < FARM_MIN_FERTILITY.to_num::<f64>() {
                [0.45, 0.12, 0.10]
            } else {
                match ((1.0 + f * FARM_REGEN_MAX as f64) as i32).max(1) {
                    2 => [0.45, 0.42, 0.18],
                    3 => [0.35, 0.62, 0.20],
                    4 => [0.15, 0.85, 0.30],
                    _ => [0.75, 1.0, 0.55],
                }
            };
            put(&mut px, tx + N as usize + 4, ty, c);
            // panel C: fresh water and its 4-tile reach - the irrigation question
            let mut c = if biome_is_fresh_water(b) {
                [0.20, 0.60, 1.0]
            } else if biome_is_water(b) {
                [0.10, 0.20, 0.35]
            } else if farm[i] {
                [0.30, 0.28, 0.22]
            } else {
                [0.16, 0.16, 0.16]
            };
            if !biome_is_water(b) {
                let mut near = false;
                'r: for dy in -4i32..=4 {
                    for dx in -4i32..=4 {
                        let (nx, ny) = (tx as i32 + dx, ty as i32 + dy);
                        if nx < 0 || ny < 0 || nx >= N || ny >= N {
                            continue;
                        }
                        if biome_is_fresh_water(g.biome[(ny * N + nx) as usize]) {
                            near = true;
                            break 'r;
                        }
                    }
                }
                if near {
                    c = [c[0] * 0.4, c[1] * 0.7 + 0.3, c[2] * 0.7 + 0.5];
                }
            }
            put(&mut px, tx + 2 * (N as usize + 4), ty, c);
        }
    }
    // start points, marked on every panel
    for slot in 0..4 {
        let s = start_point(seed, slot);
        let (sx, sy) = (s.x.to_num::<i32>(), s.y.to_num::<i32>());
        for p in 0..3usize {
            for dy in -3i32..=3 {
                for dx in -3i32..=3 {
                    if dx.abs().max(dy.abs()) < 2 {
                        continue;
                    }
                    let (x, y) = (sx + dx, sy + dy);
                    if x < 0 || y < 0 || x >= N || y >= N {
                        continue;
                    }
                    put(&mut px, x as usize + p * (N as usize + 4), y as usize, [1.0, 1.0, 1.0]);
                }
            }
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend_from_slice(&px);
    std::fs::write(path, out).unwrap();
    println!("wrote {path}  ({}, {})", world_climate(seed).label, map_preset_by_index(seed_preset(seed) as i32).label);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seeds: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(12);
    let which = args.get(1).cloned().unwrap_or_else(|| "all".into());
    let spatial = args.iter().any(|a| a == "--spatial");
    let by_climate = args.iter().any(|a| a == "--climates");

    if args.iter().any(|a| a == "--dump") {
        let base = seeds;
        let preset: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        dump(compose_seed(base, preset), &format!("/tmp/soil_{base}_{preset}.ppm"));
        return;
    }
    if args.iter().any(|a| a == "--list") {
        for base in 1..=seeds {
            println!("seed {base}: {}", world_climate(compose_seed(base, 0)).label);
        }
        return;
    }

    let presets: Vec<u8> =
        if which == "all" { (0..4).collect() } else { vec![which.parse().unwrap_or(0)] };

    let mut climates: BTreeMap<&'static str, Acc> = BTreeMap::new();
    for p in presets {
        let mut acc = Acc::default();
        for base in 1..=seeds {
            let seed = compose_seed(base, p);
            probe(seed, &mut acc, spatial);
            if by_climate {
                let c = world_climate(seed);
                let e = climates.entry(c.label).or_default();
                probe(seed, e, false);
            }
        }
        report(&format!("preset {p} ({})", map_preset_by_index(p as i32).label), &mut acc);
    }
    for (label, acc) in climates.iter_mut() {
        report(&format!("climate {label}"), acc);
    }
}
