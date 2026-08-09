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

pub(crate) use build_cmds::{
    assign_builders, build_context, build_with, finish_building, repair, spawn_trained, train,
    upgrade_building,
};
pub(crate) use economy_cmds::{market_buy_cmd, market_trade, start_research};
pub(crate) use garrison_cmds::{disembark, embark, garrison, ungarrison};
pub(crate) use unit_cmds::{assign_idle_gatherers, group_attack, group_move, move_unit};
pub use unit_cmds::path_to;

/// Player intents. Under lockstep these are the ONLY thing shipped over the wire;
/// every client applies the same ordered batch each tick and re-simulates. The
/// network layer fills `CommandQueue` for tick T with all peers' inputs in a
/// deterministic order before the sim runs.
///
/// bincode encodes the VARIANT INDEX, so new variants are APPENDED. Inserting
/// one in the middle silently renumbers every later variant — a Pause would
/// decode as garbage rather than fail. Any change here bumps PROTOCOL_VERSION.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PlayerCommand {
    Join { player_id: u64, name: String, faction: Faction, match_id: u64 },
    AddAi { player_id: u64, host: u64, difficulty: AiDifficulty, faction: Faction, match_id: u64 },
    Move { player_id: u64, unit: u64, target: V2 },
    SetStance { player_id: u64, unit: u64, stance: Stance },
    Train { player_id: u64, kind: UnitKind },
    /// Found a construction site. `builders` are the peasants sent to raise it
    /// (the client fills them from the selection, the bot from its spare hands).
    Build { player_id: u64, kind: BuildingKind, pos: V2, facing: u8, builders: Vec<u64> },
    Gather { player_id: u64, unit: u64, node: u64 },
    Attack { player_id: u64, unit: u64, target: u64 },
    SetRally { player_id: u64, building: u64, target: V2 },
    Garrison { player_id: u64, unit: u64, building: u64 },
    Ungarrison { player_id: u64, building: u64 },
    Demolish { player_id: u64, building: u64 },
    PlaceWall { player_id: u64, tiles: Vec<(i32, i32)>, builders: Vec<u64> },
    MarketTrade { player_id: u64, res: ResourceType, amount: i32 },
    MarketBuy { player_id: u64, res: ResourceType, amount: i32 },
    StartResearch { player_id: u64, building: u64, tech: u8 },
    AutoGather { player_id: u64 },
    Pause { player_id: u64 },
    Resume { player_id: u64 },
    /// Put a peasant to work on a structure. ONE command covers founding
    /// labour, repair and upgrade labour — they are the same loop.
    Repair { player_id: u64, unit: u64, building: u64 },
    CancelSite { player_id: u64, building: u64 },
    UpgradeBuilding { player_id: u64, building: u64 },
    TrainAt { player_id: u64, building: u64, kind: UnitKind },
    CancelTrain { player_id: u64, building: u64 },
    /// One click, one message, ONE path. `formation` indexes `FormationShape`;
    /// anything outside it marches loose, every man on the destination.
    GroupMove { player_id: u64, units: Vec<u64>, target: V2, formation: u8 },
    /// March and fight what turns up; the march resumes when the fight ends.
    AttackMove { player_id: u64, units: Vec<u64>, target: V2, formation: u8 },
    GroupAttack { player_id: u64, units: Vec<u64>, target: u64 },
    Stop { player_id: u64, units: Vec<u64> },
    /// Load a landing party. Cargo is `Unit::garrisoned_in` pointed at a HULL
    /// instead of a hall: already serialized, already hashed, already skipped by
    /// movement and combat. Its own verb rather than `Garrison` because that one
    /// demands a `Building` row and a kind the tower rules admit.
    Embark { player_id: u64, units: Vec<u64>, boat: u64 },
    /// Put the party ashore on the legal land nearest `target`. No harbour is
    /// needed at the far end — that is the whole point of a beach landing.
    Disembark { player_id: u64, boat: u64, target: V2 },
}

#[derive(Resource, Default)]
pub struct CommandQueue(pub Vec<PlayerCommand>);

