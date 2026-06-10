use crate::constants::WORLD_SIZE;
use crate::math::{Fx, V2, dist};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Tile A* over a passability predicate. Pure + deterministic. The core is
/// grid-agnostic (takes a `passable(x,y)` fn) so it is testable with synthetic
/// walls. Costs are fixed-point; the open set is a min-heap keyed on `(f, cell)`
/// so ties break by lowest cell index — identical expansion order everywhere.
const W: i32 = WORLD_SIZE;
const N_CELLS: usize = (WORLD_SIZE * WORLD_SIZE) as usize;

fn sqrt2() -> Fx {
    crate::math::fx_sqrt(Fx::from_num(2))
}

const ORTHO: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn floor_i(v: Fx) -> i32 {
    v.floor().to_num::<i32>()
}
fn idx(x: i32, y: i32) -> usize {
    (y * W + x) as usize
}

/// Nearest passable tile to (x, y) by deterministic ring scan (the TS version
/// rounded cos/sin samples — replaced for determinism). Returns the original
/// point if its own tile is already passable.
pub fn nearest_passable_grid<P: Fn(i32, i32) -> bool>(passable: &P, x: Fx, y: Fx) -> V2 {
    let tx = floor_i(x);
    let ty = floor_i(y);
    if passable(tx, ty) {
        return V2::new(x, y);
    }
    let half = crate::fx!("0.5");
    for r in 1..W {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let (nx, ny) = (tx + dx, ty + dy);
                if passable(nx, ny) {
                    return V2::new(Fx::from_num(nx) + half, Fx::from_num(ny) + half);
                }
            }
        }
    }
    V2::new(x, y)
}

/// The passable tile closest to the target that is actually reachable on foot
/// from `from` (same connected region). Flood-fills the mover's region and
/// returns the in-region tile nearest the goal. `None` only if the mover stands
/// on an impassable tile with no passable neighbour.
pub fn nearest_reachable_passable_grid<P: Fn(i32, i32) -> bool>(
    passable: &P,
    from: V2,
    target: V2,
    max_tiles: usize,
) -> Option<V2> {
    let start = nearest_passable_grid(passable, from.x, from.y);
    let sx = floor_i(start.x);
    let sy = floor_i(start.y);
    if !passable(sx, sy) {
        return None;
    }
    let gx = floor_i(target.x);
    let gy = floor_i(target.y);

    let mut seen = vec![false; N_CELLS];
    let mut queue: Vec<usize> = vec![idx(sx, sy)];
    seen[idx(sx, sy)] = true;
    let (mut best_x, mut best_y) = (sx, sy);
    let mut best_d = (sx - gx) * (sx - gx) + (sy - gy) * (sy - gy);
    let mut visited = 0usize;
    let mut head = 0usize;
    while head < queue.len() && visited < max_tiles {
        let cur = queue[head];
        head += 1;
        visited += 1;
        let cx = (cur as i32) % W;
        let cy = (cur as i32) / W;
        let d = (cx - gx) * (cx - gx) + (cy - gy) * (cy - gy);
        if d < best_d {
            best_d = d;
            best_x = cx;
            best_y = cy;
            if d == 0 {
                break;
            }
        }
        for (dx, dy) in ORTHO {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= W || ny >= W {
                continue;
            }
            let ni = idx(nx, ny);
            if seen[ni] || !passable(nx, ny) {
                continue;
            }
            seen[ni] = true;
            queue.push(ni);
        }
    }
    let half = crate::fx!("0.5");
    Some(V2::new(Fx::from_num(best_x) + half, Fx::from_num(best_y) + half))
}

/// Sampled line-of-sight: every sampled tile along the segment is passable.
pub fn line_of_sight<P: Fn(i32, i32) -> bool>(passable: &P, a: V2, b: V2) -> bool {
    let d = dist(a, b);
    let steps = (d * Fx::from_num(2)).ceil().to_num::<i32>().max(1);
    for i in 1..steps {
        let t = Fx::from_num(i) / Fx::from_num(steps);
        let px = floor_i(a.x + (b.x - a.x) * t);
        let py = floor_i(a.y + (b.y - a.y) * t);
        if !passable(px, py) {
            return false;
        }
    }
    true
}

