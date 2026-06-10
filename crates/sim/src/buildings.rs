use crate::buildings_defs::building_def;
use crate::constants::{TOWN_RADIUS, WORLD_SIZE};
use crate::enums::BuildingKind;
use crate::math::{Fx, V2, dist2};
use crate::terrain::{is_buildable_tile, is_passable, is_water_tile};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    pub tx: i32,
    pub ty: i32,
}

pub fn tile_key(tx: i32, ty: i32) -> i32 {
    ty * WORLD_SIZE + tx
}

const DIRS4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Defense pieces that COMPOSE with a wall line: placing one on a tile your
/// own Wall occupies absorbs the segment (full refund) instead of refusing
/// with Occupied — gates and towers slot into the wall, AoE-style. All are
/// 1x1, so a placement absorbs at most one segment.
pub fn composes_with_walls(kind: BuildingKind) -> bool {
    matches!(kind, BuildingKind::Gatehouse | BuildingKind::Tower | BuildingKind::Watchtower)
}

fn floor_i32(v: Fx) -> i32 {
    v.floor().to_num::<i32>()
}

/// Integer tiles a footprint-`f` building covers when placed near (x, y).
pub fn footprint_tiles(footprint: i32, x: Fx, y: Fx) -> Vec<Tile> {
    let cx = floor_i32(x);
    let cy = floor_i32(y);
    let r = footprint / 2;
    let mut tiles = Vec::with_capacity((footprint * footprint) as usize);
    for i in 0..footprint {
        for j in 0..footprint {
            tiles.push(Tile { tx: cx - r + i, ty: cy - r + j });
        }
    }
    tiles
}

#[derive(Clone, Copy, Debug)]
pub struct Occupant {
    pub kind: BuildingKind,
    pub pos: V2,
}

/// Tile keys covered by a set of buildings. `include_passable=false` omits
/// passable buildings (gatehouse) so units path through; true counts every
/// footprint (placement: no stacking).
pub fn occupancy_set(items: &[Occupant], include_passable: bool) -> HashSet<i32> {
    let mut s = HashSet::new();
    for it in items {
        let def = building_def(it.kind);
        if !include_passable && def.passable {
            continue;
        }
        for t in footprint_tiles(def.footprint, it.pos.x, it.pos.y) {
            s.insert(tile_key(t.tx, t.ty));
        }
    }
    s
}

/// World-space centre of the footprint (where the building model sits).
pub fn footprint_center(footprint: i32, x: Fx, y: Fx) -> V2 {
    let cx = floor_i32(x);
    let cy = floor_i32(y);
    let r = footprint / 2;
    let off = Fx::from_num(-r) + Fx::from_num(footprint - 1) / Fx::from_num(2) + crate::fx!("0.5");
    V2::new(Fx::from_num(cx) + off, Fx::from_num(cy) + off)
}

/// True when at least one tile orthogonally bordering the footprint is passable
/// — a gatherer can stand beside the building to deposit.
pub fn has_passable_approach<P: Fn(i32, i32) -> bool>(footprint: i32, x: Fx, y: Fx, passable: P) -> bool {
    let tiles = footprint_tiles(footprint, x, y);
    let inside: HashSet<i32> = tiles.iter().map(|t| tile_key(t.tx, t.ty)).collect();
    for t in &tiles {
        for (dx, dy) in DIRS4 {
            let (nx, ny) = (t.tx + dx, t.ty + dy);
            if inside.contains(&tile_key(nx, ny)) {
                continue;
            }
            if passable(nx, ny) {
                return true;
            }
        }
    }
    false
}

/// True if any tile orthogonally bordering the footprint is impassable (shore).
/// Gates water-adjacent buildings (FishingHut).
pub fn is_water_adjacent<P: Fn(i32, i32) -> bool>(footprint: i32, x: Fx, y: Fx, passable: P) -> bool {
    let tiles = footprint_tiles(footprint, x, y);
    let inside: HashSet<i32> = tiles.iter().map(|t| tile_key(t.tx, t.ty)).collect();
    for t in &tiles {
        for (dx, dy) in DIRS4 {
            let (nx, ny) = (t.tx + dx, t.ty + dy);
            if inside.contains(&tile_key(nx, ny)) {
                continue;
            }
            if !passable(nx, ny) {
                return true;
            }
        }
    }
    false
}

/// Placeable if every footprint tile is passable and unoccupied (terrain-only
/// core; the full game rule set is `check_place`).
pub fn can_place<P, O>(kind: BuildingKind, x: Fx, y: Fx, passable: P, occupied: O) -> bool
where
    P: Fn(i32, i32) -> bool,
    O: Fn(i32, i32) -> bool,
{
    let f = building_def(kind).footprint;
    for t in footprint_tiles(f, x, y) {
        if !passable(t.tx, t.ty) || occupied(t.tx, t.ty) {
            return false;
        }
    }
    true
}

