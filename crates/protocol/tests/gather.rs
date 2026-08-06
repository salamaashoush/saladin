//! Gather/resource system tests: node depletion handoff, same-tick double
//! harvest, unreachable-node handling (region filter), deposit failsafe, and
//! unit separation.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, GatherState, ResourceType, Stockpile,
    UnitKind, V2, WORLD_SIZE, ZERO, building_def, dist2, is_passable, is_sailable,
    node_reachable, region_at, unit_def,
};

fn centre(t: i32) -> Fx {
    Fx::from_num(t) + saladin_sim::fx!("0.5")
}

/// A shore a hut can stand on and a school three tiles out in the SAME water,
/// so a skiff berthed at the hut can actually sail to it. Straight out from the
/// beach: the run is contiguous water by construction.
fn find_fishing_ground(seed: u32) -> (V2, V2) {
    for ty in 8..WORLD_SIZE - 8 {
        for tx in 8..WORLD_SIZE - 8 {
            if !is_passable(seed, tx, ty) {
                continue;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if !(1..=3).all(|k| is_sailable(seed, tx + dx * k, ty + dy * k)) {
                    continue;
                }
                let shore = V2::new(centre(tx), centre(ty));
                let school = V2::new(centre(tx + dx * 3), centre(ty + dy * 3));
                let Some(berth) = saladin_sim::berth_of(seed, 1, shore) else { continue };
                if saladin_sim::water_region_at(seed, berth.x, berth.y)
                    == saladin_sim::water_region_at(seed, school.x, school.y)
                {
                    return (shore, school);
                }
            }
        }
    }
    panic!("seed {seed} has no fishing ground");
}

fn spawn_fishery(app: &mut App, id: u64, pos: V2, remaining: i32) {
    app.world_mut().spawn((
        GameId(id),
        MatchId(1),
        Pos { pos, facing: ZERO },
        ResourceNode::renewable(
            ResourceType::Food,
            remaining,
            saladin_sim::FISH_INSHORE_CAP,
            saladin_sim::FISH_INSHORE_REGEN,
        ),
    ));
}

fn spawn_skiff(app: &mut App, id: u64, owner: u64, pos: V2, state: GatherState, node: u64) {
    let def = unit_def(UnitKind::FishingSkiff);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            gather_state: state,
            target_node: node,
            hp: def.max_hp,
            ..Unit::new(UnitKind::FishingSkiff, pos)
        },
    ));
}

fn spawn_hut(app: &mut App, id: u64, owner: u64, pos: V2) {
    let def = building_def(BuildingKind::FishingHut);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(BuildingKind::FishingHut, def.max_hp, pos),
    ));
}

/// A 7x7 block of walkable ground with open water against one edge: a place a
/// shore camp can stand AND a crew can work around without treading water.
fn find_shore_block(seed: u32) -> (i32, i32, V2) {
    for cy in 8..WORLD_SIZE - 16 {
        for cx in 8..WORLD_SIZE - 16 {
            if !(0..7).all(|dx| (0..7).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                continue;
            }
            for d in -1..8 {
                for (wx, wy) in [
                    (cx + d, cy - 1),
                    (cx + d, cy + 7),
                    (cx - 1, cy + d),
                    (cx + 7, cy + d),
                ] {
                    if is_sailable(seed, wx, wy) {
                        return (cx, cy, V2::new(centre(wx), centre(wy)));
                    }
                }
            }
        }
    }
    panic!("seed {seed} has no workable shore");
}

fn build(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}

fn find_land_block(seed: u32) -> (i32, i32) {
    for cy in 16..128 {
        for cx in 16..128 {
            if (0..7).all(|dx| (0..7).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no land block");
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 0, stone: 0, food: 100, gold: 0 },
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

fn spawn_keep(app: &mut App, id: u64, owner: u64, pos: V2) {
    let def = building_def(BuildingKind::Keep);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(BuildingKind::Keep, def.max_hp, pos),
    ));
}

fn spawn_node(app: &mut App, id: u64, pos: V2, remaining: i32) {
    app.world_mut().spawn((
        GameId(id),
        MatchId(1),
        Pos { pos, facing: ZERO },
        ResourceNode::deposit(ResourceType::Wood, remaining),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_peasant(app: &mut App, id: u64, owner: u64, pos: V2, state: GatherState, node: u64, carrying: i32, timer: Fx) {
    let def = unit_def(UnitKind::Peasant);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            gather_state: state,
            target_node: node,
            carrying,
            harvest_timer: timer,
            hp: def.max_hp,
            ..Unit::new(UnitKind::Peasant, pos)
        },
    ));
}

fn wood(app: &mut App) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).next().unwrap().stock.wood
}

