//! devctl over a real socket. The load-bearing test is the last one: two peers
//! in one lockstep match, one of them driven by devctl, must stay hash-identical
//! — that is the whole claim the design rests on.

use bevy_app::prelude::*;
use saladin_protocol::devctl::{self, DevctlLink};
use saladin_protocol::*;
use saladin_sim::{BuildingKind, Faction, Fx, Stockpile, UnitKind, V2, building_def, is_passable};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::TcpStream;
use std::time::Duration;

/// One lockstep peer wired the way a host wires devctl: serve the socket, drain
/// the outbox into the driver, advance.
struct Peer {
    app: App,
    driver: LockstepDriver,
    transport: MemTransport,
}

impl Peer {
    fn new(player: u64, relay: SharedRelay, seed: u32, with_devctl: bool) -> (Peer, u16) {
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        let port = if with_devctl { devctl::attach(&mut app, 0).expect("devctl binds") } else { 0 };
        app.finish();
        app.cleanup();
        app.world_mut().insert_resource(WorldConfig { seed });
        (
            Peer {
                app,
                driver: LockstepDriver::new(player, 2),
                transport: MemTransport::new(relay),
            },
            port,
        )
    }

    fn tick(&mut self) {
        let link = DevctlLink {
            submit_tick: self.driver.tick + self.driver.delay,
            may_step: true,
            renders: false,
        };
        self.app.world_mut().insert_resource(link);
        self.app.update();
        for cmd in devctl::take_outbox(self.app.world_mut()) {
            self.driver.push(cmd);
        }
        self.driver.advance(self.app.world_mut(), &mut self.transport);
        devctl::capture_feedback(self.app.world_mut());
    }

    fn hash(&self) -> u64 {
        self.app.world().resource::<StateHash>().0
    }
}

/// A devctl client that reconnects on demand, exactly as a driver library must.
struct Client {
    port: u16,
    conn: Option<BufReader<TcpStream>>,
    line: String,
    next_id: u64,
}

impl Client {
    fn new(port: u16) -> Client {
        Client { port, conn: None, line: String::new(), next_id: 1 }
    }

    fn conn(&mut self) -> &mut BufReader<TcpStream> {
        if self.conn.is_none() {
            let s = TcpStream::connect(("127.0.0.1", self.port)).expect("devctl accepts");
            s.set_read_timeout(Some(Duration::from_millis(2))).expect("timeout sets");
            self.conn = Some(BufReader::new(s));
        }
        self.conn.as_mut().expect("just connected")
    }

    fn send(&mut self, mut req: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        req.as_object_mut().expect("request is an object").insert("id".into(), json!(id));
        let mut w = self.conn().get_ref().try_clone().expect("socket clones");
        writeln!(w, "{req}").expect("request writes");
        w.flush().expect("request flushes");
        id
    }

    /// A whole reply line if one has arrived. Partial reads accumulate — a
    /// timeout mid-line must not eat the bytes already in hand.
    fn try_recv(&mut self) -> Option<Value> {
        let Client { conn, line, .. } = self;
        let r = conn.as_mut()?;
        match r.read_line(line) {
            Ok(0) => None,
            Ok(_) if line.ends_with('\n') => {
                let raw = std::mem::take(line);
                Some(serde_json::from_str(&raw).unwrap_or_else(|e| panic!("reply {raw:?}: {e}")))
            }
            Ok(_) => None,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => None,
            Err(e) => panic!("devctl read failed: {e}"),
        }
    }

    fn drop_connection(&mut self) {
        self.conn = None;
        self.line.clear();
    }
}

/// Send, pump the peer until the reply lands, and check it answered the ask.
fn ask(peer: &mut Peer, client: &mut Client, req: Value) -> Value {
    let id = client.send(req.clone());
    for _ in 0..500 {
        peer.tick();
        if let Some(reply) = client.try_recv() {
            assert_eq!(reply["id"], json!(id), "reply must echo its request id");
            return reply;
        }
    }
    panic!("devctl never answered {req}");
}

fn land_block(seed: u32) -> (i32, i32) {
    for cy in 16..160 {
        for cx in 16..160 {
            if (0..8).all(|dx| (0..8).all(|dy| is_passable(seed, cx + dx, cy + dy))) {
                return (cx, cy);
            }
        }
    }
    panic!("no land block on seed {seed}");
}

