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
    // SALADIN_FACTION=1 plays Crusader (faction-variant architecture shots)
    if let Ok(s) = std::env::var("SALADIN_FACTION")
        && let Ok(f) = s.parse::<u8>()
    {
        app.world_mut().resource_mut::<MenuConfig>().faction =
            saladin_sim::Faction::from_u8(f).unwrap_or(saladin_sim::Faction::Ayyubid);
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
        Ok("research") | Ok("market") | Ok("keep") | Ok("hut") | Ok("harbour") | Ok("granary")
        | Ok("store") | Ok("mosque") | Ok("tower") | Ok("house") | Ok("site") | Ok("barracks")
        | Ok("stable") => {
            // conjure + select a building so the screenshot shows its panel
            // (research on the blacksmith / trade on the market)
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_select_building, debug_layout));
        }
        Ok("soil") => {
            // farm siting: the terrain wears its fertility overlay
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, arm_farm_mode));
        }
        Ok("farm") => {
            // AUDIT harness: finish five farms through the REAL construction
            // path (so `finish_building` sows their fields), then pin one field
            // to each crop stage so the whole lifecycle fits in one shot.
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_farm_demo));
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
        Ok("ferry") => {
            // the whole naval loop in one frame: a laden barge at the beach, a
            // skiff over a school, and the party that is about to cross
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_ferry));
        }
        Ok("supply") => {
            // the baggage train as one still: a column past the end of the
            // supply line, selected, with the top bar carrying what the road
            // costs. SALADIN_STARVE=1 empties the larder so the same shot shows
            // the rationing warning instead of the bill.
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_column_afield, auto_select_units));
        }
        Ok("battle") => {
            // conjure two armies facing each other and let them fight — what an
            // engagement actually LOOKS like, without playing to first contact
            app.insert_state(GameState::Playing);
            app.add_systems(Update, (auto_screenshot, auto_battle));
            if std::env::var("SALADIN_SELECT").is_ok() {
                app.add_systems(Update, auto_select_units);
            }
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
        _ => "unknown command - try 'help'".into(),
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
    use saladin_sim::{Stance, unit_def};
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
            speed: def.speed,
            hp: def.max_hp,
            stance: Stance::Defensive,
            ..Unit::new(kind, pos)
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
    q.0.push(PlayerCommand::PlaceWall { player_id: me, tiles, builders: vec![] });
    q.0.push(PlayerCommand::Build {
        player_id: me,
        kind: saladin_sim::BuildingKind::Gatehouse,
        pos: center(x0 + 3, z),
        facing: 0,
        builders: vec![],
    });
    q.0.push(PlayerCommand::Build {
        player_id: me,
        kind: saladin_sim::BuildingKind::Tower,
        pos: center(x0 + 6, z),
        facing: 0,
        builders: vec![],
    });
}

// ── screenshot harness systems (moved verbatim from main.rs) ────────────────

