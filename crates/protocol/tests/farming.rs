//! Farms: the only food that grows back, and the only node whose output is a
//! function of TIME AND CARE rather than of how many hands you put on it. A
//! field may only be sown on soil the worldgen says is worth sowing; the soil
//! sets how big the harvest is and the crew sets how fast it comes in; a growing
//! crop cannot be cut; a ripe one nobody cuts lodges; a reaped one re-sows
//! itself. Razing the farm takes the crop with it — the only way a field dies.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, FARM_CAP_MAX, FARM_CAP_MIN, FARM_MIN_FERTILITY, FARM_RIPE_GRACE, FARM_STORE,
    Faction, Fx, GatherState, ResourceType, Stockpile, UnitKind, V2, WORLD_SIZE, ZERO,
    building_def, compose_seed, field_cap, fx, is_buildable_tile, unit_def,
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

/// A building is LABOUR now: a Build order without hands founds a site and
/// nothing more, so every test that wants a standing farm hires a crew.
fn crew(app: &mut App, owner: u64, at: V2, n: u64, first: u64) -> Vec<u64> {
    let def = unit_def(UnitKind::Peasant);
    (0..n)
        .map(|i| {
            let id = first + i;
            let pos = V2::new(at.x + Fx::from_num(3 + i as i32), at.y);
            app.world_mut().spawn((
                GameId(id),
                Owner(owner),
                MatchId(1),
                Pos { pos, facing: ZERO },
                Unit {
                    speed: def.speed,
                    hp: def.max_hp,
                    ..Unit::new(UnitKind::Peasant, pos)
                },
            ));
            id
        })
        .collect()
}

/// Site a farm at `at` with a crew and run until it stands. Returns false if
/// the field was never ploughed.
fn raise_farm(app: &mut App, at: V2, first_id: u64) -> bool {
    let hands = crew(app, 1, at, 3, first_id);
    cmd(app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Farm,
        pos: at,
        facing: 0,
        builders: hands,
    });
    for _ in 0..900 {
        step(app.world_mut());
        let world = app.world_mut();
        let mut q = world.query::<&Building>();
        if q.iter(world).any(|b| b.kind == BuildingKind::Farm && b.complete()) {
            return true;
        }
    }
    false
}

fn center(tx: i32, ty: i32) -> V2 {
    V2::new(Fx::from_num(tx) + fx!("0.5"), Fx::from_num(ty) + fx!("0.5"))
}

/// A tile whose 2x2 farm footprint sits on soil that passes (`rich`) or fails
/// (`!rich`) the threshold, with clear buildable room beside it for the
/// anchoring keep. Measured with the SAME helper `check_place` uses.
fn block(seed: u32, rich: bool) -> (i32, i32) {
    for cy in 12..WORLD_SIZE - 16 {
        for cx in 12..WORLD_SIZE - 16 {
            if !(-8..3).all(|dx| (-8..3).all(|dy| is_buildable_tile(seed, cx + dx, cy + dy))) {
                continue;
            }
            let q = saladin_sim::soil_quality(seed, 2, center(cx, cy).x, center(cx, cy).y);
            if rich && q > FARM_MIN_FERTILITY + fx!("0.08") {
                return (cx, cy);
            }
            if !rich && q < FARM_MIN_FERTILITY - fx!("0.06") {
                return (cx, cy);
            }
        }
    }
    panic!("no {} block on seed {seed}", if rich { "fertile" } else { "barren" });
}

fn soil_at(seed: u32, tx: i32, ty: i32) -> Fx {
    let c = center(tx, ty);
    saladin_sim::soil_quality(seed, 2, c.x, c.y)
}

fn farms(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).filter(|b| b.kind == BuildingKind::Farm).count()
}

/// The one field on the map: its id, its node row and its crop.
fn field_row(app: &mut App) -> (u64, ResourceNode, Crop) {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &ResourceNode, &FieldOf, Option<&Crop>)>();
    q.iter(world)
        .map(|(g, n, _, c)| (g.0, *n, c.copied().unwrap_or_default()))
        .next()
        .expect("no field on the map")
}

/// Hands standing in the fields, as the sim counts them.
fn tending(app: &mut App) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).filter(|b| b.kind == BuildingKind::Farm).map(|b| b.builders).sum()
}

