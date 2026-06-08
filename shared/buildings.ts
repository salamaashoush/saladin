// Building footprints + placement validity. Pure + shared: the module validates
// placement and stamps occupancy; the client previews the same predicate as a
// ghost. Footprints feed the pathfinding passability layer (units route around).
import { WORLD_SIZE } from './constants.ts';
import { BUILDING_DEFS } from './buildings_defs.ts';
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

export interface Occupant {
  kind: BuildingKind;
  x: number;
  y: number;
}

// Tile keys (ty*WORLD_SIZE+tx) covered by a set of buildings. `includePassable`
// false omits passable buildings (gatehouse) so units path through them; true
// counts every footprint (placement: no stacking). One builder shared by the
// module's pathing/placement occupancy and the client's ghost preview, so the
// gatehouse rule can never diverge between them.
export function occupancySet(
  items: ReadonlyArray<Occupant>,
  includePassable: boolean
): Set<number> {
  const s = new Set<number>();
  for (const it of items) {
    const def = BUILDING_DEFS[it.kind];
    if (!includePassable && def.passable) continue;
    for (const { tx, ty } of footprintTiles(def.footprint, it.x, it.y))
      s.add(ty * WORLD_SIZE + tx);
  }
  return s;
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

// True when at least one tile orthogonally bordering a footprint placed at
// (x,y) is passable — i.e. a gatherer can stand next to the building to deposit.
// A keep with NO passable neighbour strands every carrier (economy stall), so
// placement must reject such spots. Pure so the client ghost can preview it.
export function hasPassableApproach(
  footprint: number,
  x: number,
  y: number,
  passable: Passable
): boolean {
  const tiles = footprintTiles(footprint, x, y);
  const inFootprint = new Set(tiles.map((t) => t.ty * WORLD_SIZE + t.tx));
  for (const { tx, ty } of tiles) {
    for (const [dx, dy] of [
      [1, 0],
      [-1, 0],
      [0, 1],
      [0, -1],
    ]) {
      const nx = tx + dx;
      const ny = ty + dy;
      if (inFootprint.has(ny * WORLD_SIZE + nx)) continue;
      if (passable(nx, ny)) return true;
    }
  }
  return false;
}

// Nearest spot where a building's WHOLE footprint sits on passable land AND has a
// passable approach tile beside it. Used for the starting keep so it never
// overhangs water and a gatherer can always reach it to deposit. Two-pass: prefer
// a spot with an open approach; only if the search exhausts (no approachable spot
// anywhere) fall back to any fitting spot so founding never hard-fails.
export function findBuildableNear(
  seed: number,
  x: number,
  y: number,
  footprint: number
): { x: number; y: number } {
  const pass: Passable = (tx, ty) => isPassable(seed, tx, ty);
  const fits = (cx: number, cy: number) =>
    footprintTiles(footprint, cx, cy).every((t) => pass(t.tx, t.ty));
  const good = (cx: number, cy: number) =>
    fits(cx, cy) && hasPassableApproach(footprint, cx, cy, pass);
  if (good(x, y)) return footprintCenter(footprint, x, y);
  let firstFit: { x: number; y: number } | null = null;
  for (let r = 1; r < WORLD_SIZE; r++) {
    for (let a = 0; a < 24; a++) {
      const ang = (a / 24) * Math.PI * 2;
      const nx = x + Math.cos(ang) * r;
      const ny = y + Math.sin(ang) * r;
      if (good(nx, ny)) return footprintCenter(footprint, nx, ny);
      if (!firstFit && fits(nx, ny)) firstFit = footprintCenter(footprint, nx, ny);
    }
  }
  return firstFit ?? footprintCenter(footprint, x, y);
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

// True if any tile orthogonally bordering the footprint is impassable (water on
// land maps). Used to gate water-adjacent buildings (FishingHut) — its footprint
// sits on land but it must touch the shore. Pure so the client ghost previews the
// same rule the module enforces.
export function isWaterAdjacent(
  footprint: number,
  x: number,
  y: number,
  passable: Passable
): boolean {
  const tiles = footprintTiles(footprint, x, y);
  const inFootprint = new Set(tiles.map((t) => t.ty * WORLD_SIZE + t.tx));
  for (const { tx, ty } of tiles) {
    for (const [dx, dy] of [
      [1, 0],
      [-1, 0],
      [0, 1],
      [0, -1],
    ]) {
      const nx = tx + dx;
      const ny = ty + dy;
      if (inFootprint.has(ny * WORLD_SIZE + nx)) continue;
      if (!passable(nx, ny)) return true;
    }
  }
  return false;
}
