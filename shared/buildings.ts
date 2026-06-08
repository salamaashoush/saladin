// Building footprints + placement validity. Pure + shared: the module validates
// placement and stamps occupancy; the client previews the same predicate as a
// ghost. Footprints feed the pathfinding passability layer (units route around).
import { WORLD_SIZE } from './constants.ts';
import { BUILDING_DEFS } from './defs.ts';
import type { BuildingKind } from './enums.ts';
import { isPassable, type Passable } from './pathfinding.ts';

export interface Tile {
  tx: number;
  ty: number;
}

// Integer tiles a footprint-f building covers when placed near (x,y).
export function footprintTiles(footprint: number, x: number, y: number): Tile[] {
  const cx = Math.floor(x);
  const cy = Math.floor(y);
  const r = Math.floor(footprint / 2);
  const tiles: Tile[] = [];
  for (let i = 0; i < footprint; i++)
    for (let j = 0; j < footprint; j++)
      tiles.push({ tx: cx - r + i, ty: cy - r + j });
  return tiles;
}

// World-space centre of the footprint (average of tile centres) — where the
// building model is placed.
export function footprintCenter(
  footprint: number,
  x: number,
  y: number
): { x: number; y: number } {
  const cx = Math.floor(x);
  const cy = Math.floor(y);
  const r = Math.floor(footprint / 2);
  const off = -r + (footprint - 1) / 2 + 0.5;
  return { x: cx + off, y: cy + off };
}

// Nearest spot where a building's WHOLE footprint sits on passable land. Used
// for the starting keep so it never overhangs water.
export function findBuildableNear(
  seed: number,
  x: number,
  y: number,
  footprint: number
): { x: number; y: number } {
  const fits = (cx: number, cy: number) =>
    footprintTiles(footprint, cx, cy).every((t) => isPassable(seed, t.tx, t.ty));
  if (fits(x, y)) return footprintCenter(footprint, x, y);
  for (let r = 1; r < WORLD_SIZE; r++) {
    for (let a = 0; a < 24; a++) {
      const ang = (a / 24) * Math.PI * 2;
      const nx = x + Math.cos(ang) * r;
      const ny = y + Math.sin(ang) * r;
      if (fits(nx, ny)) return footprintCenter(footprint, nx, ny);
    }
  }
  return footprintCenter(footprint, x, y);
}

// Placeable if every footprint tile is passable terrain and not occupied.
export function canPlace(
  kind: BuildingKind,
  x: number,
  y: number,
  passable: Passable,
  occupied: (tx: number, ty: number) => boolean
): boolean {
  const f = BUILDING_DEFS[kind].footprint;
  for (const { tx, ty } of footprintTiles(f, x, y)) {
    if (!passable(tx, ty)) return false;
    if (occupied(tx, ty)) return false;
  }
  return true;
}
