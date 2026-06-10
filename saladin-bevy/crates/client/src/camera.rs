//! Orthographic isometric RTS camera (port of src/game/camera.ts): fixed iso
//! offset, WASD/arrow + edge-scroll pan in iso screen space, wheel zoom by
//! resizing the ortho frustum, one-time framing on the player's keep.

use crate::terrain::{HeightField, height_at};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::camera::ScalingMode;
use crate::LocalPlayer;
use saladin_protocol::{Building, Owner, Pos};
use saladin_sim::{BuildingKind, WORLD_SIZE};

#[derive(Component)]
pub struct GameCamera;

/// The iso rig: a look-at `center` plus a fixed offset, zoomed by `view_size`
/// (half the vertical world-units visible).
#[derive(Resource)]
pub struct CameraState {
    pub center: Vec3,
    pub offset: Vec3,
    pub view_size: f32,
    pub framed: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        let c = WORLD_SIZE as f32 / 2.0;
        CameraState {
            center: Vec3::new(c, 0.0, c),
            offset: Vec3::new(42.0, 56.0, 42.0),
            view_size: 22.0,
            framed: false,
        }
    }
}

pub fn spawn_camera(world: &mut World) {
    let state = CameraState::default();
    let tf = Transform::from_translation(state.center + state.offset).looking_at(state.center, Vec3::Y);
    world.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: state.view_size * 2.0 },
            // near stays at 0: a negative near pulls geometry BEHIND the camera
            // (the endless ocean disc) into the depth range, painting over the map.
            far: 6000.0,
            ..OrthographicProjection::default_3d()
        }),
        tf,
        GameCamera,
        IsDefaultUiCamera,
        Msaa::Off,
    ));
    world.insert_resource(state);
}

fn aim(state: &CameraState, tf: &mut Transform) {
    *tf = Transform::from_translation(state.center + state.offset).looking_at(state.center, Vec3::Y);
}

/// WASD/arrows + screen-edge pan, in iso screen space (TS panCamera mapping).
pub fn pan_camera(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    time: Res<Time>,
    mut state: ResMut<CameraState>,
    mut q: Query<&mut Transform, With<GameCamera>>,
) {
    let mut dx = 0.0_f32;
    let mut dz = 0.0_f32;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dz -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dz += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dx -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dx += 1.0;
    }
    // edge scroll
    if let Ok(window) = windows.single() {
        if let Some(c) = window.cursor_position() {
            const EDGE: f32 = 10.0;
            if c.x <= EDGE {
                dx -= 1.0;
            }
            if c.x >= window.width() - EDGE {
                dx += 1.0;
            }
            if c.y <= EDGE {
                dz -= 1.0;
            }
            if c.y >= window.height() - EDGE {
                dz += 1.0;
            }
        }
    }
    if dx == 0.0 && dz == 0.0 {
        return;
    }
    let sp = state.view_size * 1.6 * time.delta_secs();
    state.center.x = (state.center.x + (dx + dz) * sp).clamp(-10.0, WORLD_SIZE as f32 + 10.0);
    state.center.z = (state.center.z + (dz - dx) * sp).clamp(-10.0, WORLD_SIZE as f32 + 10.0);
    if let Ok(mut tf) = q.single_mut() {
        aim(&state, &mut tf);
    }
}

/// Wheel zoom: resize the ortho frustum (clamped 10..85 world half-height).
pub fn zoom_camera(
    mut wheel: MessageReader<MouseWheel>,
    mut state: ResMut<CameraState>,
    mut q: Query<&mut Projection, With<GameCamera>>,
) {
    let mut delta = 0.0_f32;
    for e in wheel.read() {
        let step = match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y / 40.0,
        };
        delta -= step.signum() * 2.5;
    }
    if delta == 0.0 {
        return;
    }
    state.view_size = (state.view_size + delta).clamp(10.0, 85.0);
    if let Ok(mut proj) = q.single_mut() {
        if let Projection::Orthographic(o) = &mut *proj {
            o.scaling_mode = ScalingMode::FixedVertical { viewport_height: state.view_size * 2.0 };
        }
    }
}

/// One-time framing on the local player's keep when it first appears.
pub fn frame_keep(
    local: Res<LocalPlayer>,
    field: Option<Res<HeightField>>,
    mut state: ResMut<CameraState>,
    q: Query<(&Owner, &Pos, &Building)>,
    mut cam: Query<&mut Transform, With<GameCamera>>,
) {
    if state.framed {
        return;
    }
    let Some((_, pos, _)) =
        q.iter().find(|(o, _, b)| o.0 == local.0 && b.kind == BuildingKind::Keep)
    else {
        return;
    };
    let c = WORLD_SIZE as f32 / 2.0;
    let x = pos.pos.x.to_num::<f32>();
    let z = pos.pos.y.to_num::<f32>();
    let fx = x + (c - x).signum() * 5.0;
    let fz = z + (c - z).signum() * 5.0;
    let fy = field.map(|f| height_at(&f, fx, fz)).unwrap_or(0.0);
    state.center = Vec3::new(fx, fy, fz);
    state.framed = true;
    if let Ok(mut tf) = cam.single_mut() {
        aim(&state, &mut tf);
    }
}

/// Move the camera focus (minimap click navigation).
pub fn focus_on(state: &mut CameraState, tf: &mut Transform, x: f32, z: f32, field: Option<&HeightField>) {
    state.center.x = x.clamp(-10.0, WORLD_SIZE as f32 + 10.0);
    state.center.z = z.clamp(-10.0, WORLD_SIZE as f32 + 10.0);
    state.center.y = field.map(|f| height_at(f, x, z)).unwrap_or(0.0);
    aim(state, tf);
}

/// Ray-march the cursor ray against the height field for an accurate ground
/// pick on hills (the TS client raycast the terrain mesh).
pub fn pick_ground(
    camera: &Camera,
    cam_tf: &GlobalTransform,
    cursor: Vec2,
    field: Option<&HeightField>,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let o = ray.origin;
    let d = ray.direction.as_vec3();
    if let Some(field) = field {
        // coarse march, then a short bisection refine at the crossing
        let mut prev_t = 0.0_f32;
        let mut prev_above = true;
        let mut t = 0.0_f32;
        while t < 400.0 {
            let p = o + d * t;
            let above = p.y > height_at(field, p.x, p.z);
            if !above && prev_above && t > 0.0 {
                let (mut lo, mut hi) = (prev_t, t);
                for _ in 0..12 {
                    let mid = (lo + hi) / 2.0;
                    let pm = o + d * mid;
                    if pm.y > height_at(field, pm.x, pm.z) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let p = o + d * ((lo + hi) / 2.0);
                return Some(p);
            }
            prev_t = t;
            prev_above = above;
            t += 0.75;
        }
        return None;
    }
    // fallback: plane y=0
    if d.y.abs() < 1e-6 {
        return None;
    }
    let t = -o.y / d.y;
    (t >= 0.0).then(|| o + d * t)
}