fn unit(app: &mut App, id: u64) -> Unit {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone()).expect("unit")
}

/// A depleted node must hand the gatherer to the next nearest node — the full
/// chop-bank-chop cycle keeps going until the forest is gone.
#[test]
fn depleted_node_hands_off_to_next() {
    let mut app = build(1);
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, V2::new(f(cx + 1), f(cy + 1)));
    spawn_node(&mut app, 20, V2::new(f(cx + 4), f(cy + 1)), 8); // one load
    spawn_node(&mut app, 21, V2::new(f(cx + 4), f(cy + 3)), 200);
    spawn_peasant(&mut app, 30, 1, V2::new(f(cx + 4), f(cy + 2)), GatherState::ToResource, 20, 0, ZERO);

    for _ in 0..1200 {
        step(app.world_mut());
    }
    let w = wood(&mut app);
    assert!(w > 8, "gatherer must continue on the second node after depletion, banked {w}");
    // first node despawned
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &ResourceNode)>();
    assert!(q.iter(world).all(|(g, _)| g.0 != 20), "depleted node still alive");
}

/// Two harvesters finishing the same nearly-empty node on the same tick must
/// not duplicate its yield (the second sees 0 remaining and retargets).
#[test]
fn same_tick_double_harvest_does_not_dupe() {
    let mut app = build(1);
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, V2::new(f(cx + 1), f(cy + 1)));
    spawn_node(&mut app, 20, V2::new(f(cx + 4), f(cy + 1)), 8);
    // both within harvest range, timers primed to fire on the same gather tick
    let near = V2::new(f(cx + 4), f(cy + 2));
    spawn_peasant(&mut app, 30, 1, near, GatherState::Harvesting, 20, 0, Fx::lit("10"));
    spawn_peasant(&mut app, 31, 1, near, GatherState::Harvesting, 20, 0, Fx::lit("10"));

    for _ in 0..4 {
        step(app.world_mut());
    }
    let total = unit(&mut app, 30).carrying + unit(&mut app, 31).carrying;
    assert_eq!(total, 8, "the node held 8 wood; the pair must not mint more");
}

/// A gatherer whose only nodes sit in another connected region (across water)
/// idles instead of ping-ponging A* at them forever.
#[test]
fn unreachable_nodes_idle_not_pingpong() {
    // find a seed offering two distinct land regions
    let mut found = None;
    'seeds: for seed in 1..60u32 {
        let mut first: Option<(u16, V2)> = None;
        for ty in (10..134).step_by(4) {
            for tx in (10..134).step_by(4) {
                if !is_passable(seed, tx, ty) {
                    continue;
                }
                let p = V2::new(Fx::from_num(tx) + Fx::lit("0.5"), Fx::from_num(ty) + Fx::lit("0.5"));
                let r = region_at(seed, p.x, p.y);
                match first {
                    None => first = Some((r, p)),
                    // distinct region id is not enough: a node on the far
                    // rim of a 1-tile lake is still harvestable from this
                    // side (node_reachable's 3x3 ring) — demand a pair the
                    // gather brain itself calls unreachable
                    Some((r0, p0)) if r != r0 && !node_reachable(seed, p0, p) => {
                        found = Some((seed, p0, p));
                        break 'seeds;
                    }
                    _ => {}
                }
            }
        }
    }
    let Some((seed, unit_pos, node_pos)) = found else {
        eprintln!("no multi-region seed in 1..60 — nothing to test");
        return;
    };

    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_node(&mut app, 20, node_pos, 100);
    spawn_peasant(&mut app, 30, 1, unit_pos, GatherState::ToResource, 20, 0, ZERO);

    for _ in 0..200 {
        step(app.world_mut());
    }
    let u = unit(&mut app, 30);
    assert_eq!(
        u.gather_state,
        GatherState::Idle,
        "gatherer must give up on a node it can never reach (state {:?})",
        u.gather_state
    );
}

