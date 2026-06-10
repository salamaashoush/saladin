//! Projectile arcs (port of the SaladinGame `arrows` loop): each ranged strike
//! from the sim's ShotEvents spawns a short-lived arrow that flies a parabolic
//! arc from shooter to target over 0.22 s. Mangonel shots fly a slow boulder
//! on a high arc instead, and every projectile bursts into dust on impact.

use crate::render::sync::Particle;
use crate::terrain::{HeightField, height_at};
use bevy::mesh::Meshable;
use bevy::prelude::*;
use saladin_protocol::ShotEvents;

#[derive(Component)]
pub struct Arrow {
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
    pub stone: bool,
}

#[derive(Resource)]
pub struct ArrowAssets {
    pub mesh: Handle<Mesh>,
    pub stone: Handle<Mesh>,
    pub stone_mat: Handle<StandardMaterial>,
}

pub fn build_arrow_assets(
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
) -> ArrowAssets {
    ArrowAssets {
        mesh: meshes.add(Mesh::from(Cylinder::new(0.03, 0.55))),
        stone: meshes.add(Sphere::new(0.22).mesh().uv(8, 6)),
        stone_mat: mats.add(StandardMaterial {
            base_color: Color::srgb_u8(0x7a, 0x76, 0x70),
            perceptual_roughness: 0.95,
            ..default()
        }),
    }
}

/// Drain the sim's per-combat-tick shot list into projectile entities.
pub fn spawn_arrows(
    mut commands: Commands,
    mut shots: ResMut<ShotEvents>,
    assets: Res<ArrowAssets>,
    rmats: Res<crate::render::sync::RenderMaterials>,
) {
    for s in shots.0.drain(..) {
        let (mesh, mat) = if s.stone {
            (assets.stone.clone(), assets.stone_mat.clone())
        } else {
            (assets.mesh.clone(), rmats.arrow.clone())
        };
        commands.spawn((
            Arrow {
                from: Vec2::new(s.from.x.to_num(), s.from.y.to_num()),
                to: Vec2::new(s.to.x.to_num(), s.to.y.to_num()),
                t: 0.0,
                stone: s.stone,
            },
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::IDENTITY,
        ));
    }
}

pub fn fly_arrows(
    mut commands: Commands,
    time: Res<Time>,
    field: Res<HeightField>,
    assets: Res<crate::render::sync::RenderAssets>,
    rmats: Res<crate::render::sync::RenderMaterials>,
    mut q: Query<(Entity, &mut Arrow, &mut Transform)>,
) {
    for (e, mut a, mut tf) in &mut q {
        let (dur, peak) = if a.stone { (0.8, 3.2) } else { (0.22, 1.1) };
        a.t += time.delta_secs() / dur;
        if a.t >= 1.0 {
            // impact burst: dust ring for stones, a small puff for arrows
            let ty = height_at(&field, a.to.x, a.to.y);
            let n = if a.stone { 7 } else { 2 };
            let h01 = |x: f32| ((x * 43758.5453).sin() * 0.5 + 0.5).abs();
            for i in 0..n {
                let k = a.to.x * 3.7 + a.to.y * 5.1 + i as f32 * 1.618;
                let ang = h01(k) * std::f32::consts::TAU;
                let r = if a.stone { 0.5 } else { 0.2 };
                commands.spawn((
                    Particle {
                        vel: Vec3::new(ang.cos() * r * 1.4, 0.9 + h01(k + 1.0) * 0.6, ang.sin() * r * 1.4),
                        age: 0.0,
                        life: 0.35 + h01(k + 2.0) * 0.3,
                        base: if a.stone { 0.2 + h01(k + 3.0) * 0.14 } else { 0.1 },
                    },
                    Mesh3d(assets.puff.clone()),
                    MeshMaterial3d(rmats.smoke_light.clone()),
                    Transform::from_xyz(a.to.x + ang.cos() * r, ty + 0.15, a.to.y + ang.sin() * r)
                        .with_scale(Vec3::splat(0.01)),
                ));
            }
            commands.entity(e).despawn();
            continue;
        }
        let p = a.from.lerp(a.to, a.t);
        let arc = (a.t * std::f32::consts::PI).sin() * peak;
        let y = height_at(&field, p.x, p.y) + 0.6 + arc;
        let ty = height_at(&field, a.to.x, a.to.y) + 0.6;
        tf.translation = Vec3::new(p.x, y, p.y);
        if a.stone {
            // tumbling boulder
            tf.rotation = Quat::from_rotation_x(a.t * 7.0);
        } else {
            let dir = Vec3::new(a.to.x, ty, a.to.y) - tf.translation;
            if dir.length_squared() > 1e-6 {
                // a cylinder's long axis is Y: aim Y along the flight direction
                tf.rotation = Quat::from_rotation_arc(Vec3::Y, dir.normalize());
            }
        }
    }
}

/// Dust kicked up at a melee strike's apex — the same cycle math as the
/// animator's ArmR swing, so the puff lands on the beat of the chop.
pub fn melee_strike_dust(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<crate::render::sync::RenderAssets>,
    rmats: Res<crate::render::sync::RenderMaterials>,
    cam_state: Res<crate::camera::CameraState>,
    q: Query<(&crate::render::sync::AnimState, &Transform)>,
) {
    if cam_state.view_size >= 34.0 {
        return;
    }
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (anim, tf) in &q {
        if !anim.combat {
            continue;
        }
        let ranged = matches!(
            anim.kind,
            saladin_sim::UnitKind::Archer
                | saladin_sim::UnitKind::Crossbowman
                | saladin_sim::UnitKind::HorseArcher
                | saladin_sim::UnitKind::Mangonel
        );
        if ranged {
            continue;
        }
        // strike apex = sin(tp*4) peaking; fire once per swing cycle
        let tp = t + anim.phase;
        let cycles_now = ((tp * 4.0) / std::f32::consts::TAU + 0.25).floor();
        let cycles_prev = (((tp - dt) * 4.0) / std::f32::consts::TAU + 0.25).floor();
        if cycles_now <= cycles_prev {
            continue;
        }
        let fwd = tf.rotation * Vec3::Z;
        let at = tf.translation + fwd * 0.7;
        commands.spawn((
            Particle {
                vel: Vec3::new(fwd.x * 0.3, 0.7, fwd.z * 0.3),
                age: 0.0,
                life: 0.3,
                base: 0.12,
            },
            Mesh3d(assets.puff.clone()),
            MeshMaterial3d(rmats.smoke_light.clone()),
            Transform::from_translation(at + Vec3::Y * 0.3).with_scale(Vec3::splat(0.01)),
        ));
    }
}