fn tile(x: i32, y: i32) -> V2 {
    V2::new(Fx::from_num(x), Fx::from_num(y))
}

fn spawn_building(peer: &mut Peer, id: u64, owner: u64, kind: BuildingKind, at: V2) {
    let def = building_def(kind);
    peer.app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos: at, facing: Fx::ZERO },
        Building::new(kind, def.max_hp, at),
    ));
}

/// A player with a keep and a barracks — enough standing structure that the
/// injected orders below actually do something to desync on.
fn found_player(peer: &mut Peer, id: u64, at: V2) {
    peer.app.world_mut().spawn((
        GameId(900 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: "P".into(),
            faction: Faction::Ayyubid,
            stock: Stockpile { wood: 4000, stone: 4000, food: 4000, gold: 4000 },
            color: 0,
            online: true,
            keep: 100 + id,
            defeated: false,
            slot: id as u8,
            tech_mask: 0,
            hunger: 0,
        },
    ));
    spawn_building(peer, 100 + id, id, BuildingKind::Keep, at);
    spawn_building(peer, 200 + id, id, BuildingKind::Barracks, V2::new(at.x + Fx::from_num(3), at.y));
}

fn units_of(peer: &mut Peer, owner: u64, kind: UnitKind) -> usize {
    let world = peer.app.world_mut();
    let mut q = world.query::<(&Owner, &Unit)>();
    q.iter(world).filter(|(o, u)| o.0 == owner && u.kind == kind).count()
}

#[test]
fn a_train_command_over_the_socket_raises_a_real_unit() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    found_player(&mut peer, 1, tile(cx + 2, cy + 2));

    let reply =
        ask(&mut peer, &mut client, json!({"cmd": {"Train": {"player_id": 1, "kind": "Spearman"}}}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let applied = reply["applied_tick"].as_u64().expect("applied_tick is a tick");

    for _ in 0..400 {
        peer.tick();
    }
    assert!(peer.driver.tick > applied, "the command's tick must have run");
    assert_eq!(units_of(&mut peer, 1, UnitKind::Spearman), 1, "the socket trained a real man");
}

#[test]
fn a_malformed_request_is_an_error_line_and_the_game_runs_on() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    found_player(&mut peer, 1, tile(cx + 2, cy + 2));

    for req in [
        json!({"cmd": {"Nonsense": {}}}),
        json!({"cmd": {"Train": {"player_id": 1}}}),
        json!({"cmd": {"Train": {"player_id": 1, "kind": "Spear"}}}),
        json!({"query": "moon phase"}),
        json!({"nothing": true}),
    ] {
        let reply = ask(&mut peer, &mut client, req.clone());
        assert_eq!(reply["ok"], json!(false), "{req} should be refused: {reply}");
        assert!(reply["error"].is_string(), "a refusal explains itself: {reply}");
    }

    // raw garbage, not even JSON
    {
        let mut w = client.conn().get_ref().try_clone().expect("socket clones");
        writeln!(w, "{{not json").expect("write");
        w.flush().expect("flush");
    }
    let mut garbage = None;
    for _ in 0..500 {
        peer.tick();
        if let Some(r) = client.try_recv() {
            garbage = Some(r);
            break;
        }
    }
    assert_eq!(garbage.expect("an answer to garbage")["ok"], json!(false));

    let reply =
        ask(&mut peer, &mut client, json!({"cmd": {"Train": {"player_id": 1, "kind": "Spearman"}}}));
    assert_eq!(reply["ok"], json!(true), "the channel survives garbage: {reply}");
}

#[test]
fn a_dropped_socket_reconnects() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    found_player(&mut peer, 1, tile(cx + 2, cy + 2));

    let first = ask(&mut peer, &mut client, json!({"query": "tick"}));
    assert_eq!(first["ok"], json!(true), "{first}");

    client.drop_connection();
    for _ in 0..5 {
        peer.tick();
    }
    let second = ask(&mut peer, &mut client, json!({"query": "tick"}));
    assert_eq!(second["ok"], json!(true), "a new connection must answer: {second}");
    assert!(second["tick"].as_u64().expect("tick") > first["tick"].as_u64().expect("tick"));
}

