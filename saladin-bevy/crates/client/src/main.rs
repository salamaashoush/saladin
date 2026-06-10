//! Saladin client — renders the deterministic sim and feeds player input back
//! as lockstep commands. main.rs stays thin: app/plugin wiring + match
//! lifecycle; everything else lives in focused modules (camera, input,
//! selection, render, environment, vegetation, fx, minimap, perf, ui).

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

mod camera;
mod config;
mod environment;
mod fx;
mod input;
mod minimap;
mod perf;
mod render;
mod selection;
mod terrain;
mod ui;
mod vegetation;

use saladin_protocol::{
    GameId, LockstepDriver, MatchInfo, MemTransport, Player, PlayerCommand, SimPlugin, Transport,
    WorldConfig, scatter_world_nodes, shared_relay,
};
use saladin_sim::{AiDifficulty, Faction, WORLD_SIZE, enemy_faction};

/// The local player's id (1 in single-player; assigned by the relay in MP).
#[derive(Resource, Clone, Copy)]
pub struct LocalPlayer(pub u64);

/// Local intents this frame — routed through the lockstep driver (submitted to
/// the relay), NOT applied directly, so every client stays in sync.
#[derive(Resource, Default)]
pub struct LocalInput(pub Vec<PlayerCommand>);

/// The lockstep driver + its transport (in-memory for single-player, websocket
/// MP). The transport sits behind a Mutex because the ws receiver is Send but
/// not Sync; only the exclusive sim driver ever locks it.
#[derive(Resource)]
pub struct Net {
    driver: LockstepDriver,
    transport: std::sync::Mutex<Box<dyn Transport + Send>>,
}

/// Bundled UI font — Bevy's embedded default font renders blank on wasm, so
/// all text uses this (embedded: works native+wasm, no asset-path juggling).
#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameState {
    #[default]
    Menu,
    Lobby,
    /// One rendered frame of "building the world" before the heavy
    /// `OnEnter(Playing)` setup blocks — a note instead of a freeze.
    Loading,
    Playing,
    GameOver,
}

/// The connection while we sit in the multiplayer lobby. Moves into `Net` (as
/// the lockstep transport) when the host starts the match.
#[derive(Resource, Default)]
pub struct LobbyConn(pub std::sync::Mutex<Option<saladin_protocol::TcpTransport>>);

/// Everything `setup_world` needs for a multiplayer match, captured from the
/// relay's `Welcome` by `lobby_poll`: the host's map pick plus this client's
/// seat and (host only) the AI seats it must originate commands for.
#[derive(Resource, Clone)]
pub struct PendingMatch {
    pub seed: u32,
    pub preset: u8,
    pub name: String,
    pub faction: Faction,
    pub is_host: bool,
    pub ais: Vec<(u64, AiDifficulty, Faction)>,
}

/// Default port for hosted games.
pub const HOST_PORT: u16 = 5000;

#[derive(Resource, Clone)]
pub struct MenuConfig {
    pub opponents: usize,
    pub faction: Faction,
    pub difficulty: AiDifficulty,
    pub seed: u32,
    /// Index into `saladin_sim::MAP_PRESETS` (travels in the seed's top bits).
    pub preset: u8,
    /// Start the next match by restoring the save file instead of a fresh world.
    pub load: bool,
}

impl Default for MenuConfig {
    fn default() -> Self {
        MenuConfig {
            opponents: 1,
            faction: Faction::Ayyubid,
            difficulty: AiDifficulty::Normal,
            seed: 1,
            preset: 0,
            load: false,
        }
    }
}

/// Set by the Save & Quit button; the exclusive save system performs the
/// snapshot between frames.
#[derive(Resource, Default)]
pub struct PendingSave(pub bool);

/// The single save slot on disk (native only).
#[cfg(not(target_arch = "wasm32"))]
pub fn save_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
            home.join(".local/share")
        });
    base.join("saladin/save1.bin")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_exists() -> bool {
    save_path().exists()
}

#[cfg(target_arch = "wasm32")]
pub fn save_exists() -> bool {
    false
}

#[derive(Resource, Clone, Copy)]
pub struct Multiplayer(pub bool);

/// Everything spawned for one match (terrain, vegetation) — torn down on exit.
#[derive(Component)]
pub struct MatchScoped;

