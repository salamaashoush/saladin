//! Dev / test harness — EVERYTHING here is opt-in and stays out of a normal
//! game launch:
//!  - `SALADIN_AUTO` + `SALADIN_*` env overrides: the screenshot verification
//!    harness (shot.sh) — conjured units, building panels, the wall demo.
//!  - `SALADIN_DEV=1`: an in-game dev console (backquote) with cheat/test
//!    commands. SINGLE-PLAYER ONLY — direct world mutation would desync a
//!    lockstep peer, so the console refuses to run in multiplayer.

use crate::{
    GameState, HOST_PORT, LobbyConn, LocalPlayer, MenuConfig, Multiplayer, UiFont, camera, config,
    input, selection, ui,
};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use saladin_protocol::*;

/// Wire every env-gated harness hook. Called once from `main` right before
/// `app.run()`; with no SALADIN_* vars set this is a no-op.
pub fn setup(app: &mut App) {
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

    if dev_enabled() {
        app.init_resource::<DevConsole>();
        app.add_systems(
            Update,
            (console_type, console_render, console_exec).chain().run_if(in_state(GameState::Playing)),
        );
    }
}

// ── dev console (SALADIN_DEV=1, single-player only) ─────────────────────────

fn dev_enabled() -> bool {
    std::env::var("SALADIN_DEV").is_ok_and(|v| v != "0" && !v.is_empty())
}

#[derive(Resource, Default)]
pub struct DevConsole {
    open: bool,
    line: String,
    log: Vec<String>,
    pending: Vec<String>,
}

#[derive(Component)]
struct ConsoleUi;

#[derive(Component)]
struct ConsoleText;

/// Backquote toggles; typed text goes into the line; Enter queues execution.
fn console_type(mut keys: MessageReader<KeyboardInput>, mut con: ResMut<DevConsole>) {
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if ev.logical_key == Key::Character("`".into()) {
            con.open = !con.open;
            con.line.clear();
            continue;
        }
        if !con.open {
            continue;
        }
        match &ev.logical_key {
            Key::Backspace => {
                con.line.pop();
            }
            Key::Escape => {
                con.open = false;
                con.line.clear();
            }
            Key::Enter => {
                let line = std::mem::take(&mut con.line);
                if !line.trim().is_empty() {
                    con.pending.push(line);
                }
            }
            _ => {
                if let Some(text) = &ev.text {
                    for c in text.chars() {
                        if !c.is_control() && c != '`' && con.line.len() < 80 {
                            con.line.push(c);
                        }
                    }
                }
            }
        }
    }
}

/// One overlay panel: last few log lines + the prompt.
fn console_render(
    mut commands: Commands,
    con: Res<DevConsole>,
    font: Res<UiFont>,
    q_ui: Query<Entity, With<ConsoleUi>>,
    mut q_text: Query<&mut Text, With<ConsoleText>>,
) {
    if !con.open {
        for e in &q_ui {
            commands.entity(e).despawn();
        }
        return;
    }
    let mut body = String::new();
    for l in con.log.iter().rev().take(8).rev() {
        body.push_str(l);
        body.push('\n');
    }
    body.push_str("> ");
    body.push_str(&con.line);
    body.push('_');
    if let Ok(mut t) = q_text.single_mut() {
        if t.0 != body {
            t.0 = body;
        }
        return;
    }
    commands
        .spawn((
            ConsoleUi,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                top: Val::Px(48.0),
                padding: UiRect::all(Val::Px(8.0)),
                min_width: Val::Px(420.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.03, 0.02, 0.85)),
            GlobalZIndex(900),
        ))
        .with_children(|p| {
            p.spawn((
                ConsoleText,
                Text::new("> "),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: bevy::text::FontSize::Px(13.0),
                    font_smoothing: bevy::text::FontSmoothing::None,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.88, 0.75)),
            ));
        });
}

