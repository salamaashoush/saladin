use crate::math::Fx;

/// Deterministic PRNG + spatial hash. Ported from the TS game's `rng.ts`, but
/// kept in clean u32 wrapping arithmetic (the TS version leaned on JS f64 quirks
/// in `hash2`). Parity with the TS game is NOT required — only internal
/// determinism across every Rust client, which pure integer ops guarantee.
///
/// Float outputs from the original are replaced by exact fixed-point in [0,1):
/// a u32 `v` maps to `Fx::from_bits(v as i64)` == v / 2^32, with no division.

/// mulberry32 — a 32-bit counter PRNG.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rng {
    a: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng { a: seed }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.a = self.a.wrapping_add(0x6d2b79f5);
        let a = self.a;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        t ^ (t >> 14)
    }

    /// Next value in [0, 1) as exact fixed-point.
    pub fn next_fx(&mut self) -> Fx {
        Fx::from_bits(self.next_u32() as i64)
    }

    /// Integer in [lo, hi] inclusive, via rejection-free modulo (slight bias is
    /// fine for gameplay and is deterministic).
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo + 1) as u32;
        lo + (self.next_u32() % span) as i32
    }
}

/// Stateless spatial hash → u32 for an integer lattice point under `seed`.
pub fn hash2_u32(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32)
        .wrapping_mul(374761393)
        .wrapping_add((y as u32).wrapping_mul(668265263))
        .wrapping_add(seed.wrapping_mul(2246822519));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^ (h >> 16)
}

/// Spatial hash as exact fixed-point in [0, 1).
pub fn hash2(x: i32, y: i32, seed: u32) -> Fx {
    Fx::from_bits(hash2_u32(x, y, seed) as i64)
}

/// Fold a salt into a seed to derive an independent, uncorrelated stream.
pub fn mix_seed(seed: u32, salt: u32) -> u32 {
    let mut h = seed ^ salt.wrapping_mul(0x9e3779b1);
    h = (h ^ (h >> 16)).wrapping_mul(0x85ebca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2ae35);
    h ^ (h >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{ONE, ZERO};

    #[test]
    fn rng_is_reproducible() {
        let mut a = Rng::new(1234);
        let mut b = Rng::new(1234);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn rng_differs_by_seed() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u32(), b.next_u32());
    }

    #[test]
    fn next_fx_in_unit_interval() {
        let mut r = Rng::new(99);
        for _ in 0..10_000 {
            let v = r.next_fx();
            assert!(v >= ZERO && v < ONE);
        }
    }

    #[test]
    fn hash2_in_unit_interval_and_stable() {
        for x in -50..50 {
            for y in -50..50 {
                let v = hash2(x, y, 7);
                assert!(v >= ZERO && v < ONE);
                assert_eq!(v, hash2(x, y, 7));
            }
        }
    }

    #[test]
    fn range_i32_within_bounds() {
        let mut r = Rng::new(5);
        for _ in 0..10_000 {
            let n = r.range_i32(1, 6);
            assert!((1..=6).contains(&n));
        }
        assert_eq!(r.range_i32(3, 3), 3);
    }
}
