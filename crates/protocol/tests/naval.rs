//! The roster half of the naval system, in a real world: a hull launches from
//! its hall's berth and nowhere else, a harbour wants the open sea rather than
//! any old puddle, and two worlds that put hulls in the water agree on every
//! tick.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, PlaceError, Stockpile, UnitKind, V2, WORLD_SIZE, ZERO, berth_of,
    building_def, check_place, fx, is_buildable_tile, is_passable, is_sailable, main_water_body,
    start_regions, unit_def, water_region_at,
};

fn build_app(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 9000, stone: 9000, food: 9000, gold: 9000 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
}

fn spawn_building(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"))
}

fn units_of(app: &mut App, kind: UnitKind) -> Vec<V2> {
    let world = app.world_mut();
    let mut q = world.query::<(&Unit, &Pos)>();
    q.iter(world).filter(|(u, _)| u.kind == kind).map(|(_, p)| p.pos).collect()
}

/// A tile a Fishing Hut can legally stand on whose berth is on the MAIN ocean,
/// with room for an anchor keep behind it.
fn sea_shore_tile(seed: u32) -> (i32, i32) {
    let free = |_: i32, _: i32| false;
    let ocean = main_water_body(seed);
    for ty in 10..WORLD_SIZE - 10 {
        for tx in 10..WORLD_SIZE - 10 {
            if !is_buildable_tile(seed, tx, ty) {
                continue;
            }
            let c = center(tx, ty);
            let seagoing =
                berth_of(seed, 1, c).is_some_and(|b| water_region_at(seed, b.x, b.y) == ocean);
            if !seagoing {
                continue;
            }
            if check_place(seed, BuildingKind::FishingHut, c.x, c.y, free, &[]) != Ok(()) {
                continue;
            }
            // room behind for the keep that anchors the town radius
            let anchored =
                (-4..-1).all(|dx| (-4..-1).all(|dy| is_buildable_tile(seed, tx + dx, ty + dy)));
            if anchored {
                return (tx, ty);
            }
        }
    }
    panic!("no sea shore on seed {seed}");
}

/// Found a player with a keep and a finished Fishing Hut on the open coast.
/// Returns (hut id, hut position).
fn found_coastal_town(app: &mut App, seed: u32) -> (u64, V2) {
    let (sx, sy) = sea_shore_tile(seed);
    spawn_player(app, 1);
    spawn_building(app, 10, 1, BuildingKind::Keep, center(sx - 3, sy - 3));
    let hut = center(sx, sy);
    spawn_building(app, 20, 1, BuildingKind::FishingHut, hut);
    (20, hut)
}

#[test]
fn a_skiff_launches_from_its_huts_berth_and_never_onto_the_beach() {
    let seed = 1;
    let mut app = build_app(seed);
    let (hut_id, hut) = found_coastal_town(&mut app, seed);
    let berth = berth_of(seed, 1, hut).expect("a coastal hut has a berth");

    cmd(&mut app, PlayerCommand::TrainAt { player_id: 1, building: hut_id, kind: UnitKind::FishingSkiff });
    for _ in 0..300 {
        step(app.world_mut());
    }

    let skiffs = units_of(&mut app, UnitKind::FishingSkiff);
    assert_eq!(skiffs.len(), 1, "the hut launched {} skiffs", skiffs.len());
    let p = skiffs[0];
    let (tx, ty) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
    assert!(is_sailable(seed, tx, ty), "the skiff spawned on dry land at {tx},{ty}");
    assert!(!is_passable(seed, tx, ty), "the skiff spawned where a man can walk");
    assert_eq!(p, berth, "the skiff did not launch from its hall's berth");
    assert_eq!(
        water_region_at(seed, p.x, p.y),
        water_region_at(seed, berth.x, berth.y),
        "the skiff launched into a different body of water than its berth"
    );
}

