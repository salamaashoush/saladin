//! The sim with no window: `bevy_app` + `bevy_ecs`, a seeded world, and a
//! devctl socket. This is what an agent or CI drives; the client with a window
//! exists for screenshots.
//!
//! Time is SCRIPTED. The match sits still until a `{"step": N}` request grants
//! ticks, so a test asserts on a tick number rather than on a sleep. `--free`
//! runs it flat out instead, for a soak or a bot-vs-bot sweep.

use bevy_app::prelude::*;
use saladin_protocol::{
    LockstepDriver, MemTransport, PlayerCommand, SimPlugin, WorldConfig, devctl,
    scatter_world_nodes, shared_relay,
};
use saladin_sim::{AiDifficulty, Faction, MAP_PRESETS, compose_seed, enemy_faction};

const USAGE: &str = "\
saladin-headless — the deterministic sim over a devctl socket

  --port <n>          devctl port (default: $SALADIN_DEVCTL, else 7777)
  --seed <n>          map seed (default 1)
  --preset <n>        map preset index (default 0)
  --ai <n>            AI opponents (default 1)
  --difficulty <name> Easy | Normal | Hard (default Normal)
  --faction <name>    Ayyubid | Crusader for the scripted player (default Ayyubid)
  --free              run flat out instead of waiting for {\"step\": N}
  --help

Time is scripted by default: nothing moves until a step request grants ticks.";

struct Args {
    port: u16,
    seed: u32,
    preset: u8,
    ais: usize,
    difficulty: AiDifficulty,
    faction: Faction,
    free: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        port: std::env::var(devctl::PORT_ENV)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(7777),
        seed: 1,
        preset: 0,
        ais: 1,
        difficulty: AiDifficulty::Normal,
        faction: Faction::Ayyubid,
        free: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--free" => a.free = true,
            "--port" => a.port = value()?.parse().map_err(|_| "--port takes a port".to_string())?,
            "--seed" => a.seed = value()?.parse().map_err(|_| "--seed takes a number".to_string())?,
            "--preset" => {
                a.preset = value()?.parse().map_err(|_| "--preset takes an index".to_string())?;
                if a.preset as usize >= MAP_PRESETS.len() {
                    return Err(format!("--preset must be 0..{}", MAP_PRESETS.len() - 1));
                }
            }
            "--ai" => a.ais = value()?.parse().map_err(|_| "--ai takes a count".to_string())?,
            "--difficulty" => {
                a.difficulty = match value()?.to_ascii_lowercase().as_str() {
                    "easy" => AiDifficulty::Easy,
                    "normal" => AiDifficulty::Normal,
                    "hard" => AiDifficulty::Hard,
                    other => return Err(format!("unknown difficulty: {other}")),
                }
            }
            "--faction" => {
                a.faction = match value()?.to_ascii_lowercase().as_str() {
                    "ayyubid" => Faction::Ayyubid,
                    "crusader" => Faction::Crusader,
                    other => return Err(format!("unknown faction: {other}")),
                }
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(a)
}

/// Ticks a free-running loop takes between socket services. One tick per
/// `app.update()` would pay the whole Main schedule per 50 ms of game time.
const FREE_CHUNK: u64 = 64;

/// The scripted player. Bots take 1000+, exactly as the client seats them.
const YOU: u64 = 1;
const MATCH: u64 = 1;

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    let port = match devctl::attach(&mut app, args.port) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot listen on 127.0.0.1:{}: {e}", args.port);
            std::process::exit(1);
        }
    };
    app.finish();
    app.cleanup();

    let seed = compose_seed(args.seed.max(1), args.preset);
    app.world_mut().insert_resource(WorldConfig { seed });
    scatter_world_nodes(app.world_mut(), MATCH);

    // Single peer, so the relay is in-process — the lockstep path is still the
    // ONLY way a command reaches the world.
    let mut driver = LockstepDriver::new(YOU, 1);
    let mut transport = MemTransport::new(shared_relay(vec![YOU]));
    driver.push(PlayerCommand::Join {
        player_id: YOU,
        name: "Script".into(),
        faction: args.faction,
        match_id: MATCH,
    });
    for i in 0..args.ais {
        driver.push(PlayerCommand::AddAi {
            player_id: 1000 + i as u64,
            host: YOU,
            difficulty: args.difficulty,
            faction: enemy_faction(args.faction),
            match_id: MATCH,
        });
    }

    println!(
        "saladin-headless: seed {} preset {} ({}), {} AI, devctl on 127.0.0.1:{port}{}",
        args.seed,
        args.preset,
        MAP_PRESETS[args.preset as usize].label,
        args.ais,
        if args.free { ", free-running" } else { ", waiting for step requests" }
    );

    loop {
        app.world_mut().insert_resource(devctl::DevctlLink {
            submit_tick: driver.tick + driver.delay,
            may_step: true,
            renders: false,
        });
        app.update();
        for cmd in devctl::take_outbox(app.world_mut()) {
            driver.push(cmd);
        }

        let jobs = devctl::take_steps(app.world_mut());
        let granted: u64 = jobs.iter().map(|j| j.ticks).sum();
        let mut run = |driver: &mut LockstepDriver, app: &mut App, n: u64| {
            for _ in 0..n {
                if !driver.advance(app.world_mut(), &mut transport) {
                    break;
                }
                // apply_commands clears the refusals every tick, so a run of
                // 600 would leave a script only the last tick's to read
                devctl::capture_feedback(app.world_mut());
            }
        };
        run(&mut driver, &mut app, granted);
        // answered on the ticks it asked for, before any free-running resumes
        for job in jobs {
            devctl::finish_step(app.world(), job);
        }
        if args.free {
            run(&mut driver, &mut app, FREE_CHUNK);
        } else if granted == 0 {
            // A stepped runner is idle nearly all the time; without this it
            // burns a core polling its own socket.
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
