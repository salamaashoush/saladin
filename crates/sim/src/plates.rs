//! Tectonic plate field — the geology under the height map.
//!
//! A jittered Worley lattice of plate sites, each carrying a drift vector and a
//! crust type. Where two plates meet, their RELATIVE motion decides what the
//! land does: convergence throws up an orogenic belt, divergence opens a rift
//! valley with raised shoulders, shear offsets the ridges. Mountains therefore
//! arrive as long coherent chains with a fore-deep and a leeward rain shadow
//! instead of fbm blobs, and ore follows the belts that actually mineralize.

use crate::constants::WORLD_SIZE;
use crate::math::{Fx, V2, fx_sqrt};
use crate::rng::hash2;

/// Plate sites sit on a GRID x GRID jittered lattice (plus a one-cell apron so
/// boundaries reach the map edge).
const GRID: i32 = 5;
const JITTER: Fx = crate::fx!("0.34");
/// How far from a plate boundary its effects still reach, in tiles.
const BOUNDARY_REACH: Fx = crate::fx!("30");

#[derive(Clone, Copy, Debug)]
struct Site {
    pos: V2,
    drift: V2,
    continental: bool,
    /// Crust thickness 0..1 — thick continental crust rides high, thin oceanic
    /// crust sits low, and both vary plate to plate.
    thickness: Fx,
}

pub struct Plates {
    sites: Vec<Site>,
    cell: Fx,
    seed: u32,
}

// The lattice is a straight-edged Voronoi diagram; queried raw it prints its
// own cell walls onto the coastline as long straight segments. Warping the
// query point first makes every seam meander like a real suture zone.
const WARP_LO_SCALE: Fx = crate::fx!("0.008");
const WARP_LO_AMP: Fx = crate::fx!("34");
const WARP_HI_SCALE: Fx = crate::fx!("0.031");
const WARP_HI_AMP: Fx = crate::fx!("9");

/// What the plate field says about one point.
#[derive(Clone, Copy, Debug)]
pub struct PlateSample {
    /// Crust elevation contribution (roughly -0.2 .. +0.45), pre-spline.
    pub base: Fx,
    /// Orogenic belt weight 0..1 — young mountains, steep relief, high ore.
    pub belt: Fx,
    /// Rift weight 0..1 — a subsiding linear trough that collects lakes.
    pub rift: Fx,
    /// Proximity to the nearest plate boundary, 0..1.
    pub boundary: Fx,
    /// Ore potential 0..1 (subduction arcs and collision belts mineralize;
    /// stable interiors do not).
    pub ore: Fx,
}

fn dot(a: V2, b: V2) -> Fx {
    a.x * b.x + a.y * b.y
}

fn norm(v: V2) -> V2 {
    let l = v.len();
    if l <= crate::fx!("0.0001") { V2::new(Fx::ONE, Fx::ZERO) } else { V2::new(v.x / l, v.y / l) }
}

fn site(cx: i32, cy: i32, cell: Fx, seed: u32) -> Site {
    let jx = (hash2(cx, cy, seed ^ 0x5eed_1a71) - crate::fx!("0.5")) * JITTER * crate::fx!("2");
    let jy = (hash2(cx, cy, seed ^ 0x77a3_0c19) - crate::fx!("0.5")) * JITTER * crate::fx!("2");
    let pos = V2::new(
        (Fx::from_num(cx) + crate::fx!("0.5") + jx) * cell,
        (Fx::from_num(cy) + crate::fx!("0.5") + jy) * cell,
    );
    let dx = hash2(cx, cy, seed ^ 0x1f3d_9b21) - crate::fx!("0.5");
    let dy = hash2(cx, cy, seed ^ 0x8c11_4d07) - crate::fx!("0.5");
    let center = Fx::from_num(WORLD_SIZE) / crate::fx!("2");
    // The plate under the middle of the map is always continental: some
    // mainland has to exist for the eight start slots to share.
    let mid = (GRID - 1) / 2;
    let continental = (cx == mid && cy == mid)
        || hash2(cx, cy, seed ^ 0x2b7e_1516) > crate::fx!("0.42");
    let thickness = hash2(cx, cy, seed ^ 0x9e37_79b9);
    // plates near the border drift outward, so the ocean ring stays open
    let edge_bias = V2::new(pos.x - center, pos.y - center);
    let drift = norm(V2::new(
        dx * crate::fx!("2") + edge_bias.x * crate::fx!("0.004"),
        dy * crate::fx!("2") + edge_bias.y * crate::fx!("0.004"),
    ));
    Site { pos, drift, continental, thickness }
}

