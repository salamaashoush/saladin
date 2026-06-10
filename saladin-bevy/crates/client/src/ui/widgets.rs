//! Reusable HUD widget builders: panels, labels, action buttons with cost
//! lines, progress bars. Every interactive node carries a `UiAction` the
//! central interaction system dispatches on.

use super::actions::UiAction;
use super::theme::*;
use crate::UiFont;
use bevy::prelude::*;
use saladin_sim::ResourceCost;

/// Every font size the HUD/menus use — the atlas pre-warm rasterizes the whole
/// charset at each of these once at startup, so the glyph atlas never grows
/// (and never copy-resizes) mid-game. Mid-run atlas resizes corrupt glyphs on
/// this driver, which is what shredded the text.
pub const ALL_FONT_SIZES: [f32; 7] = [10.0, 11.0, 12.0, 13.0, 18.0, 30.0, 48.0];

/// One hidden text node per size, containing all printable ASCII.
pub fn prewarm_font_atlas(mut commands: Commands, font: Res<UiFont>) {
    let charset: String = (32u8..127).map(|c| c as char).collect();
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(-4000.0), ..default() },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            for size in ALL_FONT_SIZES {
                p.spawn((
                    Text::new(charset.clone()),
                    TextFont { font: font.0.clone().into(), font_size: FontSize::Px(size), font_smoothing: bevy::text::FontSmoothing::None, ..default() },
                ));
            }
        });
}

pub fn label(p: &mut ChildSpawnerCommands, font: &UiFont, text: &str, size: f32, color: Color) {
    p.spawn((
        Text::new(text),
        TextFont { font: font.0.clone().into(), font_size: FontSize::Px(size), font_smoothing: bevy::text::FontSmoothing::None, ..default() },
        TextColor(color),
        bevy::text::LineHeight::RelativeToFont(1.3),
    ));
}

/// "12 Wood, 5 Stone" cost line for a button.
pub fn cost_line(cost: &ResourceCost) -> String {
    let mut parts = Vec::new();
    for (amt, name) in [
        (cost.wood, "W"),
        (cost.stone, "S"),
        (cost.food, "F"),
        (cost.gold, "G"),
    ] {
        if amt > 0 {
            parts.push(format!("{amt}{name}"));
        }
    }
    parts.join(" ")
}

pub struct BtnStyle {
    pub bg: Color,
    pub disabled: bool,
    pub active: bool,
    pub min_width: f32,
}

impl Default for BtnStyle {
    fn default() -> Self {
        BtnStyle { bg: BTN_BG, disabled: false, active: false, min_width: 46.0 }
    }
}

/// A tool button: label on top, optional cost/sub line under it.
pub fn tool_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    action: UiAction,
    title: &str,
    sub: Option<String>,
    style: BtnStyle,
) {
    let bg = if style.disabled {
        BTN_BG_DISABLED
    } else if style.active {
        BTN_BG_ACTIVE
    } else {
        style.bg
    };
    let text_color = if style.disabled { TEXT_DIM } else { TEXT };
    let mut e = p.spawn((
        Button,
        action,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            min_width: Val::Px(style.min_width),
            padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
            margin: UiRect::all(Val::Px(1.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(if style.active { ACCENT } else { PANEL_BORDER }),
        Disabled(style.disabled),
    ));
    e.with_children(|p| {
        label(p, font, title, FONT_SM, text_color);
        if let Some(sub) = sub {
            label(p, font, &sub, 10.0, if style.disabled { TEXT_DIM } else { GOLD });
        }
    });
}

/// Marker so the interaction system skips disabled buttons (visual dimming is
/// baked at build time).
#[derive(Component, Clone, Copy)]
pub struct Disabled(pub bool);

/// A horizontal ratio bar (HP / morale / research progress).
pub fn ratio_bar(p: &mut ChildSpawnerCommands, width: f32, ratio: f32, color: Color) {
    p.spawn((
        Node {
            width: Val::Px(width),
            height: Val::Px(5.0),
            margin: UiRect::vertical(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
    ))
    .with_children(|p| {
        p.spawn((
            Node {
                width: Val::Percent((ratio * 100.0).clamp(0.0, 100.0)),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(color),
        ));
    });
}
