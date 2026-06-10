//! Menu screens: Main → Singleplayer (skirmish setup) / Multiplayer (host LAN,
//! join by IP, host internet room, join room) — plus the multiplayer lobby
//! (names, factions, ready flags, AI seats, host map pick) and the game-over
//! overlay. Screens rebuild via the digest pattern; text-input values live in
//! `MpForm` so rebuilds never eat typed text.

use super::assets::UiAssets;
use super::text_input::{TextInput, text_input};
use super::theme::*;
use super::widgets::{Disabled, label, option_button, panel_bg, backdrop_bg, screen_button, wide_button};
use crate::{GameState, MenuConfig, UiFont, config};
use bevy::prelude::*;
use saladin_protocol::JoinIntent;
use saladin_sim::{AiDifficulty, Faction};

#[derive(Component)]
pub struct MenuRoot;

/// Which menu page is showing while `GameState::Menu`.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuScreen {
    #[default]
    Main,
    Singleplayer,
    Multiplayer,
    Settings,
}

/// Menu-only actions (kept separate from the HUD's `UiAction`).
#[derive(Component, Clone, Copy, PartialEq)]
pub enum MenuAction {
    Goto(MenuScreen),
    Quit,
    LoadGame,
    // singleplayer setup
    Faction(Faction),
    AddOpponent,
    RemoveOpponent,
    Difficulty(AiDifficulty),
    CycleSeed,
    Preset(u8),
    Start,
    // multiplayer entries
    HostLan,
    JoinIp,
    HostInternet,
    JoinRoom,
}

/// Text-field backing store: survives digest rebuilds of the screen tree.
#[derive(Resource, Default)]
pub struct MpForm {
    pub name: String,
    pub ip: String,
    pub room: String,
}

/// Last multiplayer connect error, shown on the multiplayer screen.
#[derive(Resource, Default)]
pub struct MpError(pub Option<String>);

/// How we got into the lobby — drives what the lobby screen shows.
#[derive(Resource, Clone, Default, PartialEq, Debug)]
pub enum LobbyMode {
    #[default]
    Joined,
    LanHost { ips: Vec<String> },
    InternetHost,
}

#[derive(Component)]
pub struct NameInput;
#[derive(Component)]
pub struct IpInput;
#[derive(Component)]
pub struct RoomInput;

/// Digest for rebuild-on-change.
#[derive(Resource, Default, PartialEq)]
pub struct MenuDigest(String);

pub fn setup_menu(mut commands: Commands, assets: Res<UiAssets>) {
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
        backdrop_bg(&assets),
    ));
}

pub fn cleanup_menu(
    mut commands: Commands,
    q: Query<Entity, With<MenuRoot>>,
    mut digest: ResMut<MenuDigest>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
    digest.0.clear();
}

