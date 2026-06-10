/// Compile-time fixed-point literal: forces const evaluation of `Fx::lit` so
/// runtime code never pays the decimal-string parse (it dominated profiles at
/// 60%+ — `Fx::lit` is a const fn, but in a runtime position it still parses
/// its string on every call).
#[macro_export]
macro_rules! fx {
    ($lit:literal) => {{
        const __FX_LIT: $crate::math::Fx = $crate::math::Fx::lit($lit);
        __FX_LIT
    }};
}

use fixed::types::I32F32;
use serde::{Deserialize, Serialize};

/// Fixed-point scalar for all simulation math. I32F32 = 32 integer bits, 32
/// fractional bits — deterministic across native and wasm (pure integer ops, no
/// f32/transcendental nondeterminism). Coordinates are bounded by the integer
/// part; squared distances stay exact for maps up to ~32k units a side.
pub type Fx = I32F32;

pub const ZERO: Fx = Fx::ZERO;
pub const ONE: Fx = Fx::ONE;

/// Deterministic floor-sqrt of a non-negative fixed value. Computed entirely in
/// integers via u128::isqrt, so every platform agrees bit for bit.
pub fn fx_sqrt(x: Fx) -> Fx {
    debug_assert!(x >= ZERO, "fx_sqrt of negative");
    if x <= ZERO {
        return ZERO;
    }
    // x.to_bits() == value * 2^32. We want bits(sqrt(value)) == sqrt(value)*2^32
    // == isqrt(value * 2^64) == isqrt((value*2^32) << 32).
    let scaled = (x.to_bits() as u128) << 32;
    Fx::from_bits(scaled.isqrt() as i64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct V2 {
    pub x: Fx,
    pub y: Fx,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: ZERO, y: ZERO };

    pub const fn new(x: Fx, y: Fx) -> Self {
        V2 { x, y }
    }

    pub fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }

    pub fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }

    pub fn scale(self, s: Fx) -> V2 {
        V2::new(self.x * s, self.y * s)
    }

    pub fn len2(self) -> Fx {
        self.x * self.x + self.y * self.y
    }

    pub fn len(self) -> Fx {
        fx_sqrt(self.len2())
    }
}

/// Squared distance — exact, no sqrt. Prefer this for range checks
/// (`dist2 <= r*r`) on the hot path.
pub fn dist2(a: V2, b: V2) -> Fx {
    a.sub(b).len2()
}

pub fn dist(a: V2, b: V2) -> Fx {
    fx_sqrt(dist2(a, b))
}

pub struct StepResult {
    pub pos: V2,
    pub arrived: bool,
}

/// One movement integration step from `pos` toward `target`, advancing `step`
/// units. Snaps to target and reports arrival when within `step` or `eps`.
/// Arrival is decided on squared distance to keep the common case sqrt-free.
pub fn step_toward(pos: V2, target: V2, step: Fx, eps: Fx) -> StepResult {
    let d2 = dist2(pos, target);
    if d2 <= step * step || d2 < eps * eps {
        return StepResult { pos: target, arrived: true };
    }
    let d = fx_sqrt(d2);
    let delta = target.sub(pos);
    let pos = V2::new(pos.x + delta.x * step / d, pos.y + delta.y * step / d);
    StepResult { pos, arrived: false }
}

/// A positioned, id-tagged candidate for target acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Located {
    pub id: u64,
    pub pos: V2,
}

/// Nearest candidate within `range` of `p`, or None. Squared distance,
/// deterministic tie-break by lowest distance then iteration order (`<`).
pub fn nearest_within(p: V2, candidates: &[Located], range: Fx) -> Option<Located> {
    let r2 = range * range;
    let mut best: Option<Located> = None;
    let mut best_d = Fx::MAX;
    for &c in candidates {
        let d = dist2(p, c.pos);
        if d <= r2 && d < best_d {
            best_d = d;
            best = Some(c);
        }
    }
    best
}

/// Index of the nearest point to `p`, or None. Squared distance, deterministic
/// tie-break by lowest index (strict `<`).
pub fn nearest_index(p: V2, pts: &[V2]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_d = Fx::MAX;
    for (i, &q) in pts.iter().enumerate() {
        let d = dist2(p, q);
        if d < best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

/// FNV-1a over a stream of bytes — the per-tick desync checksum primitive.
#[derive(Clone, Copy)]
pub struct Fnv1a(pub u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Fnv1a(0xcbf29ce484222325)
    }
}

impl Fnv1a {
    pub fn write_u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    pub fn write_fx(&mut self, v: Fx) {
        self.write_u64(v.to_bits() as u64);
    }

    pub fn write_v2(&mut self, v: V2) {
        self.write_fx(v.x);
        self.write_fx(v.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        Fx::lit(s)
    }

    #[test]
    fn sqrt_exact_squares() {
        assert_eq!(fx_sqrt(fx("4")), fx("2"));
        assert_eq!(fx_sqrt(fx("9")), fx("3"));
        assert_eq!(fx_sqrt(fx("144")), fx("12"));
        assert_eq!(fx_sqrt(ZERO), ZERO);
    }

    #[test]
    fn sqrt_close_to_real() {
        // sqrt(2) ~ 1.41421356; floor-sqrt is within one ULP below.
        let r = fx_sqrt(fx("2"));
        assert!(r <= fx("1.4142136") && r >= fx("1.4142135"));
    }

    #[test]
    fn step_toward_arrives() {
        let r = step_toward(V2::ZERO, V2::new(fx("0.01"), ZERO), fx("0.05"), fx("0.05"));
        assert!(r.arrived);
        assert_eq!(r.pos, V2::new(fx("0.01"), ZERO));
    }

    #[test]
    fn step_toward_advances_along_line() {
        let r = step_toward(V2::ZERO, V2::new(fx("10"), ZERO), fx("1"), fx("0.05"));
        assert!(!r.arrived);
        assert_eq!(r.pos, V2::new(fx("1"), ZERO));
    }

    #[test]
    fn checksum_is_stable() {
        let mut a = Fnv1a::default();
        a.write_v2(V2::new(fx("1.5"), fx("2.25")));
        let mut b = Fnv1a::default();
        b.write_v2(V2::new(fx("1.5"), fx("2.25")));
        assert_eq!(a.0, b.0);
    }
}
