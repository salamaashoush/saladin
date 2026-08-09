//! An out-of-process control channel: a line-delimited JSON socket that drives
//! the game the way a player does and reads it back the way a test does.
//!
//! Two rules hold the whole design up.
//!
//! **Writes go through `PlayerCommand`.** A request never touches `World`; it
//! parses into a command and lands in `Devctl::outbox`, which the host drains
//! into its lockstep driver alongside the local player's clicks. So an injected
//! order is replicated, ordered and re-simulated on every peer exactly like a
//! click — which is why a devctl-driven client stays hash-identical to one that
//! has never heard of devctl.
//!
//! **Reads never mutate.** Every query is a projection of the world as it
//! stands. The one thing devctl keeps of its own is a mirror of
//! `CommandFeedback` (already outside the state hash), because `apply_commands`
//! clears it every tick and a polling script would otherwise never see why its
//! order was refused.
//!
//! Off unless `SALADIN_DEVCTL=<port>` is set: no listener, no systems, no cost.

use crate::{CommandFeedback, MatchStatuses, PlayerCommand, StateHash, Tick, WorldConfig};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use saladin_sim::{
    AiDifficulty, BuildingKind, Faction, Fx, ResourceType, Stance, UnitKind, V2, place_error_text,
};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

pub const PORT_ENV: &str = "SALADIN_DEVCTL";

/// How many refusals the mirror holds before the oldest are dropped. A script
/// that never polls must not grow the process.
const FEEDBACK_CAP: usize = 256;

// ── the socket ───────────────────────────────────────────────────────────────

struct Job {
    line: String,
    out: Sender<String>,
}

/// One request's answer. Consuming it sends exactly one line; dropping it
/// unanswered sends an error instead, because a caller blocked on a reply that
/// never comes reads as a hung game.
pub struct Reply {
    out: Sender<String>,
    id: Option<Value>,
    sent: bool,
}

impl Reply {
    fn new(out: Sender<String>, id: Option<Value>) -> Reply {
        Reply { out, id, sent: false }
    }

    pub fn ok(mut self, body: Value) {
        self.finish(true, body);
    }

    pub fn err(mut self, msg: impl std::fmt::Display) {
        self.finish(false, json!({ "error": msg.to_string() }));
    }

    fn finish(&mut self, ok: bool, body: Value) {
        let mut obj = match body {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("result".into(), other);
                m
            }
        };
        obj.insert("ok".into(), Value::Bool(ok));
        if let Some(id) = self.id.take() {
            obj.insert("id".into(), id);
        }
        self.sent = true;
        let _ = self.out.send(Value::Object(obj).to_string());
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if !self.sent {
            self.finish(false, json!({ "error": "request dropped unanswered" }));
        }
    }
}

fn serve_conn(stream: TcpStream, jobs: Sender<Job>) {
    let Ok(writer) = stream.try_clone() else { return };
    let (out, replies) = channel::<String>();
    std::thread::spawn(move || {
        let mut w = writer;
        for line in replies {
            if w.write_all(line.as_bytes()).is_err() || w.write_all(b"\n").is_err() {
                break;
            }
            let _ = w.flush();
        }
        let _ = w.shutdown(Shutdown::Both);
    });
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if jobs.send(Job { line, out: out.clone() }).is_err() {
            break;
        }
    }
}

// ── resources ────────────────────────────────────────────────────────────────

/// What the embedding app tells devctl about itself. The host owns the lockstep
/// clock and the render surface; devctl only reports them.
#[derive(Resource, Clone, Copy, Debug)]
pub struct DevctlLink {
    /// The tick a command queued right now will be applied on.
    pub submit_tick: u64,
    /// May devctl advance ticks itself? Headless/single-player only — in a
    /// multiplayer match time belongs to the lockstep clock.
    pub may_step: bool,
    /// Is there a render surface, i.e. can a screenshot be taken?
    pub renders: bool,
}

impl Default for DevctlLink {
    fn default() -> Self {
        DevctlLink { submit_tick: 0, may_step: true, renders: false }
    }
}

/// A screenshot the host must take. Protocol has no renderer, so the client
/// drains these, shoots, and answers.
pub struct ShotJob {
    pub path: String,
    pub camera: Option<CameraSpec>,
    reply: Reply,
}

