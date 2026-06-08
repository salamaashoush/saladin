import type { Vec2 } from '../../shared/index.ts';

export const MAX_WALL_LEN = 40;

// Straight tile line from s to e along the dominant axis (clamped length).
export function lineTiles(
  s: { tx: number; ty: number },
  e: { tx: number; ty: number }
): Array<{ tx: number; ty: number }> {
  const dx = e.tx - s.tx;
  const dy = e.ty - s.ty;
  const out: Array<{ tx: number; ty: number }> = [];
  if (Math.abs(dx) >= Math.abs(dy)) {
    const n = Math.min(Math.abs(dx), MAX_WALL_LEN);
    const step = Math.sign(dx);
    for (let i = 0; i <= n; i++) out.push({ tx: s.tx + step * i, ty: s.ty });
  } else {
    const n = Math.min(Math.abs(dy), MAX_WALL_LEN);
    const step = Math.sign(dy);
    for (let i = 0; i <= n; i++) out.push({ tx: s.tx, ty: s.ty + step * i });
  }
  return out;
}

// Grid offsets that spread `n` move targets around a click so units don't stack.
export function formation(n: number): Vec2[] {
  const out: Vec2[] = [];
  const cols = Math.max(1, Math.ceil(Math.sqrt(n)));
  const rows = Math.ceil(n / cols);
  const s = 0.85;
  for (let i = 0; i < n; i++) {
    const c = i % cols;
    const r = Math.floor(i / cols);
    out.push({ x: (c - (cols - 1) / 2) * s, y: (r - (rows - 1) / 2) * s });
  }
  return out;
}
