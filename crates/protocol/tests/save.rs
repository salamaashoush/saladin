//! Save/restore round-trip: a running match snapshotted, restored into a fresh
//! world, must continue bit-identically (same state hash trajectory).

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, FARM_STORE, Faction, ResourceType, Stockpile, V2, ZERO,
    building_def,
};

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    app
}

fn fields(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<(&ResourceNode, &FieldOf)>();
    q.iter(world).count()
}

#[test]
fn save_restore_resumes_bit_identically() {
    let mut a = build();
    scatter_world_nodes(a.world_mut(), 1);
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Join {
        player_id: 1,
        name: "Saladin".into(),
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1000,
        host: 1,
        difficulty: AiDifficulty::Normal,
        faction: Faction::Crusader,
        match_id: 1,
    });
    for _ in 0..200 {
        step(a.world_mut());
    }

    // snapshot mid-match, push through bytes like the real save file
    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    let snap = save::from_bytes(&bytes).expect("savegame parses");

    let mut b = build();
    save::restore(b.world_mut(), snap);

    // both worlds must now evolve identically
    for i in 0..200 {
        step(a.world_mut());
        step(b.world_mut());
        let ha = a.world().resource::<StateHash>().0;
        let hb = b.world().resource::<StateHash>().0;
        assert_eq!(ha, hb, "restored world diverged at step {i}");
    }
}

/// A farm's crop is tied to the farm by `FieldOf`. A restore that drops the
/// link leaves an immortal free-food node behind and disarms the reaper.
#[test]
fn a_restored_field_still_belongs_to_its_farm() {
    let mut a = build();
    let pos = V2::new(saladin_sim::Fx::from_num(40), saladin_sim::Fx::from_num(40));
    a.world_mut().spawn((
        GameId(900),
        MatchId(1),
        Player {
            player_id: 1,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 500, stone: 500, food: 500, gold: 500 },
            color: 0,
            online: true,
            keep: 0,
            defeated: false,
            slot: 0,
            tech_mask: 0,
            hunger: 0,
        },
    ));
    let def = building_def(BuildingKind::Farm);
    a.world_mut().spawn((
        GameId(10),
        Owner(1),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building::new(BuildingKind::Farm, def.max_hp, pos),
    ));
    a.world_mut().spawn((
        GameId(11),
        Owner(1),
        MatchId(1),
        FieldOf(10),
        Pos { pos, facing: ZERO },
        ResourceNode::renewable(ResourceType::Food, FARM_STORE / 3, FARM_STORE, 3),
    ));
    step(a.world_mut());
    assert_eq!(fields(&mut a), 1);

    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    let mut b = build();
    save::restore(b.world_mut(), save::from_bytes(&bytes).expect("savegame parses"));
    assert_eq!(fields(&mut b), 1, "the crop came back without its farm");

    b.world_mut()
        .resource_mut::<CommandQueue>()
        .0
        .push(PlayerCommand::Demolish { player_id: 1, building: 10 });
    for _ in 0..5 {
        step(b.world_mut());
    }
    assert_eq!(fields(&mut b), 0, "a loaded farm left an immortal field behind");
}

#[test]
fn a_stale_save_is_refused() {
    let mut a = build();
    step(a.world_mut());
    let good = save::to_bytes(&save::snapshot(a.world_mut()));
    assert!(save::from_bytes(&good).is_some());

    assert!(save::from_bytes(&[]).is_none(), "an empty file parsed as a game");
    let mut wrong_magic = good.clone();
    wrong_magic[0] ^= 0xFF;
    assert!(save::from_bytes(&wrong_magic).is_none(), "a foreign file parsed as a game");
    let mut wrong_version = good.clone();
    wrong_version[8] = wrong_version[8].wrapping_add(1);
    assert!(save::from_bytes(&wrong_version).is_none(), "a stale save version was accepted");
}