impl ShotJob {
    /// Answer the waiting caller once the file is on disk (or the attempt died).
    pub fn done(self, err: Option<String>) {
        match err {
            None => self.reply.ok(json!({ "path": self.path })),
            Some(e) => self.reply.err(e),
        }
    }
}

/// Where to look before the shutter. Floats because this is a camera, not the
/// sim — nothing here ever reaches gameplay math.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraSpec {
    pub pos: Option<(f32, f32)>,
    pub zoom: Option<f32>,
    pub yaw: Option<i32>,
}

/// A `{"step": N}` request waiting on N ticks of simulation.
pub struct StepJob {
    pub ticks: u64,
    reply: Reply,
}

#[derive(Resource)]
pub struct Devctl {
    rx: Mutex<Receiver<Job>>,
    outbox: Vec<PlayerCommand>,
    shots: Vec<ShotJob>,
    steps: Vec<StepJob>,
    feedback: Vec<Value>,
    /// Last tick whose `CommandFeedback` was mirrored — the same tick's batch
    /// must not be copied twice when the host renders faster than it simulates.
    mirrored: Option<u64>,
}

impl Devctl {
    fn listen(port: u16) -> std::io::Result<(Devctl, u16)> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let bound = listener.local_addr()?.port();
        let (jobs, rx) = channel::<Job>();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = stream.set_nodelay(true);
                let jobs = jobs.clone();
                std::thread::spawn(move || serve_conn(stream, jobs));
            }
        });
        Ok((
            Devctl {
                rx: Mutex::new(rx),
                outbox: Vec::new(),
                shots: Vec::new(),
                steps: Vec::new(),
                feedback: Vec::new(),
                mirrored: None,
            },
            bound,
        ))
    }
}

/// Open the channel on `port` without consulting the environment, and answer
/// with the port actually bound — pass 0 and the OS picks a free one, which is
/// how tests run several of these at once.
pub fn attach(app: &mut App, port: u16) -> std::io::Result<u16> {
    let (devctl, bound) = Devctl::listen(port)?;
    app.insert_resource(devctl).init_resource::<DevctlLink>().add_systems(Update, serve);
    Ok(bound)
}

/// Commands injected since the last drain, for the host to push into its
/// lockstep driver. Nothing else may consume them.
pub fn take_outbox(world: &mut World) -> Vec<PlayerCommand> {
    match world.get_resource_mut::<Devctl>() {
        Some(mut d) => std::mem::take(&mut d.outbox),
        None => Vec::new(),
    }
}

/// Screenshot requests waiting on a renderer.
pub fn take_shots(world: &mut World) -> Vec<ShotJob> {
    match world.get_resource_mut::<Devctl>() {
        Some(mut d) => std::mem::take(&mut d.shots),
        None => Vec::new(),
    }
}

/// Tick budgets granted by `{"step": N}`. The host runs them and then calls
/// `finish_steps`.
pub fn take_steps(world: &mut World) -> Vec<StepJob> {
    match world.get_resource_mut::<Devctl>() {
        Some(mut d) => std::mem::take(&mut d.steps),
        None => Vec::new(),
    }
}

/// Answer a `step` request once its ticks have actually run.
pub fn finish_step(world: &World, job: StepJob) {
    let tick = world.resource::<Tick>().0;
    let hash = world.resource::<StateHash>().0;
    job.reply.ok(json!({ "tick": tick, "hash": hash }));
}

/// Copy this tick's refusals into the mirror. `apply_commands` clears
/// `CommandFeedback` every tick, so a host that simulates faster than it polls
/// must call this after every `step` or the reason a command was refused is
/// gone before anyone can ask.
pub fn capture_feedback(world: &mut World) {
    if !world.contains_resource::<Devctl>() {
        return;
    }
    let tick = world.resource::<Tick>().0;
    let batch: Vec<Value> = world
        .resource::<CommandFeedback>()
        .0
        .iter()
        .map(|(pid, e)| {
            json!({
                "tick": tick,
                "player_id": pid,
                "error": format!("{e:?}"),
                "text": place_error_text(*e),
            })
        })
        .collect();
    let mut d = world.resource_mut::<Devctl>();
    if d.mirrored == Some(tick) {
        return;
    }
    d.mirrored = Some(tick);
    if batch.is_empty() {
        return;
    }
    d.feedback.extend(batch);
    let overflow = d.feedback.len().saturating_sub(FEEDBACK_CAP);
    d.feedback.drain(..overflow);
}

