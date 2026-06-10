//! Gather/resource system tests: node depletion handoff, same-tick double
//! harvest, unreachable-node handling (region filter), deposit failsafe, and
//! unit separation.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile,
    UnitKind, V2, ZERO, building_def, dist2, is_passable, region_at, unit_def,
};

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
        Building { kind: BuildingKind::Keep, hp: def.max_hp, cooldown: ZERO, rally: pos },
    ));
}

fn spawn_node(app: &mut App, id: u64, pos: V2, remaining: i32) {
    app.world_mut().spawn((
        GameId(id),
        MatchId(1),
        Pos { pos, facing: ZERO },
        ResourceNode { res_type: ResourceType::Wood, remaining },
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
            kind: UnitKind::Peasant,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state: state,
            target_node: node,
            carrying,
            carry_type: ResourceType::Wood,
            harvest_timer: timer,
            hp: def.max_hp,
            attack_target: 0,
            attack_cooldown: ZERO,
            stance: Stance::Aggressive,
            morale: Fx::ONE,
            routing: false,
            home: pos,
            garrisoned_in: 0,
            path: vec![],
            path_idx: 0,
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
                    Some((r0, p0)) if r != r0 => {
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
                    Some((r0, p0)) if r != r0 => {
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
