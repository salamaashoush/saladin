//! In-match overlays and app-level screens around the core loop: the Esc
//! pause menu (Resume / Save / Settings / Quit), the settings panel (shared
//! with the main menu), the loading screen between menu and match, and the
//! multiplayer disconnect banner.

use super::theme::*;
use super::widgets::{Disabled, label};
use crate::{GameState, Multiplayer, PendingSave, UiFont, config};
use bevy::prelude::*;

/// Which overlay is up while `GameState::Playing`.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PauseScreen {
    #[default]
    None,
    Menu,
    Settings,
}

#[derive(Component)]
pub struct PauseRoot;

#[derive(Component, Clone, Copy, PartialEq)]
pub enum PauseAction {
    Resume,
    SaveQuit,
    OpenSettings,
    BackToPause,
    QuitToMenu,
    // settings controls (shared with the main-menu settings screen)
    ToggleEdgeScroll,
    UiScale(i8),
    Volume(i8),
}

/// Esc toggles the pause overlay (only from the normal input mode, so it never
/// fights the build/demolish cancel). Singleplayer also pauses the sim via the
/// lockstep Pause command; multiplayer keeps simulating under the overlay.
pub fn pause_hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<crate::input::InputMode>,
    mut screen: ResMut<PauseScreen>,
    multiplayer: Res<Multiplayer>,
    local: Res<crate::LocalPlayer>,
    mut input: ResMut<crate::LocalInput>,
) {
    if !keys.just_pressed(KeyCode::Escape) || *mode != crate::input::InputMode::Normal {
        return;
    }
    match *screen {
        PauseScreen::None => {
            *screen = PauseScreen::Menu;
            if !multiplayer.0 {
                input.0.push(saladin_protocol::PlayerCommand::Pause { player_id: local.0 });
            }
        }
        PauseScreen::Settings => *screen = PauseScreen::Menu,
        PauseScreen::Menu => {
            *screen = PauseScreen::None;
            if !multiplayer.0 {
                input.0.push(saladin_protocol::PlayerCommand::Resume { player_id: local.0 });
            }
        }
    }
}

/// Rebuild the overlay when the screen or the settings values change.
pub fn update_pause_overlay(
    mut commands: Commands,
    font: Res<UiFont>,
    screen: Res<PauseScreen>,
    user: Res<config::UserConfig>,
    multiplayer: Res<Multiplayer>,
    q_root: Query<Entity, With<PauseRoot>>,
    mut digest: Local<String>,
) {
    let key = format!(
        "{:?}|{}|{}|{:.2}|{:.2}",
        *screen, multiplayer.0, user.edge_scroll, user.ui_scale, user.master_volume
    );
    if *digest == key {
        return;
    }
    *digest = key;
    for e in &q_root {
        commands.entity(e).despawn();
    }
    if *screen == PauseScreen::None {
        return;
    }
    commands
        .spawn((
            PauseRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(50),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    min_width: Val::Px(280.0),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                BorderColor::all(PANEL_BORDER),
            ))
            .with_children(|p| match *screen {
                PauseScreen::Menu => {
                    label(p, &font, "PAUSED", FONT_LG, GOLD);
                    if multiplayer.0 {
                        label(p, &font, "(the battle rages on - lockstep never sleeps)", FONT_SM, TEXT_DIM);
                    }
                    pause_button(p, &font, PauseAction::Resume, "Resume", false);
                    pause_button(p, &font, PauseAction::SaveQuit, "Save & Quit", multiplayer.0);
                    pause_button(p, &font, PauseAction::OpenSettings, "Settings", false);
                    pause_button(p, &font, PauseAction::QuitToMenu, "Quit to Menu", false);
                }
                PauseScreen::Settings => {
                    label(p, &font, "SETTINGS", FONT_LG, GOLD);
                    settings_controls(p, &font, &user);
                    pause_button(p, &font, PauseAction::BackToPause, "Back", false);
                }
                PauseScreen::None => {}
            });
        });
}

/// The settings rows (volume / edge scroll / UI scale) — also embedded in the
/// main menu's settings screen, dispatching the same `PauseAction`s.
pub fn settings_controls(p: &mut ChildSpawnerCommands, font: &UiFont, user: &config::UserConfig) {
    label(p, font, &format!("Master volume: {:.0}%", user.master_volume * 100.0), FONT_SM, TEXT_DIM);
    p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
        .with_children(|p| {
            pause_button(p, font, PauseAction::Volume(-1), "-", user.master_volume <= 0.0);
            pause_button(p, font, PauseAction::Volume(1), "+", user.master_volume >= 1.0);
        });
    label(p, font, "(audio arrives with a later patch)", 10.0, TEXT_DIM);

    pause_button(
        p,
        font,
        PauseAction::ToggleEdgeScroll,
        if user.edge_scroll { "Edge scroll: ON" } else { "Edge scroll: OFF" },
        false,
    );

    label(p, font, &format!("UI scale: {:.2}", user.ui_scale), FONT_SM, TEXT_DIM);
    p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
        .with_children(|p| {
            pause_button(p, font, PauseAction::UiScale(-1), "-", user.ui_scale <= 0.74);
            pause_button(p, font, PauseAction::UiScale(1), "+", user.ui_scale >= 1.51);
        });
}

