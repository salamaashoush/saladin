//! Building placement + role audit: the Build command must enforce buildable
//! biome (no towers on fords), true water adjacency for the Fishing Hut, the
//! town radius, resource-node occupancy — and the food drop-offs (Granary /
//! Fishing Hut) must actually bank food.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    Biome, BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile, UnitKind, V2,
    WORLD_SIZE, ZERO, building_def, compose_seed, fx, is_buildable_tile, is_passable,
    is_water_tile, sample_terrain, unit_def,
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
        Building { kind, hp: def.max_hp, cooldown: ZERO, rally: pos },
    ));
}

fn building_count(app: &mut App, kind: BuildingKind) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).filter(|b| b.kind == kind).count()
}

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"))
}

/// A 6x6 buildable block far from water (inland).
fn inland_block(seed: u32) -> (i32, i32) {
    for cy in 16..WORLD_SIZE - 24 {
        for cx in 16..WORLD_SIZE - 24 {
            let all_buildable =
                (-1..7).all(|dx| (-1..7).all(|dy| is_buildable_tile(seed, cx + dx, cy + dy)));
            if !all_buildable {
                continue;
            }
            let near_water = (-6..12)
                .any(|dx| (-6..12).any(|dy| is_water_tile(seed, cx + dx, cy + dy)));
            if !near_water {
                return (cx, cy);
            }
        }
    }
    panic!("no inland block on seed {seed}");
}

/// A buildable tile with open water on an orthogonal neighbour (shoreline).
fn shore_tile(seed: u32) -> (i32, i32) {
    for ty in 8..WORLD_SIZE - 8 {
        for tx in 8..WORLD_SIZE - 8 {
            if !is_buildable_tile(seed, tx, ty) {
                continue;
            }
            let watery = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| is_water_tile(seed, tx + dx, ty + dy));
            // needs a dry buildable neighbour block for the anchor keep too
            let anchored = (-3..0).all(|dx| (-3..0).all(|dy| is_buildable_tile(seed, tx + dx, ty + dy)));
            if watery && anchored {
                return (tx, ty);
            }
        }
    }
    panic!("no shoreline on seed {seed}");
}

#[test]
fn fishing_hut_needs_real_water_not_just_land() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));

    // inland: rejected even though the ground is perfectly buildable
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::FishingHut, pos: center(cx + 4, cy + 4), facing: 0 });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::FishingHut), 0, "no fishing hut on dry land");

    // shoreline: accepted (anchor keep placed beside it for the town radius)
    let (sx, sy) = shore_tile(seed);
    spawn_building(&mut app, 11, 1, BuildingKind::Keep, center(sx - 2, sy - 2));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::FishingHut, pos: center(sx, sy), facing: 0 });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::FishingHut), 1, "fishing hut builds on the shore");
}

#[test]
fn no_building_on_fords() {
    // river-valley preset guarantees fords
    let seed = compose_seed(5, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let half = fx!("0.5");
    let mut ford: Option<(i32, i32)> = None;
    'scan: for ty in 8..WORLD_SIZE - 8 {
        for tx in 8..WORLD_SIZE - 8 {
            let s = sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            if s.biome == Biome::Ford {
                ford = Some((tx, ty));
                break 'scan;
            }
        }
    }
    let (fx_, fy) = ford.expect("river-valley has fords");
    assert!(is_passable(seed, fx_, fy), "ford is walkable");
    // anchor a keep on the nearest buildable ground so only the biome rule fires
    let mut anchored = false;
    'anchor: for r in 1..20i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let (ax, ay) = (fx_ + dx, fy + dy);
                if (-1..2).all(|i| (-1..2).all(|j| is_buildable_tile(seed, ax + i, ay + j))) {
                    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(ax, ay));
                    anchored = true;
                    break 'anchor;
                }
            }
        }
    }
    assert!(anchored, "keep anchored near the ford");

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(fx_, fy), facing: 0 });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 0, "fords stay open chokepoints");
}

#[test]
fn buildings_must_rise_within_the_town_radius() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));

    // adjacent to town: fine
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: center(cx + 5, cy + 1), facing: 0 });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::House), 1);

    // across the map: rejected, even on perfect ground
    let (fx2, fy2) = {
        let mut found = None;
        'scan: for ty in (cy + 60)..WORLD_SIZE - 16 {
            for tx in 16..WORLD_SIZE - 16 {
                if (0..3).all(|dx| (0..3).all(|dy| is_buildable_tile(seed, tx + dx, ty + dy))) {
                    found = Some((tx, ty));
                    break 'scan;
                }
            }
        }
        found.expect("distant buildable spot")
    };
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: center(fx2 + 1, fy2 + 1), facing: 0 });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::House), 1, "no teleport-building across the map");
}

#[test]
fn no_building_on_resource_nodes() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));
    app.world_mut().spawn((
        GameId(50),
        MatchId(1),
        Pos { pos: center(cx + 5, cy + 5), facing: ZERO },
        ResourceNode { res_type: ResourceType::Wood, remaining: 100 },
    ));

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(cx + 5, cy + 5), facing: 0 });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 0, "the tree blocks the tile");
}

#[test]
fn granary_banks_food_without_a_keep_nearby() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    // keep far away is irrelevant; granary is the close drop-off
    spawn_building(&mut app, 10, 1, BuildingKind::Granary, center(cx + 1, cy + 1));

    let def = unit_def(UnitKind::Peasant);
    let pos = center(cx + 4, cy + 1);
    app.world_mut().spawn((
        GameId(20),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind: UnitKind::Peasant,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state: GatherState::ToStockpile,
            target_node: 0,
            carrying: 10,
            carry_type: ResourceType::Food,
            harvest_timer: ZERO,
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

    let before = {
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        q.iter(world).next().unwrap().stock.food
    };
    for _ in 0..400 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    let after = q.iter(world).next().unwrap().stock.food;
    assert_eq!(after, before + 10, "granary accepts the food deposit");
}

#[test]
fn building_roles_are_coherent() {
    use saladin_sim::BuildingKind::*;
    // every building states its role
    for &k in saladin_sim::BuildingKind::ALL {
        assert!(!building_def(k).blurb.is_empty(), "{k:?} has no role blurb");
    }
    // food drop-offs are exactly granary + fishing hut + (keep accepts all)
    assert!(building_def(Granary).food_dropoff);
    assert!(building_def(FishingHut).food_dropoff);
    assert!(building_def(FishingHut).requires_water);
    // population comes from houses and the keep, not storage buildings
    assert_eq!(building_def(Granary).pop, 0);
    assert_eq!(building_def(House).pop, 6);
    assert_eq!(building_def(Keep).pop, 8);
    // trade only via market
    assert!(building_def(Market).enables_trade);
    // only the gatehouse lets units through
    for &k in saladin_sim::BuildingKind::ALL {
        assert_eq!(building_def(k).passable, k == Gatehouse, "{k:?} passability");
    }
}

#[test]
fn build_facing_rides_the_command() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: center(cx + 5, cy + 1), facing: 3 });
    step(app.world_mut());
    let world = app.world_mut();
    let mut q = world.query::<(&Pos, &Building)>();
    let p = q
        .iter(world)
        .find(|(_, b)| b.kind == BuildingKind::House)
        .map(|(p, _)| p.facing)
        .expect("house built");
    assert_eq!(p, fx!("1.5707963") * Fx::from_num(3), "quarter turns applied deterministically");
}
