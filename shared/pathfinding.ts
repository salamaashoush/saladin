// Tile A* over a passability predicate. Pure + deterministic — runs in the
// module (authority) so units route around water/mountains. The core is grid-
// agnostic (takes a passable(x,y) fn) so it is testable with synthetic walls and
// reusable once buildings/occupancy join the passability layer.
import { WORLD_SIZE } from './constants.ts';
import { isLand } from './terrain.ts';

export interface PathPoint {
  x: number;
  y: number;
}

export type Passable = (tx: number, ty: number) => boolean;

const W = WORLD_SIZE;
const SQRT2 = 1.4142135623730951;

const ORTHO = [
  [1, 0],
  [-1, 0],
  [0, 1],
  [0, -1],
];

// A* expansion ceiling (docs/STDB_PERF.md §3 Rank 3c). A typical RTS route is found
// in ~O(path-length) expansions thanks to the octile heuristic — the ceiling only
// bites on a pathological detour (a near-full wall forcing exploration of most of
// the grid). Kept at the full tile count so a VALID long route is never silently
// abandoned (that would strand units): the real per-call A* win comes from 3a
// (occupancy cached per tick, not rebuilt per call), 3b (working buffers reused,
// not reallocated per call), and 3c's re-path throttle (units with a live path are
// never re-pathed — every movePatch caller guards on `!hasTarget`), not from a low
// cap that trades correctness for a bound 3a/3b already deliver.
const MAX_EXPANSIONS = W * W;

// Reusable A* working buffers (Rank 3b). findPathGrid allocated 4 typed arrays of
// W*W cells (~330 KB+) on EVERY call; at scale that allocation + GC churn dominated.
// A reducer instance is single-threaded, so a module-level scratch reused per call
// is safe — exactly how the SDK reuses LEAF_BUF/BINARY_WRITER. Reset per call via a
// cheap version stamp (gen[]) instead of re-zeroing 20 736 floats each time: a cell
// whose stamp != the current generation is treated as fresh (g=∞, came=-1, open).
const N_CELLS = W * W;
const SCRATCH_G = new Float64Array(N_CELLS);
const SCRATCH_F = new Float64Array(N_CELLS);
const SCRATCH_CAME = new Int32Array(N_CELLS);
const SCRATCH_GEN = new Int32Array(N_CELLS); // "touched this search" stamp (g/came valid)
const SCRATCH_CLOSED = new Int32Array(N_CELLS); // "closed this search" stamp
let scratchGen = 0; // bumped each search; a cell is fresh iff its stamp != scratchGen

// ── terrain-backed wrappers ───────────────────────────────────────────────────

export function isPassable(seed: number, tx: number, ty: number): boolean {
  if (tx < 0 || ty < 0 || tx >= W || ty >= W) return false;
  return isLand(seed, tx + 0.5, ty + 0.5);
}

export function nearestPassable(seed: number, x: number, y: number): PathPoint {
  return nearestPassableGrid((tx, ty) => isPassable(seed, tx, ty), x, y);
}

export function findPath(
  seed: number,
  sx: number,
  sy: number,
  tx: number,
  ty: number,
  maxExpansions = MAX_EXPANSIONS
): PathPoint[] {
  return findPathGrid(
    (px, py) => isPassable(seed, px, py),
    sx,
    sy,
    tx,
    ty,
    maxExpansions
  );
}

// ── grid-agnostic core ────────────────────────────────────────────────────────

export function nearestPassableGrid(
  passable: Passable,
  x: number,
  y: number
): PathPoint {
  const tx = Math.floor(x);
  const ty = Math.floor(y);
  if (passable(tx, ty)) return { x, y };
  for (let r = 1; r < W; r++) {
    for (let a = 0; a < 24; a++) {
      const ang = (a / 24) * Math.PI * 2;
      const nx = tx + Math.round(Math.cos(ang) * r);
      const ny = ty + Math.round(Math.sin(ang) * r);
      if (passable(nx, ny)) return { x: nx + 0.5, y: ny + 0.5 };
    }
  }
  return { x, y };
}

