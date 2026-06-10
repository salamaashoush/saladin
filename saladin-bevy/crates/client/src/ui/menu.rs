//! Main menu + skirmish setup (port of Menu.tsx/MainMenu.tsx/SkirmishSetup.tsx/
//! OpponentList.tsx): faction picker, opponent list with per-rival difficulty,
//! map seed cycling, and the start button. Multiplayer lobby arrives with the
//! websocket transport phase.

use super::theme::*;
use super::widgets::{Disabled, label};
use crate::{GameState, MenuConfig, UiFont};
use bevy::prelude::*;
use saladin_sim::{AiDifficulty, Faction};

#[derive(Component)]
pub struct MenuRoot;

/// Menu-only actions (kept separate from the HUD's `UiAction`).
#[derive(Component, Clone, Copy, PartialEq)]
pub enum MenuAction {
    LoadGame,
    HostGame,
    Faction(Faction),
    AddOpponent,
    RemoveOpponent,
    Difficulty(AiDifficulty),
    CycleSeed,
    Start,
}

/// Digest for rebuild-on-change.
#[derive(Resource, Default, PartialEq)]
pub struct MenuDigest(String);

pub fn setup_menu(mut commands: Commands) {
    commands.spawn((
        MenuRoot,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.05, 0.05, 0.08)),
    ));
}

pub fn cleanup_menu(mut commands: Commands, q: Query<Entity, With<MenuRoot>>, mut digest: ResMut<MenuDigest>) {
    for e in &q {
        commands.entity(e).despawn();
    }
    digest.0.clear();
}

pub fn update_menu(
    mut commands: Commands,
    font: Res<UiFont>,
    cfg: Res<MenuConfig>,
    mut digest: ResMut<MenuDigest>,
    q_root: Query<Entity, With<MenuRoot>>,
) {
    let key = format!("{:?}|{:?}|{}|{}", cfg.faction, cfg.opponents, cfg.difficulty as u8, cfg.seed);
    if digest.0 == key {
        return;
    }
    digest.0 = key;
    let Ok(root) = q_root.single() else { return };
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|p| {
            label(p, &font, "SALADIN", 30.0, GOLD);
            label(p, &font, "A real-time strategy of the Crusades", FONT_SM, TEXT_DIM);

            label(p, &font, "Faction", FONT_SM, TEXT_DIM);
            p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
                .with_children(|p| {
                    for (f, name) in [(Faction::Ayyubid, "Ayyubids"), (Faction::Crusader, "Crusaders")] {
                        menu_button(p, &font, MenuAction::Faction(f), name, cfg.faction == f, false);
                    }
                });

            label(p, &font, &format!("Opponents: {}", cfg.opponents), FONT_SM, TEXT_DIM);
            p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
                .with_children(|p| {
                    menu_button(p, &font, MenuAction::RemoveOpponent, "-", false, cfg.opponents <= 1);
                    menu_button(p, &font, MenuAction::AddOpponent, "+", false, cfg.opponents >= 7);
                });

            label(p, &font, "Difficulty", FONT_SM, TEXT_DIM);
            p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
                .with_children(|p| {
                    for (d, name) in [
                        (AiDifficulty::Easy, "Easy"),
                        (AiDifficulty::Normal, "Normal"),
                        (AiDifficulty::Hard, "Hard"),
                    ] {
                        menu_button(p, &font, MenuAction::Difficulty(d), name, cfg.difficulty == d, false);
                    }
                });

            menu_button(p, &font, MenuAction::CycleSeed, &format!("Map seed: {}", cfg.seed), false, false);
            menu_button(p, &font, MenuAction::Start, "Begin the Campaign", false, false);
            if crate::save_exists() {
                menu_button(p, &font, MenuAction::LoadGame, "Load Game", false, false);
            }
            menu_button(p, &font, MenuAction::HostGame, "Host Game (LAN)", false, false);
            label(p, &font, "Friends join with: saladin-client connect <your-ip>", 10.0, TEXT_DIM);
        });
    });
}

fn menu_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    action: MenuAction,
    title: &str,
    active: bool,
    disabled: bool,
) {
    let bg = if disabled {
        BTN_BG_DISABLED
    } else if active {
        BTN_BG_ACTIVE
    } else {
        BTN_BG
    };
    p.spawn((
        Button,
        action,
        Disabled(disabled),
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(if active { ACCENT } else { PANEL_BORDER }),
    ))
    .with_children(|p| label(p, font, title, FONT_MD, if disabled { TEXT_DIM } else { TEXT }));
}

pub fn menu_actions(
    q: Query<(&Interaction, &MenuAction, &Disabled), Changed<Interaction>>,
    mut cfg: ResMut<MenuConfig>,
    conn: Res<crate::LobbyConn>,
    mut next: ResMut<NextState<GameState>>,
) {
    for (i, action, disabled) in &q {
        if *i != Interaction::Pressed || disabled.0 {
            continue;
        }
        match *action {
            MenuAction::LoadGame => {
                cfg.load = true;
                next.set(GameState::Playing);
            }
            MenuAction::HostGame => {
                let bind = format!("0.0.0.0:{}", crate::HOST_PORT);
                match saladin_protocol::spawn_host_relay(&bind)
                    .and_then(|_| saladin_protocol::TcpTransport::connect(&format!("127.0.0.1:{}", crate::HOST_PORT)))
                {
                    Ok(t) => {
                        *conn.0.lock().unwrap() = Some(t);
                        next.set(GameState::Lobby);
                    }
                    Err(e) => eprintln!("host failed: {e}"),
                }
            }
            MenuAction::Faction(f) => cfg.faction = f,
            MenuAction::AddOpponent => cfg.opponents = (cfg.opponents + 1).min(7),
            MenuAction::RemoveOpponent => cfg.opponents = cfg.opponents.saturating_sub(1).max(1),
            MenuAction::Difficulty(d) => cfg.difficulty = d,
            MenuAction::CycleSeed => cfg.seed = cfg.seed.wrapping_mul(1664525).wrapping_add(1013904223) % 100_000,
            MenuAction::Start => next.set(GameState::Playing),
        }
    }
}

