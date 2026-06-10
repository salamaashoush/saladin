use crate::biomes::{Biome, biome_passable};
use crate::constants::WORLD_SIZE;
use crate::enums::ResourceType;
use crate::math::{Fx, V2, fx_sqrt};
use crate::noise::fbm;
use crate::rng::{Rng, hash2, mix_seed};

/// Deterministic biome terrain from a single seed. Shared by the sim
/// (authority: where land/resources are) and render. No per-tile rows — both
/// sides recompute from the seed. Fixed-point throughout so every client agrees.
#[derive(Clone, Copy, Debug)]
pub struct TerrainSample {
    pub height: Fx,
    pub moisture: Fx,
    pub biome: Biome,
}

const H_SCALE: Fx = crate::fx!("0.042");
const M_SCALE: Fx = crate::fx!("0.03");
const WARP_SCALE: Fx = crate::fx!("0.02");
const WARP_AMP: Fx = crate::fx!("9");
const SEA: Fx = crate::fx!("0.38");

/// Continent falloff: water rings the edge, land in the middle. The TS version
/// used `pow(d, 2.6)` (transcendental, nondeterministic) — replaced with
/// `d²·√d` (≈ d^2.5), deterministic and visually equivalent.
fn radial(x: Fx, y: Fx) -> Fx {
    let c = Fx::from_num(WORLD_SIZE) / Fx::from_num(2);
    let denom = Fx::from_num(WORLD_SIZE) * crate::fx!("0.5");
    let dx = x - c;
    let dy = y - c;
    let d = fx_sqrt(dx * dx + dy * dy) / denom;
    let pow = d * d * fx_sqrt(d);
    (crate::fx!("1.12") - pow * crate::fx!("0.95")).max(Fx::ZERO)
}

pub fn sample_terrain(seed: u32, x: Fx, y: Fx) -> TerrainSample {
    let half = crate::fx!("0.5");
    let two = crate::fx!("2");
    // Domain warp.
    let wx = (fbm(x * WARP_SCALE, y * WARP_SCALE, seed ^ 0x1b56, 3) - half) * two * WARP_AMP;
    let wy = (fbm(x * WARP_SCALE + Fx::from_num(31), y * WARP_SCALE + Fx::from_num(17), seed ^ 0x77c1, 3)
        - half)
        * two
        * WARP_AMP;

    let mut h = fbm((x + wx) * H_SCALE, (y + wy) * H_SCALE, seed, 5);
    h = h * crate::fx!("0.78") + crate::fx!("0.18");
    h *= radial(x, y);

    let moisture = fbm(
        (x + wx) * M_SCALE + Fx::from_num(100),
        (y + wy) * M_SCALE + Fx::from_num(50),
        seed ^ 0x9e37,
        4,
    );
    TerrainSample { height: h, moisture, biome: classify(h, moisture) }
}

fn classify(h: Fx, m: Fx) -> Biome {
    if h < SEA - crate::fx!("0.06") {
        return Biome::DeepWater;
    }
    if h < SEA {
        return Biome::ShallowWater;
    }
    if h < SEA + crate::fx!("0.04") {
        return Biome::Sand;
    }
    if h > crate::fx!("0.82") {
        return Biome::Snow;
    }
    if h > crate::fx!("0.72") {
        return Biome::Mountain;
    }
    if h > crate::fx!("0.6") {
        return Biome::Hills;
    }
    if m < crate::fx!("0.26") {
        return if h < SEA + crate::fx!("0.12") { Biome::Oasis } else { Biome::Desert };
    }
    if m < crate::fx!("0.4") {
        return Biome::Dunes;
    }
    if m < crate::fx!("0.52") {
        return Biome::Steppe;
    }
    if m < crate::fx!("0.72") {
        return Biome::Grassland;
    }
    Biome::Forest
}

pub fn is_land(seed: u32, x: Fx, y: Fx) -> bool {
    biome_passable(sample_terrain(seed, x, y).biome)
}

const ADJ4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Buildable land with open water on at least one orthogonal neighbour.
pub fn is_coastal(seed: u32, x: Fx, y: Fx) -> bool {
    if !is_land(seed, x, y) {
        return false;
    }
    for (dx, dy) in ADJ4 {
        let b = sample_terrain(seed, x + Fx::from_num(dx), y + Fx::from_num(dy)).biome;
        if b == Biome::DeepWater || b == Biome::ShallowWater {
            return true;
        }
    }
    false
}