/// Why a placement was refused — the ghost tints red for any of these and the
/// build command rejects identically (one rule set, no UI lies).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceError {
    /// A footprint tile is water/mountain/cliff/ford or out of bounds.
    Terrain,
    /// A footprint tile is covered by a building or a resource node.
    Occupied,
    /// `requires_water` building with no open water (sea/river) on its border.
    NeedsWaterside,
    /// Farther than TOWN_RADIUS from every building you own — towns grow
    /// outward, you cannot plant structures across the map.
    OutsideTown,
    /// Fully sealed footprint: no walkable tile borders it (peasants could
    /// never reach it to deposit or repair).
    NoApproach,
}

/// The COMPLETE placement rule set, shared by the build command, the wall
/// drag, the AI planner and the client's ghost preview.
pub fn check_place<O: Fn(i32, i32) -> bool>(
    seed: u32,
    kind: BuildingKind,
    x: Fx,
    y: Fx,
    occupied: O,
    own_buildings: &[V2],
) -> Result<(), PlaceError> {
    let def = building_def(kind);
    let tiles = footprint_tiles(def.footprint, x, y);
    for t in &tiles {
        if !is_buildable_tile(seed, t.tx, t.ty) {
            return Err(PlaceError::Terrain);
        }
        if occupied(t.tx, t.ty) {
            return Err(PlaceError::Occupied);
        }
    }
    if def.requires_water {
        let waterside = {
            let inside: HashSet<i32> = tiles.iter().map(|t| tile_key(t.tx, t.ty)).collect();
            tiles.iter().any(|t| {
                DIRS4.iter().any(|(dx, dy)| {
                    let (nx, ny) = (t.tx + dx, t.ty + dy);
                    !inside.contains(&tile_key(nx, ny)) && is_water_tile(seed, nx, ny)
                })
            })
        };
        if !waterside {
            return Err(PlaceError::NeedsWaterside);
        }
    }
    if !own_buildings.is_empty() {
        let c = footprint_center(def.footprint, x, y);
        let r2 = TOWN_RADIUS * TOWN_RADIUS;
        if !own_buildings.iter().any(|b| dist2(c, *b) <= r2) {
            return Err(PlaceError::OutsideTown);
        }
    }
    if !def.passable && !has_passable_approach(def.footprint, x, y, |tx, ty| is_passable(seed, tx, ty)) {
        return Err(PlaceError::NoApproach);
    }
    Ok(())
}

/// Nearest spot where the WHOLE footprint sits on passable land AND has a
/// passable approach beside it. Deterministic integer ring scan outward (the TS
/// version used cos/sin — replaced for determinism, parity not required).
pub fn find_buildable_near<P: Fn(i32, i32) -> bool>(x: Fx, y: Fx, footprint: i32, passable: P) -> V2 {
    let fits = |c: V2| footprint_tiles(footprint, c.x, c.y).iter().all(|t| passable(t.tx, t.ty));
    let good = |c: V2| fits(c) && has_passable_approach(footprint, c.x, c.y, &passable);

    let origin = V2::new(x, y);
    if good(origin) {
        return footprint_center(footprint, x, y);
    }
    let mut first_fit: Option<V2> = None;
    for r in 1..WORLD_SIZE {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue; // Chebyshev ring only
                }
                let c = V2::new(x + Fx::from_num(dx), y + Fx::from_num(dy));
                if good(c) {
                    return footprint_center(footprint, c.x, c.y);
                }
                if first_fit.is_none() && fits(c) {
                    first_fit = Some(footprint_center(footprint, c.x, c.y));
                }
            }
        }
    }
    first_fit.unwrap_or_else(|| footprint_center(footprint, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_tiles_count_and_center() {
        let t = footprint_tiles(3, crate::fx!("10"), crate::fx!("10"));
        assert_eq!(t.len(), 9);
        // 3x3 centered: cx-1..cx+1
        assert!(t.contains(&Tile { tx: 9, ty: 9 }));
        assert!(t.contains(&Tile { tx: 11, ty: 11 }));
    }

    #[test]
    fn occupancy_skips_passable_when_pathing() {
        let gate = Occupant { kind: BuildingKind::Gatehouse, pos: V2::new(crate::fx!("5"), crate::fx!("5")) };
        assert!(occupancy_set(&[gate], false).is_empty()); // passable -> walkable
        assert!(!occupancy_set(&[gate], true).is_empty()); // placement -> blocks
    }

    #[test]
    fn can_place_respects_passable_and_occupied() {
        let pass = |_: i32, _: i32| true;
        let none = |_: i32, _: i32| false;
        assert!(can_place(BuildingKind::House, crate::fx!("20"), crate::fx!("20"), pass, none));
        let water = |x: i32, _: i32| x < 19; // a wall of water at x>=19
        assert!(!can_place(BuildingKind::House, crate::fx!("20"), crate::fx!("20"), water, none));
    }

    #[test]
    fn find_buildable_falls_back_to_passable_spot() {
        // passable everywhere -> origin is fine
        let pass = |_: i32, _: i32| true;
        let c = find_buildable_near(crate::fx!("30"), crate::fx!("30"), 3, pass);
        assert_eq!(c, footprint_center(3, crate::fx!("30"), crate::fx!("30")));
    }
}
