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
/// (half the vertical world-units visible). Input mutates the `target_*`
/// fields; `smooth_camera` eases the live values toward them every frame so
/// pan/zoom glide instead of stepping.
#[derive(Resource)]
pub struct CameraState {
    pub center: Vec3,
    pub target_center: Vec3,
    pub view_size: f32,
    pub target_view: f32,
    /// Accumulating yaw in radians; Q/E step it by 90° and the rig glides.
    pub yaw: f32,
    pub target_yaw: f32,
    pub framed: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        let c = WORLD_SIZE as f32 / 2.0;
        CameraState {
            center: Vec3::new(c, 0.0, c),
            target_center: Vec3::new(c, 0.0, c),
            view_size: 22.0,
            target_view: 22.0,
            yaw: 0.0,
            target_yaw: 0.0,
            framed: false,
        }
    }
}

impl CameraState {
    /// Jump both live and target (teardown reset, initial framing).
    pub fn snap_center(&mut self, c: Vec3) {
        self.center = c;
        self.target_center = c;
    }

    /// The rig offset for the current zoom + yaw: a fixed iso ring rotated
    /// by yaw, with the camera dropping a little as you zoom in (subtle
    /// SC2-style dolly tilt — close feels closer, far feels flatter).
    pub fn rig_offset(&self) -> Vec3 {
        let t = ((self.view_size - 10.0) / 75.0).clamp(0.0, 1.0);
        let height = 48.0 + t * 14.0; // 48 close .. 62 far (56-ish at default)
        let dist = 59.4; // |(42, 42)| — the original horizontal reach
        let a = std::f32::consts::FRAC_PI_4 + self.yaw;
        Vec3::new(dist * a.cos(), height, dist * a.sin())
    }

    /// Camera-forward projected on the ground (pan basis), from yaw.
    pub fn forward_h(&self) -> Vec2 {
        let o = self.rig_offset();
        -Vec2::new(o.x, o.z).normalize()
    }
}

fn clamp_center(v: Vec3) -> Vec3 {
    let m = WORLD_SIZE as f32;
    Vec3::new(v.x.clamp(-10.0, m + 10.0), v.y, v.z.clamp(-10.0, m + 10.0))
}

/// Middle-mouse grab-pan: the ground point seized at press stays under the
/// cursor while dragging.
#[derive(Resource, Default)]
pub struct DragPan(pub Option<Vec3>);

/// Ease the live camera toward its targets and write transform + frustum.
pub fn smooth_camera(
    time: Res<Time>,
    mut state: ResMut<CameraState>,
    mut q: Query<(&mut Transform, &mut Projection), With<GameCamera>>,
) {
    let dt = time.delta_secs();
    let k_pan = 1.0 - (-12.0 * dt).exp();
    let k_zoom = 1.0 - (-10.0 * dt).exp();
    let k_rot = 1.0 - (-9.0 * dt).exp();
    let dc = state.target_center - state.center;
    let dv = state.target_view - state.view_size;
    let dy = state.target_yaw - state.yaw;
    if dc.length_squared() < 1e-6 && dv.abs() < 1e-4 && dy.abs() < 1e-5 {
        return;
    }
    state.center += dc * k_pan;
    state.view_size += dv * k_zoom;
    state.yaw += dy * k_rot;
    if dc.length_squared() < 4e-4 {
        state.center = state.target_center;
    }
    if dv.abs() < 1e-2 {
        state.view_size = state.target_view;
    }
    if dy.abs() < 1e-3 {
        state.yaw = state.target_yaw;
    }
    if let Ok((mut tf, mut proj)) = q.single_mut() {
        aim(&state, &mut tf);
        if let Projection::Orthographic(o) = &mut *proj {
            o.scaling_mode = ScalingMode::FixedVertical { viewport_height: state.view_size * 2.0 };
        }
    }
}

pub fn spawn_camera(world: &mut World) {
    let state = CameraState::default();
    let tf = Transform::from_translation(state.center + state.rig_offset()).looking_at(state.center, Vec3::Y);
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
    *tf = Transform::from_translation(state.center + state.rig_offset()).looking_at(state.center, Vec3::Y);
}

/// WASD/arrows + screen-edge pan, in iso screen space (TS panCamera mapping).
pub fn pan_camera(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    time: Res<Time>,
    user: Res<crate::config::UserConfig>,
    mut state: ResMut<CameraState>,
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
    // edge scroll (toggle in settings)
    if user.edge_scroll && let Ok(window) = windows.single() {
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
    // pan in SCREEN space whatever the yaw: right/forward from the rig
    let fwd = state.forward_h();
    let right = Vec2::new(-fwd.y, fwd.x);
    let m = (right * dx - fwd * dz) * state.view_size * 2.5 * time.delta_secs();
    let t = state.target_center + Vec3::new(m.x, 0.0, m.y);
    state.target_center = clamp_center(t);
}

/// Wheel zoom toward the CURSOR: the ground point under the mouse stays put
/// while the frustum resizes (clamped 10..85 world half-height).
pub fn zoom_camera(
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    field: Option<Res<HeightField>>,
    mut state: ResMut<CameraState>,
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
    let old = state.target_view;
    let new = (old + delta).clamp(10.0, 85.0);
    if new == old {
        return;
    }
    state.target_view = new;
    // keep the point under the cursor fixed: scale its offset from center
    if let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cam.single())
        && let Some(cursor) = window.cursor_position()
        && let Some(p) = pick_ground(camera, cam_tf, cursor, field.as_deref())
    {
        let k = new / old;
        let c = state.target_center;
        let t = Vec3::new(p.x + (c.x - p.x) * k, c.y, p.z + (c.z - p.z) * k);
        state.target_center = clamp_center(t);
    }
}

/// Middle-mouse grab-pan: drag the world; the seized point follows the cursor.
pub fn drag_pan(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    mut grab: ResMut<DragPan>,
    mut state: ResMut<CameraState>,
) {
    if !mouse.pressed(MouseButton::Middle) {
        grab.0 = None;
        return;
    }
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cam.single()) else { return };
    let Some(cursor) = window.cursor_position() else { return };
    // flat-plane ray hit at the grab height keeps the drag stable over hills
    let plane_y = grab.0.map(|g| g.y).unwrap_or(0.0);
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
    let d = ray.direction.as_vec3();
    if d.y.abs() < 1e-6 {
        return;
    }
    let t = (plane_y - ray.origin.y) / d.y;
    if t < 0.0 {
        return;
    }
    let hit = ray.origin + d * t;
    match grab.0 {
        None => grab.0 = Some(hit),
        Some(anchor) => {
            let delta = anchor - hit;
            let t = state.target_center + Vec3::new(delta.x, 0.0, delta.z);
            state.target_center = clamp_center(t);
            // the camera moves this frame, so re-anchor against the new view
        }
    }
}

/// Q/E rotate the view in 90-degree steps (the rig glides between them).
pub fn rotate_camera(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<CameraState>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        state.target_yaw += std::f32::consts::FRAC_PI_2;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        state.target_yaw -= std::f32::consts::FRAC_PI_2;
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
    state.snap_center(Vec3::new(fx, fy, fz));
    state.framed = true;
    if let Ok(mut tf) = cam.single_mut() {
        aim(&state, &mut tf);
    }
}

/// Move the camera focus (minimap click navigation).
pub fn focus_on(state: &mut CameraState, _tf: &mut Transform, x: f32, z: f32, field: Option<&HeightField>) {
    let y = field.map(|f| height_at(f, x, z)).unwrap_or(0.0);
    state.target_center = clamp_center(Vec3::new(x, y, z));
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
