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

/// Retained BFS buffers for `nearest_reachable_passable_grid`. Generation
/// stamps reset it in O(1); the alternative is a 147k-element `vec![false; _]`
/// on every call, and this one is called per walker per replan.
pub struct Flood {
    seen: Vec<u32>,
    cur_gen: u32,
    queue: Vec<u32>,
}

impl Default for Flood {
    fn default() -> Self {
        Flood { seen: vec![0; N_CELLS], cur_gen: 0, queue: Vec::with_capacity(1024) }
    }
}

impl Flood {
    pub fn new() -> Self {
        Self::default()
    }
    fn begin(&mut self) {
        self.cur_gen = self.cur_gen.wrapping_add(1);
        if self.cur_gen == 0 {
            self.seen.iter_mut().for_each(|v| *v = 0);
            self.cur_gen = 1;
        }
        self.queue.clear();
    }
    fn mark(&mut self, i: usize) -> bool {
        if self.seen[i] == self.cur_gen {
            return false;
        }
        self.seen[i] = self.cur_gen;
        true
    }

    /// Flood the walker's whole reachable region and KEEP the stamps, so the
    /// caller can then ask `saw` about any tile it likes. One flood answers
    /// "which of these nodes can I actually get to" for every candidate at once
    /// — asking per candidate means an A* per candidate.
    ///
    /// Returns false when the walker stands nowhere passable.
    pub fn explore<P: Fn(i32, i32) -> bool>(&mut self, passable: &P, from: V2, max_tiles: usize) -> bool {
        let start = nearest_passable_grid(passable, from.x, from.y);
        let (sx, sy) = (floor_i(start.x), floor_i(start.y));
        if !passable(sx, sy) {
            return false;
        }
        self.begin();
        let s = idx(sx, sy);
        self.mark(s);
        self.queue.push(s as u32);
        let mut head = 0usize;
        while head < self.queue.len() && head < max_tiles {
            let cur = self.queue[head] as i32;
            head += 1;
            let (cx, cy) = (cur % W, cur / W);
            for (dx, dy) in ORTHO {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= W || ny >= W {
                    continue;
                }
                let ni = idx(nx, ny);
                if !passable(nx, ny) || !self.mark(ni) {
                    continue;
                }
                self.queue.push(ni as u32);
            }
        }
        true
    }

    /// Was this tile reached by the last `explore`?
    pub fn saw(&self, tx: i32, ty: i32) -> bool {
        if tx < 0 || ty < 0 || tx >= W || ty >= W {
            return false;
        }
        self.seen[idx(tx, ty)] == self.cur_gen
    }
}

/// The closest a walker can get to a goal, and whether the flood ran out of
/// budget with ground still unexplored.
pub struct Reach {
    pub at: V2,
    /// `true` means "as close as I looked", NOT "as close as you can ever get".
    /// Reading the second into the first is what pins a gatherer on the tile it
    /// is already standing on: the flood runs out short of a ridge, hands back
    /// the start tile, and the caller concludes the node is unreachable.
    pub truncated: bool,
}

/// Flood budget for a walk of `d` tiles. A detour costs on the order of the
/// square of the direct distance, so the budget scales that way; the floor
/// keeps one ridge from defeating a short hop and the ceiling keeps a hopeless
/// goal from walking the whole map.
pub fn reach_budget(d: Fx) -> usize {
    const MIN: usize = 6144;
    const MAX: usize = 32768;
    let t = d.to_num::<i64>().clamp(0, 512) as usize;
    (t * t * 64).clamp(MIN, MAX)
}

/// The passable tile closest to the target that is actually reachable on foot
/// from `from` (same connected region). Flood-fills the mover's region and
/// returns the in-region tile nearest the goal. `None` only if the mover stands
/// on an impassable tile with no passable neighbour.
pub fn nearest_reachable_passable_grid<P: Fn(i32, i32) -> bool>(
    flood: &mut Flood,
    passable: &P,
    from: V2,
    target: V2,
    max_tiles: usize,
) -> Option<Reach> {
    let start = nearest_passable_grid(passable, from.x, from.y);
    let sx = floor_i(start.x);
    let sy = floor_i(start.y);
    if !passable(sx, sy) {
        return None;
    }
    let gx = floor_i(target.x);
    let gy = floor_i(target.y);

    flood.begin();
    let s = idx(sx, sy);
    flood.mark(s);
    flood.queue.push(s as u32);
    let (mut best_x, mut best_y) = (sx, sy);
    let mut best_d = (sx - gx) * (sx - gx) + (sy - gy) * (sy - gy);
    let mut visited = 0usize;
    let mut head = 0usize;
    let mut truncated = false;
    while head < flood.queue.len() {
        if visited >= max_tiles {
            truncated = true;
            break;
        }
        let cur = flood.queue[head] as i32;
        head += 1;
        visited += 1;
        let cx = cur % W;
        let cy = cur / W;
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
            if !passable(nx, ny) || !flood.mark(ni) {
                continue;
            }
            flood.queue.push(ni as u32);
        }
    }
    let half = crate::fx!("0.5");
    Some(Reach { at: V2::new(Fx::from_num(best_x) + half, Fx::from_num(best_y) + half), truncated })
}