fn build_net(_connect: Option<String>) -> (Net, u64, bool) {
    let relay = shared_relay(vec![1]);
    (
        Net {
            driver: LockstepDriver::new(1, 1),
            transport: std::sync::Mutex::new(Box::new(MemTransport::new(relay))),
        },
        1,
        false,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let connect = args.iter().position(|a| a == "connect").and_then(|i| args.get(i + 1)).cloned();
    let (net, local, multiplayer) = build_net(None);
    let user_config = config::load();
    let ui_scale = user_config.ui_scale.clamp(0.75, 1.5);

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Saladin".into(),
                    canvas: Some("#bevy".into()),
                    fit_canvas_to_parent: true,
                    ..default()
                }),
                ..default()
            })
            ,
    )
    .add_plugins(FrameTimeDiagnosticsPlugin::default())
    .add_plugins(SimPlugin)
    .init_state::<GameState>()
    .insert_resource(net)
    .insert_resource(user_config)
    .insert_resource(UiScale(ui_scale))
    .insert_resource(LocalPlayer(local))
    .insert_resource(Multiplayer(multiplayer))
    .insert_resource(Time::<Fixed>::from_hz(20.0))
    .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.09)))
    .init_resource::<LocalInput>()
    .init_resource::<MenuConfig>()
    .init_resource::<PendingSave>()
    .init_resource::<selection::Selection>()
    .init_resource::<selection::SelectionInfo>()
    .init_resource::<selection::SelectedBuilding>()
    .init_resource::<selection::ControlGroups>()
    .init_resource::<input::InputMode>()
    .init_resource::<input::DragState>()
    .init_resource::<input::WallDrag>()
    .init_resource::<input::DemolishDrag>()
    .init_resource::<input::LastClick>()
    .init_resource::<input::GhostRot>()
    .init_resource::<camera::DragPan>()
    .init_resource::<render::sync::RenderMap>()
    .init_resource::<render::sync::OccupiedTiles>()
    .init_resource::<ui::actions::BuildTab>()
    .init_resource::<ui::hud::HudDigest>()
    .init_resource::<ui::hud::Toasts>()
    .init_resource::<ui::menu::MenuDigest>()
    .init_resource::<ui::menu::MenuScreen>()
    .init_resource::<ui::menu::MpForm>()
    .init_resource::<ui::menu::MpError>()
    .init_resource::<ui::menu::LobbyMode>()
    .init_resource::<ui::text_input::CursorBlink>()
    .init_resource::<ui::preview::PreviewCache>()
    .init_resource::<perf::PerfVisible>()
    .add_systems(Startup, (perf::setup_perf, input::spawn_drag_box, ui::widgets::prewarm_font_atlas))
    .add_systems(
        Update,
        (
            ui::text_input::focus_text_inputs,
            ui::text_input::type_into_inputs,
            ui::text_input::render_text_inputs,
            ui::widgets::button_feedback,
        )
            .chain(),
    )
    .init_resource::<LobbyConn>()
    // lobby (multiplayer pre-match)
    .add_systems(OnEnter(GameState::Lobby), ui::menu::setup_lobby)
    .add_systems(OnExit(GameState::Lobby), ui::menu::cleanup_lobby)
    .add_systems(
        Update,
        (lobby_poll, ui::menu::update_lobby, ui::menu::lobby_actions).run_if(in_state(GameState::Lobby)),
    )
    // loading interstitial
    .add_systems(OnEnter(GameState::Loading), ui::pause::setup_loading)
    .add_systems(OnExit(GameState::Loading), ui::pause::cleanup_loading)
    .add_systems(Update, ui::pause::tick_loading.run_if(in_state(GameState::Loading)))
    // menu
    .add_systems(OnEnter(GameState::Menu), ui::menu::setup_menu)
    .add_systems(OnExit(GameState::Menu), ui::menu::cleanup_menu)
    .add_systems(
        Update,
        (
            ui::menu::sync_mp_form,
            ui::menu::refresh_join_buttons,
            ui::menu::update_menu,
            ui::menu::menu_actions,
        )
            .chain()
            .run_if(in_state(GameState::Menu)),
    )
    // match lifecycle
    .add_systems(
        OnEnter(GameState::Playing),
        (setup_world, setup_visuals, minimap::spawn_minimap, ui::hud::setup_hud).chain(),
    )
    .add_systems(
        OnExit(GameState::Playing),
        (
            ui::hud::cleanup_hud,
            ui::pause::cleanup_pause,
            ui::pause::cleanup_disconnects,
            minimap::despawn_minimap,
            teardown_match,
        ),
    )
    .add_systems(FixedUpdate, drive_sim.run_if(in_state(GameState::Playing)))
    .add_systems(Update, do_save.run_if(in_state(GameState::Playing)))
    // gameplay frame systems
    .add_systems(
        Update,
        (
            camera::pan_camera,
            camera::zoom_camera,
            camera::rotate_camera.run_if(not(ui::text_input::any_input_focused)),
            camera::drag_pan,
            camera::smooth_camera,
            camera::frame_keep,
            minimap::update_minimap_viewport,
            minimap::minimap_click,
            input::pointer_input,
            input::rotate_ghost,
            input::keyboard_input.run_if(not(ui::text_input::any_input_focused)),
            input::update_drag_box,
            selection::publish_selection,
        )
            .run_if(in_state(GameState::Playing)),
    )
    .add_systems(
        Update,
        (
            render::sync::rebuild_occupancy,
            render::sync::sync_render,
            render::sync::interpolate,
            render::sync::animate_units,
            render::sync::animate_animals,
            render::sync::animate_fish,
            render::sync::animate_dying,
            render::sync::building_damage_fx,
            render::sync::tick_particles,
            render::sync::update_hp_bars,
            render::sync::update_building_highlight,
            render::ghost::update_ghost,
            render::ghost::update_demolish_overlay,
            fx::spawn_arrows,
            fx::fly_arrows,
            fx::melee_strike_dust,
        )
            .run_if(in_state(GameState::Playing)),
    )
    .add_systems(
        Update,
        (
            environment::follow_camera,
            environment::shimmer_ocean,
            environment::animate_sparkle,
            environment::animate_gulls,
            environment::spawn_shore_ripples,
            ui::hud::update_resource_bar,
            ui::hud::update_bottom_bar,
            ui::hud::watch_toasts,
            ui::hud::build_mode_hint,
            ui::hud::tick_toasts,
            ui::hud::render_toasts,
            ui::actions::handle_actions,
            perf::update_perf,
            check_gameover,
        )
            .run_if(in_state(GameState::Playing)),
    )
    .init_resource::<ui::pause::PauseScreen>()
    .init_resource::<ui::pause::Disconnects>()
    .add_systems(
        Update,
        (
            ui::pause::pause_hotkey,
            ui::pause::update_pause_overlay,
            ui::pause::render_disconnect_banner,
        )
            .run_if(in_state(GameState::Playing)),
    )
    // settings buttons dispatch PauseActions from both the pause overlay and
    // the main menu's settings screen
    .add_systems(
        Update,
        ui::pause::pause_actions
            .run_if(in_state(GameState::Playing).or_else(in_state(GameState::Menu))),
    )
    // game over
    .add_systems(OnEnter(GameState::GameOver), ui::menu::setup_gameover)
    .add_systems(OnExit(GameState::GameOver), (ui::menu::cleanup_gameover, teardown_match))
    .add_systems(
        Update,
        ui::menu::gameover_actions.run_if(in_state(GameState::GameOver)),
    );

    // Insert the bundled UI font BEFORE run() — the initial state's OnEnter
    // (menu) runs before any Startup system, so a load-at-Startup is too late.
    {
        let data = include_bytes!("../assets/fonts/ui.ttf").to_vec();
        let handle =
            app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(data, "ui"));
        app.insert_resource(UiFont(handle));
    }
    // Same timing constraint for the procedurally baked UI art.
    {
        let assets = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            ui::assets::build(&mut images)
        };
        app.insert_resource(assets);
    }

    // The camera must exist before the initial OnEnter(Menu) lays out UI.
    camera::spawn_camera(app.world_mut());

    // `connect <addr>`: join a hosted game and sit in the lobby until the host
    // starts the match.
    if let Some(addr) = connect {
        let addr = if addr.contains(':') { addr } else { format!("{addr}:{HOST_PORT}") };
        let name = {
            let user = app.world().resource::<config::UserConfig>();
            if user.player_name.is_empty() { "Player".to_string() } else { user.player_name.clone() }
        };
        match saladin_protocol::TcpTransport::connect(&addr, &name, saladin_protocol::JoinIntent::Direct) {
            Ok(t) => {
                app.insert_resource(LobbyConn(std::sync::Mutex::new(Some(t))));
                app.insert_state(GameState::Lobby);
            }
            Err(e) => eprintln!("connect failed: {e}; starting single-player"),
        }
    }
    let _ = multiplayer;
    // SALADIN_AUTO: headless render verification for CI / agent runs — save a
    // framebuffer screenshot to /tmp/saladin_shot.png at ~6s. Values:
    //   1     skip the menu, shoot in-game
    //   menu  shoot the main menu
    //   mp    shoot the multiplayer screen
    //   lobby host a LAN lobby and shoot it
    // SALADIN_TAB preselects a build-bar tab (screenshot verification of tabs)
    if let Ok(s) = std::env::var("SALADIN_TAB")
        && let Ok(tab) = s.parse::<usize>()
    {
        app.world_mut().resource_mut::<ui::actions::BuildTab>().0 = tab;
    }
    // SALADIN_BUILD=<building kind u8> enters build mode (hint-chip screenshots)
    if let Ok(s) = std::env::var("SALADIN_BUILD")
        && let Ok(k) = s.parse::<u8>()
        && let Some(kind) = saladin_sim::BuildingKind::from_u8(k)
    {
        *app.world_mut().resource_mut::<input::InputMode>() = input::InputMode::Build(kind);
    }
    // SALADIN_ZOOM=<view_size> presets the camera zoom (edge-of-world shots)
    if let Ok(s) = std::env::var("SALADIN_ZOOM")
        && let Ok(v) = s.parse::<f32>()
    {
        let v = v.clamp(4.0, 85.0);
        let world = app.world_mut();
        {
            let mut st = world.resource_mut::<camera::CameraState>();
            st.view_size = v;
            st.target_view = v;
        }
        let mut q = world.query_filtered::<&mut Projection, bevy::prelude::With<camera::GameCamera>>();
        for mut proj in q.iter_mut(world) {
            if let Projection::Orthographic(o) = &mut *proj {
                o.scaling_mode = bevy::camera::ScalingMode::FixedVertical { viewport_height: v * 2.0 };
            }
        }
    }
    // SALADIN_YAW=<quarter turns> pre-rotates the camera (rotation screenshots)
    if let Ok(s) = std::env::var("SALADIN_YAW")
        && let Ok(q) = s.parse::<i32>()
    {
        let yaw = q as f32 * std::f32::consts::FRAC_PI_2;
        let mut st = app.world_mut().resource_mut::<camera::CameraState>();
        st.yaw = yaw;
        st.target_yaw = yaw;
    }
    // SALADIN_SEED / SALADIN_PRESET override the menu defaults (screenshot runs)
    if let Ok(s) = std::env::var("SALADIN_SEED")
        && let Ok(seed) = s.parse::<u32>()
    {
        app.world_mut().resource_mut::<MenuConfig>().seed = seed;
    }
    if let Ok(s) = std::env::var("SALADIN_PRESET")
        && let Ok(preset) = s.parse::<u8>()
    {
        app.world_mut().resource_mut::<MenuConfig>().preset = preset;
    }
    match std::env::var("SALADIN_AUTO").as_deref() {
        Ok("1") => {
            app.insert_state(GameState::Playing);
            app.add_systems(Update, auto_screenshot);
        }
        Ok("sp") => {
            app.insert_resource(ui::menu::MenuScreen::Singleplayer);
            app.add_systems(Update, (auto_screenshot, debug_layout));
        }
        Ok("menu") => {
            app.add_systems(Update, auto_screenshot);
        }
        Ok("mp") => {
            app.insert_resource(ui::menu::MenuScreen::Multiplayer);
            app.add_systems(Update, auto_screenshot);
        }
        Ok("settings") => {
            app.insert_resource(ui::menu::MenuScreen::Settings);
            app.add_systems(Update, auto_screenshot);
        }
        Ok("pause") => {
            app.insert_state(GameState::Playing);
            app.insert_resource(ui::pause::PauseScreen::Menu);
            app.add_systems(Update, auto_screenshot);
        }
        Ok("research") | Ok("market") | Ok("keep") | Ok("hut") => {
            // conjure + select a building so the screenshot shows its panel
            // (research on the blacksmith / trade on the market)
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_select_building, debug_layout));
        }
        Ok("layout") => {
            // in-game + computed-rect dump for HUD layout debugging
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, debug_layout));
        }
        Ok("units") => {
            // conjure one of every unit kind beside the keep (model verification)
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_spawn_units));
        }
        Ok("lobby") => {
            let bind = format!("0.0.0.0:{HOST_PORT}");
            if saladin_protocol::spawn_host_relay(&bind).is_ok()
                && let Ok(t) = saladin_protocol::TcpTransport::connect(
                    &format!("127.0.0.1:{HOST_PORT}"),
                    "Saladin",
                    saladin_protocol::JoinIntent::Direct,
                )
            {
                app.insert_resource(LobbyConn(std::sync::Mutex::new(Some(t))));
                app.insert_resource(ui::menu::LobbyMode::LanHost { ips: config::lan_ips() });
                app.insert_state(GameState::Lobby);
            }
            app.add_systems(Update, auto_screenshot);
        }
        _ => {}
    }
    app.run();
}

