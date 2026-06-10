//! Central HUD action dispatch: every button carries a `UiAction`; one system
//! turns presses into lockstep commands / input-mode changes / state moves.

use super::widgets::Disabled;
use crate::input::InputMode;
use crate::selection::{SelectedBuilding, Selection};
use crate::{GameState, LocalInput, LocalPlayer, MenuConfig, Multiplayer};
use bevy::prelude::*;
use saladin_protocol::PlayerCommand;
use saladin_sim::{BuildingKind, ResourceType, Stance, UnitKind, enemy_faction};

pub const MARKET_LOT: i32 = 20;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum UiAction {
    Tab(usize),
    Build(BuildingKind),
    ToggleDemolish,
    GatherAll,
    Sell(ResourceType),
    Train(UnitKind),
    Research(u8),
    Ungarrison,
    DemolishSelected,
    Stance(Stance),
    AddAi,
    LeaveMatch,
    SaveQuit,
    PauseToggle,
}

/// Which build-bar tab is open.
#[derive(Resource, Default)]
pub struct BuildTab(pub usize);

#[allow(clippy::too_many_arguments)]
pub fn handle_actions(
    q: Query<(&Interaction, &UiAction, &Disabled), Changed<Interaction>>,
    local: Res<LocalPlayer>,
    cfg: Res<MenuConfig>,
    multiplayer: Res<Multiplayer>,
    selection: Res<Selection>,
    sel_building: Res<SelectedBuilding>,
    mut tab: ResMut<BuildTab>,
    mut mode: ResMut<InputMode>,
    mut input: ResMut<LocalInput>,
    mut next: ResMut<NextState<GameState>>,
    mut pending_save: ResMut<crate::PendingSave>,
    mut next_ai_id: Local<u64>,
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
            UiAction::Train(kind) => input.0.push(PlayerCommand::Train { player_id: me, kind }),
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
            UiAction::AddAi => {
                if *next_ai_id == 0 {
                    *next_ai_id = 2000;
                }
                input.0.push(PlayerCommand::AddAi {
                    player_id: *next_ai_id,
                    host: me,
                    difficulty: cfg.difficulty,
                    faction: enemy_faction(cfg.faction),
                    match_id: 1,
                });
                *next_ai_id += 1;
            }
            UiAction::LeaveMatch => {
                if !multiplayer.0 {
                    next.set(GameState::Menu);
                }
            }
            UiAction::SaveQuit => {
                if !multiplayer.0 {
                    pending_save.0 = true;
                }
            }
            UiAction::PauseToggle => input.0.push(PlayerCommand::Pause { player_id: me }),
        }
    }
}