/// Corner-safe straight-line clearance via DDA grid traversal: the segment
/// enters no blocked tile AND never slips diagonally between two blocked tiles
/// (A*'s diagonal corner rule).
fn clear_straight_line<P: Fn(i32, i32) -> bool>(passable: &P, a: V2, b: V2) -> bool {
    let mut cx = floor_i(a.x);
    let mut cy = floor_i(a.y);
    let ex = floor_i(b.x);
    let ey = floor_i(b.y);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    // An axis whose delta is below EPS is treated as never crossing a grid line
    // (the segment is effectively aligned to the other axis). This also keeps the
    // reciprocal `1/d` from overflowing fixed-point — a near-zero divisor would
    // blow past Fx::MAX where f64 would merely go large.
    const EPS: Fx = crate::fx!("0.0001");
    let small_x = dx.abs() < EPS;
    let small_y = dy.abs() < EPS;
    if small_x && small_y {
        return passable(cx, cy); // degenerate (same cell) — nothing to cross
    }
    let step_x = if dx > Fx::ZERO { 1 } else { -1 };
    let step_y = if dy > Fx::ZERO { 1 } else { -1 };
    let inf = Fx::MAX;
    let t_delta_x = if small_x { inf } else { (Fx::ONE / dx).abs() };
    let t_delta_y = if small_y { inf } else { (Fx::ONE / dy).abs() };
    let mut t_max_x = if small_x {
        inf
    } else {
        let f = a.x.floor();
        (if dx > Fx::ZERO { f + Fx::ONE - a.x } else { a.x - f }) * t_delta_x
    };
    let mut t_max_y = if small_y {
        inf
    } else {
        let f = a.y.floor();
        (if dy > Fx::ZERO { f + Fx::ONE - a.y } else { a.y - f }) * t_delta_y
    };
    if !passable(cx, cy) {
        return false;
    }
    let max_steps = (ex - cx).abs() + (ey - cy).abs() + 2;
    let mut guard = 0;
    while (cx != ex || cy != ey) && guard <= max_steps {
        guard += 1;
        if t_max_x < t_max_y {
            cx += step_x;
            t_max_x = t_max_x.saturating_add(t_delta_x);
        } else if t_max_y < t_max_x {
            cy += step_y;
            t_max_y = t_max_y.saturating_add(t_delta_y);
        } else {
            if !passable(cx + step_x, cy) || !passable(cx, cy + step_y) {
                return false;
            }
            cx += step_x;
            cy += step_y;
            t_max_x = t_max_x.saturating_add(t_delta_x);
            t_max_y = t_max_y.saturating_add(t_delta_y);
        }
        if !passable(cx, cy) {
            return false;
        }
    }
    true
}

/// Reusable A* working buffers with generation stamps for O(1) reset between
/// searches. A sim system holds one of these in a resource; the `find_path_grid`
/// free function creates one per call for convenience/tests.
pub struct AStar {
    g: Vec<Fx>,
    came: Vec<i32>,
    touched: Vec<u32>,
    closed_gen: Vec<u32>,
    cur_gen: u32,
}

impl Default for AStar {
    fn default() -> Self {
        AStar {
            g: vec![Fx::ZERO; N_CELLS],
            came: vec![-1; N_CELLS],
            touched: vec![0; N_CELLS],
            closed_gen: vec![0; N_CELLS],
            cur_gen: 0,
        }
    }
}

impl AStar {
    pub fn new() -> Self {
        Self::default()
    }

    fn g_at(&self, i: usize) -> Fx {
        if self.touched[i] == self.cur_gen { self.g[i] } else { Fx::MAX }
    }
    fn is_closed(&self, i: usize) -> bool {
        self.closed_gen[i] == self.cur_gen
    }
    fn touch(&mut self, i: usize, g: Fx, from: i32) {
        self.touched[i] = self.cur_gen;
        self.g[i] = g;
        self.came[i] = from;
    }