/// A hall with no water at all must not beach its hull — it must refuse the
/// order and hand the wood back, because a queue that cannot spawn jams forever.
#[test]
fn a_landlocked_hall_refuses_the_order_instead_of_beaching_a_hull() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    // an inland block, far from any shoreline
    let (cx, cy) = {
        let mut found = None;
        'scan: for ty in 20..WORLD_SIZE - 20 {
            for tx in 20..WORLD_SIZE - 20 {
                let dry_around = (-8..9).all(|dx| {
                    (-8..9).all(|dy| !saladin_sim::is_water_tile(seed, tx + dx, ty + dy))
                });
                if dry_around && (0..3).all(|dx| (0..3).all(|dy| is_buildable_tile(seed, tx + dx, ty + dy)))
                {
                    found = Some((tx, ty));
                    break 'scan;
                }
            }
        }
        found.expect("an inland block")
    };
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));
    // a harness hut on dry ground: the placement rules would refuse it, which is
    // exactly why this case has to be handled rather than assumed away
    spawn_building(&mut app, 20, 1, BuildingKind::FishingHut, center(cx, cy));

    let before = {
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        q.iter(world).find(|p| p.player_id == 1).unwrap().stock.wood
    };
    cmd(&mut app, PlayerCommand::TrainAt { player_id: 1, building: 20, kind: UnitKind::FishingSkiff });
    for _ in 0..300 {
        step(app.world_mut());
    }
    assert!(units_of(&mut app, UnitKind::FishingSkiff).is_empty(), "a hull was beached inland");
    let (after, queued) = {
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        let w = q.iter(world).find(|p| p.player_id == 1).unwrap().stock.wood;
        let mut qb = world.query::<(&GameId, &Building)>();
        let n = qb.iter(world).find(|(g, _)| g.0 == 20).unwrap().1.queue_len;
        (w, n)
    };
    assert_eq!(after, before, "the refused order kept the wood");
    assert_eq!(queued, 0, "the refused order jammed the queue");
}

#[test]
fn the_harbour_wants_a_hut_and_the_open_sea() {
    let seed = 1;
    let mut app = build_app(seed);
    let (_, hut) = found_coastal_town(&mut app, seed);
    let count = |app: &mut App| {
        let world = app.world_mut();
        let mut q = world.query::<&Building>();
        q.iter(world).filter(|b| b.kind == BuildingKind::Harbour).count()
    };

    // a legal 2x2 sea berth beside the hut
    let (hx, hy) = (hut.x.to_num::<i32>(), hut.y.to_num::<i32>());
    let free = |_: i32, _: i32| false;
    let ocean = main_water_body(seed);
    let mut site = None;
    'scan: for r in 1..14i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let (x, y) = (Fx::from_num(hx + dx), Fx::from_num(hy + dy));
                let c = saladin_sim::footprint_center(2, x, y);
                let seagoing =
                    berth_of(seed, 2, c).is_some_and(|b| water_region_at(seed, b.x, b.y) == ocean);
                if seagoing && check_place(seed, BuildingKind::Harbour, x, y, free, &[]) == Ok(()) {
                    site = Some((x, y));
                    break 'scan;
                }
            }
        }
    }
    let (x, y) = site.expect("a sea-berthed 2x2 site beside a coastal hut");

    // the same ground, without the hut that unlocks it
    let mut bare = build_app(seed);
    spawn_player(&mut bare, 1);
    spawn_building(&mut bare, 10, 1, BuildingKind::Keep, center(hx - 3, hy - 3));
    cmd(&mut bare, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Harbour, pos: V2::new(x, y), facing: 0, builders: vec![] });
    step(bare.world_mut());
    assert_eq!(count(&mut bare), 0, "a harbour rose without a fishing hut");

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Harbour, pos: V2::new(x, y), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(count(&mut app), 1, "a harbour on the open coast beside a hut");

    // the same rule set, one clause at a time: shoreline is not enough
    let mut lake = None;
    'lake: for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            let (x, y) = (Fx::from_num(tx), Fx::from_num(ty));
            let c = saladin_sim::footprint_center(2, x, y);
            let off_ocean =
                berth_of(seed, 2, c).is_some_and(|b| water_region_at(seed, b.x, b.y) != ocean);
            if off_ocean && check_place(seed, BuildingKind::Storehouse, x, y, free, &[]) == Ok(()) {
                lake = Some((x, y));
                break 'lake;
            }
        }
    }
    if let Some((x, y)) = lake {
        assert_eq!(
            check_place(seed, BuildingKind::Harbour, x, y, free, &[]),
            Err(PlaceError::NeedsSeaBerth),
            "a lake floated a harbour"
        );
    }
}