/// Put the season where a test needs it. Growing a full field in real ticks is
/// the point of other tests; here it is a 400-tick preamble to the assertion.
fn pin_crop(app: &mut App, remaining: i32, ripe: bool, standing: i32) {
    let world = app.world_mut();
    let mut q = world.query::<(&mut ResourceNode, &mut Crop, &FieldOf)>();
    for (mut n, mut c, _) in q.iter_mut(world) {
        n.remaining = remaining;
        c.ripe = ripe;
        c.standing = standing;
    }
}

fn stock_food(app: &mut App) -> i32 {
    let world = app.world_mut();
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == 1).unwrap().stock.food
}

/// Take every hand out of the fields — the wall that has to go up now, and the
/// other half of the labour decision.
fn recall_hands(app: &mut App, ids: &[u64], to: V2) {
    for &u in ids {
        cmd(app, PlayerCommand::Move { player_id: 1, unit: u, target: to });
    }
    step(app.world_mut());
}

/// Step until `pred` holds, up to `budget` ticks. Returns the ticks it took.
fn run_until(app: &mut App, budget: u32, pred: impl Fn(&mut App) -> bool) -> Option<u32> {
    for t in 0..budget {
        step(app.world_mut());
        if pred(app) {
            return Some(t);
        }
    }
    None
}

/// A farm on good soil with a drop-off at its edge and the crew that raised it
/// still standing in the furrows. FARMING IS THE SHORT HAUL: this is the layout
/// the whole design is pitched at, and the one that used to kill a field in 11
/// seconds.
fn farmstead(seed: u32) -> (App, (i32, i32), Vec<u64>) {
    let (bx, by) = block(seed, true);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    spawn_building(&mut app, 13, 1, BuildingKind::Storehouse, center(bx + 3, by + 3));
    let hands = crew(&mut app, 1, center(bx, by), 3, 20);
    cmd(&mut app, PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::Farm,
        pos: center(bx, by),
        facing: 0,
        builders: hands.clone(),
    });
    let up = run_until(&mut app, 900, |a| {
        let world = a.world_mut();
        let mut q = world.query::<&Building>();
        q.iter(world).any(|b| b.kind == BuildingKind::Farm && b.complete())
    });
    assert!(up.is_some(), "the crew never raised the farm");
    (app, (bx, by), hands)
}

fn fields(app: &mut App) -> Vec<ResourceNode> {
    let world = app.world_mut();
    let mut q = world.query::<(&ResourceNode, &FieldOf)>();
    q.iter(world).map(|(n, _)| *n).collect()
}

#[test]
fn a_field_needs_soil_worth_sowing() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);

    let (bx, by) = block(seed, false);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(bx, by), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(farms(&mut app), 0, "nothing grows on barren ground");

    let (fx_, fy) = block(seed, true);
    spawn_keep(&mut app, 11, 1, center(fx_ - 5, fy - 5));
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: center(fx_, fy), facing: 0, builders: vec![] });
    step(app.world_mut());
    assert_eq!(farms(&mut app), 1, "good soil takes the plough");
}

#[test]
fn a_sown_field_is_harvestable_and_regrows() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");

    let sown = fields(&mut app);
    assert_eq!(sown.len(), 1, "sowing a farm plants exactly one field");
    assert_eq!(sown[0].res_type, ResourceType::Food);
    // The yield is the SOIL's now, not one flat number: a field carries between
    // the thinnest ground that clears the gate and the richest in the world.
    assert!(
        (FARM_CAP_MIN..=FARM_CAP_MAX).contains(&sown[0].cap),
        "a field's yield left the soil range: {}",
        sown[0].cap
    );
    assert_eq!(sown[0].cap, field_cap(soil_at(seed, bx, by)), "the soil did not set the yield");
    assert!(sown[0].regen > 0, "a field that never regrows is just a slow node");

    // eat most of it, then let the economy tick run
    {
        let world = app.world_mut();
        let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
        for (mut n, _) in q.iter_mut(world) {
            n.remaining = 4;
        }
    }
    for _ in 0..41 {
        step(app.world_mut());
    }
    let after = fields(&mut app);
    assert!(after[0].remaining > 4, "the field did not grow back (still {})", after[0].remaining);
    assert!(after[0].remaining <= after[0].cap, "a field grew past its capacity");
}