// ── the plugin ───────────────────────────────────────────────────────────────

pub struct DevctlPlugin;

impl Plugin for DevctlPlugin {
    fn build(&self, app: &mut App) {
        let Some(port) = std::env::var(PORT_ENV).ok().and_then(|s| s.trim().parse::<u16>().ok())
        else {
            return;
        };
        match attach(app, port) {
            Ok(bound) => println!("devctl listening on 127.0.0.1:{bound}"),
            Err(e) => eprintln!("devctl: cannot listen on 127.0.0.1:{port}: {e}"),
        }
    }
}

/// Drain the socket and answer. Exclusive so a query can walk the whole world
/// in one pass; every handler below only ever reads it.
pub fn serve(world: &mut World) {
    capture_feedback(world);
    let jobs: Vec<Job> = {
        let d = world.resource::<Devctl>();
        let Ok(rx) = d.rx.lock() else { return };
        rx.try_iter().collect()
    };
    for job in jobs {
        handle(world, job);
    }
}

fn handle(world: &mut World, job: Job) {
    let Job { line, out } = job;
    let req: Map<String, Value> = match serde_json::from_str::<Value>(&line) {
        Ok(Value::Object(m)) => m,
        Ok(_) => return Reply::new(out, None).err("request must be a JSON object"),
        Err(e) => return Reply::new(out, None).err(format!("malformed JSON: {e}")),
    };
    let reply = Reply::new(out, req.get("id").cloned());

    if let Some(v) = req.get("cmd") {
        return inject(world, v, reply);
    }
    if let Some(v) = req.get("query") {
        return query(world, v, &req, reply);
    }
    if let Some(v) = req.get("step") {
        return step_req(world, v, reply);
    }
    if let Some(v) = req.get("screenshot") {
        return shot_req(world, v, &req, reply);
    }
    if req.contains_key("feedback") {
        return reply.ok(json!({ "feedback": drain_feedback(world) }));
    }
    reply.err("request needs one of: cmd, query, step, screenshot, feedback")
}

fn inject(world: &mut World, v: &Value, reply: Reply) {
    match command_from_json(v) {
        Ok(cmd) => {
            let at = world.resource::<DevctlLink>().submit_tick;
            world.resource_mut::<Devctl>().outbox.push(cmd);
            reply.ok(json!({ "applied_tick": at }));
        }
        Err(e) => reply.err(e),
    }
}

fn step_req(world: &mut World, v: &Value, reply: Reply) {
    let link = *world.resource::<DevctlLink>();
    if !link.may_step {
        return reply
            .err("step is single-player/headless only: in a match, time belongs to the lockstep clock");
    }
    let Some(ticks) = v.as_u64() else {
        return reply.err("step takes a tick count");
    };
    world.resource_mut::<Devctl>().steps.push(StepJob { ticks, reply });
}

fn shot_req(world: &mut World, v: &Value, req: &Map<String, Value>, reply: Reply) {
    let Some(path) = v.as_str() else {
        return reply.err("screenshot takes an output path");
    };
    if !world.resource::<DevctlLink>().renders {
        return reply.err("headless");
    }
    let camera = match req.get("camera") {
        None | Some(Value::Null) => None,
        Some(Value::Object(c)) => match camera_spec(c) {
            Ok(c) => Some(c),
            Err(e) => return reply.err(e),
        },
        Some(_) => return reply.err("camera takes an object"),
    };
    world.resource_mut::<Devctl>().shots.push(ShotJob { path: path.to_string(), camera, reply });
}

fn camera_spec(c: &Map<String, Value>) -> Result<CameraSpec, String> {
    let f32_at = |k: &str| -> Result<Option<f32>, String> {
        match c.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_f64()
                .map(|n| Some(n as f32))
                .ok_or_else(|| format!("camera.{k} takes a number")),
        }
    };
    let pos = match c.get("pos") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let p = v2_from(v).map_err(|e| format!("camera.pos: {e}"))?;
            Some((p.x.to_num::<f32>(), p.y.to_num::<f32>()))
        }
    };
    Ok(CameraSpec {
        pos,
        zoom: f32_at("zoom")?,
        yaw: match c.get("yaw") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_i64().ok_or("camera.yaw takes quarter turns")? as i32),
        },
    })
}

