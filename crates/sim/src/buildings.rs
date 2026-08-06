use crate::buildings_defs::{BuildingDef, building_def, res_bit};
use crate::constants::{
    BUILDER_RATE, DEMOLISH_REFUND_PCT, MAX_BUILDERS, REPAIR_COST_PCT, SITE_HP_PCT, TOWN_RADIUS,
    WORLD_SIZE,
};
use crate::economy::{ResourceCost, Stockpile};
use crate::enums::{BuildState, BuildingKind, ResourceType};
use crate::math::{Fx, V2, dist2};
use crate::tech::has_prereq_all;
use crate::terrain::{
    fertility_at, is_buildable_tile, is_passable, is_sailable, is_water_tile, main_water_body,
    water_region_at,
};
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

// ── the construction loop ───────────────────────────────────────────────────
// Building, repairing and upgrading are ONE loop: work advances the job, hp
// advances with it. hp is authoritative and ADDITIVE (work adds, damage
// subtracts) rather than derived from progress, so a site under fire needs no
// special case and a half-built hall is a real half-health target.

/// A `Site` does nothing at all; an `Upgrading` tower still fires, still counts
/// toward prereqs and still takes deposits. The single capability choke point.
pub fn operational(state: BuildState) -> bool {
    state != BuildState::Site
}

/// Work per second from `builders`, with diminishing returns.
pub fn build_rate(builders: i32) -> Fx {
    let n = builders.clamp(0, MAX_BUILDERS) as usize;
    Fx::from_num(BUILDER_RATE[n]) / Fx::from_num(100)
}

/// Fraction of a `build_time` job that `builders` finish in `dt` seconds.
pub fn work_step(builders: i32, dt: Fx, build_time: Fx) -> Fx {
    if build_time <= Fx::ZERO {
        return Fx::ONE;
    }
    build_rate(builders) * dt / build_time
}

/// Health a founded site starts at — frail enough to be worth raiding.
pub fn site_start_hp(max_hp: i32) -> i32 {
    (max_hp * SITE_HP_PCT / 100).max(1)
}

/// Health that `work_delta` of progress adds. Flooring loses a few hp over a
/// long build; the caller snaps hp to max_hp when the job completes.
pub fn hp_step(max_hp: i32, work_delta: Fx) -> i32 {
    if work_delta <= Fx::ZERO {
        return 0;
    }
    let span = (max_hp - site_start_hp(max_hp)).max(0);
    (Fx::from_num(span) * work_delta).floor().to_num::<i32>().max(1)
}

/// `cost * num / den`, floored per resource in integer math so two peers can
/// never disagree by a rounding bit.
fn scaled_cost(cost: &ResourceCost, num: i64, den: i64) -> ResourceCost {
    let f = |c: i32| {
        if den <= 0 || num <= 0 {
            return 0;
        }
        ((c.max(0) as i64 * num) / den).min(i32::MAX as i64) as i32
    };
    ResourceCost::new(f(cost.wood), f(cost.stone), f(cost.food), f(cost.gold))
}

/// What cancelling an unfinished site hands back: the labour not yet spent.
pub fn cancel_refund(cost: &ResourceCost, work: Fx) -> ResourceCost {
    let left = Fx::ONE - work.clamp(Fx::ZERO, Fx::ONE);
    let f = |c: i32| (Fx::from_num(c.max(0)) * left).floor().to_num::<i32>().max(0);
    ResourceCost::new(f(cost.wood), f(cost.stone), f(cost.food), f(cost.gold))
}

/// Demolition returns half the build cost SCALED BY HEALTH — a burnt-out shell
/// is worth what it looks like, so razing is never a way to launder damage.
pub fn demolish_refund(cost: &ResourceCost, hp: i32, max_hp: i32) -> ResourceCost {
    let m = max_hp.max(1) as i64;
    scaled_cost(cost, DEMOLISH_REFUND_PCT as i64 * hp.clamp(0, max_hp.max(1)) as i64, 100 * m)
}

/// What healing `hp_added` costs. Full repair from a wreck is REPAIR_COST_PCT
/// of the build cost, so it never exceeds building anew.
pub fn repair_charge(cost: &ResourceCost, hp_added: i32, max_hp: i32) -> ResourceCost {
    let m = max_hp.max(1) as i64;
    scaled_cost(cost, REPAIR_COST_PCT as i64 * hp_added.clamp(0, max_hp.max(1)) as i64, 100 * m)
}

