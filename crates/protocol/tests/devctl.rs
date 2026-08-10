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
    /// tick -> hash, so a reply can be checked against the world AS IT STOOD
    /// when it was served rather than where the loop has since got to.
    history: std::collections::HashMap<u64, u64>,
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
                history: std::collections::HashMap::new(),
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
        self.history.insert(self.driver.tick, self.hash());
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

#[test]
fn the_state_capture_holds_the_whole_match() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    let keep = tile(cx + 2, cy + 2);
    found_player(&mut peer, 1, keep);
    peer.app.world_mut().spawn((
        GameId(500),
        MatchId(1),
        Pos { pos: tile(cx + 6, cy + 6), facing: Fx::ZERO },
        ResourceNode::deposit(saladin_sim::ResourceType::Wood, 120),
    ));

    let reply =
        ask(&mut peer, &mut client, json!({"cmd": {"Train": {"player_id": 1, "kind": "Spearman"}}}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    for _ in 0..400 {
        peer.tick();
    }

    let s = ask(&mut peer, &mut client, json!({"query": "state"}));
    assert_eq!(s["ok"], json!(true), "{s}");
    let at = s["tick"].as_u64().expect("the capture is stamped");
    assert!(at >= 400, "the capture must be of the match that ran");
    assert_eq!(
        s["hash"].as_u64(),
        peer.history.get(&at).copied(),
        "every capture pins determinism: its hash must be the hash of its own tick"
    );
    assert_eq!(s["seed"], json!(seed));

    let units = s["units"].as_array().expect("units");
    let man = units.iter().find(|u| u["kind"] == json!("Spearman")).expect("the trained man");
    assert_eq!(man["owner"], json!(1));
    assert_eq!(man["role"], json!("Foot"));
    assert_eq!(man["domain"], json!("Land"));
    assert!(man["hp"].as_i64().expect("hp") > 0);
    assert_eq!(man["max_hp"], json!(saladin_sim::unit_def(UnitKind::Spearman).max_hp));
    assert_eq!(man["pos"].as_array().expect("pos").len(), 2);
    assert!(man["stance"].is_string() && man["order"].is_string());

    let bs = s["buildings"].as_array().expect("buildings");
    let barracks = bs.iter().find(|b| b["id"] == json!(201)).expect("the barracks");
    assert_eq!(barracks["kind"], json!("Barracks"));
    assert_eq!(barracks["complete"], json!(true));
    assert_eq!(barracks["state"], json!("Complete"));
    assert!(barracks["queue"].is_array() && barracks["max_hp"].as_i64().expect("max_hp") > 0);

    let node = s["nodes"].as_array().expect("nodes").iter().find(|n| n["id"] == json!(500));
    let node = node.expect("the timber");
    assert_eq!(node["res"], json!("Wood"));
    assert_eq!(node["cap"], json!(120));
    assert_eq!(node["reapable"], json!(true));
    assert_eq!(node["field_of"], Value::Null);

    let me = s["players"].as_array().expect("players")[0].clone();
    assert_eq!(me["player_id"], json!(1));
    assert_eq!(me["faction"], json!("Ayyubid"));
    assert!(me["stock"]["food"].as_i64().expect("food") > 0);
    assert!(me["pop"].as_i64().expect("pop") >= 1);
    assert_eq!(me["pop_cap"], json!(saladin_sim::building_def(BuildingKind::Keep).pop));
    assert_eq!(me["stats"]["trained"], json!(1));
    assert_eq!(me["bot"], Value::Null);

    // ids come out sorted, because an agent diffs two captures
    let ids: Vec<u64> = bs.iter().map(|b| b["id"].as_u64().expect("id")).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
}

#[test]
fn a_scoped_capture_answers_only_what_was_asked() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    found_player(&mut peer, 1, tile(cx + 2, cy + 2));
    found_player(&mut peer, 2, tile(cx + 2, cy + 6));

    let only = ask(&mut peer, &mut client, json!({"query": "state", "kinds": ["buildings"]}));
    assert!(only["buildings"].is_array(), "{only}");
    for absent in ["units", "nodes", "players", "matches"] {
        assert_eq!(only[absent], Value::Null, "{absent} was not asked for: {only}");
    }
    assert!(only["hash"].is_number(), "hash rides on every capture: {only}");

    let mine = ask(
        &mut peer,
        &mut client,
        json!({"query": "state", "kinds": ["buildings", "players"], "player": 2}),
    );
    let owners: Vec<Value> =
        mine["buildings"].as_array().expect("buildings").iter().map(|b| b["owner"].clone()).collect();
    assert_eq!(owners, vec![json!(2), json!(2)], "{mine}");
    assert_eq!(mine["players"].as_array().expect("players").len(), 1);

    let near = ask(
        &mut peer,
        &mut client,
        json!({"query": "state", "kinds": ["buildings"],
               "near": {"pos": [cx + 2, cy + 2], "radius": 3.5}}),
    );
    let ids: Vec<u64> = near["buildings"]
        .as_array()
        .expect("buildings")
        .iter()
        .map(|b| b["id"].as_u64().expect("id"))
        .collect();
    assert_eq!(ids, vec![101, 201], "only player 1's pair is within 3.5 tiles: {near}");

    let bad = ask(&mut peer, &mut client, json!({"query": "state", "kinds": ["dragons"]}));
    assert_eq!(bad["ok"], json!(false), "{bad}");
}