/// A carrier that cannot route to any dropoff goes Idle instead of re-running
/// a failing pathfind every tick forever.
#[test]
fn deposit_with_no_route_goes_idle() {
    let mut found = None;
    'seeds: for seed in 1..60u32 {
        let mut first: Option<(u16, V2)> = None;
        for ty in (10..134).step_by(4) {
            for tx in (10..134).step_by(4) {
                if !is_passable(seed, tx, ty) {
                    continue;
                }
                let p = V2::new(Fx::from_num(tx) + Fx::lit("0.5"), Fx::from_num(ty) + Fx::lit("0.5"));
                let r = region_at(seed, p.x, p.y);
                match first {
                    None => first = Some((r, p)),
                    // distinct region id is not enough: a node on the far
                    // rim of a 1-tile lake is still harvestable from this
                    // side (node_reachable's 3x3 ring) — demand a pair the
                    // gather brain itself calls unreachable
                    Some((r0, p0)) if r != r0 && !node_reachable(seed, p0, p) => {
                        found = Some((seed, p0, p));
                        break 'seeds;
                    }
                    _ => {}
                }
            }
        }
    }
    let Some((seed, keep_pos, unit_pos)) = found else {
        eprintln!("no multi-region seed in 1..60 — nothing to test");
        return;
    };

    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, keep_pos);
    spawn_peasant(&mut app, 30, 1, unit_pos, GatherState::ToStockpile, 0, 8, ZERO);

    for _ in 0..40 {
        step(app.world_mut());
    }
    let u = unit(&mut app, 30);
    assert_eq!(u.gather_state, GatherState::Idle, "stranded carrier must idle, not spin");
}

/// Stacked units spread apart: after a few separation passes no two units
/// overlap (pairwise distance at least their combined radii, with slack).
#[test]
fn stacked_units_separate() {
    let mut app = build(1);
    let (cx, cy) = find_land_block(1);
    let p = V2::new(Fx::from_num(cx + 3) + Fx::lit("0.5"), Fx::from_num(cy + 3) + Fx::lit("0.5"));
    spawn_player(&mut app, 1);
    for i in 0..6 {
        spawn_peasant(&mut app, 30 + i, 1, p, GatherState::Idle, 0, 0, ZERO);
    }
    for _ in 0..60 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Pos, &Unit)>();
    let pts: Vec<V2> = q.iter(world).map(|(_, p, _)| p.pos).collect();
    let r = unit_def(UnitKind::Peasant).radius;
    let min_sep = r + r;
    let slack = min_sep * Fx::lit("0.75");
    for i in 0..pts.len() {
        for j in (i + 1)..pts.len() {
            let d2 = dist2(pts[i], pts[j]);
            assert!(
                d2 >= slack * slack,
                "units {i},{j} still stacked (d2={d2}, want >= {})",
                slack * slack
            );
        }
    }
}

/// Regression: the full chop → walk → bank → walk loop keeps producing.
#[test]
fn gather_cycle_keeps_producing() {
    let mut app = build(1);
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, V2::new(f(cx + 1), f(cy + 1)));
    spawn_node(&mut app, 20, V2::new(f(cx + 5), f(cy + 5)), 500);
    spawn_peasant(&mut app, 30, 1, V2::new(f(cx + 4), f(cy + 4)), GatherState::ToResource, 20, 0, ZERO);

    let mut last = 0;
    for round in 1..=3 {
        for _ in 0..600 {
            step(app.world_mut());
        }
        let w = wood(&mut app);
        assert!(w > last, "round {round}: banked wood must keep growing ({last} -> {w})");
        last = w;
    }
}