/// Screenshot harness only: conjure one of every unit kind in a line beside
/// the keep so SALADIN_AUTO=units captures all unit models in one shot.
fn auto_spawn_units(world: &mut World, mut stage: Local<u8>) {
    use saladin_protocol::{MatchId, NextEntityId, Owner, Pos, Unit};
    use saladin_sim::{GatherState, Stance, UnitKind, unit_def};
    let t = world.resource::<Time>().elapsed_secs();
    // stage 2: at t=5 bite a chunk out of the conjured land food nodes (shows
    // the carcass transition), load the peasants (shows the carry sack), and
    // kill a few soldiers (shows the fall-and-sink death) for screenshots
    if *stage == 1 {
        if t >= 5.0 {
            *stage = 2;
            let mut q = world.query::<&mut saladin_protocol::ResourceNode>();
            for mut n in q.iter_mut(world) {
                if n.res_type == saladin_sim::ResourceType::Food && n.remaining == 200 {
                    n.remaining = 150;
                }
            }
            let mut q = world.query::<&mut Unit>();
            for mut u in q.iter_mut(world) {
                if u.kind == UnitKind::Peasant {
                    u.carrying = 25;
                }
            }
            // burn the keep so the staged damage smoke/fire shows
            let mut q = world.query::<&mut saladin_protocol::Building>();
            for mut b in q.iter_mut(world) {
                if b.kind == saladin_sim::BuildingKind::Keep {
                    b.hp = saladin_sim::building_def(b.kind).max_hp / 5;
                }
            }
            let victims: Vec<Entity> = {
                let mut q = world.query_filtered::<(Entity, &Unit), bevy::prelude::With<GameId>>();
                q.iter(world)
                    .filter(|(_, u)| u.kind != UnitKind::Peasant && !u.has_target)
                    .map(|(e, _)| e)
                    .take(3)
                    .collect()
            };
            for e in victims {
                world.despawn(e);
            }
        }
        return;
    }
    if *stage != 0 {
        return;
    }
    if t < 3.0 {
        return;
    }
    let keep = {
        let mut q = world.query::<(&Pos, &saladin_protocol::Building)>();
        q.iter(world)
            .find(|(_, b)| b.kind == saladin_sim::BuildingKind::Keep)
            .map(|(p, _)| p.pos)
    };
    let Some(kp) = keep else { return };
    *stage = 1;
    // One node of each kind beside the lineup, plus a food node pushed onto
    // the nearest water tile so the fish-school variant shows too.
    {
        use saladin_protocol::ResourceNode;
        use saladin_sim::ResourceType;
        let spawn_node = |world: &mut World, res, x: i32, z: i32| {
            let id = world.resource_mut::<saladin_protocol::NextEntityId>().alloc();
            let pos = saladin_sim::V2::new(
                kp.x + saladin_sim::Fx::from_num(x),
                kp.y + saladin_sim::Fx::from_num(z),
            );
            world.spawn((
                GameId(id),
                saladin_protocol::MatchId(1),
                saladin_protocol::Pos { pos, facing: saladin_sim::Fx::ZERO },
                ResourceNode { res_type: res, remaining: 200 },
            ));
        };
        for (i, res) in
            [ResourceType::Wood, ResourceType::Stone, ResourceType::Food, ResourceType::Gold]
                .into_iter()
                .enumerate()
        {
            spawn_node(world, res, -3, 2 + i as i32 * 2);
        }
        // hunt outward for a water tile (render height below sea)
        let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
        'water: for ring in 2..60 {
            for (dx, dz) in [(ring, 0), (-ring, 0), (0, ring), (0, -ring)] {
                let x = kp.x + saladin_sim::Fx::from_num(dx);
                let z = kp.y + saladin_sim::Fx::from_num(dz);
                let s = saladin_sim::sample_terrain(seed, x, z);
                if !saladin_sim::biome_def(s.biome).passable
                    && matches!(
                        s.biome,
                        saladin_sim::Biome::ShallowWater | saladin_sim::Biome::DeepWater
                    )
                {
                    spawn_node(world, ResourceType::Food, dx, dz);
                    break 'water;
                }
            }
        }
    }
    for (i, &kind) in UnitKind::ALL.iter().enumerate() {
        let def = unit_def(kind);
        let pos = saladin_sim::V2::new(
            kp.x + saladin_sim::Fx::from_num(2 + (i as i32 % 5) * 2),
            kp.y + saladin_sim::Fx::from_num(3 + (i as i32 / 5) * 3),
        );
        // odd kinds march back toward the keep — the straight-line harness
        // walk has no pathfinding, and the keep's fair-start area is the only
        // ground guaranteed to be land
        let walking = i % 2 == 1;
        let target = if walking { kp } else { pos };
        let id = world.resource_mut::<NextEntityId>().alloc();
        world.spawn((
            GameId(id),
            Owner(1),
            MatchId(1),
            Pos { pos, facing: saladin_sim::Fx::ZERO },
            Unit {
                kind,
                target,
                has_target: walking,
                speed: def.speed,
                gather_state: GatherState::Idle,
                target_node: 0,
                carrying: 0,
                carry_type: saladin_sim::ResourceType::Wood,
                harvest_timer: saladin_sim::Fx::ZERO,
                hp: def.max_hp,
                attack_target: 0,
                attack_cooldown: saladin_sim::Fx::ZERO,
                stance: Stance::Defensive,
                morale: saladin_sim::MORALE_MAX,
                routing: false,
                home: pos,
                garrisoned_in: 0,
                path: vec![],
                path_idx: 0,
            },
        ));
    }
}