#[test]
fn razing_a_farm_takes_its_crop() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");
    assert_eq!(fields(&mut app).len(), 1);

    let farm_id = {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == BuildingKind::Farm).map(|(g, _)| g.0).unwrap()
    };
    cmd(&mut app, PlayerCommand::Demolish { player_id: 1, building: farm_id });
    for _ in 0..5 {
        step(app.world_mut());
    }
    assert_eq!(farms(&mut app), 0);
    assert!(fields(&mut app).is_empty(), "the crop outlived the farm");
}

/// The whole season under lockstep: a farm rises, sows, grows under a crew,
/// ripens, is reaped bare and sows itself again while a second field lodges —
/// 800 ticks, hashes compared EVERY one of them. `cap`, `regen` and the crop are
/// all hashed, so a peer that computed a different yield or a different stage is
/// caught here and not minutes later as drifted unit positions.
#[test]
fn farms_keep_two_worlds_in_lockstep() {
    let seed = compose_seed(11, 1);
    let (bx, by) = block(seed, true);
    let mut worlds: Vec<App> = (0..2)
        .map(|_| {
            let mut app = build_app(seed);
            spawn_player(&mut app, 1);
            spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
            spawn_building(&mut app, 13, 1, BuildingKind::Storehouse, center(bx + 3, by + 3));
            let hands = crew(&mut app, 1, center(bx, by), 3, 20);
            crew(&mut app, 1, center(bx, by), 1, 40);
            cmd(&mut app, PlayerCommand::Build {
                player_id: 1,
                kind: BuildingKind::Farm,
                pos: center(bx, by),
                facing: 0,
                builders: hands,
            });
            app
        })
        .collect();
    let mut sent = false;
    for t in 0..800 {
        // Every world runs the same script at the same tick, which is what
        // lockstep IS: the reaper is sent the moment the first crop is in, and
        // one field is pinned to the edge of lodging so the bleed runs too.
        if !sent && t > 400 {
            let field = field_row(&mut worlds[0]).0;
            for app in &mut worlds {
                cmd(app, PlayerCommand::Gather { player_id: 1, unit: 40, node: field });
                let world = app.world_mut();
                let mut q = world.query::<&mut Crop>();
                for mut c in q.iter_mut(world) {
                    c.standing = FARM_RIPE_GRACE;
                }
            }
            sent = true;
        }
        for app in &mut worlds {
            step(app.world_mut());
        }
        let a = worlds[0].world().resource::<StateHash>().0;
        let b = worlds[1].world().resource::<StateHash>().0;
        assert_eq!(a, b, "the crop season desynced the two worlds at tick {t}");
    }
    // and the run actually covered the season it claims to
    let (_, n, _) = field_row(&mut worlds[0]);
    assert!(n.cap >= FARM_CAP_MIN, "the run never sowed a field");
    assert!(stock_food(&mut worlds[0]) > 9000, "the run never reaped one");
}

/// A field carries its season across the disk: reloaded, it must come back at
/// the same point in the year, and go on ticking bit-identically against the
/// world it was copied from.
#[test]
fn a_reloaded_field_stays_in_lockstep_with_the_world_it_left() {
    let seed = compose_seed(11, 1);
    let (mut live, _, _) = farmstead(seed);
    // one field mid-season, and the crop pinned to the edge of the grace so the
    // restored copy has to reproduce a LODGE and not just a growth curve
    let cap = field_row(&mut live).1.cap;
    pin_crop(&mut live, cap, true, FARM_RIPE_GRACE - 2);

    let bytes = save::to_bytes(&save::snapshot(live.world_mut()));
    let mut loaded = build_app(seed);
    save::restore(loaded.world_mut(), save::from_bytes(&bytes).expect("savegame parses"));

    let (_, n, c) = field_row(&mut loaded);
    assert_eq!(n.cap, cap, "the reloaded field forgot its yield");
    assert!(c.ripe && c.standing == FARM_RIPE_GRACE - 2, "the reloaded field restarted its season");

    for t in 0..400 {
        step(live.world_mut());
        step(loaded.world_mut());
        assert_eq!(
            live.world().resource::<StateHash>().0,
            loaded.world().resource::<StateHash>().0,
            "a reloaded field drifted from the world it left at tick {t}"
        );
    }
    assert!(field_row(&mut loaded).1.remaining < cap, "the pinned crop never lodged");
}