/// Every peer has to agree on where a hull appears and what it does next. The
/// berth pick is a pure function of the seed and the footprint, so this is the
/// assertion that keeps it that way.
#[test]
fn two_worlds_that_launch_hulls_agree_every_tick() {
    let seed = 1;
    let mut a = build_app(seed);
    let mut b = build_app(seed);
    for app in [&mut a, &mut b] {
        let (hut_id, _) = found_coastal_town(app, seed);
        cmd(app, PlayerCommand::TrainAt { player_id: 1, building: hut_id, kind: UnitKind::FishingSkiff });
        cmd(app, PlayerCommand::TrainAt { player_id: 1, building: hut_id, kind: UnitKind::FishingSkiff });
    }
    for t in 0..600 {
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "two worlds diverged on tick {t}"
        );
    }
    assert_eq!(units_of(&mut a, UnitKind::FishingSkiff).len(), 2);
}

/// A hull is shipping, not a soldier and not a hand: it eats nothing, it raises
/// nothing, and it is housed as what it is.
#[test]
fn a_hull_is_not_on_the_muster_roll() {
    for k in [UnitKind::FishingSkiff, UnitKind::Barge] {
        let d = unit_def(k);
        assert!(!d.draws_rations(), "{k:?} draws rations");
        assert!(!d.builds(), "{k:?} was handed a hammer");
        assert!(d.afloat() && d.attack == 0, "{k:?}");
    }
    assert_eq!(unit_def(UnitKind::Barge).pop_cost, 2);
    assert_eq!(saladin_sim::trainer_of(UnitKind::FishingSkiff), Some(BuildingKind::FishingHut));
    assert_eq!(saladin_sim::trainer_of(UnitKind::Barge), Some(BuildingKind::Harbour));
}

// ── the ferry ────────────────────────────────────────────────────────────────

fn put_unit(app: &mut App, id: u64, owner: u64, pos: V2, u: Unit) {
    app.world_mut().spawn((GameId(id), Owner(owner), MatchId(1), Pos { pos, facing: ZERO }, u));
}

fn pos_of(app: &mut App, id: u64) -> Option<V2> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Pos)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, p)| p.pos)
}

fn unit_of(app: &mut App, id: u64) -> Option<Unit> {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone())
}

/// Sailable tiles of the main ocean that touch `region`, sparsely sampled — the
/// dense list is thousands of tiles and every pair of them gets measured below.
fn coast_of(seed: u32, region: u16) -> Vec<V2> {
    let land = saladin_sim::region_grid(seed);
    let sea = saladin_sim::water_region_grid(seed);
    let ocean = main_water_body(seed);
    let mut out = Vec::new();
    for ty in 1..WORLD_SIZE - 1 {
        for tx in 1..WORLD_SIZE - 1 {
            if sea[(ty * WORLD_SIZE + tx) as usize] != ocean {
                continue;
            }
            let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| land[((ty + dy) * WORLD_SIZE + tx + dx) as usize] == region);
            if touches {
                out.push(center(tx, ty));
            }
        }
    }
    out
}

/// The shortest open-water hop between two seatable islands: where the barge
/// starts, where it ends, and the two landmasses it joins.
fn strait(seed: u32) -> (V2, V2, u16, u16) {
    let starts = start_regions(seed);
    assert!(starts.len() >= 2, "seed {seed} has one island, so nothing to ferry between");
    let (a, b) = (starts[0], starts[1]);
    let (ca, cb) = (coast_of(seed, a), coast_of(seed, b));
    let mut best: Option<(Fx, V2, V2)> = None;
    for (i, pa) in ca.iter().enumerate() {
        if !i.is_multiple_of(3) {
            continue;
        }
        for (j, pb) in cb.iter().enumerate() {
            if !j.is_multiple_of(3) {
                continue;
            }
            let d = saladin_sim::dist2(*pa, *pb);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, *pa, *pb));
            }
        }
    }
    let (_, pa, pb) = best.expect("two coasts");
    (pa, pb, a, b)
}

/// An Archipelago seed whose first two seatable islands are a short hop apart.
fn ferry_seed() -> u32 {
    saladin_sim::compose_seed(7, 3)
}