/// Screenshot harness only: conjure a building row beside the keep (the way
/// tests spawn rows) and select it, so SALADIN_AUTO=research/market captures
/// that building's panel without playing 10 minutes of economy.
fn auto_select_building(world: &mut World) {
    use saladin_protocol::{Building, MatchId, NextEntityId, Owner, Pos};
    use saladin_sim::{BuildingKind, building_def};
    let kind = match std::env::var("SALADIN_AUTO").as_deref() {
        Ok("market") => BuildingKind::Market,
        Ok("keep") => BuildingKind::Keep,
        Ok("hut") => BuildingKind::FishingHut,
        _ => BuildingKind::Blacksmith,
    };
    let t = world.resource::<Time>().elapsed_secs();
    if t < 3.0 {
        return;
    }
    let existing = {
        let mut q = world.query::<(&GameId, &Building)>();
        q.iter(world).find(|(_, b)| b.kind == kind).map(|(g, _)| g.0)
    };
    let id = match existing {
        Some(id) => id,
        None => {
            let keep = {
                let mut q = world.query::<(&Pos, &Building)>();
                q.iter(world).find(|(_, b)| b.kind == BuildingKind::Keep).map(|(p, _)| p.pos)
            };
            if kind == BuildingKind::Keep {
                // the founded keep already exists; selection block below finds it
                return;
            }
            let Some(kp) = keep else { return };
            let pos = saladin_sim::V2::new(kp.x + saladin_sim::fx!("4"), kp.y + saladin_sim::fx!("2"));
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(1),
                MatchId(1),
                Pos { pos, facing: saladin_sim::Fx::ZERO },
                Building {
                    kind,
                    hp: building_def(kind).max_hp,
                    cooldown: saladin_sim::Fx::ZERO,
                    rally: pos,
                },
            ));
            id
        }
    };
    // select via the same source of truth the click path uses
    let mut sel = world.resource_mut::<selection::Selection>();
    if sel.building.is_none() {
        sel.building = Some(id);
    }
}

