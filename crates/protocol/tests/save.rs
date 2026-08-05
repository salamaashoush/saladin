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