/// The whole point of the barge: men who could never walk there stand there.
#[test]
fn a_barge_crosses_a_strait_and_lands_its_party_on_the_far_island() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, to, near, far) = strait(seed);
    assert_ne!(near, far);

    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));
    // the party stands on the near island, within a gangplank of the hull
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    assert_eq!(saladin_sim::region_at(seed, beach.x, beach.y), near, "the party is on the near island");
    for i in 0..6u64 {
        put_unit(&mut app, 10 + i, 1, beach, Unit::new(UnitKind::Spearman, beach));
    }

    cmd(&mut app, PlayerCommand::Embark { player_id: 1, units: (10..16).collect(), boat: 1 });
    step(app.world_mut());
    for i in 10..16u64 {
        assert_eq!(unit_of(&mut app, i).unwrap().garrisoned_in, 1, "unit {i} missed the boat");
    }

    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 1, target: to });
    for _ in 0..4000 {
        step(app.world_mut());
        if saladin_sim::dist(pos_of(&mut app, 1).unwrap(), to) <= fx!("1.5") {
            break;
        }
    }
    let hull = pos_of(&mut app, 1).unwrap();
    assert!(saladin_sim::dist(hull, to) <= fx!("1.5"), "the barge never made the crossing: {hull:?}");

    let shore = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), to.x, to.y);
    cmd(&mut app, PlayerCommand::Disembark { player_id: 1, boat: 1, target: shore });
    step(app.world_mut());
    for i in 10..16u64 {
        let u = unit_of(&mut app, i).unwrap();
        assert_eq!(u.garrisoned_in, 0, "unit {i} never got off");
        let p = pos_of(&mut app, i).unwrap();
        let (tx, ty) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
        assert!(is_passable(seed, tx, ty), "unit {i} was put ashore on water at {tx},{ty}");
        assert_eq!(
            saladin_sim::region_at(seed, p.x, p.y),
            far,
            "unit {i} landed on the wrong island"
        );
    }
}

/// A hull is a host that MOVES, which no host ever did before. A passenger whose
/// position froze at the beach would have its supply band, its foraging draw and
/// its desertion roll computed where it no longer is.
#[test]
fn a_passenger_rides_with_the_hull() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, to, _, _) = strait(seed);
    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    put_unit(&mut app, 10, 1, beach, Unit::new(UnitKind::Spearman, beach));

    cmd(&mut app, PlayerCommand::Embark { player_id: 1, units: vec![10], boat: 1 });
    step(app.world_mut());
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 1, target: to });
    let mut moved = 0;
    let start = pos_of(&mut app, 1).unwrap();
    for _ in 0..600 {
        step(app.world_mut());
        let h = pos_of(&mut app, 1).unwrap();
        assert_eq!(pos_of(&mut app, 10).unwrap(), h, "the passenger fell overboard");
        if saladin_sim::dist(h, start) > fx!("4") {
            moved += 1;
        }
    }
    assert!(moved > 0, "the hull never left the beach, so nothing was proved");
}

/// Cargo drowns with its hull. Before the unit-host death branch existed the
/// passengers of a sunk barge stayed aboard a dead GameId: invisible, unkillable,
/// permanently off the field and still on the population roll.
#[test]
fn a_sunk_barge_leaves_no_orphan_cargo() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, _, _, _) = strait(seed);
    put_unit(&mut app, 1, 1, from, Unit { hp: 1, ..Unit::new(UnitKind::Barge, from) });
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    for i in 0..4u64 {
        put_unit(&mut app, 10 + i, 1, beach, Unit::new(UnitKind::Peasant, beach));
    }
    cmd(&mut app, PlayerCommand::Embark { player_id: 1, units: (10..14).collect(), boat: 1 });
    step(app.world_mut());

    // an archer on the shore, which is the entire naval counter-play
    spawn_player(&mut app, 2);
    put_unit(&mut app, 50, 2, beach, Unit::new(UnitKind::Archer, beach));

    for _ in 0..400 {
        step(app.world_mut());
        if unit_of(&mut app, 1).is_none() {
            break;
        }
    }
    assert!(unit_of(&mut app, 1).is_none(), "a shore archer could not sink a laden hull");

    let (alive, orphans) = {
        let world = app.world_mut();
        let live: Vec<u64> = {
            let mut q = world.query::<&GameId>();
            q.iter(world).map(|g| g.0).collect()
        };
        let mut q = world.query::<(&GameId, &Unit)>();
        let orphans = q
            .iter(world)
            .filter(|(_, u)| u.garrisoned_in != 0 && !live.contains(&u.garrisoned_in))
            .count();
        let alive = (10..14u64).filter(|i| live.contains(i)).count();
        (alive, orphans)
    };
    assert_eq!(orphans, 0, "{orphans} units are aboard a hull that no longer exists");
    assert_eq!(alive, 0, "the cargo outlived the hull it was in");
}

