//! TCP host/join relay end-to-end: two clients join the lobby, the host
//! starts, both drive their own world through the relay and must stay
//! bit-identical.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{AiDifficulty, Faction};
use std::time::{Duration, Instant};

fn build() -> App {
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

#[test]
fn tcp_relay_keeps_two_clients_in_sync() {
    let addr = "127.0.0.1:39471";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    let mut t1 = TcpTransport::connect(addr, "A", JoinIntent::Direct).expect("client 1 connects");
    // seat order = handshake order; wait for client 1's seat before joining
    // (mirrors the real flow: the host sits in the lobby before anyone joins)
    wait_for(|| t1.lobby().you != 0, "client 1 seated");
    let mut t2 = TcpTransport::connect(addr, "B", JoinIntent::Direct).expect("client 2 connects");
    wait_for(
        || t1.lobby().players.len() == 2 && t2.lobby().players.len() == 2,
        "both clients in the lobby roster",
    );
    assert_eq!(t1.lobby().host, t1.lobby().you, "first client hosts");

    t2.set_ready(true);
    wait_for(|| t1.lobby().all_ready(), "client 2 ready");
    t1.request_start();
    wait_for(|| t1.lobby().started && t2.lobby().started, "match start");

    let (p1, p2) = (t1.lobby().you, t2.lobby().you);
    let mut a = build();
    let mut b = build();
    let mut d1 = LockstepDriver::new(p1, 2);
    let mut d2 = LockstepDriver::new(p2, 2);

    // each client originates only its own join; the relay carries it across
    d1.push(PlayerCommand::Join { player_id: p1, name: "A".into(), faction: Faction::Ayyubid, match_id: 1 });
    d2.push(PlayerCommand::Join { player_id: p2, name: "B".into(), faction: Faction::Crusader, match_id: 1 });
    d1.push(PlayerCommand::AddAi {
        player_id: 1000,
        host: p1,
        difficulty: AiDifficulty::Easy,
        faction: Faction::Crusader,
        match_id: 1,
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut done = (0u64, 0u64);
    while (done.0 < 120 || done.1 < 120) && Instant::now() < deadline {
        if done.0 < 120 && d1.advance(a.world_mut(), &mut t1) {
            done.0 += 1;
        }
        if done.1 < 120 && d2.advance(b.world_mut(), &mut t2) {
            done.1 += 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(done, (120, 120), "both clients should complete 120 ticks (err1={:?} err2={:?})", t1.lobby().error, t2.lobby().error);

    let ha = a.world().resource::<StateHash>().0;
    let hb = b.world().resource::<StateHash>().0;
    assert_eq!(ha, hb, "websocket lockstep clients must stay bit-identical");
}