/// Drain queued lines with full world access. Refuses to cheat in multiplayer.
fn console_exec(world: &mut World) {
    let lines = std::mem::take(&mut world.resource_mut::<DevConsole>().pending);
    if lines.is_empty() {
        return;
    }
    let mp = world.resource::<Multiplayer>().0;
    for line in lines {
        let reply = if mp {
            "dev console is single-player only (would desync lockstep)".to_string()
        } else {
            run_command(world, &line)
        };
        let mut con = world.resource_mut::<DevConsole>();
        con.log.push(format!("> {line}"));
        con.log.push(reply);
        if con.log.len() > 40 {
            let cut = con.log.len() - 40;
            con.log.drain(..cut);
        }
    }
}

fn run_command(world: &mut World, line: &str) -> String {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let me = world.resource::<LocalPlayer>().0;
    match parts.as_slice() {
        ["help"] => "give <wood|stone|food|gold|all> <n> | spawn <kind> [n] | walls | burn | ai <easy|normal|hard>".into(),
        ["give", res, n] => {
            let Ok(amt) = n.parse::<i32>() else { return "bad amount".into() };
            let mut q = world.query::<&mut Player>();
            let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == me) else {
                return "no local player".into();
            };
            match *res {
                "wood" => p.stock.wood += amt,
                "stone" => p.stock.stone += amt,
                "food" => p.stock.food += amt,
                "gold" => p.stock.gold += amt,
                "all" => {
                    p.stock.wood += amt;
                    p.stock.stone += amt;
                    p.stock.food += amt;
                    p.stock.gold += amt;
                }
                _ => return "give <wood|stone|food|gold|all> <n>".into(),
            }
            format!("granted {amt} {res}")
        }
        ["spawn", kind_str, rest @ ..] => {
            let n: usize = rest.first().and_then(|s| s.parse().ok()).unwrap_or(1);
            let Some(kind) = unit_kind_by_name(kind_str) else {
                return format!("unknown unit '{kind_str}'");
            };
            let Some(kp) = keep_pos(world, me) else { return "no keep".into() };
            for i in 0..n.min(50) {
                spawn_dev_unit(world, me, kind, kp, i as i32);
            }
            format!("spawned {} {kind:?}", n.min(50))
        }
        ["walls"] => {
            let Some(kp) = keep_pos(world, me) else { return "no keep".into() };
            conjure_wall_demo(world, me, kp);
            "wall demo queued (L-run + composed gate + tower)".into()
        }
        ["burn"] => {
            let mut q = world.query::<(&Owner, &mut Building)>();
            let mut hit = 0;
            for (o, mut b) in q.iter_mut(world) {
                if o.0 == me && b.kind == saladin_sim::BuildingKind::Keep {
                    b.hp = saladin_sim::building_def(b.kind).max_hp / 5;
                    hit += 1;
                }
            }
            format!("burned {hit} keep(s) to 20%")
        }
        ["ai", diff] => {
            let d = match *diff {
                "easy" => saladin_sim::AiDifficulty::Easy,
                "hard" => saladin_sim::AiDifficulty::Hard,
                _ => saladin_sim::AiDifficulty::Normal,
            };
            let id = 1000 + world.resource::<bevy::prelude::Time>().elapsed_secs() as u64 % 900;
            world.resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
                player_id: id,
                host: me,
                difficulty: d,
                faction: saladin_sim::enemy_faction(saladin_sim::Faction::Ayyubid),
                match_id: 1,
            });
            format!("AI {diff} seat queued (id {id})")
        }
        _ => "unknown command — try 'help'".into(),
    }
}

fn unit_kind_by_name(s: &str) -> Option<saladin_sim::UnitKind> {
    use saladin_sim::UnitKind::*;
    Some(match s.to_ascii_lowercase().as_str() {
        "peasant" => Peasant,
        "spearman" | "spear" => Spearman,
        "archer" => Archer,
        "knight" => Knight,
        "horsearcher" | "ha" => HorseArcher,
        "mamluk" => Mamluk,
        "crossbowman" | "xbow" => Crossbowman,
        "ram" => Ram,
        "mangonel" => Mangonel,
        "imam" => Imam,
        _ => return None,
    })
}

fn keep_pos(world: &mut World, owner: u64) -> Option<saladin_sim::V2> {
    let mut q = world.query::<(&Pos, &Owner, &Building)>();
    q.iter(world)
        .find(|(_, o, b)| o.0 == owner && b.kind == saladin_sim::BuildingKind::Keep)
        .map(|(p, _, _)| p.pos)
}