/// A fishing hut doubles the catch its nets cover. Both boats are ALREADY on
/// station, so this measures the aura and not the sail.
#[test]
fn fishing_hut_speeds_nearby_fish() {
    let seed = 1u32;
    let (shore, school) = find_fishing_ground(seed);

    let one = |hut: bool| -> App {
        let mut app = build(seed);
        spawn_player(&mut app, 1);
        spawn_fishery(&mut app, 20, school, 200);
        spawn_skiff(&mut app, 30, 1, school, GatherState::Harvesting, 20);
        if hut {
            spawn_hut(&mut app, 40, 1, shore);
        }
        app
    };
    let mut plain = one(false);
    let mut hutted = one(true);

    // 16 sim steps = 4 gather ticks: 4 * 0.2 = 0.8 < 1.2 unboosted,
    // 4 * 0.4 = 1.6 >= 1.2 boosted
    for _ in 0..16 {
        step(plain.world_mut());
        step(hutted.world_mut());
    }
    assert_eq!(unit(&mut plain, 30).carrying, 0, "unboosted net must still be working");
    assert!(unit(&mut hutted, 30).carrying > 0, "hut-boosted net must have landed the catch");
}

/// A hut MULTIPLIES its fishery's own regrowth. It does not supply it: a school
/// with nothing to swim back stays empty however many nets are over it, and a
/// school that does swim back comes back FASTER under a hut than beside one.
///
/// The flat top-up this replaces was measurably negative — the same aura doubles
/// the DRAW, so a tended school emptied faster than an untended one — and it
/// filled every school to `FOOD_YIELD` rather than to the node's own cap.
#[test]
fn fishing_hut_regenerates_fish() {
    let seed = 1u32;
    let (shore, school) = find_fishing_ground(seed);
    let cap = saladin_sim::FISH_INSHORE_CAP;
    let regen = saladin_sim::FISH_INSHORE_REGEN;

    let after = |hut: bool| -> i32 {
        let mut app = build(seed);
        spawn_player(&mut app, 1);
        app.world_mut().spawn((
            GameId(20),
            MatchId(1),
            Pos { pos: school, facing: ZERO },
            ResourceNode::renewable(ResourceType::Food, 50, cap, regen),
        ));
        if hut {
            spawn_hut(&mut app, 40, 1, shore);
        }
        for _ in 0..200 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &ResourceNode)>();
        q.iter(world).find(|(g, _)| g.0 == 20).expect("school alive").1.remaining
    };

    let wild = after(false);
    let tended = after(true);
    assert!(wild > 50, "an untended school still swims back (got {wild})");
    assert!(tended > wild, "nets must restock faster than open water ({wild} -> {tended})");
    assert!(tended <= cap, "a school cannot exceed its own water ({tended} > {cap})");

    // and a dead pond stays dead: multiply, not supply
    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_hut(&mut app, 40, 1, shore);
    app.world_mut().spawn((
        GameId(21),
        MatchId(1),
        Pos { pos: school, facing: ZERO },
        ResourceNode::deposit(ResourceType::Food, 50),
    ));
    for _ in 0..200 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &ResourceNode)>();
    let dead = q.iter(world).find(|(g, _)| g.0 == 21).expect("node alive").1.remaining;
    assert_eq!(dead, 50, "a hut cannot conjure fish into water that has none");
}

/// A hut's nets are over the WATER. They are not a preservation order on every
/// rock and tree within six tiles of the shore.
///
/// The retire gate used to ask "is some fishing hut near this" instead of "does
/// this grow back", and it asked it of the NODE'S OWN RESOURCE not at all: a
/// wood or stone node drawn to zero anywhere in a hut's radius became a
/// permanent zero-remaining row that never regrew, never despawned, and rode in
/// the ECS and the StateHash for the rest of the match.
#[test]
fn a_wood_node_beside_a_hut_still_retires() {
    let seed = 1u32;
    let (cx, cy, _) = find_shore_block(seed);
    let hut = V2::new(centre(cx), centre(cy));
    let node = V2::new(centre(cx + 2), centre(cy));

    let run = |with_hut: bool| -> bool {
        let mut app = build(seed);
        spawn_player(&mut app, 1);
        spawn_keep(&mut app, 10, 1, V2::new(centre(cx + 5), centre(cy + 4)));
        spawn_node(&mut app, 20, node, 8); // exactly one peasant load
        spawn_peasant(&mut app, 30, 1, V2::new(centre(cx + 2), centre(cy + 1)), GatherState::ToResource, 20, 0, ZERO);
        if with_hut {
            let def = building_def(BuildingKind::FishingHut);
            app.world_mut().spawn((
                GameId(40),
                Owner(1),
                MatchId(1),
                Pos { pos: hut, facing: ZERO },
                Building::new(BuildingKind::FishingHut, def.max_hp, hut),
            ));
        }
        for _ in 0..400 {
            step(app.world_mut());
        }
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &ResourceNode)>();
        q.iter(world).any(|(g, _)| g.0 == 20)
    };

    assert!(!run(false), "control: a felled wood node must retire");
    assert!(!run(true), "a wood node emptied beside a fishing hut is still a hole in the ground");
}

