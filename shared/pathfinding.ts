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

// Worst-case A* expansions: a long route forced to detour around water can touch
// most of the grid before reaching the goal. Cap at the full tile count so a
// valid long path on the 144² map is never abandoned mid-search. (At 96² this
// was a hard-coded 6000, which is below the 20 736 tiles a 144² map holds.)
const MAX_EXPANSIONS = W * W;

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

function lineOfSight(
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

  const N = W * W;
  const g = new Float64Array(N).fill(Infinity);
  const f = new Float64Array(N);
  const came = new Int32Array(N).fill(-1);
  const closed = new Uint8Array(N);

  const idx = (x: number, y: number) => y * W + x;
  const h = (x: number, y: number) => {
    const dx = Math.abs(x - gxT);
    const dy = Math.abs(y - gyT);
    return dx + dy + (SQRT2 - 2) * Math.min(dx, dy);
  };

  const start = idx(sxT, syT);
  const goalI = idx(gxT, gyT);
  g[start] = 0;
  f[start] = h(sxT, syT);
  const open = new Heap(f);
  open.push(start);

  let expansions = 0;
  while (open.size > 0 && expansions < maxExpansions) {
    const cur = open.pop();
    if (cur === goalI) break;
    if (closed[cur]) continue;
    closed[cur] = 1;
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
      if (closed[ni]) continue;
      const tentative = g[cur] + cost;
      if (tentative < g[ni]) {
        came[ni] = cur;
        g[ni] = tentative;
        f[ni] = tentative + h(nx, ny);
        open.push(ni);
      }
    }
  }

  if (came[goalI] === -1) return [];

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