fn debug_layout(
    time: Res<Time>,
    mut done: Local<bool>,
    q_bar: Query<(&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform), With<ui::hud::BottomCenter>>,
    q_text: Query<(&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform, &Text)>,
    q_btn: Query<(&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform, &Children), With<Button>>,
    q_txt_of: Query<&Text>,
) {
    if *done || time.elapsed_secs() < 5.0 {
        return;
    }
    *done = true;
    for (n, t) in &q_bar {
        eprintln!("BAR size={:?} pos={:?} inv_scale={}", n.size(), t.translation, n.inverse_scale_factor());
    }
    for (n, t, txt) in &q_text {
        if txt.0.len() < 24 {
            eprintln!("TEXT '{}' size={:?} pos={:?}", txt.0, n.size(), t.translation);
        }
    }
    for (n, t, children) in &q_btn {
        let label = children
            .iter()
            .find_map(|c| q_txt_of.get(c).ok())
            .map(|t| t.0.clone())
            .unwrap_or_default();
        eprintln!("BTN '{}' size={:?} pos={:?}", label, n.size(), t.translation);
    }
}

fn auto_screenshot(time: Res<Time>, mut done: Local<bool>, mut commands: Commands) {
    use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
    let at = std::env::var("SALADIN_SHOT_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    if *done || time.elapsed_secs() < at {
        return;
    }
    *done = true;
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk("/tmp/saladin_shot.png"));
}

