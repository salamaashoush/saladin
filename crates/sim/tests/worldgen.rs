//! Worldgen invariants across many seeds and every preset: fair starts,
//! river/cliff connectivity (one dominant landmass, starts on it), feature
//! presence, determinism, and preset distinctness.

use saladin_sim::*;

fn passable_count(seed: u32) -> (u32, u32) {
    let grid = region_grid(seed);
    let main = dominant_region(seed);
    let mut pass = 0u32;
    let mut dom = 0u32;
    for &r in grid {
        if r != u16::MAX {
            pass += 1;
            if r == main {
                dom += 1;
            }
        }
    }
    (pass, dom)
}

#[test]
fn fair_starts_hold_for_100_seeds_across_presets() {
    // 25 base seeds x 4 presets = 100 generated worlds
    for base in 1..=25u32 {
        for preset in 0..4u8 {
            let seed = compose_seed(base, preset);
            let nodes = scatter_nodes(seed, &node_kinds());
            let extra = fair_start_nodes(seed, &nodes, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
            let all: Vec<ScatteredNode> = nodes.into_iter().chain(extra).collect();
            let r2 = FAIR_RADIUS * FAIR_RADIUS;
            for slot in 0..8 {
                let start = start_point(seed, slot);
                let mut have = [0usize; 3];
                for n in &all {
                    let dx = n.pos.x - start.x;
                    let dy = n.pos.y - start.y;
                    if dx * dx + dy * dy > r2 {
                        continue;
                    }
                    match n.res_type {
                        ResourceType::Wood => have[0] += 1,
                        ResourceType::Stone => have[1] += 1,
                        ResourceType::Food => have[2] += 1,
                        ResourceType::Gold => {}
                    }
                }
                assert!(
                    have[0] >= FAIR_MIN_WOOD && have[1] >= FAIR_MIN_STONE && have[2] >= FAIR_MIN_FOOD,
                    "seed {base} preset {preset} slot {slot}: wood {} stone {} food {} under minimum",
                    have[0],
                    have[1],
                    have[2]
                );
            }
        }
    }
}

#[test]
fn rivers_and_cliffs_leave_one_dominant_landmass() {
    // mainland presets must stay one connected battlefield (fords + ramps work);
    // archipelago is allowed to be islands by design
    for base in [1u32, 7, 13, 21, 34, 55, 89, 99] {
        for preset in 0..3u8 {
            let seed = compose_seed(base, preset);
            let (pass, dom) = passable_count(seed);
            assert!(pass > 0, "seed {base} preset {preset}: no land at all");
            // realistic geography may split off side continents and islands;
            // the MAINLAND (where all 8 starts snap to) must stay a real
            // arena: at least the old 144x144 map's worth of connected land,
            // and never a degenerate sliver of the total
            let ratio = dom as f64 / pass as f64;
            assert!(
                ratio >= 0.25,
                "seed {base} preset {preset}: dominant region only {:.0}% of land",
                ratio * 100.0
            );
            assert!(dom >= 5500, "seed {base} preset {preset}: mainland too small ({dom} tiles)");
            // every start shares that dominant region
            let main = dominant_region(seed);
            for slot in 0..8 {
                let s = start_point(seed, slot);
                assert_eq!(
                    region_at(seed, s.x, s.y),
                    main,
                    "seed {base} preset {preset} slot {slot} stranded off the mainland"
                );
            }
        }
    }
}

#[test]
fn keep_sites_are_buildable_open_and_on_the_mainland() {
    for base in 1..=25u32 {
        for preset in 0..4u8 {
            let seed = compose_seed(base, preset);
            let main = dominant_region(seed);
            for slot in 0..8 {
                let site = find_keep_site(seed, slot, 3);
                let (sx, sy) = (site.x.to_num::<i32>(), site.y.to_num::<i32>());
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (tx, ty) = (sx + dx, sy + dy);
                        assert!(
                            is_passable(seed, tx, ty),
                            "seed {base} preset {preset} slot {slot}: keep tile ({tx},{ty}) impassable"
                        );
                        assert_eq!(
                            region_at(seed, Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5")),
                            main,
                            "seed {base} preset {preset} slot {slot}: keep off the mainland"
                        );
                        let b = sample_terrain(
                            seed,
                            Fx::from_num(tx) + fx!("0.5"),
                            Fx::from_num(ty) + fx!("0.5"),
                        )
                        .biome;
                        assert!(
                            biome_buildable(b),
                            "seed {base} preset {preset} slot {slot}: keep tile on {b:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn archipelago_keeps_playable_land() {
    for base in [1u32, 5, 7, 13, 21, 34, 55, 99] {
        let seed = compose_seed(base, 3);
        let grid = region_grid(seed);
        let main = dominant_region(seed);
        let (mut pass, mut dom) = (0u32, 0u32);
        for &r in grid {
            if r != u16::MAX {
                pass += 1;
                if r == main {
                    dom += 1;
                }
            }
        }
        assert!(pass >= 5000, "seed {base}: archipelago nearly all water ({pass} land tiles)");
        assert!(dom >= 2500, "seed {base}: main island too small ({dom} tiles)");
    }
}

#[test]
fn rivers_with_fords_exist_in_river_valley() {
    let mut river_tiles = 0;
    let mut ford_tiles = 0;
    let seed = compose_seed(5, 1); // river-valley
    for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            let s = sample_terrain(
                seed,
                Fx::from_num(tx) + fx!("0.5"),
                Fx::from_num(ty) + fx!("0.5"),
            );
            match s.biome {
                Biome::River => river_tiles += 1,
                Biome::Ford => ford_tiles += 1,
                _ => {}
            }
        }
    }
    assert!(river_tiles > 100, "river-valley should carve real rivers ({river_tiles} tiles)");
    assert!(ford_tiles > 5, "rivers need fords to cross ({ford_tiles} tiles)");
}

#[test]
fn cliffs_exist_in_highlands() {
    let seed = compose_seed(5, 2); // highlands
    let mut cliffs = 0;
    for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            let s = sample_terrain(
                seed,
                Fx::from_num(tx) + fx!("0.5"),
                Fx::from_num(ty) + fx!("0.5"),
            );
            if s.biome == Biome::Cliff {
                cliffs += 1;
            }
        }
    }
    assert!(cliffs > 30, "highlands should raise cliff walls ({cliffs} tiles)");
}

#[test]
fn presets_change_the_map_but_share_the_base_height_field() {
    let a = compose_seed(9, 0);
    let b = compose_seed(9, 3); // archipelago: higher sea level
    assert_eq!(seed_base(a), seed_base(b));
    assert_ne!(seed_preset(a), seed_preset(b));
    let (pa, _) = passable_count(a);
    let (pb, _) = passable_count(b);
    assert!(
        pb < pa,
        "archipelago drowns land vs continental ({pb} vs {pa} passable tiles)"
    );
}

#[test]
fn composed_seeds_are_deterministic() {
    let seed = compose_seed(42, 1);
    for (x, y) in [(10, 10), (70, 70), (100, 40)] {
        let a = sample_terrain(seed, Fx::from_num(x), Fx::from_num(y));
        let b = sample_terrain(seed, Fx::from_num(x), Fx::from_num(y));
        assert_eq!(a.height, b.height);
        assert_eq!(a.biome, b.biome);
    }
    let n1 = scatter_nodes(seed, &node_kinds());
    let n2 = scatter_nodes(seed, &node_kinds());
    assert_eq!(n1.len(), n2.len());
    for (a, b) in n1.iter().zip(n2.iter()) {
        assert_eq!(a.pos, b.pos);
    }
    let f1 = fair_start_nodes(seed, &n1, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
    let f2 = fair_start_nodes(seed, &n2, 8, TREE_WOOD, STONE_YIELD, FOOD_YIELD);
    assert_eq!(f1.len(), f2.len());
    for (a, b) in f1.iter().zip(f2.iter()) {
        assert_eq!(a.pos, b.pos);
        assert_eq!(a.res_type, b.res_type);
    }
}

#[test]
fn ford_passable_river_cliff_not() {
    assert!(biome_passable(Biome::Ford));
    assert!(!biome_passable(Biome::River));
    assert!(!biome_passable(Biome::Cliff));
    assert!(!biome_buildable(Biome::Ford), "no towers plugging the crossing");
}

// ── climate + biome diversity (worldgen v3) ─────────────────────────────────

fn land_biome_counts(seed: u32) -> std::collections::HashMap<Biome, u32> {
    let g = worldgrid::world_grid(seed);
    let mut counts = std::collections::HashMap::new();
    for &b in &g.biome {
        if biome_passable(b) {
            *counts.entry(b).or_insert(0) += 1;
        }
    }
    counts
}

#[test]
fn no_map_is_a_single_biome() {
    // The failure this guards against is the one the old one-axis moisture ramp
    // had: every seed reading as the same green blob. A map must hold at least
    // four land biomes with real presence, and none may swallow the continent.
    for base in [3u32, 40, 91, 212, 777, 1500, 4096, 31337] {
        for preset in 0..4u8 {
            let seed = compose_seed(base, preset);
            let counts = land_biome_counts(seed);
            let land: u32 = counts.values().sum();
            assert!(land > 1000, "seed {base}/{preset} has almost no land ({land})");
            let significant = counts.values().filter(|&&c| c * 100 / land >= 3).count();
            assert!(significant >= 4, "seed {base}/{preset} has only {significant} land biomes");
            let top = *counts.values().max().unwrap();
            assert!(
                top * 100 / land <= 72,
                "seed {base}/{preset}: one biome covers {}% of the land",
                top * 100 / land
            );
        }
    }
}

#[test]
fn climate_archetypes_actually_change_the_world() {
    // Two seeds that roll different regimes must differ in kind, not just in
    // noise: the wet ones grow forest, the dry ones grow desert.
    let mut wettest = (Fx::ZERO, 0u32);
    let mut driest = (Fx::ONE, 0u32);
    for base in 1..40u32 {
        let seed = compose_seed(base, 0);
        let c = climate::climate_archetype(seed);
        if c.target_precip > wettest.0 {
            wettest = (c.target_precip, seed);
        }
        if c.target_precip < driest.0 {
            driest = (c.target_precip, seed);
        }
    }
    let wet = land_biome_counts(wettest.1);
    let dry = land_biome_counts(driest.1);
    let tree = |m: &std::collections::HashMap<Biome, u32>| {
        m.get(&Biome::Forest).copied().unwrap_or(0) + m.get(&Biome::Pine).copied().unwrap_or(0)
    };
    let sand = |m: &std::collections::HashMap<Biome, u32>| {
        m.get(&Biome::Desert).copied().unwrap_or(0) + m.get(&Biome::Dunes).copied().unwrap_or(0)
    };
    assert!(tree(&wet) > tree(&dry), "the wet regime grew no more forest than the dry one");
    assert!(sand(&dry) > sand(&wet), "the dry regime grew no more desert than the wet one");
}

#[test]
fn highlands_preset_actually_reaches_the_high_country() {
    // "Highlands" promised mountains and used to deliver 0.3% of them.
    let mut high = 0u32;
    let mut total = 0u32;
    for base in 1..=8u32 {
        let counts = land_biome_counts(compose_seed(base, 2));
        let g = worldgrid::world_grid(compose_seed(base, 2));
        for &b in &g.biome {
            if matches!(b, Biome::Mountain | Biome::Snow | Biome::Alpine | Biome::Cliff) {
                high += 1;
            }
        }
        total += counts.values().sum::<u32>();
    }
    assert!(high * 100 / total >= 12, "highlands are only {}% high country", high * 100 / total);
}

#[test]
fn soil_is_richest_where_the_water_runs() {
    // Farms have to care about terrain: floodplain and delta soil must beat
    // the dry interior, or the fertility layer is decoration.
    let seed = compose_seed(11, 1);
    let g = worldgrid::world_grid(seed);
    let n = WORLD_SIZE as usize;
    let (mut near, mut near_n, mut far, mut far_n) = (Fx::ZERO, 0i32, Fx::ZERO, 0i32);
    for ty in 2..n - 2 {
        for tx in 2..n - 2 {
            let i = ty * n + tx;
            if !biome_passable(g.biome[i]) {
                continue;
            }
            let mut wet = false;
            for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1), (2, 0), (-2, 0), (0, 2), (0, -2)] {
                let j = (ty as i32 + dy) as usize * n + (tx as i32 + dx) as usize;
                wet |= biome_is_fresh_water(g.biome[j]);
            }
            if wet {
                near += g.fertility[i];
                near_n += 1;
            } else {
                far += g.fertility[i];
                far_n += 1;
            }
        }
    }
    assert!(near_n > 50 && far_n > 50, "not enough samples ({near_n}/{far_n})");
    let (a, b) = (near / Fx::from_num(near_n), far / Fx::from_num(far_n));
    assert!(a > b, "riverside soil ({a}) is no richer than dry ground ({b})");
}

#[test]
fn ore_follows_the_orogenic_belts_not_the_lowlands() {
    let seed = compose_seed(23, 2);
    let g = worldgrid::world_grid(seed);
    let (mut high_ore, mut low_ore) = (Fx::ZERO, Fx::ZERO);
    let (mut hn, mut ln) = (0i32, 0i32);
    for i in 0..g.biome.len() {
        if !biome_passable(g.biome[i]) {
            continue;
        }
        if matches!(g.biome[i], Biome::Hills | Biome::Alpine | Biome::Hammada) {
            high_ore += g.ore[i];
            hn += 1;
        } else {
            low_ore += g.ore[i];
            ln += 1;
        }
    }
    assert!(hn > 20 && ln > 20);
    assert!(
        high_ore / Fx::from_num(hn) > low_ore / Fx::from_num(ln),
        "the high country is no more mineralized than the plains"
    );
}
