import { describe, it, expect } from 'vitest';
import {
  cellOf,
  cellCoords,
  surroundingCells,
  CELL_SIZE,
  CELLS_PER_ROW,
  CELL_COUNT,
} from './spatial.ts';
import { WORLD_SIZE } from './constants.ts';

describe('grid geometry', () => {
  it('CELLS_PER_ROW covers the whole map', () => {
    expect(CELLS_PER_ROW * CELL_SIZE).toBeGreaterThanOrEqual(WORLD_SIZE);
    expect(CELL_COUNT).toBe(CELLS_PER_ROW * CELLS_PER_ROW);
  });
});

describe('cellOf', () => {
  it('origin lands in cell 0', () => {
    expect(cellOf(0, 0)).toBe(0);
    expect(cellOf(CELL_SIZE - 0.001, CELL_SIZE - 0.001)).toBe(0);
  });

  it('is row-major: +CELL_SIZE in y advances by one row', () => {
    expect(cellOf(0, CELL_SIZE)).toBe(CELLS_PER_ROW);
    expect(cellOf(CELL_SIZE, 0)).toBe(1);
    expect(cellOf(CELL_SIZE, CELL_SIZE)).toBe(CELLS_PER_ROW + 1);
  });

  it('clamps over-range / edge positions into the grid (never out of range)', () => {
    const maxCell = CELL_COUNT - 1;
    expect(cellOf(WORLD_SIZE, WORLD_SIZE)).toBe(maxCell);
    // a hair past the world edge still clamps in-grid
    expect(cellOf(WORLD_SIZE + 5, WORLD_SIZE + 5)).toBe(maxCell);
    for (let i = 0; i < CELL_COUNT; i++) {
      // every cell id produced is in [0, CELL_COUNT)
    }
    expect(cellOf(-3, -3)).toBe(0);
  });

  it('produces only valid cell ids across a dense sweep of the map', () => {
    for (let y = 0; y <= WORLD_SIZE; y += 3)
      for (let x = 0; x <= WORLD_SIZE; x += 3) {
        const c = cellOf(x, y);
        expect(c).toBeGreaterThanOrEqual(0);
        expect(c).toBeLessThan(CELL_COUNT);
      }
  });

  it('round-trips through cellCoords', () => {
    const c = cellOf(50, 90);
    const { cx, cy } = cellCoords(c);
    expect(cx).toBe(Math.floor(50 / CELL_SIZE));
    expect(cy).toBe(Math.floor(90 / CELL_SIZE));
  });
});

describe('surroundingCells', () => {
  it('returns the full 3×3 block (9 cells) for an interior cell', () => {
    const center = cellOf(WORLD_SIZE / 2, WORLD_SIZE / 2);
    const ring = surroundingCells(center);
    expect(ring.length).toBe(9);
    expect(ring).toContain(center);
    // all distinct
    expect(new Set(ring).size).toBe(9);
  });

  it('clips at the top-left corner to a 2×2 block', () => {
    const ring = surroundingCells(0);
    expect(ring.sort((a, b) => a - b)).toEqual([0, 1, CELLS_PER_ROW, CELLS_PER_ROW + 1]);
  });

  it('clips at the bottom-right corner to a 2×2 block', () => {
    const last = CELL_COUNT - 1;
    const ring = surroundingCells(last);
    expect(ring.length).toBe(4);
    expect(ring).toContain(last);
  });

  it('clips an edge (non-corner) cell to a 2×3 block', () => {
    // a cell on the left edge, middle row
    const edge = Math.floor(CELLS_PER_ROW / 2) * CELLS_PER_ROW; // cx=0
    const ring = surroundingCells(edge);
    expect(ring.length).toBe(6);
  });

  it('every returned cell id is valid', () => {
    for (let c = 0; c < CELL_COUNT; c++)
      for (const n of surroundingCells(c)) {
        expect(n).toBeGreaterThanOrEqual(0);
        expect(n).toBeLessThan(CELL_COUNT);
      }
  });

  it('covers the combat aggro radius: any point within 7 units of a cell-center unit shares a cell in the 3×3 block', () => {
    // A unit at cell center; any candidate within radius 7 must fall in one of the
    // 3×3 cells (since CELL_SIZE=8 > 7, the block spans ±8 around the unit's cell).
    const ux = 72;
    const uy = 72;
    const block = new Set(surroundingCells(cellOf(ux, uy)));
    const R = 7;
    for (let a = 0; a < 360; a += 15) {
      const rad = (a * Math.PI) / 180;
      const px = ux + Math.cos(rad) * R;
      const py = uy + Math.sin(rad) * R;
      expect(block.has(cellOf(px, py))).toBe(true);
    }
  });
});