/// Spawn a finished structure of any kind for a drop-off test.
fn spawn_hall(app: &mut App, id: u64, owner: u64, kind: BuildingKind, pos: V2) {
    let def = building_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(kind, def.max_hp, pos),
    ));
}

/// A laden peasant standing ON the drop-off, so the test measures the ACCEPTS
/// rule and not the walk.
fn laden(app: &mut App, id: u64, owner: u64, pos: V2, carry: ResourceType, amount: i32) {
    spawn_peasant(app, id, owner, pos, GatherState::ToStockpile, 0, amount, ZERO);
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &mut Unit)>();
    if let Some((_, mut u)) = q.iter_mut(world).find(|(g, _)| g.0 == id) {
        u.carry_type = carry;
    }
}

fn stock_of(app: &mut App, res: ResourceType) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    let s = q.iter(world).next().unwrap().stock;
    match res {
        ResourceType::Wood => s.wood,
        ResourceType::Stone => s.stone,
        ResourceType::Food => s.food,
        ResourceType::Gold => s.gold,
    }
}

/// `accepts` is a bitmask on the def, so what a structure takes in is a ROW and
/// not a branch. The Storehouse is the only reason worldgen's quarries and ore
/// belts are reachable at all; the Farm is food that GROWS, not food you carry
/// back to.
#[test]
fn a_storehouse_takes_stone_and_a_farm_takes_nothing() {
    let seed = saladin_sim::compose_seed(7, 0);
    let mut app = build(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = find_land_block(seed);
    let at = V2::new(Fx::from_num(cx) + Fx::ONE, Fx::from_num(cy) + Fx::ONE);

    spawn_hall(&mut app, 10, 1, BuildingKind::Storehouse, at);
    laden(&mut app, 20, 1, at, ResourceType::Stone, 9);
    for _ in 0..8 {
        step(app.world_mut());
    }
    assert_eq!(stock_of(&mut app, ResourceType::Stone), 9, "a storehouse must take stone");

    // the same load offered to a farm: the def accepts nothing, so the carrier
    // finds no drop-off at all and idles rather than banking
    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_hall(&mut app, 10, 1, BuildingKind::Farm, at);
    laden(&mut app, 20, 1, at, ResourceType::Stone, 9);
    for _ in 0..8 {
        step(app.world_mut());
    }
    assert_eq!(stock_of(&mut app, ResourceType::Stone), 0, "a farm is not a warehouse");
    assert_eq!(unit(&mut app, 20).gather_state, GatherState::Idle, "nowhere to bank = idle");
}

/// A hole in the ground stores nothing. Until the crew finishes it, a
/// storehouse site is a raid target and not a drop-off.
#[test]
fn a_site_is_not_a_dropoff() {
    let seed = saladin_sim::compose_seed(7, 0);
    let mut app = build(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = find_land_block(seed);
    let at = V2::new(Fx::from_num(cx) + Fx::ONE, Fx::from_num(cy) + Fx::ONE);

    let def = building_def(BuildingKind::Storehouse);
    app.world_mut().spawn((
        GameId(10),
        Owner(1),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        Building::site(BuildingKind::Storehouse, def.max_hp, at),
    ));
    laden(&mut app, 20, 1, at, ResourceType::Stone, 9);
    for _ in 0..8 {
        step(app.world_mut());
    }
    assert_eq!(stock_of(&mut app, ResourceType::Stone), 0, "a foundation banked a load");
    assert_eq!(unit(&mut app, 20).gather_state, GatherState::Idle);
}

/// A fishery is a node on WATER, and a peasant is a pair of feet. He refuses it
/// — and, refusing it, takes other work rather than standing down.
///
/// This is the beach-stander, and it was not a near miss: `harvest_reach` on a
/// water node is 1.7 tiles, so a man on the sand was inside a school's working
/// range and netted it exactly as he would a rock.
#[test]
fn a_peasant_refuses_a_fishery_and_takes_other_work() {
    let seed = 1u32;
    let (cx, cy, school) = find_shore_block(seed);
    let beach = saladin_sim::nearest_passable_grid(
        &|tx, ty| is_passable(seed, tx, ty),
        school.x,
        school.y,
    );

    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, V2::new(centre(cx + 5), centre(cy + 5)));
    spawn_fishery(&mut app, 20, school, 200);
    // an honest day's work, in reach and on dry ground
    spawn_node(&mut app, 21, V2::new(centre(cx + 2), centre(cy + 2)), 500);
    spawn_peasant(&mut app, 30, 1, beach, GatherState::ToResource, 20, 0, ZERO);

    for _ in 0..600 {
        step(app.world_mut());
    }
    let u = unit(&mut app, 30);
    assert_ne!(u.target_node, 20, "a peasant is netting fish from the beach");
    assert_ne!(u.gather_state, GatherState::Idle, "refusing the water stood the man down");
    assert!(wood(&mut app) > 0, "he refused the sea and then did nothing");
}

/// The loop, end to end and across the boundary: a skiff sails to a school,
/// fills, sails back to the hut's BERTH — ground it can never stand on — banks
/// there, and keeps doing it. `node_reachable` says the store is in another
/// region, which for a hull it always is.
#[test]
fn a_skiff_lands_its_catch_at_a_hut_berth() {
    let seed = 1u32;
    let (shore, school) = find_fishing_ground(seed);
    let berth = saladin_sim::berth_of(seed, 1, shore).expect("a shore hut has a berth");

    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_hut(&mut app, 40, 1, shore);
    spawn_fishery(&mut app, 20, school, 200);
    spawn_skiff(&mut app, 30, 1, berth, GatherState::ToResource, 20);

    let mut last = 0;
    for round in 1..=3 {
        for _ in 0..400 {
            step(app.world_mut());
        }
        let f = stock_of(&mut app, ResourceType::Food);
        assert!(f > last, "round {round}: the catch must keep landing ({last} -> {f})");
        last = f;
    }
}

/// A boat that fishes a school out HOLDS OVER IT. `gather` never looks at an
/// idle hand with no job site, and a hull has none — so standing it down at the
/// last fish loses it for the rest of the match. There is no new state here: the
/// node leaves and re-enters the candidate list on its own, and a boat on
/// station has always just banked, so it is always carrying nothing.
#[test]
fn a_skiff_holds_station_over_a_school_it_has_emptied() {
    let seed = 1u32;
    let (shore, school) = find_fishing_ground(seed);
    let berth = saladin_sim::berth_of(seed, 1, shore).expect("a shore hut has a berth");

    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_hut(&mut app, 40, 1, shore);
    // one load and no more: the school is empty by the first haul
    spawn_fishery(&mut app, 20, school, unit_def(UnitKind::FishingSkiff).carry);
    spawn_skiff(&mut app, 30, 1, berth, GatherState::ToResource, 20);

    for _ in 0..300 {
        step(app.world_mut());
    }
    let u = unit(&mut app, 30);
    assert_ne!(u.gather_state, GatherState::Idle, "the boat gave up on a school that grows back");
    assert_eq!(u.target_node, 20, "the boat left its own fishing ground");
    let first = stock_of(&mut app, ResourceType::Food);
    assert!(first > 0, "the first haul never landed");

    // and it is still there when the fish are: the flow keeps arriving
    for _ in 0..900 {
        step(app.world_mut());
    }
    let later = stock_of(&mut app, ResourceType::Food);
    assert!(later > first, "the school regrew and nobody was working it ({first} -> {later})");
}

/// No hull ever stands on dry ground. A second movement domain surfaces as boats
/// driving inland, not as failed pathfinds — `movement` walks whatever path it
/// is handed with no terrain test at all — so this is the only net under every
/// closure-construction site the fishing loop owns.
#[test]
fn no_boat_ever_stands_on_land() {
    let seed = 1u32;
    let (shore, school) = find_fishing_ground(seed);
    let berth = saladin_sim::berth_of(seed, 1, shore).expect("a shore hut has a berth");

    let mut app = build(seed);
    spawn_player(&mut app, 1);
    spawn_hut(&mut app, 40, 1, shore);
    spawn_fishery(&mut app, 20, school, 200);
    spawn_skiff(&mut app, 30, 1, berth, GatherState::ToResource, 20);
    spawn_skiff(&mut app, 31, 1, berth, GatherState::ToResource, 20);

    for t in 0..1200 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<(&Pos, &Unit)>();
        for (p, u) in q.iter(world) {
            if !unit_def(u.kind).afloat() {
                continue;
            }
            assert!(
                is_sailable(seed, p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>()),
                "tick {t}: a hull is aground at {:?}",
                p.pos
            );
        }
    }
}

