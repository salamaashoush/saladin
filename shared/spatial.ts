// Manual spatial grid (BitCraft "chunk_index" pattern). SpacetimeDB has no engine
// spatial index and a btree can range only ONE column, so a true radius query is
// impossible in one index op (see docs/STDB_PERF.md §2, §3 Rank 2). Instead every
// positioned row carries an integer `cell` = the id of the CELL_SIZE×CELL_SIZE
// grid square it sits in, btree-indexed; a neighbourhood query becomes a handful
// of point scans over the 3×3 block of cells around a unit (`entity.cell.filter`).
//
// Pure + deterministic — no DB/clock/random — so it is shared by the module
// (authority, writes the column) and is fully unit-testable.
import { WORLD_SIZE } from './constants.ts';

// One cell spans CELL_SIZE world units. WORLD_SIZE=144 → 18×18 = 324 cells with
// CELL_SIZE=8. Combat aggro/ally radii are ≤7 world units, so the 3×3 block
// around a unit (24×24 units) always covers every candidate within those radii.
export const CELL_SIZE = 8;

// Cells per row/col. Positions clamp to [0,WORLD_SIZE], so a unit exactly on the
// far edge would land one past the last cell; clamp the index to keep it in grid.
export const CELLS_PER_ROW = Math.ceil(WORLD_SIZE / CELL_SIZE); // 18
export const CELL_COUNT = CELLS_PER_ROW * CELLS_PER_ROW;

// The cell id for a world position. Row-major: floor(y/CELL)*CELLS_PER_ROW +
// floor(x/CELL). Coordinates are clamped into the grid so an edge/over-range
// position never produces an out-of-range cell (or a negative one).
export function cellOf(x: number, y: number): number {
  const cx = clampCell(Math.floor(x / CELL_SIZE));
  const cy = clampCell(Math.floor(y / CELL_SIZE));
  return cy * CELLS_PER_ROW + cx;
}

function clampCell(c: number): number {
  if (c < 0) return 0;
  if (c >= CELLS_PER_ROW) return CELLS_PER_ROW - 1;
  return c;
}

export function cellCoords(cell: number): { cx: number; cy: number } {
  return { cx: cell % CELLS_PER_ROW, cy: Math.floor(cell / CELLS_PER_ROW) };
}

// The block of cell ids within Chebyshev distance `r` of (and including) `cell`,
// clipped to the grid (corner/edge cells yield fewer). This is the set of cells a
// neighbourhood query point-scans. r=1 (the 3×3 block) covers any candidate within
// CELL_SIZE of the unit; a larger r is used when a query radius (e.g. a tower's
// fire range) exceeds one cell.
export function cellsInRadius(cell: number, r: number): number[] {
  const { cx, cy } = cellCoords(cell);
  const out: number[] = [];
  for (let dy = -r; dy <= r; dy++) {
    const ny = cy + dy;
    if (ny < 0 || ny >= CELLS_PER_ROW) continue;
    for (let dx = -r; dx <= r; dx++) {
      const nx = cx + dx;
      if (nx < 0 || nx >= CELLS_PER_ROW) continue;
      out.push(ny * CELLS_PER_ROW + nx);
    }
  }
  return out;
}

// The 3×3 block of cell ids around (and including) `cell`. Every candidate within
// one cell (CELL_SIZE world units) of the unit lives here.
export function surroundingCells(cell: number): number[] {
  return cellsInRadius(cell, 1);
}
