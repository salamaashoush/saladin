use crate::math::{Fx, ZERO};
use crate::rng::hash2;

fn smooth(t: Fx) -> Fx {
    let three = crate::fx!("3");
    let two = crate::fx!("2");
    t * t * (three - two * t)
}

/// Bilinear value noise in [0, 1].
pub fn value_noise(x: Fx, y: Fx, seed: u32) -> Fx {
    let x0 = x.floor();
    let y0 = y.floor();
    let ix0 = x0.to_num::<i32>();
    let iy0 = y0.to_num::<i32>();
    let fx = smooth(x - x0);
    let fy = smooth(y - y0);
    let v00 = hash2(ix0, iy0, seed);
    let v10 = hash2(ix0 + 1, iy0, seed);
    let v01 = hash2(ix0, iy0 + 1, seed);
    let v11 = hash2(ix0 + 1, iy0 + 1, seed);
    let a = v00 + (v10 - v00) * fx;
    let b = v01 + (v11 - v01) * fx;
    a + (b - a) * fy
}

/// Fractal Brownian motion in [0, 1].
pub fn fbm(x: Fx, y: Fx, seed: u32, octaves: u32) -> Fx {
    let mut amp = crate::fx!("0.5");
    let mut freq = 1i32;
    let mut sum = ZERO;
    let mut norm = ZERO;
    for o in 0..octaves {
        let f = Fx::from_num(freq);
        sum += amp * value_noise(x * f, y * f, seed.wrapping_add(o.wrapping_mul(1013)));
        norm += amp;
        amp >>= 1; // exact halving
        freq *= 2;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::ONE;

    #[test]
    fn value_noise_in_unit_interval() {
        let mut x = crate::fx!("-10");
        while x < crate::fx!("10") {
            let mut y = crate::fx!("-10");
            while y < crate::fx!("10") {
                let v = value_noise(x, y, 3);
                assert!(v >= ZERO && v <= ONE);
                y += crate::fx!("0.37");
            }
            x += crate::fx!("0.37");
        }
    }

    #[test]
    fn fbm_in_unit_interval_and_reproducible() {
        let a = fbm(crate::fx!("3.5"), crate::fx!("7.25"), 42, 4);
        let b = fbm(crate::fx!("3.5"), crate::fx!("7.25"), 42, 4);
        assert_eq!(a, b);
        assert!(a >= ZERO && a <= ONE);
    }
}