fn pause_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    action: PauseAction,
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

#[allow(clippy::too_many_arguments)]
pub fn pause_actions(
    q: Query<(&Interaction, &PauseAction, &Disabled), Changed<Interaction>>,
    mut screen: ResMut<PauseScreen>,
    mut user: ResMut<config::UserConfig>,
    mut ui_scale: ResMut<UiScale>,
    mut pending_save: ResMut<PendingSave>,
    multiplayer: Res<Multiplayer>,
    local: Res<crate::LocalPlayer>,
    mut input: ResMut<crate::LocalInput>,
    mut next: ResMut<NextState<GameState>>,
) {
    for (i, action, disabled) in &q {
        if *i != Interaction::Pressed || disabled.0 {
            continue;
        }
        match action {
            PauseAction::Resume => {
                *screen = PauseScreen::None;
                if !multiplayer.0 {
                    input.0.push(saladin_protocol::PlayerCommand::Resume { player_id: local.0 });
                }
            }
            PauseAction::SaveQuit => {
                // resume first so the save doesn't restore into a paused match
                if !multiplayer.0 {
                    input.0.push(saladin_protocol::PlayerCommand::Resume { player_id: local.0 });
                }
                pending_save.0 = true;
                *screen = PauseScreen::None;
            }
            PauseAction::OpenSettings => *screen = PauseScreen::Settings,
            PauseAction::BackToPause => *screen = PauseScreen::Menu,
            PauseAction::QuitToMenu => {
                *screen = PauseScreen::None;
                next.set(GameState::Menu);
            }
            PauseAction::ToggleEdgeScroll => {
                user.edge_scroll = !user.edge_scroll;
                config::save(&user);
            }
            PauseAction::UiScale(d) => {
                user.ui_scale = (user.ui_scale + 0.25 * *d as f32).clamp(0.75, 1.5);
                ui_scale.0 = user.ui_scale;
                config::save(&user);
            }
            PauseAction::Volume(d) => {
                user.master_volume = (user.master_volume + 0.1 * *d as f32).clamp(0.0, 1.0);
                config::save(&user);
            }
        }
    }
}

pub fn cleanup_pause(mut commands: Commands, q: Query<Entity, With<PauseRoot>>, mut screen: ResMut<PauseScreen>) {
    for e in &q {
        commands.entity(e).despawn();
    }
    *screen = PauseScreen::None;
}

// ── loading screen ───────────────────────────────────────────────────────────

#[derive(Component)]
pub struct LoadingRoot;

/// Frames spent in `GameState::Loading` — the world build runs on the second
/// frame's transition so this screen actually reaches the GPU first.
#[derive(Resource, Default)]
pub struct LoadingFrames(pub u32);

pub fn setup_loading(mut commands: Commands, font: Res<UiFont>, cfg: Res<crate::MenuConfig>) {
    commands.insert_resource(LoadingFrames(0));
    commands
        .spawn((
            LoadingRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.05, 0.08)),
        ))
        .with_children(|p| {
            label(p, &font, "FORGING THE HOLY LAND", FONT_LG, GOLD);
            label(p, &font, &format!("seed {}", cfg.seed), FONT_SM, TEXT_DIM);
            label(p, &font, "carving terrain, scattering groves, raising keeps...", FONT_SM, TEXT_DIM);
        });
}

pub fn tick_loading(mut frames: ResMut<LoadingFrames>, mut next: ResMut<NextState<GameState>>) {
    frames.0 += 1;
    // frame 1 lays out the UI, frame 2 presents it, then the heavy build runs
    if frames.0 >= 2 {
        next.set(GameState::Playing);
    }
}

pub fn cleanup_loading(mut commands: Commands, q: Query<Entity, With<LoadingRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ── multiplayer disconnect banner ────────────────────────────────────────────

/// Names of peers whose connections dropped mid-match.
#[derive(Resource, Default)]
pub struct Disconnects(pub Vec<String>);

#[derive(Component)]
pub struct DisconnectBanner;

pub fn render_disconnect_banner(
    mut commands: Commands,
    font: Res<UiFont>,
    list: Res<Disconnects>,
    q_banner: Query<Entity, With<DisconnectBanner>>,
    mut shown: Local<usize>,
) {
    if list.0.len() == *shown {
        return;
    }
    *shown = list.0.len();
    for e in &q_banner {
        commands.entity(e).despawn();
    }
    if list.0.is_empty() {
        return;
    }
    let text = format!("{} disconnected - their forces stand idle; the match continues", list.0.join(", "));
    commands
        .spawn((
            DisconnectBanner,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(28.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(40),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.35, 0.08, 0.05, 0.92)),
                BorderColor::all(WARN),
            ))
            .with_children(|p| label(p, &font, &text, FONT_MD, TEXT));
        });
}

pub fn cleanup_disconnects(
    mut commands: Commands,
    q: Query<Entity, With<DisconnectBanner>>,
    mut list: ResMut<Disconnects>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
    list.0.clear();
}
