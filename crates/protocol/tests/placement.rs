//! Building placement + role audit: the Build command must enforce buildable
//! biome (no towers on fords), true water adjacency for the Fishing Hut, the
//! town radius, resource-node occupancy — and the food drop-offs (Granary /
//! Fishing Hut) must actually bank food.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BUILD_SLOPE_MAX, Biome, BuildingKind, Faction, Fx, GatherState, PlaceError, ResourceType, Stockpile, UnitKind, V2, WORLD_SIZE, ZERO, building_def, check_place, compose_seed, fx,
    is_buildable_tile, is_passable, is_water_tile, sample_terrain, slope_at, unit_def,
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
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::FishingHut, pos: center(cx + 4, cy + 4), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::FishingHut), 0, "no fishing hut on dry land");

    // shoreline: accepted (anchor keep placed beside it for the town radius)
    let (sx, sy) = shore_tile(seed);
    spawn_building(&mut app, 11, 1, BuildingKind::Keep, center(sx - 2, sy - 2));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::FishingHut, pos: center(sx, sy), facing: 0, builders: vec![] });
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

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(fx_, fy), facing: 0, builders: vec![] });
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
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: center(cx + 5, cy + 1), facing: 0, builders: vec![] });
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
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: center(fx2 + 1, fy2 + 1), facing: 0, builders: vec![] });
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
        ResourceNode::deposit(ResourceType::Wood, 100),
    ));

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(cx + 5, cy + 5), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 0, "the tree blocks the tile");
}

#[test]
fn a_storehouse_banks_a_haul_without_a_keep_nearby() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    // keep far away is irrelevant; the storehouse is the close drop-off
    spawn_building(&mut app, 10, 1, BuildingKind::Storehouse, center(cx + 1, cy + 1));

    let def = unit_def(UnitKind::Peasant);
    let pos = center(cx + 4, cy + 1);
    app.world_mut().spawn((
        GameId(20),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            gather_state: GatherState::ToStockpile,
            carrying: 10,
            carry_type: ResourceType::Food,
            hp: def.max_hp,
            ..Unit::new(UnitKind::Peasant, pos)
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
    assert_eq!(after, before + 10, "the storehouse accepts the deposit");
}

#[test]
fn a_granary_is_a_farm_hub_not_a_warehouse() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Granary, center(cx + 1, cy + 1));

    let def = unit_def(UnitKind::Peasant);
    let pos = center(cx + 4, cy + 1);
    app.world_mut().spawn((
        GameId(20),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            speed: def.speed,
            gather_state: GatherState::ToStockpile,
            carrying: 10,
            carry_type: ResourceType::Food,
            hp: def.max_hp,
            ..Unit::new(UnitKind::Peasant, pos)
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
    assert_eq!(after, before, "a granary stores nothing; it WORKS the fields");
}

#[test]
fn building_roles_are_coherent() {
    use saladin_sim::BuildingKind::*;
    // every building states its role
    for &k in saladin_sim::BuildingKind::ALL {
        assert!(!building_def(k).blurb.is_empty(), "{k:?} has no role blurb");
    }
    // drop-offs: the keep and the storehouse take everything, the hut takes
    // the catch, and nothing else is a warehouse
    use saladin_sim::{ACCEPTS_ALL, ACCEPTS_FOOD};
    assert_eq!(building_def(Keep).accepts, ACCEPTS_ALL);
    assert_eq!(building_def(Storehouse).accepts, ACCEPTS_ALL);
    assert_eq!(building_def(FishingHut).accepts, ACCEPTS_FOOD);
    assert_eq!(building_def(Granary).accepts, 0);
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

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::House, pos: center(cx + 5, cy + 1), facing: 3, builders: vec![] });
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