/// Mirror typed text back into `MpForm` so screen rebuilds restore it.
pub fn sync_mp_form(
    mut form: ResMut<MpForm>,
    q_name: Query<&TextInput, (With<NameInput>, Changed<TextInput>)>,
    q_ip: Query<&TextInput, (With<IpInput>, Changed<TextInput>)>,
    q_room: Query<&TextInput, (With<RoomInput>, Changed<TextInput>)>,
) {
    // compare before writing: fresh-spawned inputs count as Changed, and a
    // no-op write would dirty the resource and trigger a rebuild loop
    if let Ok(t) = q_name.single()
        && form.name != t.value
    {
        form.name = t.value.clone();
    }
    if let Ok(t) = q_ip.single()
        && form.ip != t.value
    {
        form.ip = t.value.clone();
    }
    if let Ok(t) = q_room.single()
        && form.room != t.value
    {
        form.room = t.value.clone();
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_menu(
    mut commands: Commands,
    font: Res<UiFont>,
    cfg: Res<MenuConfig>,
    screen: Res<MenuScreen>,
    form: Res<MpForm>,
    err: Res<MpError>,
    user: Res<config::UserConfig>,
    mut digest: ResMut<MenuDigest>,
    q_root: Query<Entity, With<MenuRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut previews: ResMut<super::preview::PreviewCache>,
    assets: Res<UiAssets>,
) {
    let key = format!(
        "{:?}|{:?}|{:?}|{}|{}|{}|{:?}|{}|{:.2}|{:.2}",
        *screen,
        cfg.faction,
        cfg.opponents,
        cfg.difficulty as u8,
        cfg.seed,
        cfg.preset,
        err.0,
        user.edge_scroll,
        user.ui_scale,
        user.master_volume
    );
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
                padding: UiRect::all(Val::Px(22.0)),
                min_width: Val::Px(360.0),
                ..default()
            },
            panel_bg(&assets),
        ))
        .with_children(|p| match *screen {
            MenuScreen::Main => main_screen(p, &font, &assets),
            MenuScreen::Singleplayer => {
                let seed = saladin_sim::compose_seed(cfg.seed.max(1), cfg.preset);
                let preview = super::preview::preview_handle(&mut previews, &mut images, seed);
                sp_screen(p, &font, &assets, &cfg, preview);
            }
            MenuScreen::Multiplayer => mp_screen(p, &font, &assets, &form, &err),
            MenuScreen::Settings => {
                label(p, &font, "SETTINGS", FONT_LG, GOLD);
                super::pause::settings_controls(p, &font, &assets, &user);
                menu_button(p, &font, &assets, MenuAction::Goto(MenuScreen::Main), "Back", false, false);
            }
        });
    });
}

fn main_screen(p: &mut ChildSpawnerCommands, font: &UiFont, assets: &UiAssets) {
    p.spawn((
        Node {
            width: Val::Px(72.0),
            height: Val::Px(72.0),
            margin: UiRect::top(Val::Px(12.0)).with_bottom(Val::Px(12.0)),
            ..default()
        },
        ImageNode::new(assets.emblem.clone()),
    ));
    label(p, font, "SALADIN", 30.0, GOLD);
    label(p, font, "A real-time strategy of the Crusades", FONT_SM, TEXT_DIM);
    p.spawn(Node { height: Val::Px(10.0), ..default() });
    wide_button(p, font, assets, MenuAction::Goto(MenuScreen::Singleplayer), "Singleplayer", false, false);
    wide_button(p, font, assets, MenuAction::Goto(MenuScreen::Multiplayer), "Multiplayer", false, false);
    wide_button(p, font, assets, MenuAction::LoadGame, "Load Game", false, !crate::save_exists());
    wide_button(p, font, assets, MenuAction::Goto(MenuScreen::Settings), "Settings", false, false);
    wide_button(p, font, assets, MenuAction::Quit, "Quit", false, false);
}