impl Plates {
    pub fn new(seed: u32) -> Plates {
        let cell = Fx::from_num(WORLD_SIZE) / Fx::from_num(GRID);
        let mut sites = Vec::with_capacity(((GRID + 2) * (GRID + 2)) as usize);
        for cy in -1..=GRID {
            for cx in -1..=GRID {
                sites.push(site(cx, cy, cell, seed));
            }
        }
        Plates { sites, cell, seed }
    }

    fn warp(&self, x: Fx, y: Fx) -> V2 {
        let half = crate::fx!("0.5");
        let two = crate::fx!("2");
        let lo_x = (crate::noise::fbm(x * WARP_LO_SCALE, y * WARP_LO_SCALE, self.seed ^ 0x3ca7, 3) - half) * two;
        let lo_y = (crate::noise::fbm(
            x * WARP_LO_SCALE + Fx::from_num(41),
            y * WARP_LO_SCALE + Fx::from_num(83),
            self.seed ^ 0x91b3,
            3,
        ) - half)
            * two;
        let hi_x = (crate::noise::fbm(x * WARP_HI_SCALE, y * WARP_HI_SCALE, self.seed ^ 0x5d19, 2) - half) * two;
        let hi_y = (crate::noise::fbm(
            x * WARP_HI_SCALE + Fx::from_num(19),
            y * WARP_HI_SCALE + Fx::from_num(67),
            self.seed ^ 0x2f8d,
            2,
        ) - half)
            * two;
        V2::new(
            x + lo_x * WARP_LO_AMP + hi_x * WARP_HI_AMP,
            y + lo_y * WARP_LO_AMP + hi_y * WARP_HI_AMP,
        )
    }

    fn at(&self, cx: i32, cy: i32) -> &Site {
        let i = ((cy + 1) * (GRID + 2) + (cx + 1)) as usize;
        &self.sites[i]
    }

    fn crust(s: &Site) -> Fx {
        if s.continental {
            crate::fx!("0.30") + s.thickness * crate::fx!("0.13")
        } else {
            crate::fx!("-0.16") + s.thickness * crate::fx!("0.06")
        }
    }

