//! Headless TCP lockstep benchmark: N real clients through the real relay,
//! each re-simulating the same massive battle. Measures throughput (ticks/s),
//! per-tick sim cost, lockstep stalls, and verifies all clients stay
//! bit-identical.
//!
//! Run: `cargo run --release -p saladin-protocol --example net_bench -- [clients] [units] [ticks]`
//! (defaults: 2 clients, 2000 units, 600 ticks = 30 s of game time)

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{
    Faction, Fx, GatherState, ResourceType, Stance, Stockpile, UnitKind, V2, ZERO,
    is_passable, unit_def,
};
use std::time::{Duration, Instant};

fn build_world() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1 });
    scatter_world_nodes(app.world_mut(), 1);
    app
}

fn spawn_player(app: &mut App, id: u64) {
    app.world_mut().spawn((
        GameId(10_000_000 + id),
        MatchId(1),
        Player {
            player_id: id,
            name: format!("P{id}"),
            faction: if id % 2 == 1 { Faction::Ayyubid } else { Faction::Crusader },
            stock: Stockpile { wood: 1000, stone: 1000, food: 100_000, gold: 1000 },
            color: (id % 8) as u8,
            online: true,
            keep: 0,
            defeated: false,
            slot: (id % 8) as u8,
            tech_mask: 0,
            hunger: 0,
        },
    ));
}

fn spawn_soldier(app: &mut App, id: u64, owner: u64, kind: UnitKind, pos: V2) {
    let def = unit_def(kind);
    app.world_mut().spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: ZERO },
        Unit {
            kind,
            target: pos,
            has_target: false,
            speed: def.speed,
            gather_state: GatherState::Idle,
            target_node: 0,
            carrying: 0,
            carry_type: ResourceType::Wood,
            harvest_timer: ZERO,
            hp: def.max_hp,
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

/// Seed `total` soldiers as opposing pairs across every passable tile — an
/// instant map-wide battle (worst-case combat + morale + pathing load).
/// Deterministic, so every client seeds the identical army.
fn seed_battle(app: &mut App, total: usize) {
    let kinds = [UnitKind::Spearman, UnitKind::Archer, UnitKind::Knight, UnitKind::Crossbowman];
    let mut id = 1u64;
    let mut placed = 0usize;
    let f = |n: i32| Fx::from_num(n) + Fx::lit("0.5");
    // repeat passes over the passable tiles, stacking pairs, until the army is full
    'outer: loop {
        let before = placed;
        let mut ty = 12;
        while ty < 132 {
            let mut tx = 12;
            while tx < 132 {
                if is_passable(1, tx, ty) && is_passable(1, tx + 1, ty) {
                    let kind = kinds[(placed / 2) % kinds.len()];
                    spawn_soldier(app, id, 1, kind, V2::new(f(tx), f(ty)));
                    spawn_soldier(app, id + 1, 2, kind, V2::new(f(tx + 1), f(ty)));
                    id += 2;
                    placed += 2;
                    if placed >= total {
                        break 'outer;
                    }
                }
                tx += 2;
            }
            ty += 2;
        }
        if placed == before {
            break; // no passable tiles at all
        }
    }
    assert!(placed >= total.min(2), "could not place the requested army");
}

struct ClientReport {
    player: u64,
    ticks: u64,
    wall: Duration,
    sim_total: Duration,
    sim_max: Duration,
    stalls: u64,
    hash: u64,
    units_left: usize,
}

fn run_client(addr: &str, is_host: bool, want_clients: usize, units: usize, ticks: u64) -> ClientReport {
    let mut t = TcpTransport::connect(addr, "bench", JoinIntent::Direct).expect("connect");
    if !is_host {
        t.set_ready(true);
    }
    // lobby: host waits for the full ready roster then starts
    let deadline = Instant::now() + Duration::from_secs(30);
    while !t.lobby().started {
        assert!(Instant::now() < deadline, "lobby timed out");
        if is_host && t.lobby().players.len() == want_clients && t.lobby().all_ready() {
            t.request_start();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let you = t.lobby().you;

    let mut app = build_world();
    for p in 1..=2 {
        spawn_player(&mut app, p);
    }
    seed_battle(&mut app, units);

    let mut driver = LockstepDriver::new(you, 3);
    let wall_start = Instant::now();
    let mut sim_total = Duration::ZERO;
    let mut sim_max = Duration::ZERO;
    let mut stalls = 0u64;
    let mut done = 0u64;
    while done < ticks {
        let t0 = Instant::now();
        if driver.advance(app.world_mut(), &mut t) {
            let dt = t0.elapsed();
            sim_total += dt;
            sim_max = sim_max.max(dt);
            done += 1;
        } else {
            stalls += 1;
            std::thread::yield_now();
        }
    }
    let wall = wall_start.elapsed();
    let world = app.world_mut();
    let hash = world.resource::<StateHash>().0;
    let mut q = world.query::<&Unit>();
    let units_left = q.iter(world).count();
    ClientReport { player: you, ticks: done, wall, sim_total, sim_max, stalls, hash, units_left }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let clients: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(2);
    let units: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);

    let port = 39500 + (std::process::id() % 400) as u16;
    let addr = format!("127.0.0.1:{port}");
    spawn_host_relay(&addr).expect("relay binds");
    std::thread::sleep(Duration::from_millis(100));

    println!("net_bench: {clients} clients, {units} units, {ticks} ticks (= {}s game time)", ticks / 20);

    let mut handles = Vec::new();
    for i in 0..clients {
        let addr = addr.clone();
        handles.push(std::thread::spawn(move || run_client(&addr, i == 0, clients, units, ticks)));
        // deterministic join order so client 0 is always the host
        std::thread::sleep(Duration::from_millis(50));
    }
    let reports: Vec<ClientReport> = handles.into_iter().map(|h| h.join().expect("client thread")).collect();

    println!();
    println!("player | ticks |  wall s | ticks/s | sim avg ms | sim max ms | stalls | units left");
    for r in &reports {
        println!(
            "{:>6} | {:>5} | {:>7.2} | {:>7.0} | {:>10.3} | {:>10.2} | {:>6} | {:>10}",
            r.player,
            r.ticks,
            r.wall.as_secs_f64(),
            r.ticks as f64 / r.wall.as_secs_f64(),
            r.sim_total.as_secs_f64() * 1000.0 / r.ticks.max(1) as f64,
            r.sim_max.as_secs_f64() * 1000.0,
            r.stalls,
            r.units_left,
        );
    }
    let h0 = reports[0].hash;
    let in_sync = reports.iter().all(|r| r.hash == h0);
    println!();
    println!("state hashes {} (0x{h0:016x})", if in_sync { "IDENTICAL — lockstep held" } else { "DIVERGED!" });
    let realtime = reports.iter().map(|r| r.ticks as f64 / r.wall.as_secs_f64()).fold(f64::MAX, f64::min);
    println!("slowest client: {realtime:.0} ticks/s ({}x realtime at 20 Hz)", (realtime / 20.0).round());
    if !in_sync {
        std::process::exit(1);
    }
}