fn drain_feedback(world: &mut World) -> Vec<Value> {
    std::mem::take(&mut world.resource_mut::<Devctl>().feedback)
}

// ── queries ──────────────────────────────────────────────────────────────────

fn query(world: &mut World, v: &Value, _req: &Map<String, Value>, reply: Reply) {
    let Some(name) = v.as_str() else {
        return reply.err("query takes a name");
    };
    match name {
        "tick" => reply.ok(tick_report(world)),
        "feedback" => reply.ok(json!({ "feedback": drain_feedback(world) })),
        other => reply.err(format!("unknown query: {other} (expected one of: tick, feedback)")),
    }
}

fn tick_report(world: &World) -> Value {
    let link = *world.resource::<DevctlLink>();
    let paused = world.resource::<MatchStatuses>().0.values().any(|s| !saladin_sim::match_simulates(*s));
    json!({
        "tick": world.resource::<Tick>().0,
        "hash": world.resource::<StateHash>().0,
        "seed": world.resource::<WorldConfig>().seed,
        "paused": paused,
        "submit_tick": link.submit_tick,
        "may_step": link.may_step,
        "renders": link.renders,
    })
}

// ── PlayerCommand <-> JSON ───────────────────────────────────────────────────

/// Every command name the parser accepts. Kept beside `command_to_json`, whose
/// exhaustive match is what breaks the build when `PlayerCommand` grows.
pub const COMMAND_NAMES: &[&str] = &[
    "Join",
    "AddAi",
    "Move",
    "SetStance",
    "Train",
    "Build",
    "Gather",
    "Attack",
    "SetRally",
    "Garrison",
    "Ungarrison",
    "Demolish",
    "PlaceWall",
    "MarketTrade",
    "MarketBuy",
    "StartResearch",
    "AutoGather",
    "Pause",
    "Resume",
    "Repair",
    "CancelSite",
    "UpgradeBuilding",
    "TrainAt",
    "CancelTrain",
    "GroupMove",
    "AttackMove",
    "GroupAttack",
    "Stop",
    "Embark",
    "Disembark",
];