/// The tile a waterside structure's hulls float on: the water tile orthogonally
/// bordering its footprint, ocean first and then lowest `tile_key`. Ocean first
/// so a hut wedged between a puddle and the sea berths its skiff on the sea;
/// lowest key after that so the answer is the same on every peer forever. A
/// landlocked footprint has none, which is what refuses a beached hull.
pub fn berth_of(seed: u32, footprint: i32, pos: V2) -> Option<V2> {
    let tiles = footprint_tiles(footprint, pos.x, pos.y);
    let inside: HashSet<i32> = tiles.iter().map(|t| tile_key(t.tx, t.ty)).collect();
    let ocean = main_water_body(seed);
    let mut best: Option<((bool, i32), i32, i32)> = None;
    for t in &tiles {
        for (dx, dy) in DIRS4 {
            let (nx, ny) = (t.tx + dx, t.ty + dy);
            let key = tile_key(nx, ny);
            if inside.contains(&key) || !is_sailable(seed, nx, ny) {
                continue;
            }
            let half = crate::fx!("0.5");
            let off_ocean =
                water_region_at(seed, Fx::from_num(nx) + half, Fx::from_num(ny) + half) != ocean;
            let rank = ((off_ocean, key), nx, ny);
            if best.is_none() || rank.0 < best.unwrap().0 {
                best = Some(rank);
            }
        }
    }
    let half = crate::fx!("0.5");
    best.map(|(_, tx, ty)| V2::new(Fx::from_num(tx) + half, Fx::from_num(ty) + half))
}

/// True when a structure's berth is on the map's main body of water — the ocean
/// every coast shares, and therefore the only water a barge can cross anywhere.
pub fn berth_is_seagoing(seed: u32, footprint: i32, pos: V2) -> bool {
    berth_of(seed, footprint, pos)
        .is_some_and(|b| water_region_at(seed, b.x, b.y) == main_water_body(seed))
}

/// True when a gatherer may deposit `res` here.
pub fn accepts(def: &BuildingDef, res: ResourceType) -> bool {
    def.accepts & res_bit(res) != 0
}

/// A gate is a door in YOUR line, not a breach in it: `gates` are (tile key,
/// owner) pairs and only the owner walks through. Gates are 1x1 and few, so a
/// linear scan beats allocating a per-owner map every pathing tick.
pub fn gate_blocks(gates: &[(i32, u64)], key: i32, viewer: u64) -> bool {
    gates.iter().any(|(k, owner)| *k == key && *owner != viewer)
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
    /// Waterside, but only onto a lake or a creek: no berth on the main sea.
    NeedsSeaBerth,
    /// Farther than TOWN_RADIUS from every building you own — towns grow
    /// outward, you cannot plant structures across the map.
    OutsideTown,
    /// Fully sealed footprint: no walkable tile borders it (peasants could
    /// never reach it to deposit or repair).
    NoApproach,
    /// Ground too poor to farm — nothing grows on rock, sand or salt.
    PoorSoil,
    /// Hillside too steep, or the ground under the footprint varies more than
    /// one foundation plane can absorb without walls clipping through it.
    TooSteep,
    /// Not on the build bar at all (the Keep; the Watchtower, which is an
    /// upgrade of a standing Tower).
    NotBuildable,
    /// A prerequisite structure is missing — carries the first one lacking.
    MissingPrereq(BuildingKind),
    /// Already at this structure's `max_count`.
    TooMany,
    /// The stockpile does not cover the cost.
    CannotAfford,
}

/// One ASCII line explaining a refusal, for the ghost and the command card.
pub fn place_error_text(e: PlaceError) -> String {
    match e {
        PlaceError::Terrain => "Cannot build here".into(),
        PlaceError::Occupied => "Blocked".into(),
        PlaceError::NeedsWaterside => "Needs a shoreline".into(),
        PlaceError::NeedsSeaBerth => "Needs a sea berth".into(),
        PlaceError::OutsideTown => "Outside your town".into(),
        PlaceError::NoApproach => "No way in".into(),
        PlaceError::PoorSoil => "Soil too poor to farm".into(),
        PlaceError::TooSteep => "Ground too steep".into(),
        PlaceError::NotBuildable => "Cannot be built".into(),
        PlaceError::MissingPrereq(k) => format!("Requires {}", building_def(k).label),
        PlaceError::TooMany => "Already built".into(),
        PlaceError::CannotAfford => "Cannot afford".into(),
    }
}

