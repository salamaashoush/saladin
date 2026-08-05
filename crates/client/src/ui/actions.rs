//! Central HUD action dispatch: every button carries a `UiAction`; one system
//! turns presses into lockstep commands / input-mode changes / state moves.

use super::widgets::Disabled;
use crate::input::InputMode;
use crate::selection::{SelectedBuilding, Selection};
use crate::{LocalInput, LocalPlayer};
use bevy::prelude::*;
use saladin_protocol::{GameId, Owner, PlayerCommand, Pos, Unit};
use saladin_sim::{
    BuildingKind, GatherState, MAX_BUILDERS, ResourceType, Stance, UnitKind, dist2, unit_def,
};

pub const MARKET_LOT: i32 = 20;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum UiAction {
    Tab(usize),
    Build(BuildingKind),
    ToggleDemolish,
    GatherAll,
    Sell(ResourceType),
    Buy(ResourceType),
    Research(u8),
    Ungarrison,
    DemolishSelected,
    Stance(Stance),
    /// Queue a unit at the SELECTED hall — "which barracks" is addressed by
    /// GameId, not left to whichever one the sim happens to iterate first.
    TrainAt(UnitKind),
    CancelTrain,
    /// Put hands on the selected structure: found it, finish it or mend it.
    SendBuilders,
    UpgradeSelected,
    CancelSite,
}

/// Which build-bar tab is open.
#[derive(Resource, Default)]
pub struct BuildTab(pub usize);

/// The peasants to put on `sel`: the ones already selected if the player picked
/// any, otherwise the nearest hands not already carrying or fighting. Sorted by
/// distance then GameId so the pick never depends on ECS iteration order.
fn crew_for(
    selection: &Selection,
    sel: &SelectedBuilding,
    q_units: &Query<(&GameId, &Owner, &Pos, &Unit)>,
    me: u64,
) -> Vec<u64> {
    let free = (MAX_BUILDERS - sel.builders).max(0) as usize;
    if free == 0 {
        return Vec::new();
    }
    let carriers = || {
        q_units.iter().filter(move |(_, o, _, u)| {
            o.0 == me && u.garrisoned_in == 0 && unit_def(u.kind).carry > 0
        })
    };
    let picked: Vec<u64> =
        carriers().filter(|(g, ..)| selection.units.contains(&g.0)).map(|(g, ..)| g.0).collect();
    if !picked.is_empty() {
        let mut ids = picked;
        ids.sort_unstable();
        ids.truncate(free);
        return ids;
    }
    let mut idle: Vec<(saladin_sim::Fx, u64)> = carriers()
        .filter(|(_, _, _, u)| {
            matches!(u.gather_state, GatherState::Idle | GatherState::Constructing)
                && u.job_site != sel.id.unwrap_or(0)
        })
        .map(|(g, _, p, _)| (dist2(p.pos, sel.pos), g.0))
        .collect();
    idle.sort_unstable();
    idle.truncate(free);
    idle.into_iter().map(|(_, id)| id).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn handle_actions(
    q: Query<(&Interaction, &UiAction, &Disabled), Changed<Interaction>>,
    local: Res<LocalPlayer>,
    selection: Res<Selection>,
    sel_building: Res<SelectedBuilding>,
    q_units: Query<(&GameId, &Owner, &Pos, &Unit)>,
    mut tab: ResMut<BuildTab>,
    mut mode: ResMut<InputMode>,
    mut input: ResMut<LocalInput>,
) {
    let me = local.0;
    for (interaction, action, disabled) in &q {
        if *interaction != Interaction::Pressed || disabled.0 {
            continue;
        }
        match *action {
            UiAction::Tab(i) => tab.0 = i,
            UiAction::Build(kind) => {
                *mode = if *mode == InputMode::Build(kind) { InputMode::Normal } else { InputMode::Build(kind) };
            }
            UiAction::ToggleDemolish => {
                *mode = if *mode == InputMode::Demolish { InputMode::Normal } else { InputMode::Demolish };
            }
            UiAction::GatherAll => input.0.push(PlayerCommand::AutoGather { player_id: me }),
            UiAction::Sell(res) => {
                input.0.push(PlayerCommand::MarketTrade { player_id: me, res, amount: MARKET_LOT })
            }
            UiAction::Buy(res) => {
                input.0.push(PlayerCommand::MarketBuy { player_id: me, res, amount: MARKET_LOT })
            }
            UiAction::TrainAt(kind) => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::TrainAt { player_id: me, building: b, kind });
                }
            }
            UiAction::CancelTrain => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::CancelTrain { player_id: me, building: b });
                }
            }
            UiAction::SendBuilders => {
                if let Some(b) = sel_building.id {
                    for unit in crew_for(&selection, &sel_building, &q_units, me) {
                        input.0.push(PlayerCommand::Repair { player_id: me, unit, building: b });
                    }
                }
            }
            UiAction::UpgradeSelected => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::UpgradeBuilding { player_id: me, building: b });
                }
            }
            UiAction::CancelSite => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::CancelSite { player_id: me, building: b });
                }
            }
            UiAction::Research(tech) => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::StartResearch { player_id: me, building: b, tech });
                }
            }
            UiAction::Ungarrison => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::Ungarrison { player_id: me, building: b });
                }
            }
            UiAction::DemolishSelected => {
                if let Some(b) = sel_building.id {
                    input.0.push(PlayerCommand::Demolish { player_id: me, building: b });
                }
            }
            UiAction::Stance(stance) => {
                for &unit in &selection.units {
                    input.0.push(PlayerCommand::SetStance { player_id: me, unit, stance });
                }
            }
        }
    }
}
