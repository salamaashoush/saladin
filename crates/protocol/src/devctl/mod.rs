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

mod state;
mod wire;

pub use wire::{COMMAND_NAMES, command_from_json, command_to_json};

use crate::{CommandFeedback, PlayerCommand, StateHash, Tick};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use saladin_sim::place_error_text;
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
        return state::query(world, v, &req, reply);
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
        return reply.err(
            "step is headless-only: a running client's time belongs to its own clock, and in a \
             match to the lockstep group",
        );
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
    use wire::v2_from;
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

pub(crate) fn drain_feedback(world: &mut World) -> Vec<Value> {
    std::mem::take(&mut world.resource_mut::<Devctl>().feedback)
}

