//! F3-togglable perf overlay (port of the TS perf HUD): smoothed FPS + frame
//! time + entity tallies.

use crate::UiFont;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use saladin_protocol::{Building, ResourceNode, Unit};

#[derive(Component)]
pub struct PerfText;

#[derive(Resource, Default)]
pub struct PerfVisible(pub bool);

pub fn setup_perf(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn((
        PerfText,
        Text::new(""),
        TextFont { font: font.0.clone().into(), font_size: FontSize::Px(10.0), font_smoothing: bevy::text::FontSmoothing::None, ..default() },
        TextColor(Color::srgb(0.61, 0.94, 0.42)),
        bevy::text::LineHeight::RelativeToFont(1.35),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(26.0),
            left: Val::Px(6.0),
            ..default()
        },
        Visibility::Hidden,
        ZIndex(50),
    ));
}

pub fn update_perf(
    keys: Res<ButtonInput<KeyCode>>,
    diagnostics: Res<DiagnosticsStore>,
    mut visible: ResMut<PerfVisible>,
    q_units: Query<(), With<Unit>>,
    q_buildings: Query<(), With<Building>>,
    q_nodes: Query<(), With<ResourceNode>>,
    mut q: Query<(&mut Text, &mut Visibility), With<PerfText>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        visible.0 = !visible.0;
    }
    let Ok((mut text, mut vis)) = q.single_mut() else { return };
    *vis = if visible.0 { Visibility::Visible } else { Visibility::Hidden };
    if !visible.0 {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    **text = format!(
        "FPS {:.0}  ({:.2}ms)\nunits {}  bld {}  nodes {}",
        fps,
        ms,
        q_units.iter().count(),
        q_buildings.iter().count(),
        q_nodes.iter().count(),
    );
}