/// In the lobby: pump the socket; when the host starts the match, promote the
/// connection into the lockstep transport and enter the game.
fn lobby_poll(world: &mut World) {
    let started = {
        let conn = world.resource_mut::<LobbyConn>();
        let guard = conn.0.lock().unwrap();
        let Some(t) = guard.as_ref() else { return };
        t.lobby().started
    };
    if !started {
        return;
    }
    let t = world.resource_mut::<LobbyConn>().0.lock().unwrap().take().unwrap();
    let l = t.lobby();
    let you = l.you;
    println!("match starting — you are player {you}, seed {}", l.seed);
    let me = l.me().cloned();
    world.insert_resource(PendingMatch {
        seed: l.seed.max(1),
        preset: l.preset,
        name: me.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| format!("Player {you}")),
        faction: me.map(|m| m.faction).unwrap_or(Faction::Ayyubid),
        is_host: l.is_host(),
        ais: l
            .players
            .iter()
            .filter(|p| p.is_ai)
            .map(|p| (p.id, p.ai_difficulty, p.faction))
            .collect(),
    });
    world.insert_resource(LocalPlayer(you));
    world.insert_resource(Multiplayer(true));
    world.insert_resource(Net {
        driver: LockstepDriver::new(you, 3),
        transport: std::sync::Mutex::new(Box::new(t)),
    });
    world.resource_mut::<NextState<GameState>>().set(GameState::Loading);
}

// ── lockstep sim driver ──────────────────────────────────────────────────────

fn drive_sim(world: &mut World) {
    let inputs = std::mem::take(&mut world.resource_mut::<LocalInput>().0);
    let events = world.resource_scope::<Net, _>(|world, net| {
        let net = net.into_inner();
        for c in inputs {
            net.driver.push(c);
        }
        let mut transport = net.transport.lock().unwrap();
        net.driver.advance(world, transport.as_mut());
        transport.take_events()
    });
    for ev in events {
        let saladin_protocol::NetEvent::PeerLeft(id) = ev;
        let name = {
            let mut q = world.query::<&Player>();
            q.iter(world).find(|p| p.player_id == id).map(|p| p.name.clone())
        }
        .unwrap_or_else(|| format!("Player {id}"));
        println!("{name} disconnected");
        world.resource_mut::<ui::pause::Disconnects>().0.push(name);
    }
}

fn setup_world(world: &mut World) {
    let cfg = world.resource::<MenuConfig>().clone();
    let local = world.resource::<LocalPlayer>().0;
    let multiplayer = world.resource::<Multiplayer>().0;

    // load path: restore the snapshot instead of founding a fresh match
    #[cfg(not(target_arch = "wasm32"))]
    if cfg.load && !multiplayer {
        world.resource_mut::<MenuConfig>().load = false;
        if let Some(save) =
            std::fs::read(save_path()).ok().as_deref().and_then(saladin_protocol::save::from_bytes)
        {
            saladin_protocol::save::restore(world, save);
            return;
        }
        eprintln!("save file missing/corrupt — starting a fresh skirmish");
    }

    if multiplayer {
        // the host's Welcome fixes the seed + roster for everyone
        let pm = world.resource::<PendingMatch>().clone();
        world.resource_mut::<WorldConfig>().seed = saladin_sim::compose_seed(pm.seed.max(1), pm.preset);
        scatter_world_nodes(world, 1);
        let inp = &mut world.resource_mut::<LocalInput>().0;
        // each client originates only its OWN join; the relay broadcasts it
        inp.push(PlayerCommand::Join { player_id: local, name: pm.name.clone(), faction: pm.faction, match_id: 1 });
        // AI seats are originated by the host alone (still deterministic: the
        // commands travel the lockstep stream like any other input)
        if pm.is_host {
            for (id, difficulty, faction) in &pm.ais {
                inp.push(PlayerCommand::AddAi {
                    player_id: *id,
                    host: local,
                    difficulty: *difficulty,
                    faction: *faction,
                    match_id: 1,
                });
            }
        }
        return;
    }

    world.resource_mut::<WorldConfig>().seed = saladin_sim::compose_seed(cfg.seed.max(1), cfg.preset);
    // worldgen is deterministic + identical on every client (seeded, not networked)
    scatter_world_nodes(world, 1);
    let enemy = enemy_faction(cfg.faction);
    let inp = &mut world.resource_mut::<LocalInput>().0;
    inp.push(PlayerCommand::Join { player_id: local, name: "You".into(), faction: cfg.faction, match_id: 1 });
    for i in 0..cfg.opponents {
        inp.push(PlayerCommand::AddAi {
            player_id: 1000 + i as u64,
            host: local,
            difficulty: cfg.difficulty,
            faction: enemy,
            match_id: 1,
        });
    }
}

