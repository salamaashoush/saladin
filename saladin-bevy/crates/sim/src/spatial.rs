use crate::constants::WORLD_SIZE;
use crate::math::Fx;

/// Uniform spatial grid: a position maps to an integer `cell`; a neighbourhood
/// query scans the surrounding block of cells. CELL_SIZE 4 keeps bucket density
/// low in packed melees (the combat scan is O(units × bucket)); scan radii are
/// computed from the query range, so coverage is unchanged.
pub const CELL_SIZE: i32 = 4;
pub const CELLS_PER_ROW: i32 = (WORLD_SIZE + CELL_SIZE - 1) / CELL_SIZE; // 18
pub const CELL_COUNT: i32 = CELLS_PER_ROW * CELLS_PER_ROW;

fn clamp_cell(c: i32) -> i32 {
    c.clamp(0, CELLS_PER_ROW - 1)
}

/// Row-major cell id for a world position, clamped into the grid.
pub fn cell_of(x: Fx, y: Fx) -> i32 {
    let cs = Fx::from_num(CELL_SIZE);
    let cx = clamp_cell((x / cs).floor().to_num::<i32>());
    let cy = clamp_cell((y / cs).floor().to_num::<i32>());
    cy * CELLS_PER_ROW + cx
}

pub fn cell_coords(cell: i32) -> (i32, i32) {
    (cell % CELLS_PER_ROW, cell / CELLS_PER_ROW)
}

/// Cell ids within Chebyshev distance `r` of (and including) `cell`, clipped to
/// the grid.
pub fn cells_in_radius(cell: i32, r: i32) -> Vec<i32> {
    let (cx, cy) = cell_coords(cell);
    let mut out = Vec::new();
    for dy in -r..=r {
        let ny = cy + dy;
        if ny < 0 || ny >= CELLS_PER_ROW {
            continue;
        }
        for dx in -r..=r {
            let nx = cx + dx;
            if nx < 0 || nx >= CELLS_PER_ROW {
                continue;
            }
            out.push(ny * CELLS_PER_ROW + nx);
        }
    }
    out
}

/// The 3×3 block of cells around (and including) `cell`.
pub fn surrounding_cells(cell: i32) -> Vec<i32> {
    cells_in_radius(cell, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_math() {
        assert_eq!(CELLS_PER_ROW, 36);
        assert_eq!(cell_of(crate::fx!("0"), crate::fx!("0")), 0);
        assert_eq!(cell_of(crate::fx!("4"), crate::fx!("0")), 1);
        // edge clamps in-range
        assert_eq!(cell_of(Fx::from_num(WORLD_SIZE), Fx::from_num(WORLD_SIZE)), CELL_COUNT - 1);
    }

    #[test]
    fn surrounding_block() {
        // a central cell yields the full 3x3
        let center = cell_of(crate::fx!("72"), crate::fx!("72"));
        assert_eq!(surrounding_cells(center).len(), 9);
        // corner cell 0 yields 4 (2x2)
        assert_eq!(surrounding_cells(0).len(), 4);
    }
}