// The passable tile CLOSEST to (targetX,targetY) that is actually reachable on
// foot from (fromX,fromY) — i.e. in the same connected passable region. Plain
// nearestPassableGrid picks the geometrically nearest passable tile, which on
// coastal/cramped keeps can be a tiny pocket wedged against water on the far side
// of a wall: the deposit target then sits on an island the gatherer can never
// reach, findPathGrid returns [], and the carrier freezes (economy stall). This
// flood-fills the mover's own region (bounded by maxTiles) and returns the
// in-region tile nearest the target, so the chosen approach is always walkable.
// Returns null only if the mover stands on an impassable tile with no passable
// neighbour at all.
export function nearestReachablePassableGrid(
  passable: Passable,
  fromX: number,
  fromY: number,
  targetX: number,
  targetY: number,
  maxTiles = W * W
): PathPoint | null {
  const start = nearestPassableGrid(passable, fromX, fromY);
  const sx = Math.floor(start.x);
  const sy = Math.floor(start.y);
  if (!passable(sx, sy)) return null;

  const gx = Math.floor(targetX);
  const gy = Math.floor(targetY);

  // BFS the mover's connected region, tracking the closest tile to the goal.
  const seen = new Uint8Array(W * W);
  const queue: number[] = [sy * W + sx];
  seen[sy * W + sx] = 1;
  let bestX = sx;
  let bestY = sy;
  let bestD = (sx - gx) * (sx - gx) + (sy - gy) * (sy - gy);
  let visited = 0;
  for (let head = 0; head < queue.length && visited < maxTiles; head++) {
    const cur = queue[head];
    visited++;
    const cx = cur % W;
    const cy = (cur / W) | 0;
    const d = (cx - gx) * (cx - gx) + (cy - gy) * (cy - gy);
    if (d < bestD) {
      bestD = d;
      bestX = cx;
      bestY = cy;
      if (d === 0) break; // goal tile itself is reachable — can't beat it
    }
    for (const [dx, dy] of ORTHO) {
      const nx = cx + dx;
      const ny = cy + dy;
      if (nx < 0 || ny < 0 || nx >= W || ny >= W) continue;
      const ni = ny * W + nx;
      if (seen[ni] || !passable(nx, ny)) continue;
      seen[ni] = 1;
      queue.push(ni);
    }
  }
  return { x: bestX + 0.5, y: bestY + 0.5 };
}

export function lineOfSight(
  passable: Passable,
  ax: number,
  ay: number,
  bx: number,
  by: number
): boolean {
  const d = Math.hypot(bx - ax, by - ay);
  const steps = Math.max(1, Math.ceil(d * 2));
  for (let i = 1; i < steps; i++) {
    const t = i / steps;
    if (!passable(Math.floor(ax + (bx - ax) * t), Math.floor(ay + (by - ay) * t)))
      return false;
  }
  return true;
}