fn sp_screen(p: &mut ChildSpawnerCommands, font: &UiFont, assets: &UiAssets, cfg: &MenuConfig, preview: Handle<Image>) {
    label(p, font, "SKIRMISH", FONT_LG, GOLD);
    p.spawn(Node { height: Val::Px(8.0), ..default() });

    form_row(p, font, "Faction", |p| {
        for (f, name) in [(Faction::Ayyubid, "Ayyubids"), (Faction::Crusader, "Crusaders")] {
            option_button(p, font, assets, MenuAction::Faction(f), name, cfg.faction == f, 110.0);
        }
    });

    let opponents = format!("{}", cfg.opponents);
    form_row(p, font, "Opponents", |p| {
        option_button(p, font, assets, MenuAction::RemoveOpponent, "-", false, 36.0);
        p.spawn((
            Node {
                width: Val::Px(34.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|p| label(p, font, &opponents, FONT_MD, TEXT));
        option_button(p, font, assets, MenuAction::AddOpponent, "+", false, 36.0);
    });

    form_row(p, font, "Difficulty", |p| {
        for (d, name) in [
            (AiDifficulty::Easy, "Easy"),
            (AiDifficulty::Normal, "Normal"),
            (AiDifficulty::Hard, "Hard"),
        ] {
            option_button(p, font, assets, MenuAction::Difficulty(d), name, cfg.difficulty == d, 90.0);
        }
    });

    form_row(p, font, "Map", |p| {
        for (i, preset) in saladin_sim::MAP_PRESETS.iter().enumerate() {
            option_button(p, font, assets, MenuAction::Preset(i as u8), preset.label, cfg.preset == i as u8, 104.0);
        }
    });

    label(
        p,
        font,
        saladin_sim::map_preset_by_index(cfg.preset as i32).description,
        10.0,
        TEXT_DIM,
    );
    p.spawn(Node { height: Val::Px(4.0), ..default() });
    super::preview::preview_node(p, preview);
    p.spawn(Node { height: Val::Px(4.0), ..default() });
    wide_button(p, font, assets, MenuAction::CycleSeed, &format!("New seed (now: {})", cfg.seed), false, false);
    p.spawn(Node { height: Val::Px(8.0), ..default() });
    wide_button(p, font, assets, MenuAction::Start, "Begin the Campaign", false, false);
    wide_button(p, font, assets, MenuAction::Goto(MenuScreen::Main), "Back", false, false);
}

/// One aligned settings row: fixed-width dim label, controls to its right.
fn form_row(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    title: &str,
    controls: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((Node {
        width: Val::Px(560.0),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        margin: UiRect::vertical(Val::Px(3.0)),
        ..default()
    },))
        .with_children(|p| {
            p.spawn((Node {
                width: Val::Px(96.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },))
                .with_children(|p| label(p, font, title, FONT_SM, TEXT_DIM));
            controls(p);
        });
}

fn mp_screen(p: &mut ChildSpawnerCommands, font: &UiFont, assets: &UiAssets, form: &MpForm, err: &MpError) {
    label(p, font, "MULTIPLAYER", FONT_LG, GOLD);
    if let Some(e) = &err.0 {
        label(p, font, e, FONT_SM, WARN);
    }

    label(p, font, "Your name", FONT_SM, TEXT_DIM);
    let name = text_input(p, font, TextInput::new(&form.name, "Player", 24), 220.0);
    p.commands_mut().entity(name).insert(NameInput);

    label(p, font, "Local network", FONT_SM, TEXT_DIM);
    menu_button(p, font, assets, MenuAction::HostLan, "Host LAN Game", false, false);
    let ip = text_input(
        p,
        font,
        TextInput::new(&form.ip, "host ip (e.g. 192.168.1.10)", 45)
            .with_filter(|c| c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-'),
        220.0,
    );
    p.commands_mut().entity(ip).insert(IpInput);
    menu_button(p, font, assets, MenuAction::JoinIp, "Join by IP", false, form.ip.is_empty());

    label(p, font, "Internet (via relay)", FONT_SM, TEXT_DIM);
    menu_button(p, font, assets, MenuAction::HostInternet, "Host Internet Game", false, false);
    let room = text_input(
        p,
        font,
        TextInput::new(&form.room, "room code", 8).with_filter(|c| c.is_ascii_alphanumeric()),
        220.0,
    );
    p.commands_mut().entity(room).insert(RoomInput);
    menu_button(p, font, assets, MenuAction::JoinRoom, "Join Room", false, form.room.is_empty());

    menu_button(p, font, assets, MenuAction::Goto(MenuScreen::Main), "Back", false, false);
}

fn menu_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: MenuAction,
    title: &str,
    active: bool,
    disabled: bool,
) {
    screen_button(p, font, assets, action, title, active, disabled);
}

/// The disabled-state of Join buttons depends on live typed text, which the
/// digest doesn't track — refresh the digest when the emptiness flips.
pub fn refresh_join_buttons(form: Res<MpForm>, mut digest: ResMut<MenuDigest>, screen: Res<MenuScreen>) {
    if *screen != MenuScreen::Multiplayer || !form.is_changed() {
        return;
    }
    digest.0.clear();
}

fn connect(
    addr: &str,
    name: &str,
    intent: JoinIntent,
) -> Result<saladin_protocol::TcpTransport, String> {
    saladin_protocol::TcpTransport::connect(addr, name, intent).map_err(|e| match e.kind() {
        std::io::ErrorKind::ConnectionRefused => format!("connection refused by {addr}"),
        std::io::ErrorKind::TimedOut => format!("connection to {addr} timed out"),
        _ => e.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn menu_actions(
    q: Query<(&Interaction, &MenuAction, &Disabled), Changed<Interaction>>,
    mut cfg: ResMut<MenuConfig>,
    mut user: ResMut<config::UserConfig>,
    mut screen: ResMut<MenuScreen>,
    mut form: ResMut<MpForm>,
    mut err: ResMut<MpError>,
    conn: Res<crate::LobbyConn>,
    mut mode: ResMut<LobbyMode>,
    mut next: ResMut<NextState<GameState>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    for (i, action, disabled) in &q {
        if *i != Interaction::Pressed || disabled.0 {
            continue;
        }
        match action {
            MenuAction::Goto(s) => {
                if *s == MenuScreen::Multiplayer && form.name.is_empty() {
                    form.name = user.player_name.clone();
                }
                err.0 = None;
                *screen = *s;
            }
            MenuAction::Quit => {
                app_exit.write(AppExit::Success);
            }
            MenuAction::LoadGame => {
                cfg.load = true;
                next.set(GameState::Loading);
            }
            MenuAction::Faction(f) => cfg.faction = *f,
            MenuAction::AddOpponent => cfg.opponents = (cfg.opponents + 1).min(7),
            MenuAction::RemoveOpponent => cfg.opponents = cfg.opponents.saturating_sub(1).max(1),
            MenuAction::Difficulty(d) => cfg.difficulty = *d,
            MenuAction::CycleSeed => {
                cfg.seed = cfg.seed.wrapping_mul(1664525).wrapping_add(1013904223) % 100_000
            }
            MenuAction::Preset(i) => cfg.preset = *i,
            MenuAction::Start => next.set(GameState::Loading),
            MenuAction::HostLan | MenuAction::JoinIp | MenuAction::HostInternet | MenuAction::JoinRoom => {
                remember_name(&mut user, &form);
                let name = display_name(&user);
                let (addr, intent, new_mode) = match action {
                    MenuAction::HostLan => {
                        let bind = format!("0.0.0.0:{}", crate::HOST_PORT);
                        if let Err(e) = saladin_protocol::spawn_host_relay(&bind) {
                            err.0 = Some(format!("could not host: {e}"));
                            continue;
                        }
                        (
                            format!("127.0.0.1:{}", crate::HOST_PORT),
                            JoinIntent::Direct,
                            LobbyMode::LanHost { ips: config::lan_ips() },
                        )
                    }
                    MenuAction::JoinIp => {
                        let ip = form.ip.trim();
                        let addr = if ip.contains(':') { ip.to_string() } else { format!("{ip}:{}", crate::HOST_PORT) };
                        (addr, JoinIntent::Direct, LobbyMode::Joined)
                    }
                    MenuAction::HostInternet => {
                        (user.relay_addr.clone(), JoinIntent::CreateRoom, LobbyMode::InternetHost)
                    }
                    MenuAction::JoinRoom => (
                        user.relay_addr.clone(),
                        JoinIntent::JoinRoom { code: form.room.clone() },
                        LobbyMode::Joined,
                    ),
                    _ => unreachable!(),
                };
                match connect(&addr, &name, intent) {
                    Ok(t) => {
                        *conn.0.lock().unwrap() = Some(t);
                        *mode = new_mode;
                        err.0 = None;
                        next.set(GameState::Lobby);
                    }
                    Err(e) => err.0 = Some(e),
                }
            }
        }
    }
}

fn remember_name(user: &mut config::UserConfig, form: &MpForm) {
    let name = form.name.trim();
    if user.player_name != name {
        user.player_name = name.to_string();
        config::save(user);
    }
}

fn display_name(user: &config::UserConfig) -> String {
    if user.player_name.is_empty() { "Player".into() } else { user.player_name.clone() }
}

/// Game-over overlay with a back-to-menu button.
#[derive(Component)]
pub struct GameOverRoot;

#[derive(Component)]
pub struct GameOverAction;

pub fn setup_gameover(
    mut commands: Commands,
    font: Res<UiFont>,
    assets: Res<UiAssets>,
    local: Res<crate::LocalPlayer>,
    q_players: Query<&saladin_protocol::Player>,
    stats: Res<saladin_protocol::MatchStats>,
    tick: Res<saladin_protocol::Tick>,
) {
    let won = q_players.iter().find(|p| p.player_id == local.0).map(|p| !p.defeated).unwrap_or(false);
    let (title, color) = if won { ("VICTORY", ACCENT) } else { ("DEFEAT", WARN) };
    let s = stats.0.get(&local.0).copied().unwrap_or_default();
    let secs = tick.0 / 20; // 20 Hz base tick
    let duration = format!("{}:{:02}", secs / 60, secs % 60);
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
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    ..default()
                },
                panel_bg(&assets),
            ))
            .with_children(|p| {
                label(p, &font, &format!("Match duration   {duration}"), FONT_MD, TEXT);
                label(p, &font, &format!("Units trained    {}", s.trained), FONT_MD, TEXT);
                label(p, &font, &format!("Units lost       {}", s.lost), FONT_MD, TEXT);
                label(p, &font, &format!("Resources banked {}", s.gathered), FONT_MD, TEXT);
            });
            screen_button(p, &font, &assets, GameOverAction, "Back to Menu", false, false);
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
    ToggleReady,
    Faction(Faction),
    AddAi,
    RemoveAi(u64),
    CycleSeed,
    CyclePreset,
}

#[derive(Resource, Default)]
pub struct LobbyDigest(String);

pub fn setup_lobby(mut commands: Commands, assets: Res<UiAssets>) {
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
        backdrop_bg(&assets),
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
    mode: Res<LobbyMode>,
    mut digest: ResMut<LobbyDigest>,
    q_root: Query<Entity, With<LobbyRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut previews: ResMut<super::preview::PreviewCache>,
    assets: Res<UiAssets>,
) {
    let guard = conn.0.lock().unwrap();
    let Some(t) = guard.as_ref() else { return };
    let l = t.lobby();
    drop(guard);
    let key = format!(
        "{}|{}|{}|{:?}|{:?}|{:?}|{}|{}",
        l.connected, l.you, l.host, l.players, l.error, l.room_code, l.seed, l.preset
    );
    if digest.0 == key {
        return;
    }
    digest.0 = key;
    let Ok(root) = q_root.single() else { return };
    let is_host = l.is_host();
    let me_ready = l.me().map(|m| m.ready).unwrap_or(false);
    let my_faction = l.me().map(|m| m.faction);
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(22.0)),
                min_width: Val::Px(400.0),
                ..default()
            },
            panel_bg(&assets),
        ))
        .with_children(|p| {
            label(p, &font, "MULTIPLAYER LOBBY", FONT_LG, GOLD);
            if let Some(err) = &l.error {
                label(p, &font, err, FONT_SM, WARN);
            } else if !l.connected {
                label(p, &font, "Connecting...", FONT_SM, TEXT_DIM);
            } else {
                // how friends get in
                match &*mode {
                    LobbyMode::LanHost { ips } => {
                        if ips.is_empty() {
                            label(p, &font, "Friends join with your LAN IP", FONT_SM, TEXT_DIM);
                        } else {
                            label(p, &font, &format!("Friends join: {}", ips.join("  or  ")), FONT_SM, ACCENT);
                        }
                    }
                    LobbyMode::InternetHost => {
                        if let Some(code) = &l.room_code {
                            label(p, &font, &format!("ROOM CODE: {code}"), FONT_LG, ACCENT);
                            label(p, &font, "Friends pick Join Room and enter this code", FONT_SM, TEXT_DIM);
                        }
                    }
                    LobbyMode::Joined => {}
                }

                let preset = saladin_sim::map_preset_by_index(l.preset as i32);
                label(p, &font, &format!("Map: {} - seed {}", preset.label, l.seed), FONT_SM, TEXT_DIM);
                let composed = saladin_sim::compose_seed(l.seed.max(1), l.preset);
                super::preview::preview_node(
                    p,
                    super::preview::preview_handle(&mut previews, &mut images, composed),
                );
                if is_host {
                    p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
                        .with_children(|p| {
                            lobby_button(p, &font, &assets, LobbyAction::CycleSeed, "New seed", false);
                            lobby_button(p, &font, &assets, LobbyAction::CyclePreset, "Next map type", false);
                        });
                }

                // roster
                for pl in &l.players {
                    let mut row = String::new();
                    if pl.id == l.host && !pl.is_ai {
                        row.push_str("[host] ");
                    }
                    row.push_str(&pl.name);
                    row.push_str(match pl.faction {
                        Faction::Ayyubid => " - Ayyubids",
                        Faction::Crusader => " - Crusaders",
                    });
                    if pl.is_ai {
                        row.push_str(&format!("  (AI {:?})", pl.ai_difficulty));
                    } else if pl.ready || pl.id == l.host {
                        row.push_str("  [ready]");
                    } else {
                        row.push_str("  [not ready]");
                    }
                    let color = if pl.id == l.you { TEXT } else { TEXT_DIM };
                    p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), ..default() },))
                        .with_children(|p| {
                            label(p, &font, &row, FONT_SM, color);
                            if is_host && pl.is_ai {
                                lobby_button(p, &font, &assets, LobbyAction::RemoveAi(pl.id), "x", false);
                            }
                        });
                }

                // your seat
                label(p, &font, "Your faction", FONT_SM, TEXT_DIM);
                p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() },))
                    .with_children(|p| {
                        for (f, name) in [(Faction::Ayyubid, "Ayyubids"), (Faction::Crusader, "Crusaders")] {
                            lobby_faction_button(p, &font, &assets, f, name, my_faction == Some(f));
                        }
                    });

                if is_host {
                    lobby_button(p, &font, &assets, LobbyAction::AddAi, "Add AI opponent", l.players.len() >= 8);
                    let can_start = l.players.len() >= 2 && l.all_ready();
                    lobby_button(p, &font, &assets, LobbyAction::Start, "Start Match", !can_start);
                    if !l.all_ready() {
                        label(p, &font, "Waiting for everyone to ready up...", FONT_SM, TEXT_DIM);
                    }
                } else {
                    lobby_button(
                        p,
                        &font,
                        &assets,
                        LobbyAction::ToggleReady,
                        if me_ready { "Ready! (click to unready)" } else { "Ready up" },
                        false,
                    );
                    label(p, &font, "Waiting for the host to start...", FONT_SM, TEXT_DIM);
                }
            }
            lobby_button(p, &font, &assets, LobbyAction::Cancel, "Leave Lobby", false);
        });
    });
}

