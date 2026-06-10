//! Room-keyed relay: handshake versioning, room isolation, lobby metadata
//! (names/factions/ready/AI seats/map pick) propagation, and a full
//! internet-style 2-client match through one shared relay with hash equality.

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{AiDifficulty, Faction};
use std::time::{Duration, Instant};

fn build(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
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
fn version_mismatch_is_rejected_cleanly() {
    let addr = "127.0.0.1:39481";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    // a healthy client passes the handshake
    let ok = TcpTransport::connect(addr, "fine", JoinIntent::Direct).expect("connects");
    wait_for(|| ok.lobby().you != 0, "healthy client seated");

    // hand-roll a Hello with the wrong version over a raw socket
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let msg = bincode::serialize(&saladin_protocol::net_msg::Msg::Hello {
        version: PROTOCOL_VERSION + 999,
        name: "old build".into(),
        intent: JoinIntent::Direct,
    })
    .unwrap();
    s.write_all(&(msg.len() as u32).to_le_bytes()).unwrap();
    s.write_all(&msg).unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut buf).unwrap();
    match bincode::deserialize::<saladin_protocol::net_msg::Msg>(&buf).unwrap() {
        saladin_protocol::net_msg::Msg::Reject { reason } => {
            assert!(matches!(reason, RejectReason::VersionMismatch { .. }), "got {reason:?}");
        }
        other => panic!("expected Reject, got {other:?}"),
    }
    // the rejected client never appears in the healthy client's roster
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(ok.lobby().players.len(), 1);
}

#[test]
fn unknown_room_code_is_rejected() {
    let addr = "127.0.0.1:39482";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    let t = TcpTransport::connect(addr, "lost", JoinIntent::JoinRoom { code: "ZZZZZZ".into() })
        .expect("tcp connects");
    wait_for(|| t.lobby().error.is_some(), "reject lands");
    let err = t.lobby().error.unwrap();
    assert!(err.contains("room not found"), "got: {err}");
}

#[test]
fn two_rooms_are_isolated_on_one_relay() {
    let addr = "127.0.0.1:39483";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    // room 1: host + guest
    let h1 = TcpTransport::connect(addr, "Host1", JoinIntent::CreateRoom).expect("h1");
    wait_for(|| h1.lobby().room_code.is_some(), "room 1 code");
    let code1 = h1.lobby().room_code.unwrap();
    assert_eq!(code1.len(), ROOM_CODE_LEN);
    assert!(code1.bytes().all(|b| ROOM_CODE_ALPHABET.contains(&b)), "code from alphabet: {code1}");

    // room 2: separate host
    let h2 = TcpTransport::connect(addr, "Host2", JoinIntent::CreateRoom).expect("h2");
    wait_for(|| h2.lobby().room_code.is_some(), "room 2 code");
    let code2 = h2.lobby().room_code.unwrap();
    assert_ne!(code1, code2, "distinct rooms get distinct codes");

    let g1 = TcpTransport::connect(addr, "Guest1", JoinIntent::JoinRoom { code: code1.clone() })
        .expect("g1");
    wait_for(|| g1.lobby().players.len() == 2, "guest lands in room 1");

    // room 2 never sees room 1's guest; room 1 never sees host 2
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(h2.lobby().players.len(), 1, "room 2 isolated");
    assert_eq!(h1.lobby().players.len(), 2, "room 1 has host+guest only");
    let names: Vec<String> = h1.lobby().players.iter().map(|p| p.name.clone()).collect();
    assert!(names.contains(&"Host1".to_string()) && names.contains(&"Guest1".to_string()));

    // lowercase + separators in a typed code still resolve (normalization)
    let sloppy = format!(" {} ", code1.to_lowercase());
    let g2 = TcpTransport::connect(addr, "Guest2", JoinIntent::JoinRoom { code: sloppy }).expect("g2");
    wait_for(|| g2.lobby().players.len() == 3, "normalized code joins room 1");
}

