use crate::components::*;
use bevy_ecs::prelude::*;
use saladin_sim::*;
use std::collections::HashSet;

mod build_cmds;
mod economy_cmds;
mod garrison_cmds;
mod match_ctl;
mod spawn;
mod unit_cmds;

pub use spawn::scatter_world_nodes;

pub(crate) use build_cmds::{build, train};
pub(crate) use economy_cmds::start_research;
pub(crate) use unit_cmds::{assign_idle_gatherers, path_to};

/// Player intents. Under lockstep these are the ONLY thing shipped over the wire;
/// every client applies the same ordered batch each tick and re-simulates. The
/// network layer fills `CommandQueue` for tick T with all peers' inputs in a
/// deterministic order before the sim runs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PlayerCommand {
    Join { player_id: u64, name: String, faction: Faction, match_id: u64 },
    AddAi { player_id: u64, host: u64, difficulty: AiDifficulty, faction: Faction, match_id: u64 },
    Move { player_id: u64, unit: u64, target: V2 },
    SetStance { player_id: u64, unit: u64, stance: Stance },
    Train { player_id: u64, kind: UnitKind },
    Build { player_id: u64, kind: BuildingKind, pos: V2 },
    Gather { player_id: u64, unit: u64, node: u64 },
    Attack { player_id: u64, unit: u64, target: u64 },
    SetRally { player_id: u64, building: u64, target: V2 },
    Garrison { player_id: u64, unit: u64, building: u64 },
    Ungarrison { player_id: u64, building: u64 },
    Demolish { player_id: u64, building: u64 },
    PlaceWall { player_id: u64, tiles: Vec<(i32, i32)> },
    MarketTrade { player_id: u64, res: ResourceType, amount: i32 },
    StartResearch { player_id: u64, building: u64, tech: u8 },
    AutoGather { player_id: u64 },
    Pause { player_id: u64 },
    Resume { player_id: u64 },
}

#[derive(Resource, Default)]
pub struct CommandQueue(pub Vec<PlayerCommand>);

/// Drain and apply this tick's command batch. Exclusive (full `&mut World`) so
/// it can spawn, query and pay in one deterministic, single-threaded pass —
/// exactly the property lockstep needs.
pub fn apply_commands(world: &mut World) {
    let cmds = std::mem::take(&mut world.resource_mut::<CommandQueue>().0);
    for cmd in cmds {
        match cmd {
            PlayerCommand::Join { player_id, name, faction, match_id } => {
                spawn::found_player(world, player_id, &name, faction, match_id);
            }
            PlayerCommand::AddAi { player_id, host, difficulty, faction, match_id } => {
                spawn::spawn_ai(world, player_id, host, difficulty, faction, match_id);
            }
            PlayerCommand::Move { player_id, unit, target } => {
                unit_cmds::move_unit(world, player_id, unit, target)
            }
            PlayerCommand::SetStance { player_id, unit, stance } => {
                unit_cmds::set_stance(world, player_id, unit, stance)
            }
            PlayerCommand::Train { player_id, kind } => {
                build_cmds::train(world, player_id, kind);
            }
            PlayerCommand::Build { player_id, kind, pos } => {
                build_cmds::build(world, player_id, kind, pos);
            }
            PlayerCommand::Gather { player_id, unit, node } => {
                unit_cmds::gather(world, player_id, unit, node)
            }
            PlayerCommand::Attack { player_id, unit, target } => {
                unit_cmds::attack(world, player_id, unit, target)
            }
            PlayerCommand::SetRally { player_id, building, target } => {
                build_cmds::set_rally(world, player_id, building, target)
            }
            PlayerCommand::Garrison { player_id, unit, building } => {
                garrison_cmds::garrison(world, player_id, unit, building)
            }
            PlayerCommand::Ungarrison { player_id, building } => {
                garrison_cmds::ungarrison(world, player_id, building)
            }
            PlayerCommand::Demolish { player_id, building } => {
                build_cmds::demolish(world, player_id, building)
            }
            PlayerCommand::PlaceWall { player_id, tiles } => {
                build_cmds::place_wall(world, player_id, &tiles)
            }
            PlayerCommand::MarketTrade { player_id, res, amount } => {
                economy_cmds::market_trade(world, player_id, res, amount)
            }
            PlayerCommand::StartResearch { player_id, building, tech } => {
                economy_cmds::start_research_at(world, player_id, building, tech);
            }
            PlayerCommand::AutoGather { player_id } => unit_cmds::auto_gather(world, player_id),
            PlayerCommand::Pause { player_id } => {
                match_ctl::set_match_status(world, player_id, MatchStatus::Paused)
            }
            PlayerCommand::Resume { player_id } => {
                match_ctl::set_match_status(world, player_id, MatchStatus::Active)
            }
        }
    }
}

// ── shared lookups ───────────────────────────────────────────────────────────

pub(crate) fn tech_mask_of(world: &mut World, owner: u64) -> u64 {
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == owner).map(|p| p.tech_mask).unwrap_or(0)
}

pub(crate) fn owned_building_kinds(world: &mut World, owner: u64) -> HashSet<BuildingKind> {
    let mut q = world.query::<(&Owner, &Building)>();
    q.iter(world).filter(|(o, _)| o.0 == owner).map(|(_, b)| b.kind).collect()
}

pub(crate) fn building_occupancy(world: &mut World, include_passable: bool) -> HashSet<i32> {
    let mut q = world.query::<(&Pos, &Building)>();
    let occ: Vec<Occupant> = q.iter(world).map(|(p, b)| Occupant { kind: b.kind, pos: p.pos }).collect();
    occupancy_set(&occ, include_passable)
}

/// The caller's entity for `id` — only if the caller owns it (the lockstep
/// equivalent of the reducer's `ctx.sender` authority check).
pub(crate) fn find_owned(world: &mut World, owner: u64, id: u64) -> Option<Entity> {
    let mut q = world.query::<(Entity, &GameId, &Owner)>();
    q.iter(world).find(|(_, g, o)| g.0 == id && o.0 == owner).map(|(e, _, _)| e)
}

pub(crate) fn player_match(world: &mut World, owner: u64) -> Option<u64> {
    let mut q = world.query::<(&Player, &MatchId)>();
    q.iter(world).find(|(p, _)| p.player_id == owner).map(|(_, m)| m.0)
}

pub(crate) fn clamp_world(v: Fx) -> Fx {
    v.clamp(Fx::ZERO, Fx::from_num(WORLD_SIZE))
}