/// Rows the harness deliberately leaves unfinished; everything else it founds
/// gets topped up so the wall-connectivity demo still reads as a wall.
#[derive(bevy::prelude::Component)]
pub struct HarnessLifecycle;

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
            // burn the keep so the staged damage smoke/fire shows, and finish
            // the wall demo the real PlaceWall path founded — it is there to
            // verify ARM connectivity, which a row of sites cannot show
            let mut q = world
                .query_filtered::<&mut saladin_protocol::Building, bevy::prelude::Without<HarnessLifecycle>>();
            for mut b in q.iter_mut(world) {
                if b.kind == saladin_sim::BuildingKind::Keep {
                    b.hp = saladin_sim::building_def(b.kind).max_hp / 5;
                } else if b.state == saladin_protocol::BuildState::Site {
                    b.state = saladin_protocol::BuildState::Complete;
                    b.work = saladin_sim::Fx::ONE;
                    b.hp = saladin_sim::building_def(b.kind).max_hp;
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
    if *stage == 2 {
        // SALADIN_WORK=1: re-pose every peasant in the three work cycles each
        // frame (the gather brain would idle them — their forced state has no
        // real target node), so one shot shows chop + mine + forage tools
        if std::env::var("SALADIN_WORK").is_ok() {
            use saladin_sim::ResourceType;
            let mut q = world.query::<&mut Unit>();
            let mut i = 0;
            for mut u in q.iter_mut(world) {
                if u.kind == UnitKind::Peasant {
                    u.gather_state = GatherState::Harvesting;
                    u.carry_type =
                        [ResourceType::Wood, ResourceType::Stone, ResourceType::Food][i % 3];
                    u.carrying = 0;
                    u.has_target = false;
                    i += 1;
                }
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
    // SALADIN_LOOK=<dx>,<dz> pans the camera that far from the keep so shots
    // can frame the building/unit showcases instead of the keep itself
    if let Ok(s) = std::env::var("SALADIN_LOOK")
        && let Some((dx, dz)) = s.split_once(',')
        && let (Ok(dx), Ok(dz)) = (dx.trim().parse::<f32>(), dz.trim().parse::<f32>())
    {
        let center = bevy::prelude::Vec3::new(
            kp.x.to_num::<f32>() + dx,
            0.0,
            kp.y.to_num::<f32>() + dz,
        );
        let mut st = world.resource_mut::<crate::camera::CameraState>();
        // target only: the glide system re-aims the camera transform when
        // center != target_center (setting both leaves the transform stale)
        st.target_center = center;
        st.framed = true; // beat frame_keep to the punch — it would re-center
    }
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
                ResourceNode::deposit(res, 200),
            ));
        };
        for (i, res) in
            [ResourceType::Wood, ResourceType::Stone, ResourceType::Food, ResourceType::Gold]
                .into_iter()
                .enumerate()
        {
            spawn_node(world, res, -3, 2 + i as i32 * 2);
        }
        // One fishery on the shelf and one in the deep: they are two different
        // rules with two different meshes, and a shot that only ever finds the
        // shallow one verifies half the fishing.
        let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
        for want in [saladin_sim::Biome::ShallowWater, saladin_sim::Biome::DeepWater] {
            'water: for ring in 2..90 {
                for (dx, dz) in [(ring, 0), (-ring, 0), (0, ring), (0, -ring)] {
                    let x = kp.x + saladin_sim::Fx::from_num(dx);
                    let z = kp.y + saladin_sim::Fx::from_num(dz);
                    if saladin_sim::sample_terrain(seed, x, z).biome == want {
                        spawn_node(world, ResourceType::Food, dx, dz);
                        break 'water;
                    }
                }
            }
        }
    }
    // prop lineup: every cosmetic kind against every rung of the tint ladder,
    // so a units shot verifies each procedural fallback and the HSV bake
    {
        let meshes: Vec<bevy::prelude::Mesh> = crate::render::models::baked::prop_meshes();
        let mat = {
            let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
            mats.add(StandardMaterial {
                base_color: bevy::prelude::Color::WHITE,
                perceptual_roughness: 0.95,
                ..Default::default()
            })
        };
        let handles: Vec<bevy::prelude::Handle<bevy::prelude::Mesh>> = {
            let mut assets = world.resource_mut::<Assets<bevy::prelude::Mesh>>();
            meshes
                .iter()
                .flat_map(|m| {
                    crate::PROP_TINTS
                        .iter()
                        .map(|&(h, s, v)| crate::render::models::bake_tint(m, h, s, v))
                        .collect::<Vec<_>>()
                })
                .map(|m| assets.add(m))
                .collect()
        };
        let field = crate::terrain::build_height_field(
            world.resource::<saladin_protocol::WorldConfig>().seed,
        );
        for (i, h) in handles.into_iter().enumerate() {
            let (kind, tint) = (i / crate::vegetation::TINTS, i % crate::vegetation::TINTS);
            let x = kp.x.to_num::<f32>() + 9.0 + (kind % 5) as f32 * 1.7;
            let z = kp.y.to_num::<f32>() - 7.0 + (kind / 5) as f32 * 5.6 + tint as f32 * 1.25;
            let y = crate::terrain::height_at(&field, x, z);
            world.spawn((
                bevy::prelude::Mesh3d(h),
                bevy::prelude::MeshMaterial3d(mat.clone()),
                bevy::prelude::Transform::from_xyz(x, y, z),
                crate::MatchScoped,
            ));
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
        // one of every other building kind in a grid west of the keep so
        // SALADIN_AUTO=units verifies all building models in one shot
        use saladin_sim::BuildingKind;
        let showcase: Vec<BuildingKind> = BuildingKind::ALL
            .iter()
            .copied()
            .filter(|k| !matches!(k, BuildingKind::Keep | BuildingKind::Wall))
            .collect();
        for (i, kind) in showcase.into_iter().enumerate() {
            let pos = saladin_sim::V2::new(
                kp.x - saladin_sim::Fx::from_num(6 + (i as i32 % 3) * 4),
                kp.y - saladin_sim::Fx::from_num(2 + (i as i32 / 3) * 4),
            );
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(me),
                MatchId(1),
                Pos { pos, facing: saladin_sim::Fx::ZERO },
                saladin_protocol::Building::new(kind, saladin_sim::building_def(kind).max_hp, pos),
            ));
        }
        // the LIFECYCLE row: two sites mid-build (one per footprint size, so
        // the scaffold's scaling is verifiable) and one hall burnt to its
        // scorch stage. These are the states a screenshot cannot otherwise
        // reach — nothing else in the harness is ever unfinished or damaged.
        // Marked so the stage-1 pass that tops up the wall demo leaves them.
        let lifecycle: [(BuildingKind, saladin_protocol::BuildState, i32); 3] = [
            (BuildingKind::Barracks, saladin_protocol::BuildState::Site, 30),
            (BuildingKind::Tower, saladin_protocol::BuildState::Site, 65),
            (BuildingKind::Market, saladin_protocol::BuildState::Complete, 45),
        ];
        for (i, (kind, state, pct)) in lifecycle.into_iter().enumerate() {
            let def = saladin_sim::building_def(kind);
            let pos = saladin_sim::V2::new(
                kp.x - saladin_sim::Fx::from_num(6 + i as i32 * 4),
                kp.y - saladin_sim::Fx::from_num(22),
            );
            let mut b = match state {
                saladin_protocol::BuildState::Site => {
                    saladin_protocol::Building::site(kind, def.max_hp, pos)
                }
                _ => saladin_protocol::Building::new(kind, def.max_hp, pos),
            };
            b.hp = (def.max_hp * pct / 100).max(1);
            if state == saladin_protocol::BuildState::Site {
                b.work = saladin_sim::Fx::from_num(pct) / saladin_sim::Fx::from_num(100);
                b.builders = 3;
            }
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(me),
                MatchId(1),
                Pos { pos, facing: saladin_sim::Fx::ZERO },
                b,
                HarnessLifecycle,
            ));
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
                target,
                has_target: walking,
                speed: def.speed,
                hp: def.max_hp,
                stance: Stance::Defensive,
                ..Unit::new(kind, pos)
            },
        ));
    }
}

