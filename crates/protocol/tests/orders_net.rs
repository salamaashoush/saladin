//! Group orders over the REAL relay: two clients on localhost, one issuing
//! mass army orders, hashes compared every tick. And the v7 handshake, which
//! is the only thing stopping a v6 peer from bincode-decoding a `GroupMove` as
//! whatever the old variant table said index 24 was.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{Faction, Fx, UnitKind, V2, WORLD_SIZE, fx, is_passable, unit_def};
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn world_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    scatter_world_nodes(app.world_mut(), 1);
    app
}

fn wait_for<F: FnMut() -> bool>(mut f: F, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A build one version behind must be REFUSED, not seated. `PlayerCommand`
/// gained four appended variants in v7 and bincode reads the variant index
/// positionally, so an old peer handed a `GroupMove` would decode it as garbage
/// rather than fail. v8 moved no byte of the wire at all and is every bit as
/// fatal: the crop season changed what a TICK does, and two peers simulating
/// different rules from identical inputs drift apart in silence.
#[test]
fn a_peer_one_version_behind_is_refused() {
    let addr = "127.0.0.1:39486";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let msg = bincode::serialize(&net_msg::Msg::Hello {
        version: PROTOCOL_VERSION - 1,
        name: "v6 build".into(),
        intent: JoinIntent::Direct,
    })
    .unwrap();
    s.write_all(&(msg.len() as u32).to_le_bytes()).unwrap();
    s.write_all(&msg).unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut buf).unwrap();
    match bincode::deserialize::<net_msg::Msg>(&buf).unwrap() {
        net_msg::Msg::Reject { reason } => match reason {
            RejectReason::VersionMismatch { server, client } => {
                assert_eq!(server, PROTOCOL_VERSION);
                assert_eq!(client, PROTOCOL_VERSION - 1);
            }
            other => panic!("expected a version mismatch, got {other:?}"),
        },
        other => panic!("a v{} peer was not refused: {other:?}", PROTOCOL_VERSION - 1),
    }
    assert_eq!(
        PROTOCOL_VERSION, 9,
        "the group verbs shipped in v7, the crop season in v8, the sea in v9"
    );
}