/// The Granary's whole role: it stores nothing and instead WORKS the fields in
/// its reach. Two identical farms, one hubbed and one not.
/// The state hash was blind to everything a field carries but `remaining`.
/// `cap` and `regen` VARY per farm (the soil sets them at sowing) and the
/// whole crop state is new, so two peers could disagree about a field's yield
/// or its stage and pass the desync check until the drift leaked out minutes
/// later as drifted unit positions.
#[test]
fn the_state_hash_sees_a_crops_stage_and_a_fields_yield() {
    let seed = compose_seed(11, 1);
    let hash_of = |mutate: &dyn Fn(&mut App)| {
        let mut app = build_app(seed);
        spawn_player(&mut app, 1);
        app.world_mut().spawn((
            GameId(50),
            Owner(1),
            MatchId(1),
            FieldOf(10),
            Crop::default(),
            Pos { pos: center(40, 40), facing: ZERO },
            ResourceNode::renewable(ResourceType::Food, 40, FARM_STORE, 3),
        ));
        mutate(&mut app);
        step(app.world_mut());
        app.world().resource::<StateHash>().0
    };
    let base = hash_of(&|_| {});
    #[allow(clippy::type_complexity)]
    let variants: [(&str, &dyn Fn(&mut App)); 4] = [
        ("cap", &|app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
            for (mut n, _) in q.iter_mut(world) {
                n.cap = FARM_STORE * 2;
            }
        }),
        ("regen", &|app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
            for (mut n, _) in q.iter_mut(world) {
                n.regen = 6;
            }
        }),
        ("ripe", &|app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<&mut Crop>();
            for mut c in q.iter_mut(world) {
                c.ripe = true;
            }
        }),
        ("standing", &|app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<&mut Crop>();
            for mut c in q.iter_mut(world) {
                c.standing = 7;
            }
        }),
    ];
    for (what, mutate) in variants {
        assert_ne!(base, hash_of(mutate), "the state hash cannot see a field's {what}");
    }
}

/// A field's crop rides the same saved row as its node: a reloaded farm must
/// come back at the same point in its season, not at the start of one.
#[test]
fn a_saved_field_keeps_its_crop_stage() {
    let seed = compose_seed(11, 1);
    let mut a = build_app(seed);
    spawn_player(&mut a, 1);
    a.world_mut().spawn((
        GameId(50),
        Owner(1),
        MatchId(1),
        FieldOf(10),
        Crop { ripe: true, standing: 12 },
        Pos { pos: center(40, 40), facing: ZERO },
        ResourceNode::renewable(ResourceType::Food, 40, FARM_STORE, 3),
    ));
    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    let mut b = build_app(seed);
    save::restore(b.world_mut(), save::from_bytes(&bytes).expect("savegame parses"));
    let world = b.world_mut();
    let mut q = world.query::<(&Crop, &FieldOf)>();
    let crops: Vec<Crop> = q.iter(world).map(|(c, _)| *c).collect();
    assert_eq!(crops, vec![Crop { ripe: true, standing: 12 }], "the crop did not survive the disk");
}

#[test]
fn a_granary_makes_the_fields_around_it_grow_faster() {
    let seed = compose_seed(11, 1);
    let mut app = build_app(seed);
    spawn_player(&mut app, 1);
    let (bx, by) = block(seed, true);
    spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
    assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");
    assert_eq!(fields(&mut app).len(), 1);

    let drain = |app: &mut App| {
        let world = app.world_mut();
        let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
        for (mut n, _) in q.iter_mut(world) {
            n.remaining = 1;
        }
    };
    drain(&mut app);
    for _ in 0..41 {
        step(app.world_mut());
    }
    let bare = fields(&mut app)[0].remaining;

    // the same field, now inside a granary's reach
    spawn_building(&mut app, 12, 1, BuildingKind::Granary, center(bx + 3, by + 3));
    drain(&mut app);
    for _ in 0..41 {
        step(app.world_mut());
    }
    let hubbed = fields(&mut app)[0].remaining;
    assert!(hubbed > bare, "granary aura did nothing ({bare} -> {hubbed})");
}