    /// A* path of smoothed waypoints. Empty if unreachable. Final point is the
    /// exact target; pass a passable target for a clean finish.
    pub fn find_path<P: Fn(i32, i32) -> bool>(
        &mut self,
        passable: &P,
        sx: Fx,
        sy: Fx,
        tx: Fx,
        ty: Fx,
        max_expansions: usize,
    ) -> Vec<V2> {
        let s = nearest_passable_grid(passable, sx, sy);
        let goal = nearest_passable_grid(passable, tx, ty);
        let sx_t = floor_i(s.x);
        let sy_t = floor_i(s.y);
        let gx_t = floor_i(goal.x);
        let gy_t = floor_i(goal.y);

        if sx_t == gx_t && sy_t == gy_t {
            return vec![V2::new(tx, ty)];
        }
        if !passable(sx_t, sy_t) || !passable(gx_t, gy_t) {
            return Vec::new();
        }
        if clear_straight_line(passable, s, goal) {
            return vec![V2::new(tx, ty)];
        }

        self.cur_gen = self.cur_gen.wrapping_add(1);
        if self.cur_gen == 0 {
            // wrapped: clear stamps so stale (gen==0) cells don't read as fresh
            self.touched.iter_mut().for_each(|v| *v = u32::MAX);
            self.closed_gen.iter_mut().for_each(|v| *v = u32::MAX);
            self.cur_gen = 1;
        }

        let s2 = sqrt2();
        let h = |x: i32, y: i32| -> Fx {
            let dx = Fx::from_num((x - gx_t).abs());
            let dy = Fx::from_num((y - gy_t).abs());
            dx + dy + (s2 - Fx::from_num(2)) * dx.min(dy)
        };
        let neighbors: [(i32, i32, Fx); 8] = [
            (1, 0, Fx::ONE),
            (-1, 0, Fx::ONE),
            (0, 1, Fx::ONE),
            (0, -1, Fx::ONE),
            (1, 1, s2),
            (1, -1, s2),
            (-1, 1, s2),
            (-1, -1, s2),
        ];

        let start = idx(sx_t, sy_t);
        let goal_i = idx(gx_t, gy_t);
        self.touch(start, Fx::ZERO, -1);
        let mut open: BinaryHeap<Reverse<(Fx, u32)>> = BinaryHeap::new();
        open.push(Reverse((h(sx_t, sy_t), start as u32)));

        let mut expansions = 0usize;
        while let Some(Reverse((_, cur_u))) = open.pop() {
            if expansions >= max_expansions {
                break;
            }
            let cur = cur_u as usize;
            if cur == goal_i {
                break;
            }
            if self.is_closed(cur) {
                continue;
            }
            self.closed_gen[cur] = self.cur_gen;
            expansions += 1;
            let cx = (cur as i32) % W;
            let cy = (cur as i32) / W;
            let g_cur = self.g[cur];

            for (dx, dy, cost) in neighbors {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= W || ny >= W || !passable(nx, ny) {
                    continue;
                }
                if dx != 0 && dy != 0 && (!passable(cx + dx, cy) || !passable(cx, cy + dy)) {
                    continue;
                }
                let ni = idx(nx, ny);
                if self.is_closed(ni) {
                    continue;
                }
                let tentative = g_cur + cost;
                if tentative < self.g_at(ni) {
                    self.touch(ni, tentative, cur as i32);
                    open.push(Reverse((tentative + h(nx, ny), ni as u32)));
                }
            }
        }

        if self.touched[goal_i] != self.cur_gen || self.came[goal_i] == -1 {
            return Vec::new();
        }

        // reconstruct
        let half = crate::fx!("0.5");
        let mut tiles: Vec<V2> = Vec::new();
        let mut c = goal_i as i32;
        while c != -1 {
            let cx = c % W;
            let cy = c / W;
            tiles.push(V2::new(Fx::from_num(cx) + half, Fx::from_num(cy) + half));
            if c as usize == start {
                break;
            }
            c = self.came[c as usize];
        }
        tiles.reverse();

        // string-pull
        let mut out: Vec<V2> = Vec::new();
        let mut a = s;
        for i in 1..tiles.len() {
            if !line_of_sight(passable, a, tiles[i]) {
                out.push(tiles[i - 1]);
                a = tiles[i - 1];
            }
        }
        out.push(V2::new(tx, ty));
        out
    }
}

/// Convenience: full A* expansion ceiling.
pub const MAX_EXPANSIONS: usize = N_CELLS;

/// Allocate a scratch A* and find a path. Systems should keep an `AStar` and
/// reuse it instead of calling this per unit.
pub fn find_path_grid<P: Fn(i32, i32) -> bool>(passable: &P, sx: Fx, sy: Fx, tx: Fx, ty: Fx) -> Vec<V2> {
    AStar::new().find_path(passable, sx, sy, tx, ty, MAX_EXPANSIONS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_open_field_is_direct() {
        let pass = |_: i32, _: i32| true;
        let p = find_path_grid(&pass, crate::fx!("2.5"), crate::fx!("2.5"), crate::fx!("20.5"), crate::fx!("20.5"));
        assert_eq!(p.len(), 1); // fast path -> just the target
        assert_eq!(p[0], V2::new(crate::fx!("20.5"), crate::fx!("20.5")));
    }

    #[test]
    fn routes_around_a_wall() {
        // vertical wall at x==10 for y in 0..20, with a gap at y==0
        let pass = |x: i32, y: i32| !(x == 10 && y >= 1 && y <= 20);
        let p = find_path_grid(&pass, crate::fx!("5.5"), crate::fx!("10.5"), crate::fx!("15.5"), crate::fx!("10.5"));
        assert!(!p.is_empty(), "should find a detour around the wall");
        // ends at target
        assert_eq!(*p.last().unwrap(), V2::new(crate::fx!("15.5"), crate::fx!("10.5")));
    }

    #[test]
    fn unreachable_returns_empty() {
        // fully wall off the goal region: x==10 blocked for ALL y
        let pass = |x: i32, _y: i32| x != 10;
        let p = find_path_grid(&pass, crate::fx!("5.5"), crate::fx!("5.5"), crate::fx!("15.5"), crate::fx!("5.5"));
        assert!(p.is_empty());
    }

    #[test]
    fn reachable_region_picks_nearest_in_region() {
        let pass = |x: i32, _y: i32| x != 10;
        let r = nearest_reachable_passable_grid(
            &pass,
            V2::new(crate::fx!("5.5"), crate::fx!("5.5")),
            V2::new(crate::fx!("15.5"), crate::fx!("5.5")),
            N_CELLS,
        );
        let r = r.unwrap();
        // best reachable tile hugs the wall on the start side (x==9)
        assert_eq!(floor_i(r.x), 9);
    }
}