/// The one reliable net for the wrong-domain-closure class of bug: `movement`
/// walks whatever path it is handed with no terrain test at all, so a boat given
/// a land closure does not fail a pathfind — it drives inland.
#[test]
fn no_boat_ever_stands_on_land() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, to, _, _) = strait(seed);
    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));
    put_unit(&mut app, 2, 1, from, Unit::new(UnitKind::FishingSkiff, from));

    let inland = saladin_sim::start_point(seed, 0);
    let orders: [(V2, u8); 4] = [(to, 0), (inland, 1), (from, 2), (inland, 0)];
    let mut checked = 0;
    for (target, formation) in orders {
        cmd(&mut app, PlayerCommand::GroupMove { player_id: 1, units: vec![1, 2], target, formation });
        for _ in 0..500 {
            step(app.world_mut());
            for id in [1u64, 2] {
                let p = pos_of(&mut app, id).unwrap();
                let (tx, ty) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
                assert!(
                    is_sailable(seed, tx, ty),
                    "hull {id} stood on land at {tx},{ty} after an order to {:?}",
                    (target.x.to_num::<i32>(), target.y.to_num::<i32>())
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 3000);
}

/// A spit of land with open water on both sides: the leg no hull may walk.
fn spit(seed: u32) -> (V2, V2) {
    for ty in 4..WORLD_SIZE - 4 {
        for tx in 4..WORLD_SIZE - 4 {
            if is_sailable(seed, tx, ty) {
                continue;
            }
            for (dx, dy) in [(1, 0), (0, 1)] {
                let (a, b) = ((tx - dx * 3, ty - dy * 3), (tx + dx * 3, ty + dy * 3));
                if is_sailable(seed, a.0, a.1) && is_sailable(seed, b.0, b.1) {
                    return (center(a.0, a.1), center(b.0, b.1));
                }
            }
        }
    }
    panic!("no spit on seed {seed}");
}

/// `no_boat_ever_stands_on_land` above measures ORDERS, and every order in the
/// game lays a leg that was cleared when it was laid. That is not the same
/// guarantee: `step_toward` is fixed point, `separation` nudges, a save restores
/// a path laid on other ground, and a leg that runs along a tile boundary
/// crosses it on the drift. `movement` is the last reader of a hull's position
/// and it must be the one that refuses — MEASURED, a Hard bot's fishing fleet
/// beached itself inside fifteen minutes on four of eight archipelago seeds with
/// a corner-clean path in hand.
///
/// So this hands a hull a leg that crosses a headland OUTRIGHT, which is the
/// worst any producer could ever do, and asks only that the boat stay wet.
#[test]
fn a_hull_handed_a_leg_across_land_still_never_stands_on_it() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (near, far) = spit(seed);

    for (id, kind) in [(1u64, UnitKind::Barge), (2, UnitKind::FishingSkiff)] {
        let mut u = Unit::new(kind, near);
        // straight at the far side, through the spit — no pathfinder involved
        u.path = vec![far];
        u.path_idx = 0;
        u.target = far;
        u.has_target = true;
        put_unit(&mut app, id, 1, near, u);
    }

    let mut checked = 0;
    for _ in 0..400 {
        step(app.world_mut());
        for id in [1u64, 2] {
            let p = pos_of(&mut app, id).unwrap();
            let (tx, ty) = (p.x.to_num::<i32>(), p.y.to_num::<i32>());
            assert!(
                is_sailable(seed, tx, ty),
                "hull {id} walked its leg onto land at {tx},{ty}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 800);
    // and it did not simply freeze on the spot: a refused step still slides
    let moved = [1u64, 2].iter().any(|id| pos_of(&mut app, *id).unwrap() != near);
    assert!(moved, "both hulls sat still rather than sliding along the shore");
}

/// A mixed selection is one click and two routes. Sharing an A* would snap the
/// whole thing onto whichever domain the destination sits in, and half of it
/// would be handed a path over ground it can never enter.
#[test]
fn one_click_marches_a_column_and_a_fleet_on_their_own_ground() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, to, _, _) = strait(seed);
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));
    put_unit(&mut app, 10, 1, beach, Unit::new(UnitKind::Spearman, beach));

    cmd(&mut app, PlayerCommand::GroupMove { player_id: 1, units: vec![1, 10], target: to, formation: 0 });
    step(app.world_mut());
    assert!(unit_of(&mut app, 1).unwrap().has_target, "the hull was given no route to the far shore");
    for _ in 0..600 {
        step(app.world_mut());
        let hp = pos_of(&mut app, 1).unwrap();
        let mp = pos_of(&mut app, 10).unwrap();
        assert!(is_sailable(seed, hp.x.to_num::<i32>(), hp.y.to_num::<i32>()), "the hull beached");
        assert!(is_passable(seed, mp.x.to_num::<i32>(), mp.y.to_num::<i32>()), "the man walked into the sea");
    }
}