/// The tile a walker standing at `from` should occupy to work at `to`: the free
/// tile nearest `to` that shares the walker's own terrain region.
///
/// This is the answer the flood was being asked for and could not afford to
/// give. `region_grid` already knows, exactly and for free, which tiles a walker
/// can ever stand on; a bounded flood only knows which tiles it had budget to
/// look at, and the two disagree the moment a ridge makes the route longer than
/// the budget. `None` means the terrain itself separates them — buildings can
/// still seal a pocket the terrain leaves open, so a caller that gets `Some`
/// must still check that A* found a route.
pub fn approach_tile<P: Fn(i32, i32) -> bool>(
    seed: u32,
    passable: &P,
    from: V2,
    to: V2,
    radius: i32,
) -> Option<V2> {
    let region = crate::terrain::region_at(seed, from.x, from.y);
    if region == u16::MAX {
        return None;
    }
    let grid = crate::terrain::region_grid(seed);
    let (tx, ty) = (floor_i(to.x), floor_i(to.y));
    let half = crate::fx!("0.5");
    let mut best: Option<(Fx, Fx, V2)> = None;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (nx, ny) = (tx + dx, ty + dy);
            if nx < 0 || ny < 0 || nx >= W || ny >= W {
                continue;
            }
            if grid[idx(nx, ny)] != region || !passable(nx, ny) {
                continue;
            }
            let c = V2::new(Fx::from_num(nx) + half, Fx::from_num(ny) + half);
            let d = crate::math::dist2(c, to);
            // Closest to the goal wins; among equals, the side the walker is
            // already on. Without the tie-break a crew raising a wall picks the
            // far face of it and marches around the whole line to reach it.
            let near = crate::math::dist2(c, from);
            if best.is_none_or(|(bd, bn, _)| d < bd || (d == bd && near < bn)) {
                best = Some((d, near, c));
            }
        }
    }
    best.map(|(_, _, p)| p)
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
        self.find_path_costed(passable, &|_, _| Fx::ONE, sx, sy, tx, ty, max_expansions)
    }

    /// A* that pays for the ground it crosses. `step_cost(tx, ty)` multiplies
    /// the cost of entering a tile and must never drop below 1, or the octile
    /// heuristic stops being admissible and the search returns junk.
    #[allow(clippy::too_many_arguments)]
    pub fn find_path_costed<P: Fn(i32, i32) -> bool, C: Fn(i32, i32) -> Fx>(
        &mut self,
        passable: &P,
        step_cost: &C,
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
                let tentative = g_cur + cost * step_cost(nx, ny);
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
    #[test]
    fn a_costed_path_goes_around_the_bog() {
        // open field with a wide expensive strip down the middle: the walker
        // should detour around it rather than wade straight through
        let passable = |_x: i32, _y: i32| true;
        let bog = |x: i32, y: i32| {
            if (10..14).contains(&x) && (0..18).contains(&y) { crate::fx!("6") } else { Fx::ONE }
        };
        let mut a = AStar::new();
        let path = a.find_path_costed(
            &passable,
            &bog,
            crate::fx!("5.5"),
            crate::fx!("9.5"),
            crate::fx!("18.5"),
            crate::fx!("9.5"),
            20_000,
        );
        assert!(!path.is_empty(), "no path at all");
        let waded = path.iter().any(|p| {
            let (x, y) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
            (10..14).contains(&x) && (2..16).contains(&y)
        });
        assert!(!waded, "the walker ploughed straight through the bog: {path:?}");
    }

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
            &mut Flood::new(),
            &pass,
            V2::new(crate::fx!("5.5"), crate::fx!("5.5")),
            V2::new(crate::fx!("15.5"), crate::fx!("5.5")),
            N_CELLS,
        );
        let r = r.unwrap();
        // best reachable tile hugs the wall on the start side (x==9)
        assert_eq!(floor_i(r.at.x), 9);
        assert!(!r.truncated, "a full-budget flood of a closed region is never truncated");
    }

    #[test]
    fn a_truncated_flood_says_so() {
        // open field, goal far away, budget for a handful of tiles: the answer
        // is "as close as I looked", and the flag has to say that or the caller
        // reads a budget failure as an unreachable goal.
        let pass = |_: i32, _: i32| true;
        let mut f = Flood::new();
        let short = nearest_reachable_passable_grid(
            &mut f,
            &pass,
            V2::new(crate::fx!("5.5"), crate::fx!("5.5")),
            V2::new(crate::fx!("120.5"), crate::fx!("5.5")),
            64,
        )
        .unwrap();
        assert!(short.truncated);
        let full = nearest_reachable_passable_grid(
            &mut f,
            &pass,
            V2::new(crate::fx!("5.5"), crate::fx!("5.5")),
            V2::new(crate::fx!("120.5"), crate::fx!("5.5")),
            N_CELLS,
        )
        .unwrap();
        assert!(!full.truncated);
        assert_eq!(floor_i(full.at.x), 120);
        // and the retained buffers survive back-to-back searches
        assert!(crate::math::dist2(short.at, full.at) > crate::math::dist2(full.at, full.at));
    }

    #[test]
    fn reach_budget_grows_with_the_walk() {
        assert_eq!(reach_budget(crate::fx!("1")), reach_budget(crate::fx!("2")));
        assert!(reach_budget(crate::fx!("40")) > reach_budget(crate::fx!("10")));
        assert_eq!(reach_budget(crate::fx!("400")), reach_budget(crate::fx!("500")));
    }
}