#[test]
fn a_refusal_says_why() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    found_player(&mut peer, 1, tile(cx + 2, cy + 2));

    // a farm on the far side of the map: outside the town, and the sim says so
    let reply = ask(
        &mut peer,
        &mut client,
        json!({"cmd": {"Build": {"player_id": 1, "kind": "Farm", "pos": [340, 340]}}}),
    );
    assert_eq!(reply["ok"], json!(true), "accepted for delivery: {reply}");

    for _ in 0..20 {
        peer.tick();
    }
    let fb = ask(&mut peer, &mut client, json!({"feedback": true}));
    let list = fb["feedback"].as_array().expect("a feedback list");
    assert_eq!(list.len(), 1, "the refusal must have been mirrored: {fb}");
    assert_eq!(list[0]["player_id"], json!(1), "{fb}");
    assert!(list[0]["error"].is_string() && list[0]["text"].is_string(), "{fb}");

    let again = ask(&mut peer, &mut client, json!({"feedback": true}));
    assert!(again["feedback"].as_array().expect("a list").is_empty(), "drained once: {again}");
}

/// THE test. A devctl-driven peer and an untouched one, same match, 1200 ticks:
/// if injection went anywhere but the lockstep stream, these hashes part.
#[test]
fn a_devctl_driven_peer_stays_hash_identical_to_a_plain_one() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let relay = shared_relay(vec![1, 2]);
    let (mut driven, port) = Peer::new(1, relay.clone(), seed, true);
    let (mut plain, _) = Peer::new(2, relay, seed, false);
    let mut client = Client::new(port);

    for peer in [&mut driven, &mut plain] {
        found_player(peer, 1, tile(cx + 2, cy + 2));
        found_player(peer, 2, tile(cx + 2, cy + 6));
    }

    // The two drivers run a tick apart (each stalls until the other has
    // submitted), so the comparison is per TICK NUMBER, not per round.
    let mut seen: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    let mut compared = 0u32;
    let both = |driven: &mut Peer,
                plain: &mut Peer,
                seen: &mut std::collections::HashMap<u64, u64>,
                compared: &mut u32| {
        driven.tick();
        if let Some(&h) = seen.get(&driven.driver.tick) {
            assert_eq!(driven.hash(), h, "peers parted at tick {}", driven.driver.tick);
            *compared += 1;
        }
        plain.tick();
        seen.insert(plain.driver.tick, plain.hash());
    };

    let orders = [
        json!({"cmd": {"Train": {"player_id": 1, "kind": "Spearman"}}}),
        json!({"cmd": {"Train": {"player_id": 1, "kind": "Peasant"}}}),
        json!({"cmd": {"Train": {"player_id": 2, "kind": "Archer"}}}),
        json!({"cmd": {"AutoGather": {"player_id": 1}}}),
        json!({"cmd": {"SetRally": {"player_id": 1, "building": 201, "target": [
            cx + 5, cy + 5]}}}),
        json!({"cmd": {"Train": {"player_id": 1, "kind": "Archer"}}}),
        json!({"cmd": {"MarketTrade": {"player_id": 1, "res": "Wood", "amount": 100}}}),
    ];
    let mut orders = orders.into_iter();
    let mut sent = None;

    for round in 0..1250u32 {
        if round % 100 == 0
            && sent.is_none()
            && let Some(order) = orders.next()
        {
            sent = Some(client.send(order));
        }
        both(&mut driven, &mut plain, &mut seen, &mut compared);
        if let Some(reply) = client.try_recv() {
            assert_eq!(reply["id"], json!(sent.take().expect("a reply needs a request")));
            assert_eq!(reply["ok"], json!(true), "{reply}");
        }
    }

    assert!(driven.driver.tick >= 1200, "the match must actually have run");
    assert!(compared >= 1200, "only {compared} ticks were actually compared");
    assert_eq!(driven.hash(), seen[&driven.driver.tick]);
    // The two camps stand within reach of each other, so by tick 1200 the men
    // the socket raised have already fought and died — the running tally is
    // what proves the orders landed.
    let raised = driven.app.world_mut().resource_mut::<MatchStats>().of(1).trained;
    assert!(raised > 0, "the injected orders must have produced something to desync ON");
    assert_ne!(driven.hash(), 0, "an empty world hashes the same trivially");
}