/// Game-over overlay with a back-to-menu button.
#[derive(Component)]
pub struct GameOverRoot;

#[derive(Component)]
pub struct GameOverAction;

pub fn setup_gameover(
    mut commands: Commands,
    font: Res<UiFont>,
    local: Res<crate::LocalPlayer>,
    q_players: Query<&saladin_protocol::Player>,
) {
    let won = q_players.iter().find(|p| p.player_id == local.0).map(|p| !p.defeated).unwrap_or(false);
    let (title, color) = if won { ("VICTORY", ACCENT) } else { ("DEFEAT", WARN) };
    commands
        .spawn((
            GameOverRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        ))
        .with_children(|p| {
            label(p, &font, title, 48.0, color);
            p.spawn((
                Button,
                GameOverAction,
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(BTN_BG),
                BorderColor::all(PANEL_BORDER),
            ))
            .with_children(|p| label(p, &font, "Back to Menu", FONT_LG, TEXT));
        });
}

pub fn cleanup_gameover(mut commands: Commands, q: Query<Entity, With<GameOverRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

pub fn gameover_actions(
    q: Query<&Interaction, (Changed<Interaction>, With<GameOverAction>)>,
    mut next: ResMut<NextState<GameState>>,
) {
    for i in &q {
        if *i == Interaction::Pressed {
            next.set(GameState::Menu);
        }
    }
}

// ── multiplayer lobby ────────────────────────────────────────────────────────

#[derive(Component)]
pub struct LobbyRoot;

#[derive(Component, Clone, Copy, PartialEq)]
pub enum LobbyAction {
    Start,
    Cancel,
}

#[derive(Resource, Default)]
pub struct LobbyDigest(String);

pub fn setup_lobby(mut commands: Commands) {
    commands.init_resource::<LobbyDigest>();
    commands.spawn((
        LobbyRoot,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.05, 0.05, 0.08)),
    ));
}

pub fn cleanup_lobby(mut commands: Commands, q: Query<Entity, With<LobbyRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

pub fn update_lobby(
    mut commands: Commands,
    font: Res<UiFont>,
    conn: Res<crate::LobbyConn>,
    mut digest: ResMut<LobbyDigest>,
    q_root: Query<Entity, With<LobbyRoot>>,
) {
    let guard = conn.0.lock().unwrap();
    let Some(t) = guard.as_ref() else { return };
    let l = t.lobby();
    let key = format!("{}|{}|{}|{:?}|{:?}", l.connected, l.you, l.host, l.players, l.error);
    if digest.0 == key {
        return;
    }
    digest.0 = key;
    let Ok(root) = q_root.single() else { return };
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|p| {
            label(p, &font, "MULTIPLAYER LOBBY", FONT_LG, GOLD);
            if let Some(err) = &l.error {
                label(p, &font, err, FONT_SM, WARN);
            } else if !l.connected {
                label(p, &font, "Connecting...", FONT_SM, TEXT_DIM);
            } else {
                label(p, &font, &format!("You are player {}", l.you), FONT_SM, TEXT);
                label(p, &font, &format!("Players in lobby: {}", l.players.len()), FONT_SM, TEXT);
                for pid in &l.players {
                    let tag = if *pid == l.host { " (host)" } else { "" };
                    label(p, &font, &format!("Player {pid}{tag}"), FONT_SM, TEXT_DIM);
                }
                if l.you == l.host {
                    lobby_button(p, &font, LobbyAction::Start, "Start Match", l.players.len() < 2);
                } else {
                    label(p, &font, "Waiting for the host to start...", FONT_SM, TEXT_DIM);
                }
            }
            lobby_button(p, &font, LobbyAction::Cancel, "Leave Lobby", false);
        });
    });
}

fn lobby_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    action: LobbyAction,
    title: &str,
    disabled: bool,
) {
    p.spawn((
        Button,
        action,
        Disabled(disabled),
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(if disabled { BTN_BG_DISABLED } else { BTN_BG }),
        BorderColor::all(PANEL_BORDER),
    ))
    .with_children(|p| label(p, font, title, FONT_MD, if disabled { TEXT_DIM } else { TEXT }));
}

pub fn lobby_actions(
    q: Query<(&Interaction, &LobbyAction, &Disabled), Changed<Interaction>>,
    conn: Res<crate::LobbyConn>,
    mut next: ResMut<NextState<GameState>>,
) {
    for (i, action, disabled) in &q {
        if *i != Interaction::Pressed || disabled.0 {
            continue;
        }
        match *action {
            LobbyAction::Start => {
                if let Some(t) = conn.0.lock().unwrap().as_mut() {
                    t.request_start();
                }
            }
            LobbyAction::Cancel => {
                *conn.0.lock().unwrap() = None;
                next.set(GameState::Menu);
            }
        }
    }
}