/// `{"Train": {"player_id": 1, "kind": "Spearman"}}` — serde's externally
/// tagged shape, written out by hand because `Fx` serializes as raw bits and a
/// coordinate has to be readable in a shell pipeline.
pub fn command_to_json(cmd: &PlayerCommand) -> Value {
    use PlayerCommand::*;
    let (name, body) = match cmd {
        Join { player_id, name, faction, match_id } => (
            "Join",
            json!({"player_id": player_id, "name": name, "faction": faction, "match_id": match_id}),
        ),
        AddAi { player_id, host, difficulty, faction, match_id } => (
            "AddAi",
            json!({"player_id": player_id, "host": host, "difficulty": difficulty, "faction": faction, "match_id": match_id}),
        ),
        Move { player_id, unit, target } => {
            ("Move", json!({"player_id": player_id, "unit": unit, "target": v2_json(*target)}))
        }
        SetStance { player_id, unit, stance } => {
            ("SetStance", json!({"player_id": player_id, "unit": unit, "stance": stance}))
        }
        Train { player_id, kind } => ("Train", json!({"player_id": player_id, "kind": kind})),
        Build { player_id, kind, pos, facing, builders } => (
            "Build",
            json!({"player_id": player_id, "kind": kind, "pos": v2_json(*pos), "facing": facing, "builders": builders}),
        ),
        Gather { player_id, unit, node } => {
            ("Gather", json!({"player_id": player_id, "unit": unit, "node": node}))
        }
        Attack { player_id, unit, target } => {
            ("Attack", json!({"player_id": player_id, "unit": unit, "target": target}))
        }
        SetRally { player_id, building, target } => (
            "SetRally",
            json!({"player_id": player_id, "building": building, "target": v2_json(*target)}),
        ),
        Garrison { player_id, unit, building } => {
            ("Garrison", json!({"player_id": player_id, "unit": unit, "building": building}))
        }
        Ungarrison { player_id, building } => {
            ("Ungarrison", json!({"player_id": player_id, "building": building}))
        }
        Demolish { player_id, building } => {
            ("Demolish", json!({"player_id": player_id, "building": building}))
        }
        PlaceWall { player_id, tiles, builders } => (
            "PlaceWall",
            json!({"player_id": player_id, "tiles": tiles, "builders": builders}),
        ),
        MarketTrade { player_id, res, amount } => {
            ("MarketTrade", json!({"player_id": player_id, "res": res, "amount": amount}))
        }
        MarketBuy { player_id, res, amount } => {
            ("MarketBuy", json!({"player_id": player_id, "res": res, "amount": amount}))
        }
        StartResearch { player_id, building, tech } => (
            "StartResearch",
            json!({"player_id": player_id, "building": building, "tech": tech}),
        ),
        AutoGather { player_id } => ("AutoGather", json!({ "player_id": player_id })),
        Pause { player_id } => ("Pause", json!({ "player_id": player_id })),
        Resume { player_id } => ("Resume", json!({ "player_id": player_id })),
        Repair { player_id, unit, building } => {
            ("Repair", json!({"player_id": player_id, "unit": unit, "building": building}))
        }
        CancelSite { player_id, building } => {
            ("CancelSite", json!({"player_id": player_id, "building": building}))
        }
        UpgradeBuilding { player_id, building } => {
            ("UpgradeBuilding", json!({"player_id": player_id, "building": building}))
        }
        TrainAt { player_id, building, kind } => {
            ("TrainAt", json!({"player_id": player_id, "building": building, "kind": kind}))
        }
        CancelTrain { player_id, building } => {
            ("CancelTrain", json!({"player_id": player_id, "building": building}))
        }
        GroupMove { player_id, units, target, formation } => (
            "GroupMove",
            json!({"player_id": player_id, "units": units, "target": v2_json(*target), "formation": formation}),
        ),
        AttackMove { player_id, units, target, formation } => (
            "AttackMove",
            json!({"player_id": player_id, "units": units, "target": v2_json(*target), "formation": formation}),
        ),
        GroupAttack { player_id, units, target } => (
            "GroupAttack",
            json!({"player_id": player_id, "units": units, "target": target}),
        ),
        Stop { player_id, units } => ("Stop", json!({"player_id": player_id, "units": units})),
        Embark { player_id, units, boat } => {
            ("Embark", json!({"player_id": player_id, "units": units, "boat": boat}))
        }
        Disembark { player_id, boat, target } => (
            "Disembark",
            json!({"player_id": player_id, "boat": boat, "target": v2_json(*target)}),
        ),
    };
    json!({ name: body })
}

