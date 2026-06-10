use super::garrison_cmds::eject_all;
use super::{building_occupancy, clamp_world, find_owned, owned_building_kinds, spawn};
use crate::components::*;
use crate::{PathScratch, SimRng, WorldConfig};
use bevy_ecs::prelude::*;
use saladin_sim::*;

fn pop_room(world: &mut World, owner: u64) -> bool {
    let mut bq = world.query::<(&Owner, &Building)>();
    let cap: i32 = bq.iter(world).filter(|(o, _)| o.0 == owner).map(|(_, b)| building_def(b.kind).pop).sum();
    let mut uq = world.query::<(&Owner, &Unit)>();
    let pop = uq.iter(world).filter(|(o, _)| o.0 == owner).count() as i32;
    pop < cap
}

/// Train `kind` from the owner's matching production building (afford + prereq +
/// pop checked). The spawn point is jittered beside the trainer and snapped to
/// passable ground; a rally point set away from the building sends the fresh
/// unit marching there. Mirrors the SpacetimeDB `trainFrom`.
pub(crate) fn train(world: &mut World, owner: u64, kind: UnitKind) -> bool {
    let def = unit_def(kind);
    let owned = owned_building_kinds(world, owner);
    if !has_prereq(&owned, def.requires) {
        return false;
    }
    // find a building that trains this kind, with its position + rally
    let trainer = {
        let mut q = world.query::<(&Owner, &Building, &Pos)>();
        q.iter(world)
            .find(|(o, b, _)| o.0 == owner && building_def(b.kind).trains.contains(&kind))
            .map(|(_, b, p)| (b.kind, b.rally, p.pos))
    };
    let Some((bkind, rally, bpos)) = trainer else { return false };
    if !pop_room(world, owner) {
        return false;
    }
    let (paid, match_id) = {
        let mut q = world.query::<(&mut Player, &MatchId)>();
        let Some((mut p, m)) = q.iter_mut(world).find(|(p, _)| p.player_id == owner) else { return false };
        if !p.stock.can_afford(&def.cost) {
            (false, 0)
        } else {
            p.stock.pay(&def.cost);
            (true, m.0)
        }
    };
    if !paid {
        return false;
    }
    // jittered spawn beside the building's south edge, snapped onto passable,
    // unoccupied ground so a hemmed-in trainer never strands its unit.
    let fp = building_def(bkind).footprint;
    let (jx, jy) = {
        let mut rng = world.resource_mut::<SimRng>();
        ((rng.0.next_fx() - saladin_sim::fx!("0.5")) * Fx::from_num(2), rng.0.next_fx())
    };
    let raw_x = clamp_world(bpos.x + jx);
    let raw_y = clamp_world(bpos.y + Fx::from_num(fp) / Fx::from_num(2) + saladin_sim::fx!("0.8") + jy);
    let seed = world.resource::<WorldConfig>().seed;
    let occ = building_occupancy(world, false);
    let passable = |tx: i32, ty: i32| is_passable(seed, tx, ty) && !occ.contains(&tile_key(tx, ty));
    let snap = nearest_passable_grid(&passable, raw_x, raw_y);
    let id = spawn::spawn_unit(world, owner, kind, snap, match_id, GatherState::Idle, 0);
    world.resource_mut::<crate::MatchStats>().of(owner).trained += 1;

    // march to the rally point when it was moved off the building
    if dist(rally, bpos) > saladin_sim::fx!("1.2") {
        let path = {
            let mut scratch = world.resource_mut::<PathScratch>();
            scratch.0.find_path(&passable, snap.x, snap.y, rally.x, rally.y, MAX_EXPANSIONS)
        };
        if !path.is_empty() {
            let mut q = world.query::<(&GameId, &mut Unit)>();
            if let Some((_, mut u)) = q.iter_mut(world).find(|(g, _)| g.0 == id) {
                u.target = path[0];
                u.path = path;
                u.path_idx = 0;
                u.has_target = true;
            }
        }
    }
    true
}