/// Terrain, height field, model/material caches, sky/ocean/light rig,
/// vegetation — everything visual for the seeded map.
fn setup_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    cfg: Res<WorldConfig>,
    ui_assets: Res<ui::assets::UiAssets>,
) {
    // low-poly terrain from the same worldgen the sim uses
    let terrain_mesh = meshes.add(terrain::build_terrain_mesh(cfg.seed));
    let terrain_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE, // vertex colors carry the biome palette
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((Mesh3d(terrain_mesh), MeshMaterial3d(terrain_mat), Transform::IDENTITY, MatchScoped));
    let field = terrain::build_height_field(cfg.seed);

    // vegetation: shared prop meshes, one entity per placement (auto-instanced)
    let props: Vec<Handle<Mesh>> =
        render::models::props::prop_meshes().into_iter().map(|m| meshes.add(m)).collect();
    let prop_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    for p in vegetation::vegetation_placements(cfg.seed) {
        let y = terrain::height_at(&field, p.x, p.z);
        commands.spawn((
            Mesh3d(props[p.mesh].clone()),
            MeshMaterial3d(prop_mat.clone()),
            Transform::from_xyz(p.x, y, p.z)
                .with_rotation(Quat::from_rotation_y(p.rot))
                .with_scale(Vec3::splat(p.scale)),
            MatchScoped,
        ));
    }

    environment::spawn_environment(&mut commands, &mut meshes, &mut materials);

    // ── living world: shore anchors, glinting sea, wheeling gulls ────────────
    {
        use saladin_sim::{Biome, Fx, is_passable, sample_terrain};
        let half = Fx::lit("0.5");
        let mut shore: Vec<Vec3> = Vec::new();
        for ty in 1..WORLD_SIZE - 1 {
            for tx in 1..WORLD_SIZE - 1 {
                if is_passable(cfg.seed, tx, ty) {
                    continue;
                }
                let s = sample_terrain(cfg.seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half);
                if !matches!(s.biome, Biome::ShallowWater | Biome::DeepWater | Biome::River) {
                    continue;
                }
                if [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| is_passable(cfg.seed, tx + dx, ty + dy))
                {
                    let (x, z) = (tx as f32 + 0.5, ty as f32 + 0.5);
                    shore.push(Vec3::new(x, terrain::height_at(&field, x, z), z));
                }
            }
        }

        // two counter-drifting glint layers over the sea
        let sparkle_tex = images.add(environment::sparkle_image());
        let center = WORLD_SIZE as f32 / 2.0;
        for (y, speed, alpha, tile) in [
            (-0.035f32, Vec2::new(0.010, 0.006), 0.34f32, 36.0f32),
            (-0.05, Vec2::new(-0.007, 0.011), 0.26, 22.0),
        ] {
            let mut mat = StandardMaterial {
                base_color: Color::WHITE.with_alpha(alpha),
                base_color_texture: Some(sparkle_tex.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            };
            mat.uv_transform = bevy::math::Affine2 {
                matrix2: Mat2::from_diagonal(Vec2::splat(tile)),
                translation: Vec2::ZERO,
            };
            commands.spawn((
                environment::SparkleLayer { speed },
                Mesh3d(meshes.add(bevy::mesh::PlaneMeshBuilder::new(Dir3::Y, Vec2::ONE).build())),
                MeshMaterial3d(materials.add(mat)),
                Transform::from_xyz(center, y, center).with_scale(Vec3::splat(WORLD_SIZE as f32 + 380.0)),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
                MatchScoped,
            ));
        }

        // gulls wheeling over the coastline
        if !shore.is_empty() {
            let white = materials.add(StandardMaterial {
                base_color: Color::srgb(0.96, 0.96, 0.94),
                perceptual_roughness: 0.8,
                ..default()
            });
            let body = meshes.add(Mesh::from(Cuboid::new(0.14, 0.06, 0.34)));
            let wing_l = meshes.add(
                Mesh::from(Cuboid::new(0.42, 0.015, 0.13))
                    .transformed_by(Transform::from_xyz(-0.24, 0.0, 0.0)),
            );
            let wing_r = meshes.add(
                Mesh::from(Cuboid::new(0.42, 0.015, 0.13))
                    .transformed_by(Transform::from_xyz(0.24, 0.0, 0.0)),
            );
            // wheel most of the flock over the player's home coast
            let home = saladin_sim::start_point(cfg.seed, 0);
            let hx = home.x.to_num::<f32>();
            let hz = home.y.to_num::<f32>();
            let mut near: Vec<Vec3> = shore.clone();
            near.sort_by(|a, b| {
                let da = (a.x - hx).powi(2) + (a.z - hz).powi(2);
                let db = (b.x - hx).powi(2) + (b.z - hz).powi(2);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
            for i in 0..9usize {
                let at = if i < 5 {
                    near[(i * 13).min(near.len() - 1)]
                } else {
                    shore[(i * 7919) % shore.len()]
                };
                let g = environment::Gull {
                    center: at + Vec3::Y * 3.2,
                    r: 2.5 + (i % 4) as f32 * 1.1,
                    w: (0.22 + (i % 3) as f32 * 0.08) * if i % 2 == 0 { 1.0 } else { -1.0 },
                    phase: i as f32 * 0.81,
                };
                commands
                    .spawn((g, Transform::from_translation(at), Visibility::Inherited, MatchScoped))
                    .with_children(|p| {
                        p.spawn((Mesh3d(body.clone()), MeshMaterial3d(white.clone())));
                        p.spawn((
                            environment::GullWing { left: true },
                            Mesh3d(wing_l.clone()),
                            MeshMaterial3d(white.clone()),
                        ));
                        p.spawn((
                            environment::GullWing { left: false },
                            Mesh3d(wing_r.clone()),
                            MeshMaterial3d(white.clone()),
                        ));
                    });
            }
        }
        commands.insert_resource(environment::ShoreList(shore));
    }

    commands.insert_resource(field);
    commands.insert_resource(render::sync::build_assets(&mut meshes));
    commands.insert_resource(render::sync::build_materials(
        &mut materials,
        ui_assets.ring.clone(),
        ui_assets.flag.clone(),
    ));
    commands.insert_resource(fx::build_arrow_assets(&mut meshes, &mut materials));
}

fn check_gameover(
    local: Res<LocalPlayer>,
    q_players: Query<&Player>,
    mut next: ResMut<NextState<GameState>>,
) {
    let players: Vec<&Player> = q_players.iter().collect();
    if players.len() < 2 {
        return; // not fully set up yet
    }
    let me = local.0;
    let i_lost = players.iter().find(|p| p.player_id == me).map(|p| p.defeated).unwrap_or(true);
    let enemy_alive = players.iter().any(|p| p.player_id != me && !p.defeated);
    if i_lost || !enemy_alive {
        next.set(GameState::GameOver);
    }
}

/// Snapshot the sim to disk when Save & Quit was pressed, then back to menu.
fn do_save(world: &mut World) {
    if !world.resource::<PendingSave>().0 {
        return;
    }
    world.resource_mut::<PendingSave>().0 = false;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let snap = saladin_protocol::save::snapshot(world);
        let path = save_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&path, saladin_protocol::save::to_bytes(&snap)) {
            Ok(()) => println!("saved to {}", path.display()),
            Err(e) => eprintln!("save failed: {e}"),
        }
    }
    world.resource_mut::<NextState<GameState>>().set(GameState::Menu);
}

/// Tear down one match completely (sim rows, render trees, terrain, env, fx)
/// and reset the lockstep plumbing so the menu can start a fresh one.
#[allow(clippy::type_complexity)]
fn teardown_match(world: &mut World) {
    // sim entities (units/buildings/nodes/players/research) + match rows
    let sim: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, Or<(With<GameId>, With<MatchInfo>)>>();
        q.iter(world).collect()
    };
    // render trees, overlays, fx, environment, terrain/vegetation
    let vis: Vec<Entity> = {
        let mut q = world.query_filtered::<
            Entity,
            Or<(
                With<render::sync::RenderRoot>,
                With<render::sync::HpBar>,
                With<render::sync::BuildingSelRing>,
                With<render::sync::RallyFlag>,
                With<render::ghost::GhostCell>,
                With<render::ghost::DemolishOverlay>,
                With<fx::Arrow>,
                With<MatchScoped>,
                With<environment::SkyDome>,
                With<environment::OceanPlane>,
                With<environment::SunLight>,
            )>,
        >();
        q.iter(world).collect()
    };
    for e in sim.into_iter().chain(vis) {
        world.despawn(e);
    }

    // reset lockstep + per-match client state
    *world.resource_mut::<saladin_protocol::CommandQueue>() = default();
    *world.resource_mut::<saladin_protocol::NextEntityId>() = default();
    *world.resource_mut::<saladin_protocol::Tick>() = default();
    *world.resource_mut::<saladin_protocol::StateHash>() = default();
    *world.resource_mut::<saladin_protocol::SimRng>() = default();
    world.resource_mut::<saladin_protocol::MatchStats>().0.clear();
    world.resource_mut::<ui::pause::Disconnects>().0.clear();
    *world.resource_mut::<ui::pause::PauseScreen>() = default();
    world.resource_mut::<saladin_protocol::GameIndex>().0.clear();
    world.resource_mut::<saladin_protocol::MatchStatuses>().0.clear();
    world.resource_mut::<saladin_protocol::ShotEvents>().0.clear();
    *world.resource_mut::<selection::Selection>() = default();
    *world.resource_mut::<selection::ControlGroups>() = default();
    *world.resource_mut::<input::InputMode>() = default();
    *world.resource_mut::<render::sync::RenderMap>() = default();
    *world.resource_mut::<ui::hud::HudDigest>() = default();
    *world.resource_mut::<LocalInput>() = default();
    world.resource_mut::<camera::CameraState>().framed = false;
    let c = WORLD_SIZE as f32 / 2.0;
    world.resource_mut::<camera::CameraState>().snap_center(Vec3::new(c, 0.0, c));

    // a fresh in-memory relay for the next single-player match
    if !world.resource::<Multiplayer>().0 {
        let (net, _, _) = build_net(None);
        world.insert_resource(net);
    }
}