pub fn command_from_json(v: &Value) -> Result<PlayerCommand, String> {
    let Value::Object(outer) = v else {
        return Err("cmd takes an object: {\"Train\": {...}}".into());
    };
    if outer.len() != 1 {
        return Err("cmd takes exactly one PlayerCommand variant".into());
    }
    let (name, body) = outer.iter().next().expect("length checked");
    let empty = Map::new();
    let m = match body {
        Value::Object(m) => m,
        Value::Null => &empty,
        _ => return Err(format!("{name} takes an object of its fields")),
    };
    let p = || u64_at(m, "player_id");
    use PlayerCommand as C;
    let cmd = match name.as_str() {
        "Join" => C::Join {
            player_id: p()?,
            name: str_at(m, "name")?.to_string(),
            faction: enum_at::<Faction>(m, "faction")?,
            match_id: u64_or(m, "match_id", 1)?,
        },
        "AddAi" => C::AddAi {
            player_id: p()?,
            host: u64_at(m, "host")?,
            difficulty: enum_at::<AiDifficulty>(m, "difficulty")?,
            faction: enum_at::<Faction>(m, "faction")?,
            match_id: u64_or(m, "match_id", 1)?,
        },
        "Move" => C::Move { player_id: p()?, unit: u64_at(m, "unit")?, target: v2_at(m, "target")? },
        "SetStance" => C::SetStance {
            player_id: p()?,
            unit: u64_at(m, "unit")?,
            stance: enum_at::<Stance>(m, "stance")?,
        },
        "Train" => C::Train { player_id: p()?, kind: enum_at::<UnitKind>(m, "kind")? },
        "Build" => C::Build {
            player_id: p()?,
            kind: enum_at::<BuildingKind>(m, "kind")?,
            pos: v2_at(m, "pos")?,
            facing: u64_or(m, "facing", 0)? as u8,
            builders: ids_at(m, "builders")?,
        },
        "Gather" => {
            C::Gather { player_id: p()?, unit: u64_at(m, "unit")?, node: u64_at(m, "node")? }
        }
        "Attack" => {
            C::Attack { player_id: p()?, unit: u64_at(m, "unit")?, target: u64_at(m, "target")? }
        }
        "SetRally" => C::SetRally {
            player_id: p()?,
            building: u64_at(m, "building")?,
            target: v2_at(m, "target")?,
        },
        "Garrison" => C::Garrison {
            player_id: p()?,
            unit: u64_at(m, "unit")?,
            building: u64_at(m, "building")?,
        },
        "Ungarrison" => C::Ungarrison { player_id: p()?, building: u64_at(m, "building")? },
        "Demolish" => C::Demolish { player_id: p()?, building: u64_at(m, "building")? },
        "PlaceWall" => {
            C::PlaceWall { player_id: p()?, tiles: tiles_at(m)?, builders: ids_at(m, "builders")? }
        }
        "MarketTrade" => C::MarketTrade {
            player_id: p()?,
            res: enum_at::<ResourceType>(m, "res")?,
            amount: i32_at(m, "amount")?,
        },
        "MarketBuy" => C::MarketBuy {
            player_id: p()?,
            res: enum_at::<ResourceType>(m, "res")?,
            amount: i32_at(m, "amount")?,
        },
        "StartResearch" => C::StartResearch {
            player_id: p()?,
            building: u64_at(m, "building")?,
            tech: tech_at(m)?,
        },
        "AutoGather" => C::AutoGather { player_id: p()? },
        "Pause" => C::Pause { player_id: p()? },
        "Resume" => C::Resume { player_id: p()? },
        "Repair" => {
            C::Repair { player_id: p()?, unit: u64_at(m, "unit")?, building: u64_at(m, "building")? }
        }
        "CancelSite" => C::CancelSite { player_id: p()?, building: u64_at(m, "building")? },
        "UpgradeBuilding" => {
            C::UpgradeBuilding { player_id: p()?, building: u64_at(m, "building")? }
        }
        "TrainAt" => C::TrainAt {
            player_id: p()?,
            building: u64_at(m, "building")?,
            kind: enum_at::<UnitKind>(m, "kind")?,
        },
        "CancelTrain" => C::CancelTrain { player_id: p()?, building: u64_at(m, "building")? },
        "GroupMove" => C::GroupMove {
            player_id: p()?,
            units: ids_at(m, "units")?,
            target: v2_at(m, "target")?,
            formation: u64_or(m, "formation", 0)? as u8,
        },
        "AttackMove" => C::AttackMove {
            player_id: p()?,
            units: ids_at(m, "units")?,
            target: v2_at(m, "target")?,
            formation: u64_or(m, "formation", 0)? as u8,
        },
        "GroupAttack" => C::GroupAttack {
            player_id: p()?,
            units: ids_at(m, "units")?,
            target: u64_at(m, "target")?,
        },
        "Stop" => C::Stop { player_id: p()?, units: ids_at(m, "units")? },
        "Embark" => {
            C::Embark { player_id: p()?, units: ids_at(m, "units")?, boat: u64_at(m, "boat")? }
        }
        "Disembark" => C::Disembark {
            player_id: p()?,
            boat: u64_at(m, "boat")?,
            target: v2_at(m, "target")?,
        },
        other => {
            return Err(format!(
                "unknown PlayerCommand variant: {other} (expected one of: {})",
                COMMAND_NAMES.join(", ")
            ));
        }
    };
    Ok(cmd)
}

// ── field readers ────────────────────────────────────────────────────────────

fn at<'a>(m: &'a Map<String, Value>, k: &str) -> Result<&'a Value, String> {
    m.get(k).ok_or_else(|| format!("missing field \"{k}\""))
}

fn u64_at(m: &Map<String, Value>, k: &str) -> Result<u64, String> {
    at(m, k)?.as_u64().ok_or_else(|| format!("field \"{k}\" takes a non-negative integer"))
}

fn u64_or(m: &Map<String, Value>, k: &str, default: u64) -> Result<u64, String> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(_) => u64_at(m, k),
    }
}

