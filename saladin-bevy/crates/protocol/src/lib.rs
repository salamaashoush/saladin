//! Deterministic Bevy simulation layer for Saladin. ECS components mirror the
//! old SpacetimeDB tables; systems run in a dedicated `SimSchedule` driven one
//! fixed step at a time (by the server loop or, under lockstep, by the netcode),
//! so every client re-simulates to a bit-identical state. No wall-clock time, no
//! `rand` — all gameplay math goes through `saladin_sim` (fixed-point).

pub mod commands;
pub mod components;
pub mod net;
pub mod net_msg;
#[cfg(not(target_arch = "wasm32"))]
pub mod net_tcp;
pub mod net_ws;
#[cfg(not(target_arch = "wasm32"))]
pub mod relay_core;
pub mod save;
pub mod systems;

#[cfg(not(target_arch = "wasm32"))]
pub use net_tcp::{TcpTransport, run_relay, spawn_host_relay};
#[cfg(not(target_arch = "wasm32"))]
pub use net_ws::run_relay_ws;
pub use net_msg::{
    JoinIntent, LobbyPlayer, LobbyState, PROTOCOL_VERSION, ROOM_CODE_ALPHABET, ROOM_CODE_LEN,
    RejectReason, normalize_room_code,
};
pub use net_ws::WsTransport;

pub use commands::{CommandQueue, PlayerCommand, apply_commands, scatter_world_nodes};
pub use components::*;
pub use net::{LockstepDriver, MemTransport, RelayState, SharedRelay, Transport, shared_relay};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_platform::collections::HashMap;
use saladin_sim::{AStar, MapBias, MatchStatus, NEUTRAL_BIAS, Rng, match_simulates};

/// The schedule that advances the simulation exactly one base tick (50 ms of
/// game time). Run it N times to simulate N ticks — deterministically.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SimSchedule;

/// System ordering within one base tick. Explicit + chained so parallel system
/// execution can never reorder mutations (a lockstep determinism requirement).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimSet {
    Index,
    Movement,
    Gather,
    Combat,
    Economy,
    Brain,
    Research,
    Cleanup,
}

/// Monotonic base-tick counter. Drives sub-rate scheduling and is part of the
/// state hash.
#[derive(Resource, Clone, Copy, Default, Debug)]
pub struct Tick(pub u64);

/// Source of fresh, deterministic `GameId`s. Every client allocates in the same
/// order, so ids match across the lockstep group.
#[derive(Resource, Clone, Copy, Debug)]
pub struct NextEntityId(pub u64);

impl Default for NextEntityId {
    fn default() -> Self {
        NextEntityId(1)
    }
}

impl NextEntityId {
    pub fn alloc(&mut self) -> u64 {
        let id = self.0;
        self.0 += 1;
        id
    }
}

/// `GameId` → `Entity` lookup, maintained each tick. Uses bevy_platform's fixed
/// hasher so iteration/order is deterministic.
#[derive(Resource, Default)]
pub struct GameIndex(pub HashMap<u64, Entity>);

impl GameIndex {
    pub fn get(&self, id: u64) -> Option<Entity> {
        self.0.get(&id).copied()
    }
}

/// World generation parameters, fixed at match start.
#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    pub seed: u32,
    pub bias: MapBias,
}

impl Default for WorldConfig {
    fn default() -> Self {
        WorldConfig { seed: 1, bias: NEUTRAL_BIAS }
    }
}

/// Reusable A* scratch so pathfinding never reallocates per unit.
#[derive(Resource, Default)]
pub struct PathScratch(pub AStar);

/// The deterministic state checksum of the most recent tick (desync detection).
#[derive(Resource, Clone, Copy, Default, Debug)]
pub struct StateHash(pub u64);

/// One ranged strike this combat tick, for the client's arrow-arc visuals.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub from: saladin_sim::V2,
    pub to: saladin_sim::V2,
}

/// Render-only firing events from the most recent combat tick. Replaces the old
/// SpacetimeDB `shot` event table: combat refills it each combat tick, the
/// client drains it. Never part of the state hash (visual only — though under
/// lockstep it is identical on every client anyway).
#[derive(Resource, Default)]
pub struct ShotEvents(pub Vec<Shot>);

/// Deterministic gameplay RNG (train-spawn jitter etc.). Seeded with a fixed
/// constant so every lockstep client draws the identical stream — commands are
/// applied in the same order everywhere, so the streams stay aligned.
#[derive(Resource, Clone, Copy, Debug)]
pub struct SimRng(pub Rng);

impl Default for SimRng {
    fn default() -> Self {
        SimRng(Rng::new(0x5a1a_d1aa))
    }
}

/// `match_id → status` snapshot, rebuilt each tick from the `MatchInfo` rows so
/// every sub-rate system can cheaply skip entities in Paused/Ended matches.
#[derive(Resource, Default)]
pub struct MatchStatuses(pub HashMap<u64, MatchStatus>);

impl MatchStatuses {
    /// A match with no row simulates (keeps bare test worlds running).
    pub fn simulates(&self, match_id: u64) -> bool {
        self.0.get(&match_id).copied().is_none_or(match_simulates)
    }
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tick>()
            .init_resource::<NextEntityId>()
            .init_resource::<GameIndex>()
            .init_resource::<WorldConfig>()
            .init_resource::<PathScratch>()
            .init_resource::<StateHash>()
            .init_resource::<SimRng>()
            .init_resource::<MatchStatuses>()
            .init_resource::<ShotEvents>()
            .init_resource::<CommandQueue>()
            .init_schedule(SimSchedule);

        systems::register(app);
    }
}

/// Advance the simulation exactly one base tick.
pub fn step(world: &mut World) {
    world.run_schedule(SimSchedule);
}

/// Run condition: true every `n` base ticks (for sub-rate systems).
pub fn every(n: u64) -> impl FnMut(Res<Tick>) -> bool + Clone {
    move |tick: Res<Tick>| tick.0 % n == 0
}