/// A half-built hall, the crew on it and the orders in its queue are sim state,
/// so they have to survive the disk. If the state hash trajectory holds, so did
/// every field the construction loop reads.
#[test]
fn a_site_under_construction_survives_the_disk() {
    let mut a = build();
    scatter_world_nodes(a.world_mut(), 1);
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Join {
        player_id: 1,
        name: "Saladin".into(),
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(a.world_mut());

    let (keep, keep_pos, hands) = {
        let world = a.world_mut();
        let mut bq = world.query::<(&GameId, &Pos, &Building)>();
        let (g, p, _) =
            bq.iter(world).find(|(_, _, b)| b.kind == BuildingKind::Keep).expect("a keep");
        let (keep, keep_pos) = (g.0, p.pos);
        let hands: Vec<u64> = {
            let mut q = world.query::<(&GameId, &Unit)>();
            q.iter(world).map(|(g, _)| g.0).take(3).collect()
        };
        (keep, keep_pos, hands)
    };
    // an order in the queue and a hall going up beside the keep
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::TrainAt {
        player_id: 1,
        building: keep,
        kind: saladin_sim::UnitKind::Peasant,
    });
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Build {
        player_id: 1,
        kind: BuildingKind::House,
        pos: V2::new(keep_pos.x + saladin_sim::fx!("5"), keep_pos.y),
        facing: 0,
        builders: hands,
    });
    for _ in 0..60 {
        step(a.world_mut());
    }
    {
        let world = a.world_mut();
        let mut q = world.query::<&Building>();
        let house = q.iter(world).find(|b| b.kind == BuildingKind::House).expect("a house site");
        assert!(!house.complete(), "the house finished before the snapshot");
        assert!(house.work > ZERO, "no labour was banked to save");
        let keep_row = q.iter(world).find(|b| b.kind == BuildingKind::Keep).unwrap();
        assert_eq!(keep_row.queue_len, 1, "the keep's order vanished");
    }

    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    let mut b = build();
    save::restore(b.world_mut(), save::from_bytes(&bytes).expect("savegame parses"));

    for i in 0..400 {
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "restored construction diverged at step {i}"
        );
    }
    let world = b.world_mut();
    let mut q = world.query::<&Building>();
    assert!(
        q.iter(world).any(|x| x.kind == BuildingKind::House && x.complete()),
        "the restored crew never finished the house"
    );
}

