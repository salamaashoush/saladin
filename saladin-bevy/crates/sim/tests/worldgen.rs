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