/// Steepest ground a foundation may sit on. A third of the cliff threshold, so
/// there is a real band that is walkable but not buildable.
pub const BUILD_SLOPE_MAX: Fx = crate::fx!("0.35");
/// How much the ground may rise across a whole footprint. A building has ONE
/// foundation plane; past this the walls stand in mid-air on the low side.
pub const FOUNDATION_RELIEF: Fx = crate::fx!("0.60");

/// Rise from the lowest to the highest point of the meshed surface under a
/// footprint — what a single foundation plane has to hide.
pub fn footprint_relief(seed: u32, footprint: i32, x: Fx, y: Fx) -> Fx {
    let g = crate::worldgrid::world_grid(seed);
    let gain = crate::terrain::seed_bias(seed).elev_gain;
    let half = crate::fx!("0.5");
    let (mut lo, mut hi) = (Fx::MAX, Fx::MIN);
    for t in footprint_tiles(footprint, x, y) {
        let i = crate::worldgrid::tile_index(Fx::from_num(t.tx) + half, Fx::from_num(t.ty) + half);
        let s = crate::terrain::surface_height(g.tile_h[i], gain);
        lo = lo.min(s);
        hi = hi.max(s);
    }
    if hi < lo { Fx::ZERO } else { hi - lo }
}

/// Mean soil fertility under a footprint — a field is only as good as the
/// worst of the ground it covers, so this averages rather than samples.
pub fn soil_quality(seed: u32, footprint: i32, x: Fx, y: Fx) -> Fx {
    let tiles = footprint_tiles(footprint, x, y);
    if tiles.is_empty() {
        return Fx::ZERO;
    }
    let half = crate::fx!("0.5");
    let sum = tiles.iter().fold(Fx::ZERO, |acc, t| {
        acc + fertility_at(seed, Fx::from_num(t.tx) + half, Fx::from_num(t.ty) + half)
    });
    sum / Fx::from_num(tiles.len() as i32)
}