/// Separation exists to stop bodies stacking into one sprite, and it does it by
/// pushing them onto ground they can stand on. A hull and a man share no such
/// ground, so the pair is not a pair: shoving them apart would put the column in
/// the water and the barge on the shingle, and both landings would then be
/// refused anyway. Two HULLS must still spread, or the filter has disabled the
/// system for boats.
#[test]
fn a_hull_and_a_man_do_not_shove_each_other_off_their_own_ground() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, _, _, _) = strait(seed);
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));
    put_unit(&mut app, 10, 1, beach, Unit::new(UnitKind::Spearman, beach));
    // two hulls stacked exactly, which is what separation is FOR
    put_unit(&mut app, 2, 1, from, Unit::new(UnitKind::FishingSkiff, from));
    put_unit(&mut app, 3, 1, from, Unit::new(UnitKind::FishingSkiff, from));

    for _ in 0..80 {
        step(app.world_mut());
    }
    let hull = pos_of(&mut app, 1).unwrap();
    let man = pos_of(&mut app, 10).unwrap();
    assert!(is_sailable(seed, hull.x.to_num::<i32>(), hull.y.to_num::<i32>()), "the barge was shoved ashore");
    assert!(is_passable(seed, man.x.to_num::<i32>(), man.y.to_num::<i32>()), "the man was shoved into the sea");

    let (a, b) = (pos_of(&mut app, 2).unwrap(), pos_of(&mut app, 3).unwrap());
    let apart = saladin_sim::dist(a, b);
    let want = unit_def(UnitKind::FishingSkiff).radius * Fx::from_num(2);
    assert!(apart >= want - fx!("0.05"), "two stacked skiffs never spread: {apart} apart, want {want}");
    for p in [a, b] {
        assert!(is_sailable(seed, p.x.to_num::<i32>(), p.y.to_num::<i32>()), "a skiff was pushed ashore");
    }
}

/// A hull told to march inland. `move_unit` snaps a SEA target onto the nearest
/// water, so the order becomes "get as close as you can" — but the boat must
/// never climb out to serve it, and it must never wedge itself trying either.
#[test]
fn a_hull_ordered_onto_dry_land_neither_beaches_nor_hangs() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, _, _, _) = strait(seed);
    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));

    // the driest tile the probe can find: land with no water within six tiles
    let mut inland = None;
    'scan: for ty in (10..WORLD_SIZE - 10).step_by(3) {
        for tx in (10..WORLD_SIZE - 10).step_by(3) {
            if !is_passable(seed, tx, ty) {
                continue;
            }
            let wet = (-6..=6i32)
                .any(|dy| (-6..=6i32).any(|dx| is_sailable(seed, tx + dx, ty + dy)));
            if !wet {
                inland = Some(center(tx, ty));
                break 'scan;
            }
        }
    }
    let inland = inland.expect("somewhere on this map is out of sight of the sea");
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 1, target: inland });
    for _ in 0..1200 {
        step(app.world_mut());
        let p = pos_of(&mut app, 1).unwrap();
        assert!(
            is_sailable(seed, p.x.to_num::<i32>(), p.y.to_num::<i32>()),
            "the barge climbed ashore chasing an inland order, at {p:?}"
        );
    }
}