fn spawn_dev_unit(world: &mut World, owner: u64, kind: saladin_sim::UnitKind, kp: saladin_sim::V2, i: i32) {
    use saladin_sim::{GatherState, Stance, unit_def};
    let def = unit_def(kind);
    let pos = saladin_sim::V2::new(
        kp.x + saladin_sim::Fx::from_num(3 + i % 6),
        kp.y + saladin_sim::Fx::from_num(3 + i / 6),
    );
    let id = world.resource_mut::<NextEntityId>().alloc();
    world.spawn((
        GameId(id),
        Owner(owner),
        MatchId(1),
        Pos { pos, facing: saladin_sim::Fx::ZERO },
        Unit {
            kind,
            target: pos,
            has_target: false,
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

/// The wall showcase: an L-run with a gate composed mid-run and a tower at the
/// corner, through the REAL PlaceWall/Build path. Also used by SALADIN_AUTO=units.
pub fn conjure_wall_demo(world: &mut World, me: u64, kp: saladin_sim::V2) {
    let kx = kp.x.to_num::<f32>().floor() as i32;
    let kz = kp.y.to_num::<f32>().floor() as i32;
    let seed = world.resource::<WorldConfig>().seed;
    let ok = |tx: i32, tz: i32| saladin_sim::is_buildable_tile(seed, tx, tz);
    let mut found = None;
    'scan: for r in 4..20 {
        for dz in [-r, r] {
            for x0 in (kx - 12)..(kx + 6) {
                let z = kz + dz;
                let clears_keep = z.abs_diff(kz) > 2 || x0 > kx + 2 || x0 + 6 < kx - 2;
                if clears_keep
                    && (0..7).all(|i| ok(x0 + i, z))
                    && (1..5).all(|j| ok(x0 + 6, z - j))
                {
                    found = Some((x0, z));
                    break 'scan;
                }
            }
        }
    }
    let Some((x0, z)) = found else { return };
    let mut tiles: Vec<(i32, i32)> = (0..7).map(|i| (x0 + i, z)).collect();
    tiles.extend((1..5).map(|j| (x0 + 6, z - j)));
    let center = |tx: i32, tz: i32| {
        saladin_sim::V2::new(
            saladin_sim::Fx::from_num(tx) + saladin_sim::fx!("0.5"),
            saladin_sim::Fx::from_num(tz) + saladin_sim::fx!("0.5"),
        )
    };
    let mut q = world.resource_mut::<CommandQueue>();
    q.0.push(PlayerCommand::PlaceWall { player_id: me, tiles });
    q.0.push(PlayerCommand::Build {
        player_id: me,
        kind: saladin_sim::BuildingKind::Gatehouse,
        pos: center(x0 + 3, z),
        facing: 0,
    });
    q.0.push(PlayerCommand::Build {
        player_id: me,
        kind: saladin_sim::BuildingKind::Tower,
        pos: center(x0 + 6, z),
        facing: 0,
    });
}

// ── screenshot harness systems (moved verbatim from main.rs) ────────────────

/// Screenshot harness only: conjure one of every unit kind in a line beside
/// the keep so SALADIN_AUTO=units captures all unit models in one shot.
pub fn auto_spawn_units(world: &mut World, mut stage: Local<u8>) {
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
    // wall demo: gate + tower composed into an L-run via the REAL command path
    {
        let me = world.resource::<LocalPlayer>().0;
        {
            let mut q = world.query::<&mut saladin_protocol::Player>();
            for mut p in q.iter_mut(world) {
                if p.player_id == me {
                    p.stock = saladin_sim::Stockpile { wood: 999, stone: 999, food: 999, gold: 999 };
                }
            }
        }
        conjure_wall_demo(world, me, kp);
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
pub fn auto_select_building(world: &mut World) {
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

pub fn debug_layout(
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

pub fn auto_screenshot(time: Res<Time>, mut done: Local<bool>, mut commands: Commands) {
    use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
    let at = std::env::var("SALADIN_SHOT_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    if *done || time.elapsed_secs() < at {
        return;
    }
    *done = true;
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk("/tmp/saladin_shot.png"));
}