/// The aura is a work bonus its OWNER pays for. A granary the enemy built next
/// to your fields tended them for free, because the regen loop matched on
/// geography alone and never asked whose crop it was.
#[test]
fn an_enemy_granary_does_not_tend_your_fields() {
    let seed = compose_seed(11, 1);
    let (bx, by) = block(seed, true);

    let run = |granary_owner: Option<u64>| -> i32 {
        let mut app = build_app(seed);
        spawn_player(&mut app, 1);
        spawn_player(&mut app, 2);
        spawn_keep(&mut app, 10, 1, center(bx - 5, by - 5));
        assert!(raise_farm(&mut app, center(bx, by), 20), "the crew never raised the farm");
        if let Some(o) = granary_owner {
            spawn_building(&mut app, 12, o, BuildingKind::Granary, center(bx + 3, by + 3));
        }
        {
            let world = app.world_mut();
            let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
            for (mut n, _) in q.iter_mut(world) {
                n.remaining = 1;
            }
        }
        for _ in 0..41 {
            step(app.world_mut());
        }
        fields(&mut app)[0].remaining
    };

    let bare = run(None);
    let mine = run(Some(1));
    let theirs = run(Some(2));
    assert!(mine > bare, "your own granary must tend your fields ({bare} -> {mine})");
    assert_eq!(theirs, bare, "an enemy granary tended your crop ({bare} -> {theirs})");
}

/// The whole point of a farm: a peasant walks to the crop, cuts it and carries
/// it home. The field node sits at the farm's own footprint CENTRE, which on an
/// even footprint is the corner shared by four tiles the building occupies — so
/// the nearest ground a harvester can stand on is over a tile away and a plain
/// HARVEST_RANGE test can never be satisfied. Every earlier farming test
/// asserted the field EXISTED and REGREW; none of them ever ate from it.
///
/// `food > before` was BLIND: the field handed over most of its store as it
/// destroyed itself, so the one assertion that was supposed to prove farming
/// worked was satisfied by farming failing. It now has to bring in more than one
/// whole field's worth, which only a second season can pay for.
#[test]
fn a_peasant_can_actually_eat_from_a_farm() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), _) = farmstead(seed);
    let (field, node, _) = field_row(&mut app);
    let cap = node.cap;

    let hand = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    let before = stock_food(&mut app);
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: hand, node: field });
    let mut was_ripe = false;
    let mut hauled = 0;
    for _ in 0..6000 {
        step(app.world_mut());
        let (_, _, c) = field_row(&mut app);
        // the season comes round again on its own; the reaper has to be sent
        // back to it (the tending crew does that itself once the labour loop
        // lands — what is under test here is that there IS a second harvest)
        if c.ripe && !was_ripe {
            cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: hand, node: field });
        }
        was_ripe = c.ripe;
        hauled = stock_food(&mut app) - before;
        if hauled > cap {
            break;
        }
    }
    assert!(hauled > cap, "one field, two seasons: only {hauled} came in against {cap}");
    assert!(alive(&mut app, field), "the field paid for that harvest with its life");
}


/// THE assertion the suite never had. `gather.rs` deleted any node it drew to
/// zero, and a peasant's carry divides a field's yield exactly, so every worked
/// farm in the game deleted its own crop and left 50 wood of scenery. A field is
/// STUBBLE when it is cut, not a hole: the row survives and sows itself again.
#[test]
fn a_worked_field_survives_being_reaped_bare() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), _) = farmstead(seed);
    let (field, node, _) = field_row(&mut app);
    let cap = node.cap;
    pin_crop(&mut app, cap, true, 0);

    let reaper = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: reaper, node: field });
    let before = stock_food(&mut app);

    // cut it down to nothing, checking the row is still there every single tick
    let bare = run_until(&mut app, 3000, |a| {
        assert!(alive(a, field), "the field was DELETED the moment it was reaped bare");
        field_row(a).1.remaining == 0
    });
    assert!(bare.is_some(), "the reaper never emptied the field");
    assert!(alive(&mut app, field), "the field did not outlive its own harvest");
    let (_, _, crop) = field_row(&mut app);
    assert!(!crop.ripe, "a field cut to stubble is not a standing crop");

    // and a SECOND season comes in off the same ground
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: reaper, node: field });
    let two = run_until(&mut app, 6000, |a| stock_food(a) >= before + cap + cap / 2);
    assert!(
        two.is_some(),
        "one field, two seasons: only {} came in against {}",
        stock_food(&mut app) - before,
        cap + cap / 2
    );
}