fn find_land_block(seed: u32) -> (i32, i32) {
    for cy in 16..128 {
        for cx in 16..128 {
            if (0..10).all(|dx| (0..10).all(|dy| saladin_sim::is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no 10x10 land block found");
}

/// The whole combat layer has to survive the disk: a man running from the line,
/// an archer on a parapet, a wall someone has been hitting and an order in a
/// queue. Every one of those was invisible to the state hash until T2, so this
/// test could not have been written before it.
#[test]
fn a_battle_in_progress_survives_the_disk() {
    let seed = 1u32;
    let (cx, cy) = find_land_block(seed);
    let f = |n: i32| saladin_sim::Fx::from_num(n) + saladin_sim::fx!("0.5");
    let mut a = build();
    a.world_mut().insert_resource(WorldConfig { seed });
    for owner in [1u64, 2u64] {
        a.world_mut().spawn((
            GameId(900 + owner),
            MatchId(1),
            Player {
                player_id: owner,
                name: format!("P{owner}"),
                faction: Faction::Ayyubid,
                stock: Stockpile { wood: 999, stone: 999, food: 999, gold: 999 },
                color: 0,
                online: true,
                keep: 0,
                defeated: false,
                slot: owner as u8,
                tech_mask: 0,
                hunger: 0,
            },
        ));
    }
    let keep_pos = V2::new(f(cx + 2), f(cy + 2));
    a.world_mut().spawn((
        GameId(10),
        Owner(1),
        MatchId(1),
        Pos { pos: keep_pos, facing: ZERO },
        Building::new(BuildingKind::Keep, building_def(BuildingKind::Keep).max_hp, keep_pos),
    ));
    // houses, so the keep's order has population to fill
    for i in 0..3i32 {
        let hp_pos = V2::new(f(cx + 8), f(cy + 2 + i * 2));
        a.world_mut().spawn((
            GameId(12 + i as u64),
            Owner(1),
            MatchId(1),
            Pos { pos: hp_pos, facing: ZERO },
            Building::new(BuildingKind::House, building_def(BuildingKind::House).max_hp, hp_pos),
        ));
    }
    // a wall someone has been working on
    let wall_pos = V2::new(f(cx + 6), f(cy + 2));
    let wall_max = building_def(BuildingKind::Wall).max_hp;
    let mut wall = Building::new(BuildingKind::Wall, wall_max, wall_pos);
    wall.hp = wall_max / 3;
    a.world_mut().spawn((GameId(11), Owner(1), MatchId(1), Pos { pos: wall_pos, facing: ZERO }, wall));
    // an archer on the keep's parapet
    a.world_mut().spawn((
        GameId(20),
        Owner(1),
        MatchId(1),
        Pos { pos: keep_pos, facing: ZERO },
        Unit { garrisoned_in: 10, ..Unit::new(saladin_sim::UnitKind::Archer, keep_pos) },
    ));
    // a man already running
    let flee_pos = V2::new(f(cx + 3), f(cy + 5));
    a.world_mut().spawn((
        GameId(21),
        Owner(1),
        MatchId(1),
        Pos { pos: flee_pos, facing: ZERO },
        Unit {
            morale: saladin_sim::MORALE_MIN,
            routing: true,
            ..Unit::new(saladin_sim::UnitKind::Spearman, flee_pos)
        },
    ));
    // and a line still fighting
    for i in 0..6i32 {
        let ap = V2::new(f(cx + 2 + i), f(cy + 6));
        let bp = V2::new(f(cx + 2 + i), f(cy + 7));
        a.world_mut().spawn((
            GameId(30 + i as u64),
            Owner(1),
            MatchId(1),
            Pos { pos: ap, facing: ZERO },
            Unit::new(saladin_sim::UnitKind::Spearman, ap),
        ));
        a.world_mut().spawn((
            GameId(40 + i as u64),
            Owner(2),
            MatchId(1),
            Pos { pos: bp, facing: ZERO },
            Unit::new(saladin_sim::UnitKind::Knight, bp),
        ));
    }
    // an order in the keep's queue
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::TrainAt {
        player_id: 1,
        building: 10,
        kind: saladin_sim::UnitKind::Peasant,
    });
    for _ in 0..24 {
        step(a.world_mut());
    }
    {
        let world = a.world_mut();
        let mut q = world.query::<&Unit>();
        assert!(q.iter(world).any(|u| u.routing), "nobody was routing at the snapshot");
        assert!(q.iter(world).any(|u| u.garrisoned_in != 0), "the parapet emptied before the snapshot");
        assert!(q.iter(world).any(|u| u.attack_target != 0), "no one had picked a fight");
        let mut b = world.query::<&Building>();
        assert_eq!(b.iter(world).find(|x| x.kind == BuildingKind::Keep).unwrap().queue_len, 1);
    }

    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    let mut b = build();
    b.world_mut().insert_resource(WorldConfig { seed });
    save::restore(b.world_mut(), save::from_bytes(&bytes).expect("savegame parses"));

    for i in 0..400 {
        step(a.world_mut());
        step(b.world_mut());
        assert_eq!(
            a.world().resource::<StateHash>().0,
            b.world().resource::<StateHash>().0,
            "the restored battle diverged at step {i}"
        );
    }
}

/// bincode 1.3 is POSITIONAL: an older save fed to a newer row decodes as
/// garbage rather than failing, so the version header has to refuse it
/// outright. Every superseded version, not just the one before this one.
#[test]
fn a_stale_save_is_refused_not_misread() {
    let mut a = build();
    step(a.world_mut());
    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    const { assert!(save::SAVE_VERSION >= 3, "SAVE_VERSION 3 added the Crop component to the row") };
    assert!(save::from_bytes(&bytes).is_some(), "the current version refused its own save");
    for v in 0..save::SAVE_VERSION {
        let mut stale = bytes.clone();
        stale[8..12].copy_from_slice(&v.to_le_bytes());
        assert!(
            save::from_bytes(&stale).is_none(),
            "a v{v} save was accepted into a v{} world",
            save::SAVE_VERSION
        );
    }
}
