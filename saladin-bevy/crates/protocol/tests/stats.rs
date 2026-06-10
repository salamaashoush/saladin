//! MatchStats: the sim's running totals (trained / lost / gathered) feed the
//! victory screen — verify each counter at its source and that two lockstep
//! worlds tally identically.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    BuildingKind, Faction, Fx, GatherState, ResourceType, Stance, Stockpile,
    UnitKind, V2, ZERO, building_def, is_passable, unit_def,
};

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    app
}

fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn find_land_block(seed: u32) -> (i32, i32) {
    for cy in 16..128 {
        for cx in 16..128 {
            if (0..6).all(|dx| (0..6).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no 6x6 land block found");
}

fn rich() -> Stockpile {
    Stockpile { wood: 10_000, stone: 10_000, food: 10_000, gold: 10_000 }
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: format!("P{id}"),
            faction: Faction::Ayyubid,
            stock: rich(),
            color: id as u8,
            online: true,
            keep: 0,
            defeated: false,
            slot: id as u8,
            tech_mask: 0,
            hunger: 0,
        },
    ));
}

fn spawn_keep(app: &mut App, gid: u64, owner: u64, pos: V2) {
    let def = building_def(BuildingKind::Keep);
    app.world_mut().spawn((
        GameId(gid),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Building { kind: BuildingKind::Keep, hp: def.max_hp, cooldown: ZERO, rally: pos },
    ));
}

fn spawn_soldier(app: &mut App, gid: u64, owner: u64, pos: V2, hp: i32) {
    let def = unit_def(UnitKind::Spearman);
    app.world_mut().spawn((
        GameId(gid),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind: UnitKind::Spearman,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp,
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

#[test]
fn training_and_combat_losses_are_counted() {
    let mut app = build();
    let (cx, cy) = find_land_block(1);
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    spawn_player(&mut app, 1);
    spawn_player(&mut app, 2);
    spawn_keep(&mut app, 10, 1, V2::new(f(cx + 1), f(cy + 1)));

    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    cmd(&mut app, PlayerCommand::Train { player_id: 1, kind: UnitKind::Peasant });
    step(app.world_mut());
    assert_eq!(
        app.world().resource::<MatchStats>().0.get(&1).map(|s| s.trained),
        Some(2),
        "two peasants trained"
    );

    // a doomed 1-hp defender beside a full-strength attacker
    spawn_soldier(&mut app, 20, 1, V2::new(f(cx + 3), f(cy + 3)), 1);
    spawn_soldier(&mut app, 21, 2, V2::new(f(cx + 4), f(cy + 3)), 200);
    for _ in 0..200 {
        step(app.world_mut());
    }
    let world = app.world_mut();
    let lost = world.resource::<MatchStats>().0.get(&1).map(|s| s.lost).unwrap_or(0);
    assert!(lost >= 1, "the 1-hp spearman's death is tallied (lost={lost})");
}

#[test]
fn stats_match_across_lockstep_worlds() {
    let relay = shared_relay(vec![1, 2]);
    let mut worlds = Vec::new();
    for player in [1u64, 2u64] {
        let mut app = build();
        scatter_world_nodes(app.world_mut(), 1);
        let mut driver = LockstepDriver::new(player, 1);
        driver.push(PlayerCommand::Join {
            player_id: player,
            name: format!("P{player}"),
            faction: if player == 1 { Faction::Ayyubid } else { Faction::Crusader },
            match_id: 1,
        });
        worlds.push((app, driver, MemTransport::new(relay.clone())));
    }
    let mut done = [0u32; 2];
    while done.iter().any(|&d| d < 400) {
        for (i, (app, driver, transport)) in worlds.iter_mut().enumerate() {
            if done[i] < 400 && driver.advance(app.world_mut(), transport) {
                done[i] += 1;
            }
        }
    }
    let collect = |app: &mut App| {
        let mut v: Vec<(u64, PlayerStats)> =
            app.world().resource::<MatchStats>().0.iter().map(|(k, s)| (*k, *s)).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    };
    let (mut a, mut b) = {
        let mut it = worlds.into_iter();
        (it.next().unwrap(), it.next().unwrap())
    };
    assert_eq!(
        a.0.world().resource::<StateHash>().0,
        b.0.world().resource::<StateHash>().0,
        "worlds in sync"
    );
    let sa = collect(&mut a.0);
    let sb = collect(&mut b.0);
    assert_eq!(sa, sb, "stat tallies identical on every client");
    // founding players start gathering immediately — gathered grows
    assert!(
        sa.iter().any(|(_, s)| s.gathered > 0),
        "auto-gathering peasants banked something in 400 ticks: {sa:?}"
    );
}