// Corner-safe straight-line clearance: true iff a unit can walk the segment
// (ax,ay)→(bx,by) without entering a blocked tile AND without slipping diagonally
// between two blocked tiles (the same corner-cut rule A* enforces with its
// `passable(cx+dx,cy) && passable(cx,cy+dy)` guard on diagonal edges). Used only by
// the findPathGrid fast path, where a plain LoS is NOT sufficient — sampling can
// skip the pinched corner cells (see the "diagonal pinch" test). Walks every tile
// the segment crosses via a DDA grid traversal and checks the shared edge at each
// diagonal step.
function clearStraightLine(
  passable: Passable,
  ax: number,
  ay: number,
  bx: number,
  by: number
): boolean {
  let cx = Math.floor(ax);
  let cy = Math.floor(ay);
  const ex = Math.floor(bx);
  const ey = Math.floor(by);
  const dx = bx - ax;
  const dy = by - ay;
  const stepX = dx > 0 ? 1 : -1;
  const stepY = dy > 0 ? 1 : -1;
  // distance (in t along the segment) to the next vertical / horizontal grid line
  const tDeltaX = dx !== 0 ? Math.abs(1 / dx) : Infinity;
  const tDeltaY = dy !== 0 ? Math.abs(1 / dy) : Infinity;
  let tMaxX =
    dx !== 0
      ? (dx > 0 ? Math.floor(ax) + 1 - ax : ax - Math.floor(ax)) * tDeltaX
      : Infinity;
  let tMaxY =
    dy !== 0
      ? (dy > 0 ? Math.floor(ay) + 1 - ay : ay - Math.floor(ay)) * tDeltaY
      : Infinity;
  if (!passable(cx, cy)) return false;
  let guard = 0;
  const maxSteps = Math.abs(ex - cx) + Math.abs(ey - cy) + 2;
  while ((cx !== ex || cy !== ey) && guard++ <= maxSteps) {
    if (tMaxX < tMaxY) {
      cx += stepX;
      tMaxX += tDeltaX;
    } else if (tMaxY < tMaxX) {
      cy += stepY;
      tMaxY += tDeltaY;
    } else {
      // exact diagonal crossing through a grid corner — both orthogonal neighbours
      // must be open or the move cuts the corner (A*'s diagonal rule).
      if (!passable(cx + stepX, cy) || !passable(cx, cy + stepY)) return false;
      cx += stepX;
      cy += stepY;
      tMaxX += tDeltaX;
      tMaxY += tDeltaY;
    }
    if (!passable(cx, cy)) return false;
  }
  return true;
}

class Heap {
  private readonly items: number[] = [];
  constructor(private readonly f: Float64Array) {}
  get size() {
    return this.items.length;
  }
  push(i: number) {
    const a = this.items;
    a.push(i);
    let c = a.length - 1;
    while (c > 0) {
      const p = (c - 1) >> 1;
      if (this.f[a[p]] <= this.f[a[c]]) break;
      [a[p], a[c]] = [a[c], a[p]];
      c = p;
    }
  }
  pop(): number {
    const a = this.items;
    const top = a[0];
    const last = a.pop()!;
    if (a.length > 0) {
      a[0] = last;
      let p = 0;
      for (;;) {
        const l = p * 2 + 1;
        const r = l + 1;
        let s = p;
        if (l < a.length && this.f[a[l]] < this.f[a[s]]) s = l;
        if (r < a.length && this.f[a[r]] < this.f[a[s]]) s = r;
        if (s === p) break;
        [a[p], a[s]] = [a[s], a[p]];
        p = s;
      }
    }
    return top;
  }
}

const NEI = [
  [1, 0, 1],
  [-1, 0, 1],
  [0, 1, 1],
  [0, -1, 1],
  [1, 1, SQRT2],
  [1, -1, SQRT2],
  [-1, 1, SQRT2],
  [-1, -1, SQRT2],
];