fn alive(app: &mut App, id: u64) -> bool {
    let world = app.world_mut();
    let mut q = world.query::<&GameId>();
    q.iter(world).any(|g| g.0 == id)
}

/// A GROWING CROP CANNOT BE CUT. Everything else in the season rests on this
/// one gate: draw can never outrun growth, because until the harvest is in there
/// is nothing to take.
#[test]
fn an_unripe_field_cannot_be_cut() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), hands) = farmstead(seed);
    recall_hands(&mut app, &hands, center(bx - 6, by));
    let (field, node, crop) = field_row(&mut app);
    assert!(!crop.ripe, "a field is sown part-grown, not ready to cut");
    let sown = node.remaining;

    let reaper = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: reaper, node: field });
    let before = stock_food(&mut app);
    for _ in 0..600 {
        step(app.world_mut());
        let (_, n, c) = field_row(&mut app);
        assert!(!c.ripe, "the field ripened under an untended season - test is blind");
        assert!(n.remaining >= sown, "a growing crop was cut ({} of {sown})", n.remaining);
    }
    assert_eq!(stock_food(&mut app), before, "the standing green went into the granary");
    assert_eq!(unit_of(&mut app, reaper).carrying, 0, "the reaper walked off with the seedlings");
}

fn unit_of(app: &mut App, id: u64) -> Unit {
    let world = app.world_mut();
    let mut q = world.query::<(&GameId, &Unit)>();
    q.iter(world).find(|(g, _)| g.0 == id).map(|(_, u)| u.clone()).unwrap()
}

/// The recurring decision, and the only one: HOW MANY HANDS DO I LEAVE IN THE
/// FIELDS. Same soil, same field, three staffings.
#[test]
fn hands_bring_the_season_in_faster() {
    let seed = compose_seed(11, 1);
    let grown = |keep: usize| -> i32 {
        let (mut app, (bx, by), hands) = farmstead(seed);
        recall_hands(&mut app, &hands[keep..], center(bx - 6, by));
        // start well short of capacity so the whole window is GROWTH, never a
        // clamp at the top
        pin_crop(&mut app, 8, false, 0);
        for _ in 0..201 {
            step(app.world_mut());
        }
        assert_eq!(tending(&mut app), keep as i32, "the crew did not stay in the field");
        let (_, n, c) = field_row(&mut app);
        assert!(!c.ripe && n.remaining < n.cap, "the window hit the cap and stopped measuring");
        n.remaining - 8
    };
    let (idle, one, two) = (grown(0), grown(1), grown(2));
    assert!(idle > 0, "a field nobody works must still creep in on the rain");
    assert!(one > idle, "one hand added nothing ({idle} -> {one})");
    assert!(two > one, "a second hand added nothing ({one} -> {two})");
    // and the curve diminishes, which is why three fields beat one field
    assert!(two - one < one - idle, "the tending curve does not diminish");
}

/// This is what finally pays off the fertility overlay the shader already
/// paints while siting: the number it shows still matters after the ghost
/// disappears. A truncated `1 + soil * 7` built the same farm on 0.30 and 0.42.
#[test]
fn rich_soil_grows_a_bigger_harvest() {
    let seed = compose_seed(11, 1);
    let thin = block_soil(seed, FARM_MIN_FERTILITY, FARM_MIN_FERTILITY + fx!("0.04"));
    let rich = block_soil(seed, FARM_MIN_FERTILITY + fx!("0.09"), fx!("1"));
    assert!(soil_at(seed, rich.0, rich.1) > soil_at(seed, thin.0, thin.1));

    let sow = |at: (i32, i32)| -> i32 {
        let mut app = build_app(seed);
        spawn_player(&mut app, 1);
        spawn_keep(&mut app, 10, 1, center(at.0 - 5, at.1 - 5));
        assert!(raise_farm(&mut app, center(at.0, at.1), 20), "the crew never raised the farm");
        field_row(&mut app).1.cap
    };
    let (poor, good) = (sow(thin), sow(rich));
    assert!(
        good > poor,
        "soil {} and soil {} built the same farm ({poor})",
        soil_at(seed, thin.0, thin.1),
        soil_at(seed, rich.0, rich.1)
    );
    assert!((FARM_CAP_MIN..=FARM_CAP_MAX).contains(&poor));
    assert!((FARM_CAP_MIN..=FARM_CAP_MAX).contains(&good));
}

