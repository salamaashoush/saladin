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
    /// Start the next match by restoring the save file instead of a fresh world.
    pub load: bool,
}

impl Default for MenuConfig {
    fn default() -> Self {
        MenuConfig { opponents: 1, faction: Faction::Ayyubid, difficulty: AiDifficulty::Normal, seed: 1, load: false }
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
    .insert_resource(config::load())
    .init_resource::<perf::PerfVisible>()
    .add_systems(Startup, (perf::setup_perf, input::spawn_drag_box, ui::widgets::prewarm_font_atlas))
    .add_systems(
        Update,
        (
            ui::text_input::focus_text_inputs,
            ui::text_input::type_into_inputs,
            ui::text_input::render_text_inputs,
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
        (ui::hud::cleanup_hud, minimap::despawn_minimap, teardown_match),
    )
    .add_systems(FixedUpdate, drive_sim.run_if(in_state(GameState::Playing)))
    .add_systems(Update, do_save.run_if(in_state(GameState::Playing)))
    // gameplay frame systems
    .add_systems(
        Update,
        (
            camera::pan_camera,
            camera::zoom_camera,
            camera::frame_keep,
            minimap::update_minimap_viewport,
            minimap::minimap_click,
            input::pointer_input,
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
            render::sync::update_hp_bars,
            render::sync::update_building_highlight,
            render::ghost::update_ghost,
            render::ghost::update_demolish_overlay,
            fx::spawn_arrows,
            fx::fly_arrows,
        )
            .run_if(in_state(GameState::Playing)),
    )
    .add_systems(
        Update,
        (
            environment::follow_camera,
            environment::shimmer_ocean,
            ui::hud::update_resource_bar,
            ui::hud::update_bottom_bar,
            ui::hud::watch_toasts,
            ui::hud::tick_toasts,
            ui::hud::render_toasts,
            ui::actions::handle_actions,
            ui::actions::button_hover,
            perf::update_perf,
            check_gameover,
        )
            .run_if(in_state(GameState::Playing)),
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
    match std::env::var("SALADIN_AUTO").as_deref() {
        Ok("1") => {
            app.insert_state(GameState::Playing);
            app.add_systems(Update, auto_screenshot);
        }
        Ok("menu") => {
            app.add_systems(Update, auto_screenshot);
        }
        Ok("mp") => {
            app.insert_resource(ui::menu::MenuScreen::Multiplayer);
            app.add_systems(Update, auto_screenshot);
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

fn auto_screenshot(time: Res<Time>, mut done: Local<bool>, mut commands: Commands) {
    use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
    if *done || time.elapsed_secs() < 6.0 {
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
    world.resource_mut::<NextState<GameState>>().set(GameState::Playing);
}

// ── lockstep sim driver ──────────────────────────────────────────────────────

fn drive_sim(world: &mut World) {
    let inputs = std::mem::take(&mut world.resource_mut::<LocalInput>().0);
    world.resource_scope::<Net, _>(|world, net| {
        let net = net.into_inner();
        for c in inputs {
            net.driver.push(c);
        }
        let mut transport = net.transport.lock().unwrap();
        net.driver.advance(world, transport.as_mut());
    });
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
        world.resource_mut::<WorldConfig>().seed = pm.seed;
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

    world.resource_mut::<WorldConfig>().seed = cfg.seed.max(1);
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
    cfg: Res<WorldConfig>,
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

    commands.insert_resource(field);
    commands.insert_resource(render::sync::build_assets(&mut meshes));
    commands.insert_resource(render::sync::build_materials(&mut materials));
    commands.insert_resource(fx::build_arrow_assets(&mut meshes));
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
    world.resource_mut::<camera::CameraState>().center = Vec3::new(c, 0.0, c);

    // a fresh in-memory relay for the next single-player match
    if !world.resource::<Multiplayer>().0 {
        let (net, _, _) = build_net(None);
        world.insert_resource(net);
    }
}