// A* path of smoothed waypoints from (sx,sy) to (tx,ty). [] if unreachable.
// Final point is the exact target; caller should pass a passable target.
export function findPathGrid(
  passable: Passable,
  sx: number,
  sy: number,
  tx: number,
  ty: number,
  maxExpansions = MAX_EXPANSIONS
): PathPoint[] {
  const s = nearestPassableGrid(passable, sx, sy);
  const goal = nearestPassableGrid(passable, tx, ty);
  const sxT = Math.floor(s.x);
  const syT = Math.floor(s.y);
  const gxT = Math.floor(goal.x);
  const gyT = Math.floor(goal.y);

  if (sxT === gxT && syT === gyT) return [{ x: tx, y: ty }];
  if (!passable(sxT, syT) || !passable(gxT, gyT)) return [];

  // Fast path (Rank 3, docs/STDB_PERF.md §3): if the corner-safe straight line
  // start→goal is clear, skip the A* grid search entirely and return the direct
  // waypoint — the common open-field case (a soldier closing on an enemy with no
  // wall between them). Behaviour-equivalent: A* would find this same clear route
  // and the string-pull below would collapse it to the straight line. Uses the
  // corner-safe check (not plain LoS) so it never green-lights a corner-cut A*
  // would have refused.
  if (clearStraightLine(passable, s.x, s.y, goal.x, goal.y))
    return [{ x: tx, y: ty }];

  // Reuse the module-level scratch (Rank 3b): bump the generation so every cell's
  // stale stamp now reads as "fresh" — g=∞ / came=-1 / not-closed — without zeroing
  // 20 736 cells. A cell is only meaningful this search once its GEN stamp is set.
  scratchGen++;
  const g = SCRATCH_G;
  const f = SCRATCH_F;
  const came = SCRATCH_CAME;
  const gen = SCRATCH_GEN;
  const closedGen = SCRATCH_CLOSED;
  const cur_gen = scratchGen;
  const gAt = (i: number): number => (gen[i] === cur_gen ? g[i] : Infinity);
  const isClosed = (i: number): boolean => closedGen[i] === cur_gen;
  const touch = (i: number, gVal: number, fVal: number, from: number) => {
    gen[i] = cur_gen;
    g[i] = gVal;
    f[i] = fVal;
    came[i] = from;
  };

  const idx = (x: number, y: number) => y * W + x;
  const h = (x: number, y: number) => {
    const dx = Math.abs(x - gxT);
    const dy = Math.abs(y - gyT);
    return dx + dy + (SQRT2 - 2) * Math.min(dx, dy);
  };

  const start = idx(sxT, syT);
  const goalI = idx(gxT, gyT);
  touch(start, 0, h(sxT, syT), -1);
  const open = new Heap(f);
  open.push(start);

  let expansions = 0;
  while (open.size > 0 && expansions < maxExpansions) {
    const cur = open.pop();
    if (cur === goalI) break;
    if (isClosed(cur)) continue;
    closedGen[cur] = cur_gen;
    expansions++;
    const cx = cur % W;
    const cy = (cur / W) | 0;

    for (const [dx, dy, cost] of NEI) {
      const nx = cx + dx;
      const ny = cy + dy;
      if (nx < 0 || ny < 0 || nx >= W || ny >= W) continue;
      if (!passable(nx, ny)) continue;
      if (dx !== 0 && dy !== 0) {
        if (!passable(cx + dx, cy) || !passable(cx, cy + dy)) continue;
      }
      const ni = idx(nx, ny);
      if (isClosed(ni)) continue;
      const tentative = g[cur] + cost;
      if (tentative < gAt(ni)) {
        touch(ni, tentative, tentative + h(nx, ny), cur);
        open.push(ni);
      }
    }
  }

  // goal never reached (unreachable, or cut off by the expansion cap).
  if (gen[goalI] !== cur_gen || came[goalI] === -1) return [];

  const tiles: PathPoint[] = [];
  let c = goalI;
  while (c !== -1) {
    tiles.push({ x: (c % W) + 0.5, y: ((c / W) | 0) + 0.5 });
    if (c === start) break;
    c = came[c];
  }
  tiles.reverse();

  // String-pull: drop waypoints that line-of-sight lets us skip.
  const out: PathPoint[] = [];
  let ax = s.x;
  let ay = s.y;
  for (let i = 1; i < tiles.length; i++) {
    if (!lineOfSight(passable, ax, ay, tiles[i].x, tiles[i].y)) {
      out.push(tiles[i - 1]);
      ax = tiles[i - 1].x;
      ay = tiles[i - 1].y;
    }
  }
  out.push({ x: tx, y: ty });
  return out;
}
