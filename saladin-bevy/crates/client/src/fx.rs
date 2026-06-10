//! Projectile arcs (port of the SaladinGame `arrows` loop): each ranged strike
//! from the sim's ShotEvents spawns a short-lived arrow that flies a parabolic
//! arc from shooter to target over 0.22 s.

use crate::terrain::{HeightField, height_at};
use bevy::prelude::*;
use saladin_protocol::ShotEvents;

#[derive(Component)]
pub struct Arrow {
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
}

#[derive(Resource)]
pub struct ArrowAssets {
    pub mesh: Handle<Mesh>,
}

pub fn build_arrow_assets(meshes: &mut Assets<Mesh>) -> ArrowAssets {
    ArrowAssets { mesh: meshes.add(Mesh::from(Cylinder::new(0.03, 0.55))) }
}

/// Drain the sim's per-combat-tick shot list into arrow entities.
pub fn spawn_arrows(
    mut commands: Commands,
    mut shots: ResMut<ShotEvents>,
    assets: Res<ArrowAssets>,
    rmats: Res<crate::render::sync::RenderMaterials>,
) {
    for s in shots.0.drain(..) {
        commands.spawn((
            Arrow {
                from: Vec2::new(s.from.x.to_num(), s.from.y.to_num()),
                to: Vec2::new(s.to.x.to_num(), s.to.y.to_num()),
                t: 0.0,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(rmats.arrow.clone()),
            Transform::IDENTITY,
        ));
    }
}

pub fn fly_arrows(
    mut commands: Commands,
    time: Res<Time>,
    field: Res<HeightField>,
    mut q: Query<(Entity, &mut Arrow, &mut Transform)>,
) {
    for (e, mut a, mut tf) in &mut q {
        a.t += time.delta_secs() / 0.22;
        if a.t >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        let p = a.from.lerp(a.to, a.t);
        let arc = (a.t * std::f32::consts::PI).sin() * 1.1;
        let y = height_at(&field, p.x, p.y) + 0.6 + arc;
        let ty = height_at(&field, a.to.x, a.to.y) + 0.6;
        tf.translation = Vec3::new(p.x, y, p.y);
        let dir = Vec3::new(a.to.x, ty, a.to.y) - tf.translation;
        if dir.length_squared() > 1e-6 {
            // a cylinder's long axis is Y: aim Y along the flight direction
            tf.rotation = Quat::from_rotation_arc(Vec3::Y, dir.normalize());
        }
    }
}
