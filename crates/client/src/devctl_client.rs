//! The client half of devctl: publish the lockstep clock, hand injected
//! commands to `LocalInput` so they travel the same road as a click, and take
//! the screenshots the protocol crate has no renderer for.
//!
//! `step` is refused here. In a running client — single-player included — time
//! belongs to the fixed-update clock and, in a match, to the lockstep group; a
//! control channel that could advance one peer's ticks is a desync.

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use saladin_protocol::devctl::{self, CameraSpec, ShotJob};

use crate::camera::{self, CameraState, GameCamera};
use crate::terrain::{HeightField, height_at};
use crate::{LocalInput, Net};

/// Env: seconds of game time each rendered frame advances. With it set, the
/// whole app is a function of FRAME COUNT — the fixed-update sim runs exactly
/// one tick per frame at 0.05, and every pose in the renderer, which is wall
/// time and nothing else, lands on the same phase every run. Without it two
/// screenshots of one unchanged world are never the same image and a pixel
/// diff means nothing. Bevy's own `TimeUpdateStrategy` does the work.
pub const FIXED_DT_ENV: &str = "SALADIN_FIXED_DT";

pub fn register(app: &mut App) {
    if let Ok(dt) = std::env::var(FIXED_DT_ENV)
        && let Ok(secs) = dt.trim().parse::<f32>()
        && secs > 0.0
    {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(secs),
        ));
    }
    app.add_plugins(saladin_protocol::DevctlPlugin);
    if !app.world().contains_resource::<devctl::DevctlLink>() {
        return; // SALADIN_DEVCTL unset: the plugin added nothing
    }
    app.init_resource::<PendingShots>()
        .init_resource::<crate::render::inspect::FreezeAt>()
        .init_resource::<crate::render::inspect::FreezeDone>()
        .add_systems(
            Update,
            (
                crate::render::inspect::freeze_at_tick,
                publish_clock.before(devctl::serve),
                (route_commands, answer_asks, collect_shots, fire_shots)
                    .chain()
                    .after(devctl::serve),
            ),
        );
}

/// Where a command queued right now will land, and what this host can do.
fn publish_clock(net: Res<Net>, mut link: ResMut<devctl::DevctlLink>) {
    link.submit_tick = net.driver.tick + net.driver.delay;
    link.may_step = false;
    link.renders = true;
}

fn route_commands(world: &mut World) {
    let cmds = devctl::take_outbox(world);
    if !cmds.is_empty() {
        world.resource_mut::<LocalInput>().0.extend(cmds);
    }
}

/// Queries the protocol crate parked for whoever owns the renderer.
fn answer_asks(world: &mut World) {
    for ask in devctl::take_asks(world) {
        crate::render::inspect::answer(world, ask);
    }
}

// ── screenshots ──────────────────────────────────────────────────────────────

struct Pending {
    job: ShotJob,
    /// Frames the snapped camera needs to reach the render app before the
    /// shutter — one to be extracted, one for the capture scheduled against it.
    settle: u32,
}

#[derive(Resource, Default)]
struct PendingShots(Vec<Pending>);

const SETTLE_FRAMES: u32 = 2;

fn collect_shots(world: &mut World) {
    for job in devctl::take_shots(world) {
        if let Some(spec) = job.camera {
            aim_camera(world, &spec);
        }
        world.resource_mut::<PendingShots>().0.push(Pending { job, settle: SETTLE_FRAMES });
    }
}

fn aim_camera(world: &mut World, spec: &CameraSpec) {
    let ground = spec
        .pos
        .map(|(x, z)| world.get_resource::<HeightField>().map_or(0.0, |f| height_at(f, x, z)));
    world.resource_scope::<CameraState, _>(|world, mut state| {
        if let (Some((x, z)), Some(y)) = (spec.pos, ground) {
            state.target_center = Vec3::new(x, y, z);
            // beat frame_keep to it: an unframed camera re-centres on the keep
            state.framed = true;
        }
        if let Some(zoom) = spec.zoom {
            state.target_view = zoom.max(4.0);
        }
        if let Some(yaw) = spec.yaw {
            state.target_yaw = yaw as f32 * std::f32::consts::FRAC_PI_2;
        }
        let mut q = world.query_filtered::<(&mut Transform, &mut Projection), With<GameCamera>>();
        if let Ok((mut tf, mut proj)) = q.single_mut(world) {
            camera::snap_to_targets(&mut state, &mut tf, &mut proj);
        }
    });
}

fn fire_shots(mut commands: Commands, mut pending: ResMut<PendingShots>) {
    let mut waiting = Vec::new();
    for mut p in std::mem::take(&mut pending.0) {
        if p.settle > 0 {
            p.settle -= 1;
            waiting.push(p);
            continue;
        }
        let path = p.job.path.clone();
        commands.spawn(Screenshot::primary_window()).observe(save_and_answer(path, p.job));
    }
    pending.0 = waiting;
}

/// Save the capture, THEN answer. The write is synchronous on native, so
/// completing the reply inside the observer is what makes "the file is on
/// disk" true by the time the caller reads the line — the race `shot.sh`
/// survives only by `rm -f`ing the stale file first.
fn save_and_answer(
    path: String,
    job: ShotJob,
) -> impl FnMut(On<ScreenshotCaptured>) + Send + Sync + 'static {
    let mut job = Some(job);
    move |captured: On<ScreenshotCaptured>| {
        let Some(job) = job.take() else { return };
        job.done(write_png(captured.image.clone(), &path).err());
    }
}

fn write_png(image: Image, path: &str) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let dynamic = image.try_into_dynamic().map_err(|e| format!("{e}"))?;
    // the alpha channel carries brightness under HDR; the RGB is the frame
    dynamic.to_rgb8().save(path).map_err(|e| format!("{path}: {e}"))
}
