use crate::math::Fx;
use crate::terrain::sample_terrain;

/// Elevation as a gameplay layer (distinct from render height): high ground lets
/// a ranged attacker reach and see farther. All pure + deterministic.

/// Normalized 0..1 elevation at (seed, x, y).
pub fn elevation(seed: u32, x: Fx, y: Fx) -> Fx {
    sample_terrain(seed, x, y).height.clamp(Fx::ZERO, Fx::ONE)
}

/// Tile-resolution elevation from a per-seed cache (computed once, leaked,
/// thread-local memo for the last seed — same pattern as `passable_grid`).
/// Combat reads elevation per attacker/target pair; sampling fbm there
/// dominated profiles. Tile granularity is the gameplay-correct reading
/// anyway ("this unit stands on that hill").
pub fn elevation_at(seed: u32, x: Fx, y: Fx) -> Fx {
    use crate::constants::WORLD_SIZE;
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const EMPTY: &[Fx] = &[];
    thread_local! {
        static LAST: Cell<(u32, &'static [Fx])> = const { Cell::new((u32::MAX, EMPTY)) };
    }
    let (last_seed, last_grid) = LAST.with(|c| c.get());
    let grid = if last_seed == seed && !last_grid.is_empty() {
        last_grid
    } else {
        static GRIDS: OnceLock<Mutex<HashMap<u32, &'static [Fx]>>> = OnceLock::new();
        let grids = GRIDS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut g = grids.lock().unwrap();
        let grid: &'static [Fx] = match g.get(&seed) {
            Some(&grid) => grid,
            None => {
                let half = crate::fx!("0.5");
                let mut v = Vec::with_capacity((WORLD_SIZE * WORLD_SIZE) as usize);
                for ty in 0..WORLD_SIZE {
                    for tx in 0..WORLD_SIZE {
                        v.push(elevation(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half));
                    }
                }
                let leaked: &'static [Fx] = Box::leak(v.into_boxed_slice());
                g.insert(seed, leaked);
                leaked
            }
        };
        LAST.with(|c| c.set((seed, grid)));
        grid
    };
    let tx = x.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    let ty = y.to_num::<i32>().clamp(0, WORLD_SIZE - 1);
    grid[(ty * WORLD_SIZE + tx) as usize]
}

pub const ELEV_BONUS_SPAN: Fx = crate::fx!("0.25");
pub const ELEV_BONUS_MAX: Fx = crate::fx!("0.25");

/// Range/vision multiplier for an attacker at `attacker_elev` firing at a target
/// at `target_elev` (both 0..1). 1.0 = no change; uphill >1 (up to
/// 1+ELEV_BONUS_MAX), downhill <1. Linear in the delta, clamped at ±SPAN.
pub fn elevation_range_bonus(attacker_elev: Fx, target_elev: Fx) -> Fx {
    let delta = (attacker_elev - target_elev).clamp(-ELEV_BONUS_SPAN, ELEV_BONUS_SPAN);
    Fx::ONE + (delta / ELEV_BONUS_SPAN) * ELEV_BONUS_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uphill_helps_downhill_hurts() {
        assert_eq!(elevation_range_bonus(crate::fx!("0.5"), crate::fx!("0.5")), Fx::ONE);
        assert_eq!(elevation_range_bonus(crate::fx!("1"), crate::fx!("0")), crate::fx!("1.25"));
        assert_eq!(elevation_range_bonus(crate::fx!("0"), crate::fx!("1")), crate::fx!("0.75"));
    }
}