#[test]
fn lobby_metadata_propagates_and_match_stays_in_sync() {
    let addr = "127.0.0.1:39484";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    let mut host =
        TcpTransport::connect(addr, "Saladin", JoinIntent::CreateRoom).expect("host connects");
    wait_for(|| host.lobby().room_code.is_some(), "room code");
    let code = host.lobby().room_code.unwrap();

    let mut guest = TcpTransport::connect(addr, "Richard", JoinIntent::JoinRoom { code })
        .expect("guest connects");
    wait_for(|| guest.lobby().players.len() == 2, "guest seated");

    // names propagate both ways
    let names = |t: &TcpTransport| -> Vec<String> {
        t.lobby().players.iter().map(|p| p.name.clone()).collect()
    };
    wait_for(|| names(&host).contains(&"Richard".to_string()), "guest name at host");
    assert!(names(&guest).contains(&"Saladin".to_string()));

    // faction picks propagate
    guest.set_faction(Faction::Crusader);
    wait_for(
        || {
            host.lobby()
                .players
                .iter()
                .any(|p| p.name == "Richard" && p.faction == Faction::Crusader)
        },
        "guest faction at host",
    );

    // host map pick propagates
    host.set_map(777, 1);
    wait_for(|| guest.lobby().seed == 777 && guest.lobby().preset == 1, "map pick at guest");

    // host AI seat propagates
    host.add_ai(AiDifficulty::Hard, Faction::Crusader);
    wait_for(|| guest.lobby().players.iter().any(|p| p.is_ai), "AI seat at guest");
    let ai_id = guest.lobby().players.iter().find(|p| p.is_ai).unwrap().id;

    // start gating: not until the guest is ready
    host.request_start();
    std::thread::sleep(Duration::from_millis(150));
    assert!(!host.lobby().started, "start refused while guest not ready");

    guest.set_ready(true);
    wait_for(|| host.lobby().all_ready(), "ready flag at host");
    host.request_start();
    wait_for(|| host.lobby().started && guest.lobby().started, "match start");

    // Welcome carried the host's map pick + full roster on both sides
    for l in [host.lobby(), guest.lobby()] {
        assert_eq!(l.seed, 777);
        assert_eq!(l.preset, 1);
        assert_eq!(l.players.len(), 3);
    }

    // both clients build the SAME world from the Welcome seed and play through
    // the relay: every client originates its own Join (name+faction from the
    // roster); only the host originates AddAi commands.
    let (hl, gl) = (host.lobby(), guest.lobby());
    let mut a = build(hl.seed);
    let mut b = build(gl.seed);
    let mut dh = LockstepDriver::new(hl.you, 2);
    let mut dg = LockstepDriver::new(gl.you, 2);

    let me_h = hl.me().unwrap().clone();
    let me_g = gl.me().unwrap().clone();
    dh.push(PlayerCommand::Join { player_id: me_h.id, name: me_h.name, faction: me_h.faction, match_id: 1 });
    dg.push(PlayerCommand::Join { player_id: me_g.id, name: me_g.name, faction: me_g.faction, match_id: 1 });
    let ai = hl.players.iter().find(|p| p.is_ai).unwrap();
    assert_eq!(ai.id, ai_id);
    dh.push(PlayerCommand::AddAi {
        player_id: ai.id,
        host: hl.you,
        difficulty: ai.ai_difficulty,
        faction: ai.faction,
        match_id: 1,
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut done = (0u64, 0u64);
    while (done.0 < 120 || done.1 < 120) && Instant::now() < deadline {
        if done.0 < 120 && dh.advance(a.world_mut(), &mut host) {
            done.0 += 1;
        }
        if done.1 < 120 && dg.advance(b.world_mut(), &mut guest) {
            done.1 += 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(done, (120, 120), "both clients complete 120 ticks");

    // the AI player founded by the host's command exists in BOTH worlds
    for app in [&mut a, &mut b] {
        let world = app.world_mut();
        let mut q = world.query::<&Player>();
        let players: Vec<&Player> = q.iter(world).collect();
        assert_eq!(players.len(), 3, "two humans + one AI");
        assert!(players.iter().any(|p| p.player_id == ai_id));
        assert!(players.iter().any(|p| p.name == "Saladin"));
        assert!(players.iter().any(|p| p.name == "Richard" && p.faction == Faction::Crusader));
    }

    let ha = a.world().resource::<StateHash>().0;
    let hb = b.world().resource::<StateHash>().0;
    assert_eq!(ha, hb, "room lockstep clients must stay bit-identical");
}

#[test]
fn peer_drop_mid_match_notifies_and_lockstep_continues() {
    use saladin_protocol::net::NetEvent;
    let addr = "127.0.0.1:39485";
    spawn_host_relay(addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    let mut t1 = TcpTransport::connect(addr, "stays", JoinIntent::Direct).expect("t1");
    wait_for(|| t1.lobby().you != 0, "t1 seated first (hosts)");
    let mut t2 = TcpTransport::connect(addr, "drops", JoinIntent::Direct).expect("t2");
    wait_for(|| t1.lobby().players.len() == 2, "roster");
    t2.set_ready(true);
    wait_for(|| t1.lobby().all_ready(), "ready");
    t1.request_start();
    wait_for(|| t1.lobby().started && t2.lobby().started, "start");

    let (p1, p2) = (t1.lobby().you, t2.lobby().you);
    let mut a = build(1);
    let mut b = build(1);
    let mut d1 = LockstepDriver::new(p1, 2);
    let mut d2 = LockstepDriver::new(p2, 2);
    d1.push(PlayerCommand::Join { player_id: p1, name: "A".into(), faction: Faction::Ayyubid, match_id: 1 });
    d2.push(PlayerCommand::Join { player_id: p2, name: "B".into(), faction: Faction::Crusader, match_id: 1 });

    // both play 30 ticks, then client 2 vanishes
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut done = (0u64, 0u64);
    while (done.0 < 30 || done.1 < 30) && Instant::now() < deadline {
        if done.0 < 30 && d1.advance(a.world_mut(), &mut t1) {
            done.0 += 1;
        }
        if done.1 < 30 && d2.advance(b.world_mut(), &mut t2) {
            done.1 += 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(done, (30, 30));
    drop(t2);
    drop(b);

    // the survivor keeps completing ticks and hears about the drop
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut more = 0u64;
    let mut saw_leave = false;
    while more < 60 && Instant::now() < deadline {
        if d1.advance(a.world_mut(), &mut t1) {
            more += 1;
        }
        if t1.take_events().contains(&NetEvent::PeerLeft(p2)) {
            saw_leave = true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(more, 60, "remaining player's ticks complete without the leaver");
    assert!(saw_leave, "client is told the peer left");
}