/// A 2x2 farm block whose soil lands in `[lo, hi)`, with buildable room beside
/// it for the anchoring keep. Same helper `check_place` measures with.
fn block_soil(seed: u32, lo: Fx, hi: Fx) -> (i32, i32) {
    for cy in 12..WORLD_SIZE - 16 {
        for cx in 12..WORLD_SIZE - 16 {
            if !(-8..3).all(|dx| (-8..3).all(|dy| is_buildable_tile(seed, cx + dx, cy + dy))) {
                continue;
            }
            let q = soil_at(seed, cx, cy);
            if q >= lo && q < hi {
                return (cx, cy);
            }
        }
    }
    panic!("no block on seed {seed} with soil in [{lo}, {hi})");
}

/// The failure state for neglect, and the reason the wheel does not stop at a
/// gold field sitting forever as free storage. A grace first, then a slow
/// VISIBLE bleed, salvageable the whole way down, and the row is never deleted.
#[test]
fn a_ripe_crop_left_standing_lodges_and_can_still_be_salvaged() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), hands) = farmstead(seed);
    // NEGLECT is the premise: the crew that raised the plot reaps its own crop
    // the moment it comes in, so measuring a crop nobody cuts means taking the
    // hands out first. That is the decision this state prices.
    recall_hands(&mut app, &hands, center(bx - 6, by));
    let (field, node, _) = field_row(&mut app);
    let cap = node.cap;

    // the grace: a ripe crop is not punished for the first minute
    pin_crop(&mut app, cap, true, 0);
    for _ in 0..401 {
        step(app.world_mut());
    }
    let held = field_row(&mut app).1.remaining;
    assert_eq!(held, cap, "the crop started falling inside its grace ({held} of {cap})");

    // past it, it lodges — gradually, and it is still a legal harvest
    pin_crop(&mut app, cap, true, FARM_RIPE_GRACE);
    for _ in 0..401 {
        step(app.world_mut());
    }
    let (_, lodged, crop) = field_row(&mut app);
    assert!(lodged.remaining < cap, "a crop nobody cut stood forever");
    assert!(lodged.remaining > cap / 2, "the crop vanished instead of lodging");
    assert!(crop.ripe, "a lodging crop is falling, not forbidden");
    assert!(alive(&mut app, field), "lodging deleted the field");

    // salvage: hands sent back in still bring the fallen crop home
    let reaper = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: reaper, node: field });
    let before = stock_food(&mut app);
    let saved = run_until(&mut app, 2000, |a| stock_food(a) > before);
    assert!(saved.is_some(), "a lodged crop could not be salvaged at all");
}

/// The wheel: sow -> grow -> ripen -> reap -> STUBBLE -> sow again, with no
/// click anywhere in it. Three full seasons off one plot.
#[test]
fn a_field_reaped_bare_sows_itself_again() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), _) = farmstead(seed);
    let (field, _, _) = field_row(&mut app);
    // A SMALL field, so three whole seasons fit in a test: `cap` is a per-field
    // number the soil chose, and choosing it here is the only way to run the
    // loop three times without running the clock for ten minutes.
    {
        let world = app.world_mut();
        let mut q = world.query::<(&mut ResourceNode, &FieldOf)>();
        for (mut n, _) in q.iter_mut(world) {
            n.cap = 24;
            n.remaining = 0;
        }
    }
    let reaper = crew(&mut app, 1, center(bx, by), 1, 40)[0];
    let before = stock_food(&mut app);

    let mut seasons = 0;
    let mut was_ripe = false;
    for _ in 0..4000 {
        step(app.world_mut());
        assert!(alive(&mut app, field), "the field died mid-wheel");
        let (_, _, c) = field_row(&mut app);
        if c.ripe && !was_ripe {
            seasons += 1;
            // the harvest is in: send the reaper (the tending crew does this
            // itself once the labour loop lands — the SEASON is what is under
            // test here)
            cmd(&mut app, PlayerCommand::Gather { player_id: 1, unit: reaper, node: field });
        }
        was_ripe = c.ripe;
        if seasons >= 3 && stock_food(&mut app) >= before + 48 {
            break;
        }
    }
    assert!(seasons >= 3, "the field ran {seasons} seasons, not three");
    assert!(
        stock_food(&mut app) >= before + 48,
        "three seasons brought in {}",
        stock_food(&mut app) - before
    );
}

