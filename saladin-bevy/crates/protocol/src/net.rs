//! Deterministic lockstep core. The transport ships only `PlayerCommand`s; every
//! peer applies the same ordered batch each tick and re-simulates — so a match of
//! any unit count costs `players × inputs` on the wire, never `entities`. This is
//! the model the whole port is built for. The driver is transport-agnostic; an
//! in-memory transport proves it in tests, a TCP one (net_tcp) plays it for real.

use crate::{CommandQueue, PlayerCommand, step};
use bevy_ecs::prelude::World;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Carries each peer's inputs for a future tick and returns the complete,
/// deterministically-ordered batch for a tick once every peer has submitted.
pub trait Transport {
    fn submit(&mut self, tick: u64, player_id: u64, cmds: Vec<PlayerCommand>);
    /// All peers' inputs for `tick` (sorted by player id) if everyone submitted,
    /// else `None` (the sim must stall — never advance on partial input).
    fn batch(&mut self, tick: u64) -> Option<Vec<(u64, Vec<PlayerCommand>)>>;
}

/// Drives one client through lockstep ticks. Local input is buffered and shipped
/// `delay` ticks ahead so the batch for the current tick is usually already in.
pub struct LockstepDriver {
    pub local_player: u64,
    pub delay: u64,
    pub tick: u64,
    pending: Vec<PlayerCommand>,
    primed: bool,
}

impl LockstepDriver {
    pub fn new(local_player: u64, delay: u64) -> Self {
        LockstepDriver { local_player, delay: delay.max(1), tick: 0, pending: Vec::new(), primed: false }
    }

    /// Queue a local intent for the next outgoing submission.
    pub fn push(&mut self, cmd: PlayerCommand) {
        self.pending.push(cmd);
    }

    /// Try to advance one lockstep tick. Submits local input for `tick+delay`,
    /// then applies + steps the batch for `tick` if it is complete. Returns false
    /// (stall) when the batch hasn't arrived yet.
    pub fn advance(&mut self, world: &mut World, transport: &mut dyn Transport) -> bool {
        if !self.primed {
            // seed the first `delay` ticks (nobody submits for them) so their
            // batches complete immediately
            for t in 0..self.delay {
                transport.submit(t, self.local_player, Vec::new());
            }
            self.primed = true;
        }
        let local = std::mem::take(&mut self.pending);
        transport.submit(self.tick + self.delay, self.local_player, local);

        match transport.batch(self.tick) {
            Some(batch) => {
                let cq = &mut world.resource_mut::<CommandQueue>().0;
                for (_pid, cmds) in batch {
                    cq.extend(cmds);
                }
                step(world);
                self.tick += 1;
                true
            }
            None => false,
        }
    }
}

// ── in-memory transport (tests + single-process hotseat) ─────────────────────

#[derive(Default)]
pub struct RelayState {
    players: Vec<u64>,
    subs: HashMap<(u64, u64), Vec<PlayerCommand>>,
}

impl RelayState {
    pub fn new(players: Vec<u64>) -> Self {
        RelayState { players, subs: HashMap::new() }
    }

    fn submit(&mut self, tick: u64, pid: u64, cmds: Vec<PlayerCommand>) {
        self.subs.entry((tick, pid)).or_insert(cmds);
    }

    fn batch(&self, tick: u64) -> Option<Vec<(u64, Vec<PlayerCommand>)>> {
        let mut out = Vec::with_capacity(self.players.len());
        for &p in &self.players {
            out.push((p, self.subs.get(&(tick, p))?.clone()));
        }
        out.sort_by_key(|(p, _)| *p);
        Some(out)
    }
}

pub type SharedRelay = Arc<Mutex<RelayState>>;

pub fn shared_relay(players: Vec<u64>) -> SharedRelay {
    Arc::new(Mutex::new(RelayState::new(players)))
}

/// A transport backed by a shared in-process relay — every peer in the test (or
/// a hotseat) holds one cloned handle.
pub struct MemTransport {
    relay: SharedRelay,
}

impl MemTransport {
    pub fn new(relay: SharedRelay) -> Self {
        MemTransport { relay }
    }
}

impl Transport for MemTransport {
    fn submit(&mut self, tick: u64, player_id: u64, cmds: Vec<PlayerCommand>) {
        self.relay.lock().unwrap().submit(tick, player_id, cmds);
    }
    fn batch(&mut self, tick: u64) -> Option<Vec<(u64, Vec<PlayerCommand>)>> {
        self.relay.lock().unwrap().batch(tick)
    }
}