/// Two worlds, the whole fishing loop, hashes compared EVERY tick. The loop
/// crosses a domain boundary and holds a state open across an economy tick, and
/// either is a place a peer can disagree.
#[test]
fn the_fishing_loop_is_deterministic() {
    let seed = 1u32;
    let (shore, school) = find_fishing_ground(seed);
    let berth = saladin_sim::berth_of(seed, 1, shore).expect("a shore hut has a berth");

    let one = || {
        let mut app = build(seed);
        spawn_player(&mut app, 1);
        spawn_hut(&mut app, 40, 1, shore);
        spawn_fishery(&mut app, 20, school, 60);
        spawn_skiff(&mut app, 30, 1, berth, GatherState::ToResource, 20);
        spawn_skiff(&mut app, 31, 1, berth, GatherState::Idle, 0);
        spawn_peasant(&mut app, 32, 1, shore, GatherState::Idle, 0, 0, ZERO);
        app
    };
    let (mut a, mut b) = (one(), one());
    for t in 0..600 {
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "two worlds diverged on tick {t}"
        );
    }
    assert!(stock_of(&mut a, ResourceType::Food) > 0, "nothing happened to be deterministic about");
}

/// A node walled in on every side is on the same landmass, so the region filter
/// waves it through, and the walk to it re-plans to the tile the walker already
/// stands on. `ToResource` was an ABSORBING state: the gatherer marched at it
/// for the rest of the match, and the AI's famine bias funnels a whole town onto
/// one node, so a single walled-in food node froze fourteen peasants and the
/// economy behind them.
#[test]
fn a_walled_in_node_is_given_up_on_not_marched_at_forever() {
    let seed = 1u32;
    let (cx, cy) = find_land_block(seed);
    let mut app = build(seed);
    spawn_player(&mut app, 1);
    let c = |t: i32| Fx::from_num(t) + saladin_sim::fx!("0.5");
    // a node in the middle of the block, sealed by a ring of the player's walls
    let walled = V2::new(c(cx + 3), c(cy + 3));
    spawn_keep(&mut app, 10, 1, V2::new(c(cx + 3), c(cy + 12)));
    spawn_node(&mut app, 20, walled, 500);
    let wdef = building_def(BuildingKind::Wall);
    let mut wid = 100;
    for dx in -1..=1 {
        for dy in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let p = V2::new(c(cx + 3 + dx), c(cy + 3 + dy));
            app.world_mut().spawn((
                GameId(wid),
                Owner(1),
                MatchId(1),
                Pos { pos: p, facing: ZERO },
                Building::new(BuildingKind::Wall, wdef.max_hp, p),
            ));
            wid += 1;
        }
    }
    // a perfectly ordinary node further out, and a peasant sent at the sealed one
    spawn_node(&mut app, 21, V2::new(c(cx + 3), c(cy + 9)), 500);
    spawn_peasant(&mut app, 30, 1, V2::new(c(cx + 3), c(cy + 10)), GatherState::ToResource, 20, 0, ZERO);

    for _ in 0..200 {
        step(app.world_mut());
    }
    assert_ne!(unit(&mut app, 30).target_node, 20, "still marching at the sealed node");
    assert!(wood(&mut app) > 0, "the gatherer never found honest work");
}