#[test]
fn nothing_is_founded_on_a_hillside() {
    // Defect 9: the placement rules used to read the biome label only, so a
    // ridge crest counted as ground. Steepness is now part of the rule set the
    // command, the AI and the ghost all share.
    let seed = compose_seed(5, 2); // highlands
    let mut steepest = (fx!("0"), 0i32, 0i32);
    for ty in 8..WORLD_SIZE - 8 {
        for tx in 8..WORLD_SIZE - 8 {
            if !(-1..=1).all(|dx| (-1..=1).all(|dy| is_buildable_tile(seed, tx + dx, ty + dy))) {
                continue;
            }
            let s = slope_at(seed, Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"));
            if s > steepest.0 {
                steepest = (s, tx, ty);
            }
        }
    }
    assert!(
        steepest.0 > BUILD_SLOPE_MAX,
        "highlands has no buildable-biome tile steep enough to test ({})",
        steepest.0
    );
    let pos = center(steepest.1, steepest.2);
    assert_eq!(
        check_place(seed, BuildingKind::House, pos.x, pos.y, |_, _| false, |_, _| true, &[]),
        Err(PlaceError::TooSteep),
        "a house on a {} slope at ({},{})",
        steepest.0,
        steepest.1,
        steepest.2
    );
    // and the same rule refuses the whole footprint when only the ground under
    // it varies, not the individual tiles
    let (cx, cy) = inland_block(seed);
    assert!(
        check_place(seed, BuildingKind::House, center(cx + 2, cy + 2).x, center(cx + 2, cy + 2).y, |_, _| false, |_, _| true, &[])
            .is_ok(),
        "flat inland ground must still take a building"
    );
}

/// The prereq graph is only real if the COMMAND enforces it. Two genuine
/// multi-prereq gates, and the one-per-town structure.
#[test]
fn the_full_prereq_set_gates_the_build_command() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));
    spawn_building(&mut app, 11, 1, BuildingKind::Barracks, center(cx + 5, cy + 1));

    // a Stable needs the forge as well as the hall
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Stable, pos: center(cx + 5, cy + 5), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Stable), 0, "a stable without a blacksmith");

    spawn_building(&mut app, 12, 1, BuildingKind::Blacksmith, center(cx + 9, cy + 1));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Stable, pos: center(cx + 5, cy + 5), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Stable), 1, "both prereqs met");

    // one great mosque per town
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Mosque, pos: center(cx + 9, cy + 5), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Mosque), 1);
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Mosque, pos: center(cx + 1, cy + 9), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Mosque), 1, "a second mosque was raised");
}

/// The town radius is the ONLY spatial containment rule in the game, and a wall
/// drag used to walk straight out of it: every segment placed was pushed into
/// the anchor set, so a 120-tile line reached 115 tiles from the keep for a few
/// wood a tile. The anchor set is snapshotted once per command.
#[test]
fn a_long_wall_drag_cannot_walk_out_of_the_town() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    let keep = center(cx + 1, cy + 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, keep);

    // 100 tiles marching away in a straight line, far past TOWN_RADIUS
    let tiles: Vec<(i32, i32)> = (0..100).map(|i| (cx + 3, cy + 3 + i)).collect();
    cmd(&mut app, PlayerCommand::PlaceWall { player_id: 1, tiles, builders: vec![] });
    step(app.world_mut());

    let world = app.world_mut();
    let mut q = world.query::<(&Pos, &Building)>();
    let walls: Vec<V2> =
        q.iter(world).filter(|(_, b)| b.kind == BuildingKind::Wall).map(|(p, _)| p.pos).collect();
    assert!(!walls.is_empty(), "the drag laid no wall at all");
    let reach = saladin_sim::TOWN_RADIUS;
    for w in &walls {
        let d = saladin_sim::dist(*w, keep);
        assert!(d <= reach, "a segment crept to {d} from the keep (radius {reach})");
    }
    assert!(walls.len() < 100, "the whole line went up, so nothing was contained");
}