/// How many farms the `farm` harness stands up: one per crop stage.
const FARM_DEMO_STAGES: usize = 5;

/// Stamp the five crop stages onto fields 0..4 (sorted by `GameId`, so the
/// order is stable across runs). `SALADIN_CROP=<n>` pins every field to that
/// level with no latch instead — two runs at different n diff to zero over the
/// farm mesh, which is how you prove the crop is not baked into the building.
fn pin_crop_stages(world: &mut World) -> Vec<(u64, Entity)> {
    use saladin_protocol::{Crop, ResourceNode};
    use saladin_sim::{FARM_RIPE_GRACE, FARM_STORE};
    let pin: Option<i32> = std::env::var("SALADIN_CROP").ok().and_then(|s| s.parse().ok());
    let mut fields: Vec<(u64, Entity)> = {
        let mut q = world.query::<(Entity, &GameId, &saladin_protocol::FieldOf)>();
        q.iter(world).map(|(e, g, _)| (g.0, e)).collect()
    };
    fields.sort_unstable();
    for (i, (_, e)) in fields.iter().enumerate() {
        // each plot keeps the cap ITS OWN soil bought — overwriting them all
        // with the reference store made the card's soil word say the same thing
        // on every farm, which is exactly the collapse the word exists to avoid
        let cap = world.get::<ResourceNode>(*e).map(|n| n.cap).filter(|c| *c > 0).unwrap_or(FARM_STORE);
        let stages: [(i32, Crop); FARM_DEMO_STAGES] = [
            (0, Crop::default()),
            (cap / 5, Crop::default()),
            (cap / 2, Crop::default()),
            (cap, Crop { ripe: true, standing: 0 }),
            (cap * 7 / 10, Crop { ripe: true, standing: FARM_RIPE_GRACE + 10 }),
        ];
        let (rem, crop) = match pin {
            Some(n) => (n, Crop::default()),
            None => stages[i.min(FARM_DEMO_STAGES - 1)],
        };
        if let Some(mut n) = world.get_mut::<ResourceNode>(*e) {
            n.remaining = rem;
        }
        if let Some(mut c) = world.get_mut::<Crop>(*e) {
            *c = crop;
        }
    }
    fields
}

