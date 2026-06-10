use bevy_ecs::prelude::*;
use saladin_sim::{
    AiDifficulty, AiPhase, BuildingKind, Faction, Fx, GatherState, MatchStatus, ResourceType,
    Stance, Stockpile, UnitKind, V2,
};
use serde::{Deserialize, Serialize};

/// Stable, deterministic game id. Bevy `Entity` ids are NOT identical across
/// lockstep clients, so cross-references (targets, keep, garrison host) use this
/// instead. The `GameIndex` resource maps it back to an `Entity`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameId(pub u64);

/// The owning player's stable id (0..N for humans, high ids for bots).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Owner(pub u64);

/// The match an entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MatchId(pub u64);

/// Hot positional state — written every move tick. `facing` is a render hint
/// (radians) recomputed from movement; it does not affect the sim.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Pos {
    pub pos: V2,
    pub facing: Fx,
}

/// A mobile unit: ownership + movement intent + gather/combat state.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub kind: UnitKind,
    pub target: V2,
    pub has_target: bool,
    pub speed: Fx,
    pub gather_state: GatherState,
    pub target_node: u64,
    pub carrying: i32,
    pub carry_type: ResourceType,
    pub harvest_timer: Fx,
    pub hp: i32,
    pub attack_target: u64,
    pub attack_cooldown: Fx,
    pub stance: Stance,
    pub morale: Fx,
    pub routing: bool,
    pub home: V2,
    pub garrisoned_in: u64,
    pub path: Vec<V2>,
    pub path_idx: usize,
}

/// A static structure.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Building {
    pub kind: BuildingKind,
    pub hp: i32,
    pub cooldown: Fx,
    pub rally: V2,
}

/// A harvestable resource node (position lives in `Pos`).
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ResourceNode {
    pub res_type: ResourceType,
    pub remaining: i32,
}

/// A player (human or bot) — its own entity carrying the stockpile + faction.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub player_id: u64,
    pub name: String,
    pub faction: Faction,
    pub stock: Stockpile,
    pub color: u8,
    pub online: bool,
    pub keep: u64,
    pub defeated: bool,
    pub slot: u8,
    pub tech_mask: u64,
}

/// AI driver state attached to a bot player entity.
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bot {
    pub host: u64,
    pub difficulty: AiDifficulty,
    pub decision_cd: Fx,
    pub wave_timer: Fx,
    pub phase: AiPhase,
    pub scout_id: u64,
    pub threat_timer: Fx,
}

/// One in-flight/completed research, attached to a player entity (one per tech).
#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Research {
    pub owner: u64,
    pub tech: u8,
    pub progress: Fx,
    pub done: bool,
}

/// Marker for an entity whose owning player has been defeated / should despawn
/// at end of tick (deferred cleanup keeps sim mutation ordered).
#[derive(Component, Clone, Copy, Debug)]
pub struct Despawn;

/// One match's lifecycle row (mirrors the SpacetimeDB `match` table). Systems
/// simulate only `Active` matches, so `Paused` freezes one in place.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct MatchInfo {
    pub match_id: u64,
    pub name: String,
    pub host: u64,
    pub status: MatchStatus,
    pub seed: u32,
}