/// Tile keys occupied by resource nodes (no building on a tree/quarry/etc.).
pub(crate) fn node_occupancy(world: &mut World) -> std::collections::HashSet<i32> {
    let mut q = world.query::<(&Pos, &ResourceNode)>();
    q.iter(world)
        .map(|(p, _)| tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()))
        .collect()
}

/// Positions of the owner's standing buildings (town-radius anchor set).
pub(crate) fn own_building_positions(world: &mut World, owner: u64) -> Vec<V2> {
    let mut q = world.query::<(&Owner, &Pos, &Building)>();
    q.iter(world).filter(|(o, _, _)| o.0 == owner).map(|(_, p, _)| p.pos).collect()
}

/// Place `kind` at `pos` — the full `check_place` rule set (buildable biome,
/// node/building occupancy, waterside, town radius, approach) + prereq + cost.
/// `facing` = quarter turns; square footprints make rotation purely visual,
/// but it rides the command so every client renders the same yaw.
///
/// Defense composition: a gate or tower placed on the player's OWN wall tile
/// absorbs that segment (full refund) instead of being refused — walls are a
/// canvas the other defense pieces slot into. A gate dropped into a wall run
/// also auto-orients its passage across the run.
pub(crate) fn build(world: &mut World, owner: u64, kind: BuildingKind, pos: V2, facing: u8) -> bool {
    let def = building_def(kind);
    if !def.buildable {
        return false;
    }
    let owned = owned_building_kinds(world, owner);
    if !has_prereq(&owned, def.requires) {
        return false;
    }
    let seed = world.resource::<WorldConfig>().seed;
    let mut occ = building_occupancy(world, true);
    occ.extend(node_occupancy(world));
    // own wall segments are transparent to a composing piece
    let own_walls: Vec<(u64, i32)> = if composes_with_walls(kind) {
        let mut q = world.query::<(&GameId, &Pos, &Owner, &Building)>();
        q.iter(world)
            .filter(|(_, _, o, b)| o.0 == owner && b.kind == BuildingKind::Wall)
            .map(|(g, p, _, _)| (g.0, tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>())))
            .collect()
    } else {
        Vec::new()
    };
    for (_, k) in &own_walls {
        occ.remove(k);
    }
    let own = own_building_positions(world, owner);
    let occupied = |tx: i32, ty: i32| occ.contains(&tile_key(tx, ty));
    if check_place(seed, kind, pos.x, pos.y, occupied, &own).is_err() {
        return false;
    }
    let (paid, match_id) = {
        let mut q = world.query::<(&mut Player, &MatchId)>();
        let Some((mut p, m)) = q.iter_mut(world).find(|(p, _)| p.player_id == owner) else { return false };
        if !p.stock.can_afford(&def.cost) {
            (false, 0)
        } else {
            p.stock.pay(&def.cost);
            (true, m.0)
        }
    };
    if !paid {
        return false;
    }

    // absorb the overlapped segment: refund in full, pop any parapet garrison
    let fp: Vec<i32> =
        footprint_tiles(def.footprint, pos.x, pos.y).iter().map(|t| tile_key(t.tx, t.ty)).collect();
    let mut absorbed_run = (false, false); // own wall continues along (x, z)
    if !own_walls.is_empty() {
        let (tx, ty) = (pos.x.floor().to_num::<i32>(), pos.y.floor().to_num::<i32>());
        let wall_at = |dx: i32, dy: i32| own_walls.iter().any(|(_, k)| *k == tile_key(tx + dx, ty + dy));
        absorbed_run = (wall_at(1, 0) || wall_at(-1, 0), wall_at(0, 1) || wall_at(0, -1));
        let wall_cost = building_def(BuildingKind::Wall).cost;
        let absorbed: Vec<u64> =
            own_walls.iter().filter(|(_, k)| fp.contains(k)).map(|(id, _)| *id).collect();
        for wid in absorbed {
            super::garrison_cmds::eject_all(world, wid);
            if let Some(e) = super::find_owned(world, owner, wid) {
                world.despawn(e);
            }
            let mut q = world.query::<&mut Player>();
            if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
                p.stock.refund(&wall_cost, Fx::ONE);
            }
        }
    }

    let center = footprint_center(def.footprint, pos.x, pos.y);
    let id = spawn::spawn_building(world, owner, kind, center, match_id);
    // a gate in a clear X- or Z-run turns its passage across the run; the
    // player's chosen facing wins when the neighborhood is ambiguous
    let facing = if kind == BuildingKind::Gatehouse {
        match absorbed_run {
            (true, false) => 0,
            (false, true) => 1,
            _ => facing,
        }
    } else {
        facing
    };
    if facing % 4 != 0 {
        let yaw = saladin_sim::fx!("1.5707963") * Fx::from_num(facing % 4);
        let mut q = world.query::<(&GameId, &mut Pos)>();
        if let Some((_, mut p)) = q.iter_mut(world).find(|(g, _)| g.0 == id) {
            p.facing = yaw;
        }
    }
    true
}

