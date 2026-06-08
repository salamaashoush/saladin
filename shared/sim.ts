// Pure simulation math — no SpacetimeDB/Three deps so it is unit-testable and
// shared by the module (authority) and client. Keep deterministic.

export interface Vec2 {
  x: number;
  y: number;
}

export function dist(ax: number, ay: number, bx: number, by: number): number {
  return Math.hypot(bx - ax, by - ay);
}

export interface StepResult {
  x: number;
  y: number;
  facing: number;
  arrived: boolean;
}

// One movement integration step from (x,y) toward (tx,ty), advancing `step`
// units. Snaps to target and reports arrival when within `step`/`eps`.
export function stepToward(
  x: number,
  y: number,
  tx: number,
  ty: number,
  step: number,
  eps: number
): StepResult {
  const dx = tx - x;
  const dy = ty - y;
  const d = Math.hypot(dx, dy);
  const facing = d > 1e-6 ? Math.atan2(dy, dx) : 0;
  if (d <= step || d < eps) return { x: tx, y: ty, facing, arrived: true };
  return { x: x + (dx / d) * step, y: y + (dy / d) * step, facing, arrived: false };
}

// Subtract damage from hp, clamped at 0.
export function applyDamage(hp: number, dmg: number): number {
  return Math.max(0, hp - dmg);
}

export function inRange(d: number, range: number): boolean {
  return d <= range;
}

// Index of the nearest point to (px,py), or -1 if none.
export function nearestIndex(
  px: number,
  py: number,
  pts: ReadonlyArray<Vec2>
): number {
  let best = -1;
  let bestD = Infinity;
  for (let i = 0; i < pts.length; i++) {
    const dx = pts[i].x - px;
    const dy = pts[i].y - py;
    const d = dx * dx + dy * dy;
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  }
  return best;
}