/// Screenshot harness only (AUDIT): stand FIVE farms up beside the keep and
/// drive their fields to the five crop stages, so one frame shows the whole
/// lifecycle side by side.
///
/// Stage 0 founds Farm SITES one tick from done and puts a real Constructing
/// hand on each, so `construction::labour` completes them and `finish_building`
/// sows their `FieldOf` nodes exactly as a played game would.
/// Stage 1 frames the camera on the plots and starts pinning stubble / shoots /
/// green / ripe / lodged onto fields 0..4 EVERY frame (the economy tick would
/// otherwise regrow them out from under the shot).
pub fn auto_farm_demo(world: &mut World, mut stage: Local<u8>) {
    use saladin_protocol::{Building, BuildState, MatchId, NextEntityId, Owner, Pos, ResourceNode, Unit};
    use saladin_sim::{BuildingKind, Fx, GatherState, UnitKind, V2, building_def, unit_def};
    let t = world.resource::<Time>().elapsed_secs();
    // SALADIN_WORK=1: pin every peasant into the food work cycle so a shot
    // shows the tool and pose a reaper actually uses on a field
    if *stage >= 2 && std::env::var("SALADIN_WORK").is_ok() {
        // target_node has to be a FIELD or the renderer reads the pose as
        // foraging a wild herd — the reap cycle is keyed on the node, not the
        // resource (an unripe field routes a real order through the tend path,
        // which clears target_node, so the harness has to pin it)
        let fields: Vec<(u64, V2)> = {
            let mut q = world.query::<(&GameId, &Pos, &saladin_protocol::FieldOf)>();
            q.iter(world).map(|(g, p, _)| (g.0, p.pos)).collect()
        };
        let mut q = world.query::<(&Pos, &mut Unit)>();
        for (p, mut u) in q.iter_mut(world) {
            if u.kind != UnitKind::Peasant {
                continue;
            }
            u.gather_state = GatherState::Harvesting;
            u.carry_type = saladin_sim::ResourceType::Food;
            u.carrying = 0;
            u.has_target = false;
            if let Some((id, _)) = fields
                .iter()
                .min_by_key(|(_, fp)| saladin_sim::dist2(*fp, p.pos).to_bits())
            {
                u.target_node = *id;
            }
        }
    }
    if *stage == 0 {
        if t < 2.0 {
            return;
        }
        *stage = 1;
        let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
        let me = world.resource::<LocalPlayer>().0;
        let Some(kp) = ({
            let mut q = world.query::<(&Pos, &Building)>();
            q.iter(world).find(|(_, b)| b.kind == BuildingKind::Keep).map(|(p, _)| p.pos)
        }) else {
            return;
        };
        let (kx, kz) = (kp.x.to_num::<i32>(), kp.y.to_num::<i32>());
        let center =
            |tx: i32, tz: i32| V2::new(Fx::from_num(tx) + saladin_sim::fx!("0.5"), Fx::from_num(tz) + saladin_sim::fx!("0.5"));
        // fertile, buildable, clear of the keep — the same rule the ghost uses
        let mut spots: Vec<V2> = Vec::new();
        'scan: for r in 3i32..24 {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let p = center(kx + dx, kz + dz);
                    if saladin_sim::check_place(seed, BuildingKind::Farm, p.x, p.y, |_, _| false, &[])
                        .is_ok()
                        && spots.iter().all(|q| saladin_sim::dist(*q, p) > saladin_sim::fx!("3"))
                    {
                        spots.push(p);
                        if spots.len() == FARM_DEMO_STAGES {
                            break 'scan;
                        }
                    }
                }
            }
        }
        // SALADIN_HUB=1: a finished Granary at the plot centroid, so the card's
        // "Tended by" line and the aura ring a selected FARM borrows from its
        // hub can both be seen
        if std::env::var("SALADIN_HUB").is_ok()
            && let Some(first) = spots.first().copied()
        {
            let mut sum = V2::ZERO;
            for p in &spots {
                sum = sum.add(*p);
            }
            let n = Fx::from_num(spots.len() as i32);
            let at = V2::new(sum.x / n, sum.y / n);
            let at = if saladin_sim::check_place(seed, BuildingKind::Granary, at.x, at.y, |_, _| false, &[])
                .is_ok()
            {
                at
            } else {
                first.add(V2::new(saladin_sim::fx!("2"), Fx::ZERO))
            };
            let gdef = building_def(BuildingKind::Granary);
            let mut g = Building::site(BuildingKind::Granary, gdef.max_hp, at);
            g.state = BuildState::Complete;
            g.work = Fx::ONE;
            g.hp = gdef.max_hp;
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((GameId(id), Owner(me), MatchId(1), Pos { pos: at, facing: Fx::ZERO }, g));
        }
        let def = building_def(BuildingKind::Farm);
        for pos in spots {
            let mut b = Building::site(BuildingKind::Farm, def.max_hp, pos);
            b.work = saladin_sim::fx!("0.99");
            b.hp = def.max_hp;
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(me),
                MatchId(1),
                Pos { pos, facing: Fx::ZERO },
                b,
            ));
            // one hand standing on the site: crew_up counts it, labour finishes
            let pdef = unit_def(UnitKind::Peasant);
            let hid = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(hid),
                Owner(me),
                MatchId(1),
                Pos { pos, facing: Fx::ZERO },
                Unit {
                    speed: pdef.speed,
                    hp: pdef.max_hp,
                    gather_state: GatherState::Constructing,
                    job_site: id,
                    ..Unit::new(UnitKind::Peasant, pos)
                },
            ));
        }
        return;
    }
    if *stage == 1 {
        if t < 4.0 {
            return;
        }
        *stage = 2;
        let fields = pin_crop_stages(world);
        eprintln!("FARMDEMO fields={}", fields.len());
        // frame the plots themselves: the harness founds them off the keep, so
        // the default keep framing leaves them out of shot at close zoom
        if !fields.is_empty() {
            let mut sum = V2::ZERO;
            for (_, e) in &fields {
                if let Some(p) = world.get::<Pos>(*e) {
                    sum = sum.add(p.pos);
                }
            }
            let n = Fx::from_num(fields.len() as i32);
            let (cx, cz) = ((sum.x / n).to_num::<f32>(), (sum.y / n).to_num::<f32>());
            let y = world
                .get_resource::<crate::terrain::HeightField>()
                .map(|f| crate::terrain::height_at(f, cx, cz))
                .unwrap_or(0.0);
            let mut cam = world.resource_mut::<crate::camera::CameraState>();
            cam.snap_center(bevy::prelude::Vec3::new(cx, y, cz));
            cam.framed = true;
        }
        let farms: Vec<(u64, V2, BuildState, i32)> = {
            let mut q = world.query::<(&GameId, &Pos, &Building)>();
            q.iter(world)
                .filter(|(_, _, b)| b.kind == BuildingKind::Farm)
                .map(|(g, p, b)| (g.0, p.pos, b.state, b.hp))
                .collect()
        };
        for (id, p, st, hp) in &farms {
            eprintln!("FARMDEMO farm {id} at {},{} {st:?} hp {hp}", p.x.to_num::<f32>(), p.y.to_num::<f32>());
        }
        for (gid, e, _) in {
            let mut q = world.query::<(&GameId, Entity, &ResourceNode)>();
            let v: Vec<_> = q
                .iter(world)
                .filter(|(_, _, n)| n.res_type == saladin_sim::ResourceType::Food)
                .map(|(g, e, n)| (g.0, e, (n.remaining, n.cap, n.regen)))
                .collect();
            v
        } {
            let field = world.get::<saladin_protocol::FieldOf>(e).map(|f| f.0);
            if field.is_some() {
                eprintln!("FARMDEMO field {gid} of {field:?}");
            }
        }
        // SALADIN_FARM=<0..4> selects the farm owning the field pinned to that
        // stage, so the command card can be shot at every point of the season
        let want: usize =
            std::env::var("SALADIN_FARM").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        let pick = fields
            .get(want.min(fields.len().saturating_sub(1)))
            .and_then(|(_, e)| world.get::<saladin_protocol::FieldOf>(*e).map(|f| f.0))
            .or_else(|| farms.first().map(|f| f.0));
        if let Some(id) = pick {
            world.resource_mut::<selection::Selection>().building = Some(id);
        }
        // put every idle peasant on the fields: what WORKING a farm looks like
        let me = world.resource::<LocalPlayer>().0;
        let peasants: Vec<u64> = {
            let mut q = world.query::<(&GameId, &Owner, &Unit)>();
            q.iter(world)
                .filter(|(_, o, u)| o.0 == me && u.kind == UnitKind::Peasant)
                .map(|(g, ..)| g.0)
                .collect()
        };
        let field_ids: Vec<u64> = fields.iter().map(|(g, _)| *g).collect();
        let farm_ids: Vec<u64> = farms.iter().map(|(g, ..)| *g).collect();
        if !field_ids.is_empty() {
            let mut q = world.resource_mut::<CommandQueue>();
            for (i, u) in peasants.into_iter().enumerate() {
                // both orders a player can give a plot, alternating: the Gather
                // the balancer issues, and the Repair a RIGHT-CLICK on the farm
                // now emits. Both have to end with the man in the furrows.
                if i % 2 == 0 {
                    q.0.push(PlayerCommand::Gather {
                        player_id: me,
                        unit: u,
                        node: field_ids[i % field_ids.len()],
                    });
                } else {
                    q.0.push(PlayerCommand::Repair {
                        player_id: me,
                        unit: u,
                        building: farm_ids[i % farm_ids.len()],
                    });
                }
            }
        }
        return;
    }
    // the economy tick regrows a field and the reapers draw it down, so the
    // pinned stages have to be re-stamped every frame or the shot lies
    pin_crop_stages(world);
    if *stage == 2 {
        if t < 6.5 {
            return;
        }
        *stage = 3;
        // what the RENDERER thinks every worker is doing. A reaper misread as a
        // forager is invisible in a still (same sickle) but audible as an axe
        {
            use crate::render::sync::{Activity, AnimState, Particle};
            let mut tally: std::collections::BTreeMap<String, u32> = Default::default();
            let mut q = world.query::<&AnimState>();
            for a in q.iter(world) {
                if a.harvest || a.activity != Activity::None {
                    *tally.entry(format!("{:?}", a.activity)).or_default() += 1;
                }
            }
            let chaff = world.query::<&Particle>().iter(world).count();
            eprintln!("FARMDEMO activity {tally:?} particles {chaff}");
            let crews: Vec<i32> = {
                let mut q = world.query::<&Building>();
                q.iter(world)
                    .filter(|b| b.kind == BuildingKind::Farm)
                    .map(|b| b.builders)
                    .collect()
            };
            eprintln!("FARMDEMO crews {crews:?}");
        }
        // SALADIN_SURVEY=1: over every farm-eligible tile of this map, tally
        // what the food-node variant table would draw on it
        if std::env::var("SALADIN_SURVEY").is_ok() {
            let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
            let mut tally = [0u32; 6];
            let mut biomes: std::collections::BTreeMap<String, u32> = Default::default();
            let step = 3;
            let n = saladin_sim::WORLD_SIZE;
            let mut tiles = 0u32;
            for tz in (0..n).step_by(step) {
                for tx in (0..n).step_by(step) {
                    let (x, z) = (tx as f32 + 0.5, tz as f32 + 0.5);
                    if saladin_sim::check_place(
                        seed,
                        BuildingKind::Farm,
                        Fx::from_num(x),
                        Fx::from_num(z),
                        |_, _| false,
                        &[],
                    )
                    .is_err()
                    {
                        continue;
                    }
                    tiles += 1;
                    let roll = (saladin_sim::hash2(
                        x as i32,
                        z as i32,
                        saladin_sim::rng::mix_seed(seed, 0x3b17),
                    )
                    .to_num::<f32>()
                        * 997.0) as usize;
                    let idx = crate::render::sync::node_variant(
                        saladin_sim::ResourceType::Food,
                        seed,
                        x,
                        z,
                        roll,
                        6,
                    );
                    tally[idx] += 1;
                    let b = saladin_sim::sample_terrain(seed, Fx::from_num(x), Fx::from_num(z)).biome;
                    *biomes.entry(format!("{b:?}")).or_default() += 1;
                }
            }
            let names =
                ["DEER", "BOAR", "BERRY", "DEER_GRAZING", "DEER_CARCASS", "BOAR_CARCASS"];
            eprintln!("FARMSURVEY seed {seed}: {tiles} farm-eligible sample tiles");
            for (i, c) in tally.iter().enumerate() {
                if *c > 0 {
                    eprintln!(
                        "FARMSURVEY   {:<13} {c:>6}  {:.1}%",
                        names[i],
                        *c as f32 * 100.0 / tiles.max(1) as f32
                    );
                }
            }
            for (b, c) in biomes {
                eprintln!("FARMSURVEY   biome {b:<14} {c}");
            }
        }
        // what the RENDERER actually made of each field
        let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
        let fields: Vec<(u64, V2, i32)> = {
            let mut q = world.query::<(&GameId, &Pos, &ResourceNode, &saladin_protocol::FieldOf)>();
            q.iter(world).map(|(g, p, n, _)| (g.0, p.pos, n.remaining)).collect()
        };
        for (gid, p, rem) in fields {
            let (x, z) = (p.x.to_num::<f32>(), p.y.to_num::<f32>());
            let biome =
                saladin_sim::sample_terrain(seed, Fx::from_num(x), Fx::from_num(z)).biome;
            let roll = (saladin_sim::hash2(
                x as i32,
                z as i32,
                saladin_sim::rng::mix_seed(seed, 0x3b17),
            )
            .to_num::<f32>()
                * 997.0) as usize;
            let idx = crate::render::sync::node_variant(
                saladin_sim::ResourceType::Food,
                seed,
                x,
                z,
                roll,
                6,
            );
            let name = ["DEER", "BOAR", "BERRY", "DEER_GRAZING", "DEER_CARCASS", "BOAR_CARCASS"]
                [idx];
            let root = world.resource::<crate::render::sync::RenderMap>().0.get(&gid).copied();
            let has_brain = root
                .map(|e| world.get::<crate::render::sync::AnimalNode>(e).is_some())
                .unwrap_or(false);
            let carcass = root
                .and_then(|e| world.get::<crate::render::sync::AnimalNode>(e))
                .map(|a| (a.carcass, a.full, a.remaining));
            let crop = world.get::<saladin_protocol::Crop>(
                world.resource::<saladin_protocol::GameIndex>().0[&gid],
            );
            let stage = root
                .and_then(|e| world.get::<crate::render::sync::CropField>(e))
                .map(|c| c.stage);
            eprintln!(
                "FARMDEMO field {gid} biome {biome:?} remaining {rem} crop {crop:?} render_stage {stage:?} -> variant {idx} {name} animal_brain={has_brain} {carcass:?}"
            );
        }
    }
}