/// The lockstep guarantee for the new verbs: nothing crosses the wire but the
/// four group commands, and both worlds stay bit-identical while an army of 24
/// marches, attack-moves, charges and halts.
#[test]
fn mass_army_orders_stay_in_sync_through_the_relay() {
    let addr = "127.0.0.1:39487";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    let mut t1 = TcpTransport::connect(addr, "host", JoinIntent::Direct).expect("t1");
    wait_for(|| t1.lobby().you != 0, "host seated");
    let mut t2 = TcpTransport::connect(addr, "guest", JoinIntent::Direct).expect("t2");
    wait_for(|| t1.lobby().players.len() == 2, "roster");
    t1.set_ready(true);
    t2.set_ready(true);
    wait_for(|| t1.lobby().all_ready(), "ready");
    t1.request_start();
    wait_for(|| t1.lobby().started && t2.lobby().started, "start");

    let (p1, p2) = (t1.lobby().you, t2.lobby().you);
    let mut a = world_app();
    let mut b = world_app();
    // real terrain: find ground both armies can actually stand and walk on
    let (bx0, by0) = {
        let mut found = None;
        'search: for cy in 24..(WORLD_SIZE - 32) {
            for cx in 24..(WORLD_SIZE - 32) {
                if (0..20).all(|dx| (0..20).all(|dy| is_passable(1, cx + dx, cy + dy))) {
                    found = Some((cx, cy));
                    break 'search;
                }
            }
        }
        found.expect("a 20x20 passable block on seed 1")
    };
    // an army on each side, spawned identically on both peers before a tick runs
    let at = |x: i32, y: i32| V2::new(Fx::from_num(x) + fx!("0.5"), Fx::from_num(y) + fx!("0.5"));
    for (app, _) in [(&mut a, 0), (&mut b, 1)] {
        for i in 0..24u64 {
            let owner = if i < 16 { p1 } else { p2 };
            let (bx, by) = if i < 16 { (bx0 + 2, by0 + 2) } else { (bx0 + 12, by0 + 12) };
            let p = at(bx + (i as i32 % 4), by + (i as i32 / 4));
            app.world_mut().spawn((
                GameId(9000 + i),
                Owner(owner),
                MatchId(1),
                Pos { pos: p, facing: Fx::ZERO },
                Unit::new(if i % 3 == 0 { UnitKind::Archer } else { UnitKind::Spearman }, p),
            ));
        }
    }
    let mut d1 = LockstepDriver::new(p1, 2);
    let mut d2 = LockstepDriver::new(p2, 2);
    d1.push(PlayerCommand::Join {
        player_id: p1,
        name: "A".into(),
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    d2.push(PlayerCommand::Join {
        player_id: p2,
        name: "B".into(),
        faction: Faction::Crusader,
        match_id: 1,
    });

    let mine: Vec<u64> = (9000..9016).collect();
    let theirs: Vec<u64> = (9016..9024).collect();
    let script: Vec<(u64, u64, PlayerCommand)> = vec![
        (
            4,
            p1,
            PlayerCommand::GroupMove {
                player_id: p1,
                units: mine.clone(),
                target: at(bx0 + 8, by0 + 8),
                formation: 3,
            },
        ),
        (
            20,
            p2,
            PlayerCommand::AttackMove {
                player_id: p2,
                units: theirs.clone(),
                target: at(bx0 + 2, by0 + 2),
                formation: 0,
            },
        ),
        (40, p1, PlayerCommand::GroupAttack { player_id: p1, units: mine.clone(), target: 9016 }),
        (80, p2, PlayerCommand::Stop { player_id: p2, units: theirs.clone() }),
        (
            100,
            p1,
            PlayerCommand::AttackMove {
                player_id: p1,
                units: mine.clone(),
                target: at(bx0 + 16, by0 + 16),
                formation: 2,
            },
        ),
    ];

    let mut h1: HashMap<u64, u64> = HashMap::new();
    let mut h2: HashMap<u64, u64> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(120);
    let (mut n1, mut n2) = (0u64, 0u64);
    let mut contact = false;
    while (n1 < 400 || n2 < 400) && Instant::now() < deadline {
        for (tick, who, cmd) in &script {
            if *who == p1 && *tick == d1.tick {
                d1.push(cmd.clone());
            }
            if *who == p2 && *tick == d2.tick {
                d2.push(cmd.clone());
            }
        }
        if n1 < 400 && d1.advance(a.world_mut(), &mut t1) {
            h1.insert(d1.tick - 1, a.world().resource::<StateHash>().0);
            n1 += 1;
            let world = a.world_mut();
            let mut q = world.query::<&Unit>();
            contact |= q.iter(world).any(|u| u.attack_target != 0);
        }
        if n2 < 400 && d2.advance(b.world_mut(), &mut t2) {
            h2.insert(d2.tick - 1, b.world().resource::<StateHash>().0);
            n2 += 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!((n1, n2), (400, 400), "both clients complete 400 ticks");

    let mut common = 0;
    for (tick, x) in &h1 {
        if let Some(y) = h2.get(tick) {
            assert_eq!(x, y, "desync at tick {tick}");
            common += 1;
        }
    }
    assert!(common > 300, "only {common} ticks were comparable");

    // the orders were REAL: the two worlds agreed about a fight, not merely
    // about two frozen armies
    assert!(contact, "two armies attack-moved at each other and never met");
    let world = a.world_mut();
    let mut q = world.query::<&Unit>();
    assert!(
        q.iter(world).any(|u| u.hp < unit_def(u.kind).max_hp),
        "the armies met and nobody was hurt"
    );
}