fn i32_at(m: &Map<String, Value>, k: &str) -> Result<i32, String> {
    let n = at(m, k)?.as_i64().ok_or_else(|| format!("field \"{k}\" takes an integer"))?;
    i32::try_from(n).map_err(|_| format!("field \"{k}\" is out of range"))
}

fn str_at<'a>(m: &'a Map<String, Value>, k: &str) -> Result<&'a str, String> {
    at(m, k)?.as_str().ok_or_else(|| format!("field \"{k}\" takes a string"))
}

fn enum_at<T: serde::de::DeserializeOwned>(m: &Map<String, Value>, k: &str) -> Result<T, String> {
    serde_json::from_value(at(m, k)?.clone()).map_err(|e| format!("field \"{k}\": {e}"))
}

fn tech_at(m: &Map<String, Value>) -> Result<u8, String> {
    match at(m, "tech")? {
        Value::String(_) => Ok(enum_at::<saladin_sim::Tech>(m, "tech")? as u8),
        v => v
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| "field \"tech\" takes a Tech name or index".to_string()),
    }
}

fn ids_at(m: &Map<String, Value>, k: &str) -> Result<Vec<u64>, String> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_u64().ok_or_else(|| format!("field \"{k}\" takes game ids")))
            .collect(),
        Some(_) => Err(format!("field \"{k}\" takes an array of game ids")),
    }
}

fn tiles_at(m: &Map<String, Value>) -> Result<Vec<(i32, i32)>, String> {
    let Some(Value::Array(a)) = m.get("tiles") else {
        return Err("field \"tiles\" takes an array of [x, y] tiles".into());
    };
    a.iter()
        .map(|v| match v {
            Value::Array(p) if p.len() == 2 => {
                let x = p[0].as_i64().ok_or("tile x must be an integer")?;
                let y = p[1].as_i64().ok_or("tile y must be an integer")?;
                Ok((x as i32, y as i32))
            }
            _ => Err("each tile is [x, y]".to_string()),
        })
        .collect()
}

fn v2_at(m: &Map<String, Value>, k: &str) -> Result<V2, String> {
    v2_from(at(m, k)?).map_err(|e| format!("field \"{k}\": {e}"))
}

fn v2_from(v: &Value) -> Result<V2, String> {
    let (x, y) = match v {
        Value::Array(a) if a.len() == 2 => (&a[0], &a[1]),
        Value::Object(o) => (at(o, "x")?, at(o, "y")?),
        _ => return Err("a position is [x, y] or {\"x\": .., \"y\": ..}".into()),
    };
    Ok(V2::new(fx_from(x)?, fx_from(y)?))
}

/// A JSON number to `Fx` without a float in the path. `Fx::from_num(f64)` would
/// do it in one line and put an f64 in the protocol crate for the sake of one
/// dev command; serde_json hands back the literal decimal, so parse that.
fn fx_from(v: &Value) -> Result<Fx, String> {
    let text = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return Err(format!("expected a number, got {v}")),
    };
    parse_fx(&text)
}

fn parse_fx(src: &str) -> Result<Fx, String> {
    let s = src.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && frac.is_empty()) || !digits(whole) || !digits(frac) {
        return Err(format!("not a plain decimal number: {src}"));
    }
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| range(src))? };
    if w > i32::MAX as i64 {
        return Err(range(src));
    }
    let mut out = Fx::from_num(w);
    // I32F32 resolves ~9 decimal places; the rest is noise, not precision
    let frac = &frac[..frac.len().min(9)];
    if !frac.is_empty() {
        let num: i64 = frac.parse().map_err(|_| range(src))?;
        out += Fx::from_num(num) / Fx::from_num(10i64.pow(frac.len() as u32));
    }
    Ok(if neg { -out } else { out })
}

fn range(src: &str) -> String {
    format!("number out of range: {src}")
}

fn v2_json(v: V2) -> Value {
    json!([fx_json(v.x), fx_json(v.y)])
}