    pub fn sample(&self, x: Fx, y: Fx) -> PlateSample {
        let p = self.warp(x, y);
        let gx = (p.x / self.cell).floor().to_num::<i32>().clamp(-1, GRID);
        let gy = (p.y / self.cell).floor().to_num::<i32>().clamp(-1, GRID);
        let (mut d1, mut d2) = (Fx::MAX, Fx::MAX);
        let (mut i1, mut i2) = ((gx, gy), (gx, gy));
        for oy in -1..=1 {
            for ox in -1..=1 {
                let (cx, cy) = (gx + ox, gy + oy);
                if cx < -1 || cy < -1 || cx > GRID || cy > GRID {
                    continue;
                }
                let d = crate::math::dist2(p, self.at(cx, cy).pos);
                if d < d1 {
                    d2 = d1;
                    i2 = i1;
                    d1 = d;
                    i1 = (cx, cy);
                } else if d < d2 {
                    d2 = d;
                    i2 = (cx, cy);
                }
            }
        }
        let s1 = *self.at(i1.0, i1.1);
        let s2 = *self.at(i2.0, i2.1);
        // half the gap between the two nearest sites is the distance to the
        // perpendicular bisector — the boundary itself
        let edge = (fx_sqrt(d2) - fx_sqrt(d1)) / crate::fx!("2");
        let boundary = (Fx::ONE - (edge / BOUNDARY_REACH).min(Fx::ONE)).max(Fx::ZERO);
        let b2 = boundary * boundary;

        let n = norm(s2.pos.sub(s1.pos));
        let rel = s1.drift.sub(s2.drift);
        let conv = dot(rel, n);
        let shear = (dot(rel, V2::new(-n.y, n.x))).abs();

        // crust base, blended across the seam so plates do not step
        let mut base = Self::crust(&s1) + (Self::crust(&s2) - Self::crust(&s1)) * boundary * crate::fx!("0.5");

        let mut belt = Fx::ZERO;
        let mut rift = Fx::ZERO;
        let mut ore = Fx::ZERO;
        if conv > Fx::ZERO {
            let strength = conv * b2;
            if s1.continental && s2.continental {
                // continent-continent collision: the big double-crust ranges
                belt = strength;
                base += strength * crate::fx!("0.34");
                ore = strength * crate::fx!("0.8");
            } else if s1.continental != s2.continental {
                // subduction: arc on the continental side, trench on the ocean side
                belt = strength * crate::fx!("0.85");
                if s1.continental {
                    base += strength * crate::fx!("0.30");
                } else {
                    base -= strength * crate::fx!("0.10");
                }
                ore = strength; // porphyry arcs are where the metal is
            } else {
                // ocean-ocean: island arc rising out of deep water
                belt = strength * crate::fx!("0.5");
                base += strength * crate::fx!("0.42");
                ore = strength * crate::fx!("0.6");
            }
        } else if conv < Fx::ZERO {
            let strength = -conv * b2;
            rift = strength;
            base -= strength * crate::fx!("0.24");
            // rift shoulders: the flanks are uplifted while the axis drops
            let shoulder = (boundary - crate::fx!("0.30")).max(Fx::ZERO)
                * (crate::fx!("0.80") - boundary).max(Fx::ZERO)
                * crate::fx!("4");
            base += shoulder * (-conv) * crate::fx!("0.30");
            ore = strength * crate::fx!("0.35");
        }
        base += shear * b2 * crate::fx!("0.06");

        PlateSample { base, belt: belt.min(Fx::ONE), rift: rift.min(Fx::ONE), boundary, ore: ore.min(Fx::ONE) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plate_field_is_deterministic() {
        let a = Plates::new(99);
        let b = Plates::new(99);
        for i in 0..40 {
            let x = Fx::from_num(i * 7 % WORLD_SIZE);
            let y = Fx::from_num(i * 13 % WORLD_SIZE);
            assert_eq!(a.sample(x, y).base, b.sample(x, y).base);
        }
    }

    #[test]
    fn every_seed_grows_a_belt_and_a_rift() {
        // over the whole map, plate motion has to produce both convergent and
        // divergent seams — otherwise the world has no mountains or no basins
        for seed in [1u32, 7, 42, 1234, 90210] {
            let p = Plates::new(seed);
            let (mut belt, mut rift) = (Fx::ZERO, Fx::ZERO);
            let mut y = 0;
            while y < WORLD_SIZE {
                let mut x = 0;
                while x < WORLD_SIZE {
                    let s = p.sample(Fx::from_num(x), Fx::from_num(y));
                    belt = belt.max(s.belt);
                    rift = rift.max(s.rift);
                    x += 4;
                }
                y += 4;
            }
            assert!(belt > crate::fx!("0.15"), "seed {seed} has no orogenic belt ({belt})");
            assert!(rift > crate::fx!("0.10"), "seed {seed} has no rift ({rift})");
        }
    }

    #[test]
    fn every_map_carries_both_crust_types() {
        // all-continental gives a featureless slab, all-oceanic gives no map
        for seed in [3u32, 11, 555, 7777] {
            let p = Plates::new(seed);
            let (mut high, mut low) = (false, false);
            let mut y = 0;
            while y < WORLD_SIZE {
                let mut x = 0;
                while x < WORLD_SIZE {
                    let b = p.sample(Fx::from_num(x), Fx::from_num(y)).base;
                    high |= b > crate::fx!("0.25");
                    low |= b < Fx::ZERO;
                    x += 6;
                }
                y += 6;
            }
            assert!(high && low, "seed {seed} has only one kind of crust");
        }
    }

    #[test]
    fn seams_are_not_straight_lines() {
        // the warp has to bend the lattice: sample a horizontal scanline and
        // check the boundary field is not periodic with the raw cell pitch
        let p = Plates::new(4242);
        let y = Fx::from_num(WORLD_SIZE / 2);
        let mut wiggle = 0;
        let mut prev = p.sample(Fx::ZERO, y).boundary;
        let mut rising = false;
        for x in 1..WORLD_SIZE {
            let b = p.sample(Fx::from_num(x), y).boundary;
            if (b > prev) != rising {
                rising = b > prev;
                wiggle += 1;
            }
            prev = b;
        }
        // a raw 5-cell lattice would turn a handful of times; a warped one far more
        assert!(wiggle > 12, "plate seams look unwarped (only {wiggle} turns)");
    }
}