/// Tile-space passability for the pathfinder: in-bounds land at the tile centre.
pub fn is_passable(seed: u32, tx: i32, ty: i32) -> bool {
    if tx < 0 || ty < 0 || tx >= WORLD_SIZE || ty >= WORLD_SIZE {
        return false;
    }
    passable_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// Per-seed passability bitmap, computed once and leaked (a process touches a
/// handful of seeds at most). Terrain sampling is fbm noise — pricey enough
/// that the old compute-per-call `is_passable` dominated A*-heavy profiles.
/// A thread-local memo of the last seed keeps the hot path lock-free.
pub fn passable_grid(seed: u32) -> &'static [bool] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[bool] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [bool])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [bool]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [bool] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let mut v = vec![false; (WORLD_SIZE * WORLD_SIZE) as usize];
            for ty in 0..WORLD_SIZE {
                for tx in 0..WORLD_SIZE {
                    v[(ty * WORLD_SIZE + tx) as usize] =
                        is_land(seed, Fx::from_num(tx) + crate::fx!("0.5"), Fx::from_num(ty) + crate::fx!("0.5"));
                }
            }
            let leaked: &'static [bool] = Box::leak(v.into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Connected-region id per tile (flood fill over `passable_grid`), cached per
/// seed like the grids above. `u16::MAX` = impassable. Lets gameplay ask
/// "can this unit ever walk there?" in O(1) — the cure for gatherers
/// ping-ponging between nodes on islands they can never reach.
pub fn region_grid(seed: u32) -> &'static [u16] {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[u16] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [u16])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    if last_seed == seed && !last_grid.is_empty() {
        return last_grid;
    }

    static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [u16]>>> = OnceLock::new();
    let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = grids.lock().unwrap();
    let grid: &'static [u16] = match g.get(&seed) {
        Some(&grid) => grid,
        None => {
            let pass = passable_grid(seed);
            let n = (WORLD_SIZE * WORLD_SIZE) as usize;
            let mut v = vec![u16::MAX; n];
            let mut next_region: u16 = 0;
            let mut stack: Vec<i32> = Vec::new();
            for start in 0..n {
                if !pass[start] || v[start] != u16::MAX {
                    continue;
                }
                let region = next_region;
                next_region += 1;
                v[start] = region;
                stack.push(start as i32);
                while let Some(idx) = stack.pop() {
                    let (tx, ty) = (idx % WORLD_SIZE, idx / WORLD_SIZE);
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (tx + dx, ty + dy);
                        if nx < 0 || ny < 0 || nx >= WORLD_SIZE || ny >= WORLD_SIZE {
                            continue;
                        }
                        let ni = (ny * WORLD_SIZE + nx) as usize;
                        if pass[ni] && v[ni] == u16::MAX {
                            v[ni] = region;
                            stack.push(ni as i32);
                        }
                    }
                }
            }
            let leaked: &'static [u16] = Box::leak(v.into_boxed_slice());
            g.insert(seed, leaked);
            leaked
        }
    };
    LAST.with(|c| c.set((seed, grid)));
    grid
}

/// Region id at a world position (`u16::MAX` = impassable tile).
pub fn region_at(seed: u32, x: Fx, y: Fx) -> u16 {
    let tx = x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    region_grid(seed)[(ty * WORLD_SIZE + tx) as usize]
}

/// Can a walker standing at `from` ever harvest a node at `node`? True when the
/// node's tile — or any neighbouring tile (coastal fish sit on water, harvested
/// from the adjacent shore) — shares the walker's connected region.
pub fn node_reachable(seed: u32, from: V2, node: V2) -> bool {
    let region = region_at(seed, from.x, from.y);
    if region == u16::MAX {
        return true; // walker on a weird tile: do not over-filter
    }
    let grid = region_grid(seed);
    let tx = node.x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = node.y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= WORLD_SIZE || ny >= WORLD_SIZE {
                continue;
            }
            if grid[(ny * WORLD_SIZE + nx) as usize] == region {
                return true;
            }
        }
    }
    false
}

/// Render elevation in world units (client mesh only — never feeds the sim).
pub fn render_height(h: Fx, emphasis: Fx, elev_gain: Fx) -> Fx {
    if h < SEA {
        return crate::fx!("-0.5") * ((SEA - h) / SEA) - crate::fx!("0.05");
    }
    let base = (h - SEA) * Fx::from_num(9);
    let relief = base + base * base * crate::fx!("0.18");
    relief * emphasis * elev_gain
}