/// Screenshot harness only: drop the player into farm-siting mode so the soil
/// overlay is on when the shot is taken.
pub fn arm_farm_mode(mut mode: ResMut<crate::input::InputMode>) {
    if !matches!(*mode, crate::input::InputMode::Build(saladin_sim::BuildingKind::Farm)) {
        *mode = crate::input::InputMode::Build(saladin_sim::BuildingKind::Farm);
    }
}

/// Screenshot harness only: two armies, twelve tiles apart, Aggressive, left to
/// resolve. `SALADIN_SHOT_AT` picks the moment (contact lands around t=7).
pub fn auto_battle(world: &mut World, mut done: Local<bool>) {
    use saladin_protocol::{MatchId, NextEntityId, Owner, Pos, Unit};
    use saladin_sim::{Fx, UnitKind, V2, unit_def};
    if *done || world.resource::<Time>().elapsed_secs() < 3.0 {
        return;
    }
    *done = true;
    let Some(kp) = keep_pos(world, 1) else { return };
    {
        let exists = {
            let mut q = world.query::<&saladin_protocol::Player>();
            q.iter(world).any(|p| p.player_id == 2)
        };
        if !exists {
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                MatchId(1),
                saladin_protocol::Player {
                    player_id: 2,
                    name: "Foe".into(),
                    faction: saladin_sim::Faction::Crusader,
                    stock: saladin_sim::Stockpile::default(),
                    color: 1,
                    online: true,
                    keep: 0,
                    defeated: false,
                    slot: 1,
                    tech_mask: 0,
                    hunger: 0,
                },
            ));
        }
    }
    let mine = [
        UnitKind::Spearman,
        UnitKind::Spearman,
        UnitKind::Spearman,
        UnitKind::Archer,
        UnitKind::Archer,
        UnitKind::Mamluk,
    ];
    let theirs = [
        UnitKind::Spearman,
        UnitKind::Spearman,
        UnitKind::Crossbowman,
        UnitKind::Crossbowman,
        UnitKind::Knight,
        UnitKind::Knight,
    ];
    let place = |world: &mut World, owner: u64, kind: UnitKind, x: Fx, y: Fx| {
        let def = unit_def(kind);
        let pos = V2::new(x, y);
        let id = world.resource_mut::<NextEntityId>().alloc();
        world.spawn((
            GameId(id),
            Owner(owner),
            MatchId(1),
            Pos { pos, facing: Fx::ZERO },
            Unit {
                speed: def.speed,
                hp: def.max_hp,
                ..Unit::new(kind, pos)
            },
        ));
    };
    for row in 0..4 {
        for (i, k) in mine.iter().enumerate() {
            let x = kp.x + Fx::from_num(4 + i as i32);
            let y = kp.y + Fx::from_num(4 + row);
            place(world, 1, *k, x, y);
        }
        for (i, k) in theirs.iter().enumerate() {
            let x = kp.x + Fx::from_num(4 + i as i32);
            let y = kp.y + Fx::from_num(11 + row);
            place(world, 2, *k, x, y);
        }
    }
}