/// Fixed point out as a plain number. Output only — nothing reads this back
/// into the sim, so the f64 cannot reach gameplay math.
fn fx_json(v: Fx) -> f64 {
    v.to_num::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commands() -> Vec<PlayerCommand> {
        use PlayerCommand::*;
        let at = V2::new(Fx::from_num(12), saladin_sim::fx!("30.5"));
        vec![
            Join { player_id: 1, name: "You".into(), faction: Faction::Ayyubid, match_id: 1 },
            AddAi {
                player_id: 1000,
                host: 1,
                difficulty: AiDifficulty::Hard,
                faction: Faction::Crusader,
                match_id: 1,
            },
            Move { player_id: 1, unit: 7, target: at },
            SetStance { player_id: 1, unit: 7, stance: Stance::HoldGround },
            Train { player_id: 1, kind: UnitKind::Spearman },
            Build {
                player_id: 1,
                kind: BuildingKind::Farm,
                pos: at,
                facing: 2,
                builders: vec![7, 8],
            },
            Gather { player_id: 1, unit: 7, node: 42 },
            Attack { player_id: 1, unit: 7, target: 42 },
            SetRally { player_id: 1, building: 3, target: at },
            Garrison { player_id: 1, unit: 7, building: 3 },
            Ungarrison { player_id: 1, building: 3 },
            Demolish { player_id: 1, building: 3 },
            PlaceWall { player_id: 1, tiles: vec![(4, 5), (4, 6)], builders: vec![7] },
            MarketTrade { player_id: 1, res: ResourceType::Wood, amount: 100 },
            MarketBuy { player_id: 1, res: ResourceType::Stone, amount: 50 },
            StartResearch { player_id: 1, building: 3, tech: 2 },
            AutoGather { player_id: 1 },
            Pause { player_id: 1 },
            Resume { player_id: 1 },
            Repair { player_id: 1, unit: 7, building: 3 },
            CancelSite { player_id: 1, building: 3 },
            UpgradeBuilding { player_id: 1, building: 3 },
            TrainAt { player_id: 1, building: 3, kind: UnitKind::Archer },
            CancelTrain { player_id: 1, building: 3 },
            GroupMove { player_id: 1, units: vec![7, 8], target: at, formation: 1 },
            AttackMove { player_id: 1, units: vec![7, 8], target: at, formation: 0 },
            GroupAttack { player_id: 1, units: vec![7, 8], target: 42 },
            Stop { player_id: 1, units: vec![7, 8] },
            Embark { player_id: 1, units: vec![7, 8], boat: 9 },
            Disembark { player_id: 1, boat: 9, target: at },
        ]
    }

    /// `command_to_json`'s match is exhaustive, so a new `PlayerCommand` variant
    /// breaks the build there; this is what makes the PARSER keep up with it.
    #[test]
    fn every_command_round_trips_through_json() {
        let samples = sample_commands();
        assert_eq!(samples.len(), COMMAND_NAMES.len(), "one sample per command variant");
        for cmd in &samples {
            let wire = command_to_json(cmd);
            let name = wire.as_object().unwrap().keys().next().unwrap().clone();
            assert!(COMMAND_NAMES.contains(&name.as_str()), "{name} missing from COMMAND_NAMES");
            let back = command_from_json(&wire).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                format!("{back:?}"),
                format!("{cmd:?}"),
                "{name} did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_coordinate_parses_without_a_float() {
        assert_eq!(parse_fx("12").unwrap(), Fx::from_num(12));
        assert_eq!(parse_fx("30.5").unwrap(), saladin_sim::fx!("30.5"));
        assert_eq!(parse_fx("-0.25").unwrap(), saladin_sim::fx!("-0.25"));
        assert_eq!(parse_fx("+7").unwrap(), Fx::from_num(7));
        assert!(parse_fx("1e5").is_err());
        assert!(parse_fx("twelve").is_err());
        assert!(parse_fx("").is_err());
    }

    #[test]
    fn a_bad_request_is_a_value_not_a_panic() {
        assert!(command_from_json(&json!({"Nonsense": {}})).is_err());
        assert!(command_from_json(&json!({"Train": {"player_id": 1}})).is_err());
        assert!(command_from_json(&json!({"Train": {"player_id": 1, "kind": "Spear"}})).is_err());
        assert!(command_from_json(&json!({"Move": {"player_id": 1, "unit": 1}})).is_err());
        assert!(command_from_json(&json!([1, 2])).is_err());
        assert!(
            command_from_json(&json!({"Train": {"player_id": -1, "kind": "Archer"}})).is_err()
        );
    }
}