/// The planner query is the bot's OWN numbers, published by its brain. A
/// re-derivation here would be a second implementation and would go stale.
#[test]
fn the_planner_query_reports_what_the_brain_saw() {
    let seed = 1;
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);

    let reply = ask(
        &mut peer,
        &mut client,
        json!({"cmd": {"AddAi": {"player_id": 1000, "host": 1,
                                 "difficulty": "Normal", "faction": "Crusader"}}}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    for _ in 0..200 {
        peer.tick();
    }

    let p = ask(&mut peer, &mut client, json!({"query": "planner", "player": 1000}));
    assert_eq!(p["ok"], json!(true), "{p}");
    let bot = &p["bots"][0];
    assert_eq!(bot["player_id"], json!(1000));
    assert!(bot["seen_at_tick"].as_u64().expect("a beat") > 0, "{bot}");
    // the branch points the gatherer steer actually reads
    for key in ["crisis", "food_emergency", "food_surplus", "scarce_build", "want_food"] {
        assert!(!bot["steer"][key].is_null(), "steer.{key} missing: {bot}");
    }
    assert!(bot["stock"]["food"].is_number() && bot["town"]["peasants"].is_number(), "{bot}");
    assert!(bot["phase"].is_string(), "{bot}");

    let none = ask(&mut peer, &mut client, json!({"query": "planner", "player": 1}));
    assert_eq!(none["ok"], json!(false), "a human seat has no brain: {none}");
}

/// `probe` and `gather` are dry runs through the game's OWN gates: a probe that
/// can answer differently from the order it stands in for is worse than none.
#[test]
fn the_dry_runs_answer_through_the_real_rules() {
    let seed = 1;
    let (cx, cy) = land_block(seed);
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    found_player(&mut peer, 1, tile(cx + 2, cy + 2));
    // tile CENTRES: `scatter_nodes` puts every node on one, and the harvest
    // reach is measured from the stander's tile centre to the node
    let centre = |x: i32, y: i32| {
        V2::new(Fx::from_num(x) + saladin_sim::fx!("0.5"), Fx::from_num(y) + saladin_sim::fx!("0.5"))
    };
    let hand = centre(cx + 4, cy + 4);
    peer.app.world_mut().spawn((
        GameId(700),
        Owner(1),
        MatchId(1),
        Pos { pos: hand, facing: Fx::ZERO },
        Unit::new(UnitKind::Peasant, hand),
    ));
    peer.app.world_mut().spawn((
        GameId(701),
        MatchId(1),
        Pos { pos: centre(cx + 6, cy + 4), facing: Fx::ZERO },
        ResourceNode::deposit(saladin_sim::ResourceType::Wood, 100),
    ));
    for _ in 0..20 {
        peer.tick();
    }

    // a placement dry run costs nothing and founds nothing
    let before = g_buildings(&mut peer);
    let pr = ask(
        &mut peer,
        &mut client,
        json!({"query": "probe", "player": 1, "kind": "House", "near": [cx + 4, cy + 2], "radius": 3}),
    );
    assert_eq!(pr["ok"], json!(true), "{pr}");
    let results = pr["results"].as_array().expect("a verdict per tile");
    assert_eq!(results.len(), 49, "a radius of 3 is a 7x7 square");
    assert!(results.iter().any(|r| r["ok"] == json!(true)), "open ground took nothing: {pr}");
    assert!(
        results.iter().filter(|r| r["ok"] == json!(false)).all(|r| r["error"].is_string()),
        "a refusal must name itself: {pr}"
    );
    assert_eq!(g_buildings(&mut peer), before, "a DRY run founded something");

    // and the gather probe names the gate for every node
    let gr = ask(&mut peer, &mut client, json!({"query": "gather", "unit": 700}));
    assert_eq!(gr["ok"], json!(true), "{gr}");
    assert_eq!(gr["nearest_workable"], json!(701), "the timber two tiles away: {gr}");
    let node = &gr["nodes"][0];
    assert_eq!(node["gate"], json!("ok"), "{gr}");
    assert_eq!(gr["flooded"], json!(true), "{gr}");

    let bad = ask(&mut peer, &mut client, json!({"query": "gather", "unit": 999}));
    assert_eq!(bad["ok"], json!(false), "{bad}");
}

fn g_buildings(peer: &mut Peer) -> usize {
    let world = peer.app.world_mut();
    let mut q = world.query::<&Building>();
    q.iter(world).count()
}

/// A render-side query has nowhere to go without a renderer, and must say so
/// rather than be silently swallowed.
#[test]
fn a_render_query_without_a_renderer_is_refused() {
    let seed = 1;
    let (mut peer, port) = Peer::new(1, shared_relay(vec![1]), seed, true);
    let mut client = Client::new(port);
    let reply = ask(&mut peer, &mut client, json!({"query": "render"}));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert!(
        reply["error"].as_str().expect("an error").contains("client"),
        "it must say a client is needed: {reply}"
    );
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
    // submitted), so the comparison is per TICK NUMBER, not per round — a
    // per-round compare is vacuous, it never fires.
    let mut compared = 0u32;
    let both = |driven: &mut Peer, plain: &mut Peer, compared: &mut u32| {
        driven.tick();
        if let Some(&h) = plain.history.get(&driven.driver.tick) {
            assert_eq!(driven.hash(), h, "peers parted at tick {}", driven.driver.tick);
            *compared += 1;
        }
        plain.tick();
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
        both(&mut driven, &mut plain, &mut compared);
        if let Some(reply) = client.try_recv() {
            assert_eq!(reply["id"], json!(sent.take().expect("a reply needs a request")));
            assert_eq!(reply["ok"], json!(true), "{reply}");
        }
    }

    assert!(driven.driver.tick >= 1200, "the match must actually have run");
    assert!(compared >= 1200, "only {compared} ticks were actually compared");
    assert_eq!(driven.hash(), plain.history[&driven.driver.tick]);
    // The two camps stand within reach of each other, so by tick 1200 the men
    // the socket raised have already fought and died — the running tally is
    // what proves the orders landed.
    let raised = driven.app.world_mut().resource_mut::<MatchStats>().of(1).trained;
    assert!(raised > 0, "the injected orders must have produced something to desync ON");
    assert_ne!(driven.hash(), 0, "an empty world hashes the same trivially");
}