/// A Watchtower is what a Tower BECOMES; it is not on the bar and the command
/// refuses it outright.
#[test]
fn a_watchtower_cannot_be_bought() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));
    spawn_building(&mut app, 11, 1, BuildingKind::Tower, center(cx + 5, cy + 1));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Watchtower, pos: center(cx + 5, cy + 5), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Watchtower), 0);
}

/// `has_passable_approach` only ever promised that a walkable tile BORDERS the
/// footprint. Ring a plot with your own buildings and that stays true while the
/// pocket is sealed — and nothing downstream notices: `walk_to` hands the crew
/// an empty A*, drops them, and the foundation stands at zero work forever with
/// its cost already paid. Found by devctl on seed 4, a farm at (92, 88).
#[test]
fn a_foundation_the_crew_cannot_walk_to_is_refused() {
    let seed = 1;
    let (cx, cy) = inland_block(seed);
    let (px, py) = (cx + 3, cy + 3);
    let at = center(px, py);
    // the town's own ground, which is everything but the sealed pocket
    let outside = |tx: i32, ty: i32| (tx - px).abs() > 1 || (ty - py).abs() > 1;

    assert_eq!(
        check_place(seed, BuildingKind::Tower, at.x, at.y, |_, _| false, outside, &[]),
        Err(PlaceError::NoApproach),
        "a sealed pocket is not an approach, however walkable the tile beside it"
    );
    assert!(
        check_place(seed, BuildingKind::Tower, at.x, at.y, |_, _| false, |_, _| true, &[]).is_ok(),
        "open ground must still take a building"
    );
    assert!(
        check_place(seed, BuildingKind::Tower, at.x, at.y, |_, _| false, |tx, ty| !outside(tx, ty), &[])
            .is_ok(),
        "the same pocket with the town INSIDE it is fine"
    );
}

/// `town_reach` is the set the rule reads: four-connected open ground flooded
/// from the town, which is exactly what A* can traverse (a diagonal step needs
/// both its orthogonals open, so there is no corner to cut through).
#[test]
fn the_town_flood_stops_at_a_sealed_wall() {
    let seed = 1;
    let (cx, cy) = inland_block(seed);
    let (px, py) = (cx + 5, cy + 5);
    let sealed: Vec<(i32, i32)> = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .map(|(dx, dy)| (px + dx, py + dy))
        .collect();
    let walkable = |tx: i32, ty: i32| {
        is_passable(seed, tx, ty) && !sealed.contains(&(tx, ty))
    };
    // flooded from where a hand STANDS, not from a hall: the pocket that
    // started this borders the keep, so a flood seeded on the keep's own ring
    // declares it reachable from inside itself
    let hand = center(cx + 1, cy + 2);
    let reach = saladin_sim::town_reach(walkable, &[hand], &[center(cx + 1, cy + 1)], 32768);
    assert!(reach.contains(&saladin_sim::tile_key(cx + 2, cy + 1)), "the town's own ground");
    assert!(
        !reach.contains(&saladin_sim::tile_key(px, py)),
        "the walled pocket is not the town's ground"
    );
    assert!(!reach.contains(&saladin_sim::tile_key(px + 1, py)), "and neither is the wall itself");
}