/// And the mirror: a man told to march into open water. Land refuses a water
/// click by snapping it back to the shore — the comment used to claim it
/// "refused" and it did not, so the spearman pathed and walked in.
#[test]
fn a_man_ordered_into_open_water_stays_dry() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, _, _, _) = strait(seed);
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    put_unit(&mut app, 10, 1, beach, Unit::new(UnitKind::Spearman, beach));

    let ocean = main_water_body(seed);
    let mut deep = (Fx::ZERO, from);
    for ty in (10..WORLD_SIZE - 10).step_by(4) {
        for tx in (10..WORLD_SIZE - 10).step_by(4) {
            if is_sailable(seed, tx, ty) && water_region_at(seed, center(tx, ty).x, center(tx, ty).y) == ocean {
                let d = saladin_sim::dist2(beach, center(tx, ty));
                if d > deep.0 {
                    deep = (d, center(tx, ty));
                }
            }
        }
    }
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 10, target: deep.1 });
    for _ in 0..1500 {
        step(app.world_mut());
        let p = pos_of(&mut app, 10).unwrap();
        assert!(
            is_passable(seed, p.x.to_num::<i32>(), p.y.to_num::<i32>()),
            "the spearman walked into the sea at {p:?}"
        );
    }
}

/// Unloading over deep water. `landing_spot` searches for LAND, so the only two
/// legal outcomes are "nobody moved" and "everybody is ashore" — never a party
/// standing on the waves, and never a party that stops existing.
#[test]
fn a_party_is_never_put_over_the_side_into_open_water() {
    let seed = ferry_seed();
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (from, _, _, _) = strait(seed);
    put_unit(&mut app, 1, 1, from, Unit::new(UnitKind::Barge, from));
    let beach = saladin_sim::nearest_passable_grid(&|tx, ty| is_passable(seed, tx, ty), from.x, from.y);
    for i in 0..3u64 {
        put_unit(&mut app, 10 + i, 1, beach, Unit::new(UnitKind::Spearman, beach));
    }
    cmd(&mut app, PlayerCommand::Embark { player_id: 1, units: (10..13).collect(), boat: 1 });
    step(app.world_mut());
    assert_eq!(unit_of(&mut app, 10).unwrap().garrisoned_in, 1, "nobody boarded");

    // open water: a sailable tile with no dry ground within five tiles of it
    let ocean = main_water_body(seed);
    let mut open = None;
    'scan: for ty in 10..WORLD_SIZE - 10 {
        for tx in 10..WORLD_SIZE - 10 {
            let c = center(tx, ty);
            if !is_sailable(seed, tx, ty) || water_region_at(seed, c.x, c.y) != ocean {
                continue;
            }
            let dry = (-5..=5i32).any(|dy| (-5..=5i32).any(|dx| is_passable(seed, tx + dx, ty + dy)));
            if !dry {
                open = Some(c);
                break 'scan;
            }
        }
    }
    let open = open.expect("this map has open sea somewhere");
    cmd(&mut app, PlayerCommand::Move { player_id: 1, unit: 1, target: open });
    for _ in 0..6000 {
        step(app.world_mut());
        if saladin_sim::dist(pos_of(&mut app, 1).unwrap(), open) <= fx!("2") {
            break;
        }
    }
    assert!(
        saladin_sim::dist(pos_of(&mut app, 1).unwrap(), open) <= fx!("2"),
        "the barge never reached open water, so nothing is proved"
    );

    cmd(&mut app, PlayerCommand::Disembark { player_id: 1, boat: 1, target: open });
    step(app.world_mut());
    for i in 10..13u64 {
        let u = unit_of(&mut app, i).expect("a passenger stopped existing");
        if u.garrisoned_in == 0 {
            let p = pos_of(&mut app, i).unwrap();
            assert!(
                is_passable(seed, p.x.to_num::<i32>(), p.y.to_num::<i32>()),
                "unit {i} was put over the side at {p:?}"
            );
        }
    }
}