/// Supply harness: march a column PAST the supply radius and frame it. A
/// garrison draws nothing, so the only way to photograph the model at work is
/// to put men where the road costs something. `SALADIN_STARVE=1` empties the
/// larder, which turns the same shot from "here is the bill" into "here is what
/// happens when you cannot pay it".
pub fn auto_column_afield(world: &mut World, mut done: Local<bool>) {
    use saladin_protocol::{MatchId, NextEntityId, Owner, Player, Pos, Unit};
    use saladin_sim::{Fx, SUPPLY_RADIUS, UnitKind, V2, unit_def};
    if *done || world.resource::<Time>().elapsed_secs() < 2.0 {
        return;
    }
    let Some(kp) = keep_pos(world, 1) else { return };
    *done = true;
    // a full ration-length past the line, so every man is well out of supply
    let out = SUPPLY_RADIUS.to_num::<f32>() + 12.0;
    let column = [
        UnitKind::Spearman,
        UnitKind::Spearman,
        UnitKind::Spearman,
        UnitKind::Archer,
        UnitKind::Archer,
        UnitKind::Naffatun,
    ];
    for row in 0..2 {
        for (i, k) in column.iter().enumerate() {
            let def = unit_def(*k);
            let pos = V2::new(
                kp.x + Fx::from_num(out as i32 + i as i32),
                kp.y + Fx::from_num(row * 2),
            );
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(1),
                MatchId(1),
                Pos { pos, facing: Fx::ZERO },
                Unit { speed: def.speed, hp: def.max_hp, ..Unit::new(*k, pos) },
            ));
        }
    }
    let starve = std::env::var("SALADIN_STARVE").is_ok();
    {
        let mut q = world.query::<&mut Player>();
        for mut p in q.iter_mut(world) {
            if p.player_id == 1 {
                p.stock.food = if starve { 0 } else { 640 };
            }
        }
    }
    let mut cam = world.resource_mut::<crate::camera::CameraState>();
    cam.target_center =
        bevy::prelude::Vec3::new(kp.x.to_num::<f32>() + out + 3.0, 0.0, kp.y.to_num::<f32>() + 1.0);
    cam.framed = true;
}