#[derive(Clone, Copy, Debug)]
pub struct ScatteredNode {
    pub pos: V2,
    pub res_type: ResourceType,
    pub yield_: i32,
}

/// One scatter rule: count, yield, per-biome accept-probability, coastal-only.
#[derive(Clone, Copy)]
pub struct ScatterRule {
    pub res_type: ResourceType,
    pub count: i32,
    pub yield_: i32,
    pub density: fn(Biome) -> Fx,
    pub coastal_only: bool,
}

/// Deterministically place all resource nodes for a seed. Each rule draws from
/// its own RNG stream (via `mix_seed`) so adding/removing a kind never shifts
/// the others.
pub fn scatter_nodes(seed: u32, rules: &[ScatterRule]) -> Vec<ScatteredNode> {
    let mut out = Vec::new();
    let span = Fx::from_num(WORLD_SIZE - 6);
    let three = crate::fx!("3");
    for (ri, rule) in rules.iter().enumerate() {
        let ri = ri as u32;
        let mut rand = Rng::new(mix_seed(seed, 1013u32.wrapping_mul(ri + 1)));
        let mut placed = 0;
        let mut attempts = 0;
        let budget = rule.count.max(60) * 80;
        let roll_seed = mix_seed(seed, ri + 1);
        while placed < rule.count && attempts < budget {
            attempts += 1;
            let x = three + rand.next_fx() * span;
            let y = three + rand.next_fx() * span;
            let reachable =
                if rule.coastal_only { is_coastal(seed, x, y) } else { is_land(seed, x, y) };
            if !reachable {
                continue;
            }
            let roll = hash2(x.floor().to_num::<i32>(), y.floor().to_num::<i32>(), roll_seed);
            let biome = sample_terrain(seed, x, y).biome;
            if roll < (rule.density)(biome) {
                out.push(ScatteredNode { pos: V2::new(x, y), res_type: rule.res_type, yield_: rule.yield_ });
                placed += 1;
            }
        }
    }
    out
}

/// Nearest buildable land near (x, y), via deterministic integer ring scan.
pub fn find_land_near(seed: u32, x: Fx, y: Fx) -> V2 {
    if is_land(seed, x, y) {
        return V2::new(x, y);
    }
    let lo = Fx::from_num(3);
    let hi = Fx::from_num(WORLD_SIZE - 3);
    for r in 1..WORLD_SIZE {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let nx = (x + Fx::from_num(dx)).clamp(lo, hi);
                let ny = (y + Fx::from_num(dy)).clamp(lo, hi);
                if is_land(seed, nx, ny) {
                    return V2::new(nx, ny);
                }
            }
        }
    }
    let c = Fx::from_num(WORLD_SIZE) / Fx::from_num(2);
    V2::new(c, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_is_reproducible() {
        let a = sample_terrain(7, crate::fx!("40.5"), crate::fx!("60.25"));
        let b = sample_terrain(7, crate::fx!("40.5"), crate::fx!("60.25"));
        assert_eq!(a.height, b.height);
        assert_eq!(a.biome, b.biome);
    }

    #[test]
    fn map_has_land_and_water() {
        let mut land = 0;
        let mut water = 0;
        let mut y = 4;
        while y < WORLD_SIZE - 4 {
            let mut x = 4;
            while x < WORLD_SIZE - 4 {
                if is_passable(11, x, y) { land += 1 } else { water += 1 }
                x += 4;
            }
            y += 4;
        }
        assert!(land > 0 && water > 0, "expected mixed land/water, got {land}/{water}");
    }

    #[test]
    fn scatter_is_deterministic_and_reachable() {
        let rules = [ScatterRule {
            res_type: ResourceType::Wood,
            count: 50,
            yield_: 120,
            density: crate::biomes::tree_density,
            coastal_only: false,
        }];
        let a = scatter_nodes(3, &rules);
        let b = scatter_nodes(3, &rules);
        assert_eq!(a.len(), b.len());
        for (na, nb) in a.iter().zip(b.iter()) {
            assert_eq!(na.pos, nb.pos);
        }
        // every placed node sits on land
        for n in &a {
            assert!(is_land(3, n.pos.x, n.pos.y));
        }
    }
}