/// The one way the tend -> reap -> haul -> tend round trip could lose a man.
/// Every failure on the reaping leg (the crop gone, no route, and this one — the
/// drop-off razed under a loaded carrier) stands him down HOLDING his `job_site`,
/// and construction only ever looks at hands already in `Constructing`. Nothing
/// was looking for him, so a farmhand who lost his stockpile stood in the furrows
/// for the rest of the match — reading as exactly the complaint this work answers.
#[test]
fn a_farmhand_who_loses_his_stockpile_goes_back_to_his_plot() {
    let seed = compose_seed(11, 1);
    let (mut app, _, hands) = farmstead(seed);
    let (field, node, _) = field_row(&mut app);
    // the crew that raised the plot stays in it — that is what makes them
    // farmhands rather than builders who finished a job
    let staffed = run_until(&mut app, 200, |a| tending(a) > 0);
    assert!(staffed.is_some(), "the crew did not stay on the farm");
    pin_crop(&mut app, node.cap, true, 0);

    // the crew reaps its own crop without an order, then the town burns
    let carrying = run_until(&mut app, 2000, |a| {
        hands.iter().any(|&h| unit_of(a, h).carrying > 0)
    });
    assert!(carrying.is_some(), "the tending crew never went to cut the ripe crop");
    for id in [10u64, 13] {
        let e = {
            let world = app.world_mut();
            let mut q = world.query::<(bevy_ecs::entity::Entity, &GameId)>();
            q.iter(world).find(|(_, g)| g.0 == id).map(|(e, _)| e)
        };
        if let Some(e) = e {
            app.world_mut().entity_mut(e).despawn();
        }
    }

    // stranded: no drop-off exists, so the carrier is stood down holding a load
    let stranded = run_until(&mut app, 200, |a| {
        hands.iter().any(|&h| unit_of(a, h).gather_state == GatherState::Idle)
    });
    assert!(stranded.is_some(), "premise gone: nobody was stood down by the razed town");

    // and the farm gets its hands back on its own
    let recovered = run_until(&mut app, 400, |a| tending(a) > 0);
    assert!(
        recovered.is_some(),
        "the crew stood in the field forever: {:?}",
        hands.iter().map(|&h| unit_of(&mut app, h).gather_state).collect::<Vec<_>>()
    );
    assert!(alive(&mut app, field), "the field died with the town");
}

/// The stale half of the same rescue: a hand booked on a site that no longer
/// exists must be let go, not walked back to rubble forever.
#[test]
fn a_hand_booked_on_a_razed_site_is_let_go() {
    let seed = compose_seed(11, 1);
    let (mut app, (bx, by), _) = farmstead(seed);
    let idler = crew(&mut app, 1, center(bx, by), 1, 60)[0];
    {
        let world = app.world_mut();
        let mut q = world.query::<(&GameId, &mut Unit)>();
        for (g, mut u) in q.iter_mut(world) {
            if g.0 == idler {
                u.gather_state = GatherState::Idle;
                u.job_site = 9_999; // a building id that never existed
            }
        }
    }
    for _ in 0..40 {
        step(app.world_mut());
    }
    let u = unit_of(&mut app, idler);
    assert_eq!(u.job_site, 0, "the hand is still booked on rubble");
    assert_eq!(u.gather_state, GatherState::Idle, "a ghost site put him to work");
}