fn lobby_faction_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    f: Faction,
    name: &str,
    active: bool,
) {
    screen_button(p, font, assets, LobbyAction::Faction(f), name, active, false);
}

fn lobby_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: LobbyAction,
    title: &str,
    disabled: bool,
) {
    screen_button(p, font, assets, action, title, false, disabled);
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
        let mut guard = conn.0.lock().unwrap();
        let Some(t) = guard.as_mut() else { continue };
        match *action {
            LobbyAction::Start => t.request_start(),
            LobbyAction::ToggleReady => {
                let ready = t.lobby().me().map(|m| m.ready).unwrap_or(false);
                t.set_ready(!ready);
            }
            LobbyAction::Faction(f) => t.set_faction(f),
            LobbyAction::AddAi => {
                let l = t.lobby();
                // alternate AI factions against the host's pick for variety
                let f = match l.me().map(|m| m.faction) {
                    Some(Faction::Ayyubid) => Faction::Crusader,
                    _ => Faction::Ayyubid,
                };
                t.add_ai(AiDifficulty::Normal, f);
            }
            LobbyAction::RemoveAi(id) => t.remove_ai(id),
            LobbyAction::CycleSeed => {
                let l = t.lobby();
                let seed = l.seed.wrapping_mul(1664525).wrapping_add(1013904223) % 100_000;
                t.set_map(seed.max(1), l.preset);
            }
            LobbyAction::CyclePreset => {
                let l = t.lobby();
                let next = (l.preset + 1) % saladin_sim::MAP_PRESETS.len() as u8;
                t.set_map(l.seed, next);
            }
            LobbyAction::Cancel => {
                *guard = None;
                next.set(GameState::Menu);
            }
        }
    }
}