/// The same rule through the COMMAND, with the pocket made of real walls. A*
/// forbids corner cutting (a diagonal step needs both its orthogonals open), so
/// four orthogonal walls are a true seal and a four-way flood is the right
/// question to ask.
#[test]
fn walling_yourself_out_of_a_plot_refuses_the_build() {
    let seed = 1;
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (cx, cy) = inland_block(seed);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));
    let (px, py) = (cx + 5, cy + 5);
    for (i, (dx, dy)) in [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().enumerate() {
        spawn_building(&mut app, 20 + i as u64, 1, BuildingKind::Wall, center(px + dx, py + dy));
    }
    // a hand OUTSIDE the pocket: the rule is about who can get there, so with
    // nobody to be cut off it is rightly waived
    let hand = center(cx + 1, cy + 3);
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: hand, facing: ZERO },
        Unit::new(UnitKind::Peasant, hand),
    ));

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(px, py), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 0, "raised inside a sealed pocket");
    let why: Vec<PlaceError> =
        app.world().resource::<CommandFeedback>().0.iter().map(|(_, e)| *e).collect();
    assert_eq!(why, vec![PlaceError::NoApproach], "and it must say so");

    // knock one wall out and the same plot is buildable
    let gap = {
        let world = app.world_mut();
        let mut q = world.query::<(bevy_ecs::prelude::Entity, &GameId)>();
        q.iter(world).find(|(_, g)| g.0 == 20).map(|(e, _)| e).expect("a wall to remove")
    };
    app.world_mut().despawn(gap);
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(px, py), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 1, "one gap is a way in");
}

/// A bot's town is dense and every building in it is legal on its own. On River
/// Valley seed 3 one grew around its own peasants and left EIGHT of fourteen
/// standing in two-tile pockets for the rest of the match: nothing crushes
/// them, nothing tells them, and an A* out of a sealed pocket returns nothing
/// every time they are asked to work. Found by the devctl soak.
#[test]
fn founding_never_walls_your_own_people_in() {
    let seed = 1;
    let (cx, cy) = inland_block(seed);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));

    // a one-tile alcove: three walls, the mouth still open
    let (px, py) = (cx + 5, cy + 5);
    for (i, (dx, dy)) in [(-1, 0), (1, 0), (0, -1)].into_iter().enumerate() {
        spawn_building(&mut app, 20 + i as u64, 1, BuildingKind::Wall, center(px + dx, py + dy));
    }
    let man = center(px, py);
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: man, facing: ZERO },
        Unit::new(UnitKind::Peasant, man),
    ));

    // stopping the mouth would leave him one tile to live in
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(px, py + 1), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 0, "the mouth was stopped on him");
    let why: Vec<PlaceError> =
        app.world().resource::<CommandFeedback>().0.iter().map(|(_, e)| *e).collect();
    assert_eq!(why, vec![PlaceError::NoApproach]);

    // with nobody in the alcove the same placement is fine
    let out = center(cx + 1, cy + 4);
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Pos)>();
        let (_, mut p) = q.iter_mut(world).find(|(g, _)| g.0 == 30).expect("the man");
        p.pos = out;
    }
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Tower, pos: center(px, py + 1), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Tower), 1, "an empty alcove may be closed");
}

/// A hall displaces the villagers standing in it — it does not entomb them.
#[test]
fn a_foundation_shoves_the_men_standing_on_it_clear() {
    let seed = 1;
    let (cx, cy) = inland_block(seed);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    spawn_building(&mut app, 10, 1, BuildingKind::Keep, center(cx + 1, cy + 1));

    let at = center(cx + 5, cy + 5);
    app.world_mut().spawn((
        GameId(30),
        Owner(1),
        MatchId(1),
        Pos { pos: at, facing: ZERO },
        Unit::new(UnitKind::Peasant, at),
    ));

    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Barracks, pos: at, facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(building_count(&mut app, BuildingKind::Barracks), 1, "the hall went up");

    let (moved, footprint) = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Pos)>();
        let man = q.iter(world).find(|(g, _)| g.0 == 30).map(|(_, p)| p.pos).expect("the man");
        let fp = saladin_sim::footprint_tiles(
            building_def(BuildingKind::Barracks).footprint,
            at.x,
            at.y,
        );
        (man, fp)
    };
    let on = footprint.iter().any(|t| {
        t.tx == moved.x.to_num::<i32>() && t.ty == moved.y.to_num::<i32>()
    });
    assert!(!on, "the man was entombed under the hall at {moved:?}");
    assert!(
        is_passable(seed, moved.x.to_num::<i32>(), moved.y.to_num::<i32>()),
        "he was shoved onto ground he cannot stand on"
    );
}