/// Screenshot harness only: select every own field soldier, so a shot captures
/// the unit command card (stances, orders, marching order, rations) instead of
/// the no-selection help text.
pub fn auto_select_units(
    local: Res<LocalPlayer>,
    mut sel: ResMut<selection::Selection>,
    q: Query<(&GameId, &Owner, &saladin_protocol::Unit)>,
) {
    let want: Vec<u64> = q
        .iter()
        .filter(|(_, o, u)| {
            o.0 == local.0 && u.garrisoned_in == 0 && saladin_sim::unit_def(u.kind).attack > 0
        })
        .map(|(g, ..)| g.0)
        .collect();
    if want.len() != sel.len() {
        sel.set(want);
    }
}

/// Screenshot harness only: conjure a building row beside the keep (the way
/// tests spawn rows) and select it, so SALADIN_AUTO=research/market captures
/// that building's panel without playing 10 minutes of economy.
pub fn auto_select_building(world: &mut World) {
    use saladin_protocol::{Building, MatchId, NextEntityId, Owner, Pos};
    use saladin_sim::{BuildingKind, building_def};
    let mode = std::env::var("SALADIN_AUTO");
    let kind = match mode.as_deref() {
        Ok("market") => BuildingKind::Market,
        Ok("keep") => BuildingKind::Keep,
        Ok("hut") => BuildingKind::FishingHut,
        Ok("harbour") => BuildingKind::Harbour,
        Ok("granary") => BuildingKind::Granary,
        Ok("store") => BuildingKind::Storehouse,
        Ok("mosque") => BuildingKind::Mosque,
        Ok("tower") => BuildingKind::Tower,
        Ok("house") => BuildingKind::House,
        Ok("site") => BuildingKind::Barracks,
        Ok("barracks") => BuildingKind::Barracks,
        Ok("stable") => BuildingKind::Stable,
        _ => BuildingKind::Blacksmith,
    };
    let as_site = mode.as_deref() == Ok("site");
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
            let def = building_def(kind);
            let mut b = if as_site {
                Building::site(kind, def.max_hp, pos)
            } else {
                Building::new(kind, def.max_hp, pos)
            };
            if as_site {
                b.work = saladin_sim::fx!("0.45");
                b.builders = 2;
                b.hp = (def.max_hp * 45 / 100).max(1);
            }
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(1),
                MatchId(1),
                Pos { pos, facing: saladin_sim::Fx::ZERO },
                b,
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
    hud_rects: Res<ui::hud::HudRects>,
    mut done: Local<bool>,
    q_bar: Query<(&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform), With<ui::hud::BottomCenter>>,
    q_card: Query<(&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform), With<ui::hud::BottomLeft>>,
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
    for r in &hud_rects.0 {
        eprintln!("HUDRECT {:?} .. {:?}", r.min, r.max);
    }
    for (n, t) in &q_card {
        eprintln!(
            "CARD size={:?} pos={:?} content={:?} pad={:?}",
            n.size(), t.translation, n.content_size(), n.padding()
        );
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

/// Naval harness: put a hull at the nearest berth to the keep, a party on the
/// sand beside it, a skiff over the nearest school, and frame the anchorage.
/// A crossing is a thing you watch happen; this is the still that shows what is
/// crossing.
pub fn auto_ferry(world: &mut World, mut done: Local<bool>) {
    use saladin_protocol::{MatchId, NextEntityId, Owner, Pos, Unit};
    use saladin_sim::{Fx, UnitKind, V2, WORLD_SIZE, is_passable, is_sailable};
    if *done || world.resource::<Time>().elapsed_secs() < 3.0 {
        return;
    }
    let Some(kp) = keep_pos(world, 1) else { return };
    *done = true;
    let seed = world.resource::<saladin_protocol::WorldConfig>().seed;
    let centre = |tx: i32, ty: i32| {
        V2::new(Fx::from_num(tx) + saladin_sim::fx!("0.5"), Fx::from_num(ty) + saladin_sim::fx!("0.5"))
    };
    // the berth nearest the keep that has dry land right beside it
    let (kx, ky) = (kp.x.to_num::<i32>(), kp.y.to_num::<i32>());
    let mut anchorage = None;
    'ring: for r in 1..60i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let (tx, ty) = (kx + dx, ky + dy);
                if tx < 1 || ty < 1 || tx >= WORLD_SIZE - 1 || ty >= WORLD_SIZE - 1 {
                    continue;
                }
                if !is_sailable(seed, tx, ty) {
                    continue;
                }
                for (ax, ay) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if is_passable(seed, tx + ax, ty + ay) {
                        anchorage = Some((centre(tx, ty), centre(tx + ax, ty + ay)));
                        break 'ring;
                    }
                }
            }
        }
    }
    let Some((berth, beach)) = anchorage else { return };

    let put = |world: &mut World, kind: UnitKind, pos: V2, aboard: u64| -> u64 {
        let id = world.resource_mut::<NextEntityId>().alloc();
        let mut u = Unit::new(kind, pos);
        u.garrisoned_in = aboard;
        world.spawn((GameId(id), Owner(1), MatchId(1), Pos { pos, facing: Fx::ZERO }, u));
        id
    };
    // The barge stands OFF the beach and is coming in — a hull tied up under a
    // keep is hidden by the keep from an iso camera, and a landing is the shot.
    // Open water near the landing: of every sailable tile within 9 of the
    // berth, the one with the most sea around it (ties to the farthest out).
    // Stepping "away from the beach" along one axis strands the hull behind the
    // keep the moment the coast does not run square to the grid.
    let offing = {
        let (bx, by) = (berth.x.to_num::<i32>(), berth.y.to_num::<i32>());
        let mut best = (-1i32, 0i32, berth);
        for dy in -9..=9i32 {
            for dx in -9..=9i32 {
                let (tx, ty) = (bx + dx, by + dy);
                if !is_sailable(seed, tx, ty) {
                    continue;
                }
                let mut open = 0;
                for oy in -2..=2i32 {
                    for ox in -2..=2i32 {
                        open += is_sailable(seed, tx + ox, ty + oy) as i32;
                    }
                }
                let far = dx * dx + dy * dy;
                if (open, far) > (best.0, best.1) {
                    best = (open, far, centre(tx, ty));
                }
            }
        }
        best.2
    };
    let barge = put(world, UnitKind::Barge, offing, 0);
    {
        let mut q = world.query::<(&GameId, &mut Unit)>();
        for (g, mut u) in q.iter_mut(world) {
            if g.0 == barge {
                u.target = berth;
                u.has_target = true;
            }
        }
    }
    for i in 0..3 {
        let at = V2::new(beach.x + Fx::from_num(i), beach.y);
        put(world, UnitKind::Spearman, at, 0);
    }
    for _ in 0..2 {
        put(world, UnitKind::Spearman, offing, barge);
    }
    // a skiff over the nearest school, which is what the sea is FOR most of the
    // time
    let school = {
        let mut q = world.query::<(&Pos, &saladin_protocol::ResourceNode)>();
        q.iter(world)
            .filter(|(p, n)| {
                n.res_type == saladin_sim::ResourceType::Food
                    && is_sailable(seed, p.pos.x.to_num::<i32>(), p.pos.y.to_num::<i32>())
            })
            .map(|(p, _)| p.pos)
            .min_by_key(|p| saladin_sim::dist2(*p, kp).to_bits())
    };
    let skiff = put(world, UnitKind::FishingSkiff, school.unwrap_or(berth), 0);
    // put the skiff under way down the coast: a wake is only a wake on a boat
    // that is going somewhere, and it is the one thing a still cannot fake
    {
        // the longest UNBROKEN sailable run off one of the four axes — the
        // harness walks a straight line, so a destination it cannot swim to in
        // a straight line beaches the boat
        let at = school.unwrap_or(berth);
        let (ax, ay) = (at.x.to_num::<i32>(), at.y.to_num::<i32>());
        let mut best: Option<(i32, V2)> = None;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut r = 1;
            while r < 26 && is_sailable(seed, ax + dx * r, ay + dy * r) {
                r += 1;
            }
            let run = r - 1;
            if run >= 3 && best.is_none_or(|(b, _)| run > b) {
                best = Some((run, centre(ax + dx * run, ay + dy * run)));
            }
        }
        if let Some((_, dest)) = best {
            let mut q = world.query::<(&GameId, &mut Unit)>();
            for (g, mut u) in q.iter_mut(world) {
                if g.0 == skiff {
                    u.target = dest;
                    u.has_target = true;
                }
            }
        }
    }

    // A quay on the sand beside the anchorage. The Harbour is the one structure
    // this harness had no way to show — `auto_select_building` plants its subject
    // four tiles off the keep, which is grass, and a dock inland is not a dock.
    {
        use saladin_sim::{BuildingKind, building_def, check_place, is_buildable_tile};
        let (bx, by) = (beach.x.to_num::<i32>(), beach.y.to_num::<i32>());
        let free = |_: i32, _: i32| false;
        let mut quay = None;
        'q: for r in 0..8i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let c = saladin_sim::footprint_center(
                        2,
                        Fx::from_num(bx + dx),
                        Fx::from_num(by + dy),
                    );
                    if is_buildable_tile(seed, bx + dx, by + dy)
                        && check_place(seed, BuildingKind::Harbour, c.x, c.y, free, &[]) == Ok(())
                    {
                        quay = Some(c);
                        break 'q;
                    }
                }
            }
        }
        if let Some(at) = quay {
            let def = building_def(BuildingKind::Harbour);
            let id = world.resource_mut::<NextEntityId>().alloc();
            world.spawn((
                GameId(id),
                Owner(1),
                MatchId(1),
                Pos { pos: at, facing: Fx::ZERO },
                saladin_protocol::Building::new(BuildingKind::Harbour, def.max_hp, at),
            ));
        }
    }
    // Select the laden hull so the shot also carries its command card.
    world.resource_mut::<selection::Selection>().set(vec![barge]);

    // Frame the whole landing: half way between where she stands off and the
    // sand she is running for.
    let (cx, cz) = (
        (offing.x + beach.x).to_num::<f32>() * 0.5,
        (offing.y + beach.y).to_num::<f32>() * 0.5,
    );
    let y = world
        .get_resource::<crate::terrain::HeightField>()
        .map(|f| crate::terrain::height_at(f, cx, cz))
        .unwrap_or(0.0);
    let mut cam = world.resource_mut::<crate::camera::CameraState>();
    // target only: the glide system re-aims the transform when center differs,
    // and `framed` beats `frame_keep` to the punch
    cam.target_center = bevy::prelude::Vec3::new(cx, y, cz);
    cam.framed = true;
}