/// Every tile of a footprint must be buildable ground, clear of other work and
/// no steeper than a foundation can sit on.
fn check_ground<O: Fn(i32, i32) -> bool>(
    seed: u32,
    tiles: &[Tile],
    occupied: O,
) -> Result<(), PlaceError> {
    let half = crate::fx!("0.5");
    for t in tiles {
        if !is_buildable_tile(seed, t.tx, t.ty) {
            return Err(PlaceError::Terrain);
        }
        if occupied(t.tx, t.ty) {
            return Err(PlaceError::Occupied);
        }
        if crate::terrain::slope_at(seed, Fx::from_num(t.tx) + half, Fx::from_num(t.ty) + half)
            > BUILD_SLOPE_MAX
        {
            return Err(PlaceError::TooSteep);
        }
    }
    Ok(())
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
    check_ground(seed, &tiles, occupied)?;
    if footprint_relief(seed, def.footprint, x, y) > FOUNDATION_RELIEF {
        return Err(PlaceError::TooSteep);
    }
    if def.min_fertility > Fx::ZERO && soil_quality(seed, def.footprint, x, y) < def.min_fertility {
        return Err(PlaceError::PoorSoil);
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
    if def.needs_sea_berth
        && !berth_is_seagoing(seed, def.footprint, footprint_center(def.footprint, x, y))
    {
        return Err(PlaceError::NeedsSeaBerth);
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

/// `check_place` plus the rules that used to live in the build COMMAND —
/// buildability, the full prereq set, the per-kind limit and affordability. The
/// ghost, the command and the AI all ask this, so a preview can never turn
/// green on a placement the command will refuse.
#[allow(clippy::too_many_arguments)]
pub fn check_build<O: Fn(i32, i32) -> bool>(
    seed: u32,
    kind: BuildingKind,
    x: Fx,
    y: Fx,
    occupied: O,
    own_buildings: &[V2],
    owned_kinds: &HashSet<BuildingKind>,
    own_counts: &[i32],
    stock: &Stockpile,
) -> Result<(), PlaceError> {
    let def = building_def(kind);
    if !def.buildable {
        return Err(PlaceError::NotBuildable);
    }
    if let Some(missing) = has_prereq_all(owned_kinds, def) {
        return Err(PlaceError::MissingPrereq(missing));
    }
    if def.max_count > 0 && own_counts.get(kind as usize).copied().unwrap_or(0) >= def.max_count {
        return Err(PlaceError::TooMany);
    }
    check_place(seed, kind, x, y, occupied, own_buildings)?;
    if !stock.can_afford(&def.cost) {
        return Err(PlaceError::CannotAfford);
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
    fn check_ground_refuses_water_and_other_peoples_walls() {
        let seed = 1;
        let site = crate::terrain::find_keep_site(seed, 0, 2);
        let tiles = footprint_tiles(2, site.x, site.y);
        assert_eq!(check_ground(seed, &tiles, |_, _| false), Ok(()));
        assert_eq!(check_ground(seed, &tiles, |_, _| true), Err(PlaceError::Occupied));
        // out in the ocean there is no ground at all
        let sea = footprint_tiles(2, crate::fx!("1"), crate::fx!("1"));
        assert_eq!(check_ground(seed, &sea, |_, _| false), Err(PlaceError::Terrain));
    }

    #[test]
    fn builders_help_less_the_more_of_them_there_are() {
        let mut last = build_rate(0);
        for n in 1..=MAX_BUILDERS {
            let r = build_rate(n);
            assert!(r > last, "{n} builders must beat {}", n - 1);
            assert!(
                r - last <= build_rate(n - 1) - build_rate((n - 2).max(0)) || n <= 1,
                "returns must diminish at {n}"
            );
            last = r;
        }
        assert_eq!(build_rate(MAX_BUILDERS + 40), build_rate(MAX_BUILDERS), "saturates");
        assert_eq!(build_rate(-3), build_rate(0));
    }

    #[test]
    fn work_and_hp_advance_together() {
        let t = crate::fx!("25");
        // one builder finishes a 25 s job in 25 s of steps (fixed-point leaves
        // the last step short of exactly 1, so it tops out on the next one)
        let step = work_step(1, crate::fx!("0.2"), t);
        assert!(step * Fx::from_num(124) < Fx::ONE);
        assert!(step * Fx::from_num(126) >= Fx::ONE);
        assert!(work_step(4, crate::fx!("0.2"), t) > step * Fx::from_num(2), "a crew is faster");
        // no build time == stands on the spot
        assert_eq!(work_step(0, crate::fx!("0.2"), Fx::ZERO), Fx::ONE);
        // a site is frail but real, and the build tops it up to full
        assert_eq!(site_start_hp(1500), 150);
        assert_eq!(site_start_hp(4), 1, "even the frailest site has a hit point");
        assert_eq!(hp_step(1000, Fx::ONE), 900);
        assert_eq!(hp_step(1000, Fx::ZERO), 0);
        assert!(hp_step(1000, crate::fx!("0.0001")) >= 1, "progress is always visible");
    }

    #[test]
    fn refunds_never_pay_more_than_was_spent() {
        let cost = crate::economy::ResourceCost::new(70, 20, 0, 0);
        // cancel returns the unspent remainder
        assert_eq!(cancel_refund(&cost, Fx::ZERO), cost);
        assert_eq!(cancel_refund(&cost, Fx::ONE), crate::economy::ResourceCost::ZERO);
        let half = cancel_refund(&cost, crate::fx!("0.5"));
        assert_eq!((half.wood, half.stone), (35, 10));
        // demolish scales with what is left standing
        assert_eq!(demolish_refund(&cost, 500, 500).wood, 35);
        assert_eq!(demolish_refund(&cost, 1, 500).wood, 0, "a shell is worth a shell");
        assert_eq!(demolish_refund(&cost, 0, 500), crate::economy::ResourceCost::ZERO);
        // repair never costs more than the building did
        for hp in 0..=500 {
            let c = repair_charge(&cost, hp, 500);
            assert!(c.wood <= cost.wood && c.stone <= cost.stone, "repair overcharged at {hp}");
        }
        assert_eq!(repair_charge(&cost, 500, 500).wood, 35);
    }

    #[test]
    fn a_gate_is_a_door_not_a_breach() {
        let gates = [(tile_key(10, 10), 7u64)];
        assert!(!gate_blocks(&gates, tile_key(10, 10), 7), "the owner walks through");
        assert!(gate_blocks(&gates, tile_key(10, 10), 9), "the enemy does not");
        assert!(!gate_blocks(&gates, tile_key(11, 10), 9), "and only on its own tile");
    }

    #[test]
    fn a_site_does_nothing_until_it_is_finished() {
        assert!(!operational(BuildState::Site));
        assert!(operational(BuildState::Complete));
        assert!(operational(BuildState::Upgrading), "an upgrading tower still fires");
    }

    #[test]
    fn check_build_refuses_what_the_command_would_refuse() {
        let seed = 1;
        let site = crate::terrain::find_keep_site(seed, 0, 2);
        let free = |_: i32, _: i32| false;
        let rich = Stockpile { wood: 999, stone: 999, food: 999, gold: 999 };
        let broke = Stockpile::default();
        let mut owned: HashSet<BuildingKind> = HashSet::new();
        owned.insert(BuildingKind::Keep);
        let go = |kind, owned: &HashSet<BuildingKind>, counts: &[i32], stock: &Stockpile| {
            check_build(seed, kind, site.x, site.y, free, &[], owned, counts, stock)
        };

        assert_eq!(
            go(BuildingKind::Keep, &owned, &[], &rich),
            Err(PlaceError::NotBuildable),
            "the keep is not on the bar"
        );
        assert_eq!(
            go(BuildingKind::Stable, &owned, &[], &rich),
            Err(PlaceError::MissingPrereq(BuildingKind::Barracks))
        );
        // a house needs nothing but ground and coin
        assert_eq!(go(BuildingKind::House, &owned, &[], &rich), Ok(()));
        assert_eq!(go(BuildingKind::House, &owned, &[], &broke), Err(PlaceError::CannotAfford));
    }

    #[test]
    fn every_refusal_reaches_the_player_as_ascii() {
        let all = [
            PlaceError::Terrain,
            PlaceError::Occupied,
            PlaceError::NeedsWaterside,
            PlaceError::NeedsSeaBerth,
            PlaceError::OutsideTown,
            PlaceError::NoApproach,
            PlaceError::PoorSoil,
            PlaceError::TooSteep,
            PlaceError::NotBuildable,
            PlaceError::MissingPrereq(BuildingKind::Barracks),
            PlaceError::TooMany,
            PlaceError::CannotAfford,
        ];
        for e in all {
            let t = place_error_text(e);
            assert!(t.is_ascii() && !t.is_empty(), "{e:?} -> {t:?}");
        }
    }

    /// A berth is a place, not a search: the same footprint has to hand back the
    /// same tile forever, and a hut in the middle of a field has none at all.
    #[test]
    fn a_berth_is_the_same_tile_every_time_and_only_beside_water() {
        let seed = 1;
        let mut found = 0;
        for slot in 0..8 {
            let start = crate::terrain::start_point(seed, slot);
            // sweep the start's quarter for a shoreline tile a hut could sit on
            let (mut shore, mut sx, mut sy) = (None, 0, 0);
            for r in 1..60i32 {
                if shore.is_some() {
                    break;
                }
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dy.abs()) != r {
                            continue;
                        }
                        let (tx, ty) =
                            (start.x.to_num::<i32>() + dx, start.y.to_num::<i32>() + dy);
                        let c = footprint_center(1, Fx::from_num(tx), Fx::from_num(ty));
                        if is_buildable_tile(seed, tx, ty) && berth_of(seed, 1, c).is_some() {
                            shore = Some(c);
                            (sx, sy) = (tx, ty);
                            break;
                        }
                    }
                    if shore.is_some() {
                        break;
                    }
                }
            }
            let Some(c) = shore else { continue };
            found += 1;
            let first = berth_of(seed, 1, c).unwrap();
            for _ in 0..100 {
                assert_eq!(berth_of(seed, 1, c), Some(first), "berth wandered at {sx},{sy}");
            }
            let (bx, by) = (first.x.to_num::<i32>(), first.y.to_num::<i32>());
            assert!(is_sailable(seed, bx, by), "berth is on dry land");
            assert!(!is_passable(seed, bx, by), "a hull cannot berth where a man walks");
            assert_eq!((bx - sx).abs() + (by - sy).abs(), 1, "berth is not beside the hut");
        }
        assert!(found > 0, "no start on seed {seed} has a shoreline at all");
        // deep inland there is no berth, and that is what refuses a beached hull
        let dry = footprint_center(1, Fx::from_num(WORLD_SIZE / 2), Fx::from_num(WORLD_SIZE / 2));
        let inland = (0..WORLD_SIZE)
            .flat_map(|ty| (0..WORLD_SIZE).map(move |tx| (tx, ty)))
            .find(|(tx, ty)| {
                is_passable(seed, *tx, *ty)
                    && berth_of(seed, 1, footprint_center(1, Fx::from_num(*tx), Fx::from_num(*ty)))
                        .is_none()
            });
        assert!(inland.is_some(), "every tile on the map touches water? {dry:?}");
    }

    /// The hut and the harbour ask DIFFERENT questions of the same shoreline —
    /// which is the whole reason a naval base is a decision.
    /// The hut and the harbour ask DIFFERENT questions of the same shoreline —
    /// which is the whole reason a naval base is a decision and not a formality.
    /// The Storehouse is the control: same 2x2 ground rules, no water rule at
    /// all, so a site it accepts isolates the berth clause exactly.
    #[test]
    fn a_lake_takes_a_hut_and_refuses_a_harbour() {
        let free = |_: i32, _: i32| false;
        let (mut huts, mut quays) = (0, 0);
        for seed in 1..40u32 {
            let ocean = crate::terrain::main_water_body(seed);
            let off_ocean = |fp: i32, c: V2| {
                berth_of(seed, fp, c).is_some_and(|b| water_region_at(seed, b.x, b.y) != ocean)
            };
            for ty in 0..WORLD_SIZE {
                for tx in 0..WORLD_SIZE {
                    if !is_buildable_tile(seed, tx, ty) {
                        continue;
                    }
                    let (x, y) = (Fx::from_num(tx), Fx::from_num(ty));
                    if huts < 3 && off_ocean(1, footprint_center(1, x, y)) {
                        // the hut only wants a shoreline, and a lake is one
                        if check_place(seed, BuildingKind::FishingHut, x, y, free, &[]) == Ok(()) {
                            huts += 1;
                        }
                    }
                    if quays < 3
                        && off_ocean(2, footprint_center(2, x, y))
                        && check_place(seed, BuildingKind::Storehouse, x, y, free, &[]) == Ok(())
                    {
                        assert_eq!(
                            check_place(seed, BuildingKind::Harbour, x, y, free, &[]),
                            Err(PlaceError::NeedsSeaBerth),
                            "seed {seed} sited a harbour on a lake at {tx},{ty}"
                        );
                        quays += 1;
                    }
                }
            }
            if huts >= 3 && quays >= 3 {
                break;
            }
        }
        assert!(huts > 0, "no seed in 1..40 accepted a hut on fresh water");
        assert!(quays > 0, "no seed in 1..40 grew a lake big enough to test a harbour against");
    }

    /// And the ocean shore takes both — otherwise the rule above would pass by
    /// refusing every harbour everywhere.
    #[test]
    fn the_ocean_shore_takes_a_harbour() {
        let free = |_: i32, _: i32| false;
        let mut sited = 0;
        for seed in 1..12u32 {
            let ocean = crate::terrain::main_water_body(seed);
            'seed: for ty in 0..WORLD_SIZE {
                for tx in 0..WORLD_SIZE {
                    let (x, y) = (Fx::from_num(tx), Fx::from_num(ty));
                    let c = footprint_center(2, x, y);
                    let on_ocean =
                        berth_of(seed, 2, c).is_some_and(|b| water_region_at(seed, b.x, b.y) == ocean);
                    if !on_ocean || check_place(seed, BuildingKind::Storehouse, x, y, free, &[]) != Ok(())
                    {
                        continue;
                    }
                    assert_eq!(
                        check_place(seed, BuildingKind::Harbour, x, y, free, &[]),
                        Ok(()),
                        "seed {seed} refused a harbour on the open coast at {tx},{ty}"
                    );
                    sited += 1;
                    break 'seed;
                }
            }
        }
        assert!(sited >= 10, "only {sited} of 11 seeds had a legal harbour site anywhere");
    }

    #[test]
    fn find_buildable_falls_back_to_passable_spot() {
        // passable everywhere -> origin is fine
        let pass = |_: i32, _: i32| true;
        let c = find_buildable_near(crate::fx!("30"), crate::fx!("30"), 3, pass);
        assert_eq!(c, footprint_center(3, crate::fx!("30"), crate::fx!("30")));
    }
}