/// Drain and apply this tick's command batch. Exclusive (full `&mut World`) so
/// it can spawn, query and pay in one deterministic, single-threaded pass —
/// exactly the property lockstep needs.
pub fn apply_commands(world: &mut World) {
    world.resource_mut::<crate::CommandFeedback>().0.clear();
    let cmds = std::mem::take(&mut world.resource_mut::<CommandQueue>().0);
    let mut paths = unit_cmds::GROUP_PATHS_PER_TICK;
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
            PlayerCommand::Build { player_id, kind, pos, facing, builders } => {
                if let Err(e) = build_cmds::build(world, player_id, kind, pos, facing, &builders) {
                    world.resource_mut::<crate::CommandFeedback>().0.push((player_id, e));
                }
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
            PlayerCommand::PlaceWall { player_id, tiles, builders } => {
                build_cmds::place_wall(world, player_id, &tiles, &builders)
            }
            PlayerCommand::MarketTrade { player_id, res, amount } => {
                economy_cmds::market_trade(world, player_id, res, amount)
            }
            PlayerCommand::MarketBuy { player_id, res, amount } => {
                economy_cmds::market_buy_cmd(world, player_id, res, amount)
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
            PlayerCommand::Repair { player_id, unit, building } => {
                build_cmds::repair(world, player_id, unit, building);
            }
            PlayerCommand::CancelSite { player_id, building } => {
                build_cmds::cancel_site(world, player_id, building)
            }
            PlayerCommand::UpgradeBuilding { player_id, building } => {
                build_cmds::upgrade_building(world, player_id, building);
            }
            PlayerCommand::TrainAt { player_id, building, kind } => {
                build_cmds::train_at(world, player_id, building, kind);
            }
            PlayerCommand::CancelTrain { player_id, building } => {
                build_cmds::cancel_train(world, player_id, building)
            }
            PlayerCommand::GroupMove { player_id, units, target, formation } => {
                unit_cmds::group_move(world, player_id, &units, target, formation, &mut paths)
            }
            PlayerCommand::AttackMove { player_id, units, target, formation } => {
                unit_cmds::attack_move(world, player_id, &units, target, formation, &mut paths)
            }
            PlayerCommand::GroupAttack { player_id, units, target } => {
                unit_cmds::group_attack(world, player_id, &units, target, &mut paths)
            }
            PlayerCommand::Stop { player_id, units } => {
                unit_cmds::stop(world, player_id, &units)
            }
            PlayerCommand::Embark { player_id, units, boat } => {
                garrison_cmds::embark(world, player_id, &units, boat)
            }
            PlayerCommand::Disembark { player_id, boat, target } => {
                garrison_cmds::disembark(world, player_id, boat, target)
            }
        }
    }
}

// ── shared lookups ───────────────────────────────────────────────────────────

pub(crate) fn tech_mask_of(world: &mut World, owner: u64) -> u64 {
    let mut q = world.query::<&Player>();
    q.iter(world).find(|p| p.player_id == owner).map(|p| p.tech_mask).unwrap_or(0)
}

/// What the owner can DO, not what stands on the ground: a site under
/// construction unlocks nothing until it is finished.
pub(crate) fn owned_building_kinds(world: &mut World, owner: u64) -> HashSet<BuildingKind> {
    let mut q = world.query::<(&Owner, &Building)>();
    q.iter(world)
        .filter(|(o, b)| o.0 == owner && operational(b.state))
        .map(|(_, b)| b.kind)
        .collect()
}

pub(crate) fn building_occupancy(world: &World, include_passable: bool) -> HashSet<i32> {
    occupancy_and_gates(world, include_passable).0
}

/// Occupancy plus the (tile, owner) list of standing gatehouses. A gate is a
/// door in YOUR line, not a breach in it, so every pathing closure needs both:
/// the tile is walkable, but only for the owner. Unfinished gates are just a
/// foundation — they gate nobody.
pub(crate) fn occupancy_and_gates(
    world: &World,
    include_passable: bool,
) -> (HashSet<i32>, Vec<(i32, u64)>) {
    let Some(mut q) = world.try_query::<(&Pos, &Building, Option<&Owner>)>() else {
        return (HashSet::new(), Vec::new());
    };
    let mut occ = Vec::new();
    let mut gates = Vec::new();
    for (p, b, owner) in q.iter(world) {
        occ.push(Occupant { kind: b.kind, pos: p.pos });
        if building_def(b.kind).passable && operational(b.state) {
            let key = tile_key(p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>());
            gates.push((key, owner.map(|o| o.0).unwrap_or(0)));
        }
    }
    (occupancy_set(&occ, include_passable), gates)
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