/// Batched wall placement for a dragged line: places every affordable, valid
/// Wall tile and skips the rest silently. Occupancy is computed once and stamped
/// incrementally — O(line), not O(line × buildings). Mirrors `placeWall`.
pub(crate) fn place_wall(world: &mut World, owner: u64, tiles: &[(i32, i32)]) {
    let def = building_def(BuildingKind::Wall);
    let seed = world.resource::<WorldConfig>().seed;
    let mut occ = building_occupancy(world, true);
    occ.extend(node_occupancy(world));
    let mut own = own_building_positions(world, owner);
    let (mut bal, match_id) = {
        let mut q = world.query::<(&Player, &MatchId)>();
        let Some((p, m)) = q.iter(world).find(|(p, _)| p.player_id == owner) else { return };
        (p.stock, m.0)
    };
    let mut spent = false;
    for &(tx, ty) in tiles {
        if !bal.can_afford(&def.cost) {
            break;
        }
        let x = Fx::from_num(tx);
        let y = Fx::from_num(ty);
        let occupied = |px: i32, py: i32| occ.contains(&tile_key(px, py));
        if check_place(seed, BuildingKind::Wall, x, y, occupied, &own).is_err() {
            continue;
        }
        let c = footprint_center(def.footprint, x, y);
        spawn::spawn_building(world, owner, BuildingKind::Wall, c, match_id);
        // each placed segment extends the town anchor — drags can snake outward
        own.push(c);
        for k in occupancy_set(&[Occupant { kind: BuildingKind::Wall, pos: V2::new(x, y) }], true) {
            occ.insert(k);
        }
        bal.pay(&def.cost);
        spent = true;
    }
    if spent {
        let mut q = world.query::<&mut Player>();
        if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
            p.stock = bal;
        }
    }
}

/// Tear down an owned building (never the Keep): refund half its cost, pop any
/// sheltered units back to the field, then raze it. Mirrors `demolishBuilding`.
pub(crate) fn demolish(world: &mut World, owner: u64, building: u64) {
    let Some(e) = find_owned(world, owner, building) else { return };
    let Some(b) = world.get::<Building>(e) else { return };
    if b.kind == BuildingKind::Keep {
        return;
    }
    let def = building_def(b.kind);
    {
        let mut q = world.query::<&mut Player>();
        if let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) {
            p.stock.refund(&def.cost, saladin_sim::fx!("0.5"));
        }
    }
    eject_all(world, building);
    world.despawn(e);
}

/// Move a building's rally flag. Trained units march there on spawn.
pub(crate) fn set_rally(world: &mut World, owner: u64, building: u64, target: V2) {
    let Some(e) = find_owned(world, owner, building) else { return };
    if let Some(mut b) = world.get_mut::<Building>(e) {
        b.rally = V2::new(clamp_world(target.x), clamp_world(target.y));
    }
}
