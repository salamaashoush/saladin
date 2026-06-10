//! Reusable HUD widget builders on top of the baked UI art (`assets.rs`):
//! parchment panels, bronze image-buttons with optional pixel-art icons and
//! cost lines, framed ratio bars. Every interactive node carries a `UiAction`
//! (or screen-local action component) the central interaction systems
//! dispatch on; button states are ImageNode tints.

use super::actions::UiAction;
use super::assets::UiAssets;
use super::theme::*;
use crate::UiFont;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
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

// ── parchment panels ─────────────────────────────────────────────────────────

/// Sliced parchment background for a panel container.
pub fn panel_bg(assets: &UiAssets) -> ImageNode {
    ImageNode::new(assets.panel.clone()).with_mode(NodeImageMode::Sliced(UiAssets::panel_slicer()))
}

/// Darker variant (HUD bottom bars, toasts).
pub fn panel_bg_dark(assets: &UiAssets) -> ImageNode {
    ImageNode::new(assets.panel_dark.clone())
        .with_mode(NodeImageMode::Sliced(UiAssets::panel_slicer()))
}

/// Fullscreen leather backdrop. MUST be Stretch: the default Auto mode makes
/// the image impose its 1:1 aspect on the node, squaring the "fullscreen"
/// root and leaving bare side bars.
pub fn backdrop_bg(assets: &UiAssets) -> ImageNode {
    ImageNode::new(assets.backdrop.clone()).with_mode(NodeImageMode::Stretch)
}

// ── bronze buttons ───────────────────────────────────────────────────────────

/// Per-state ImageNode tints (the bronze plate texture is shared).
pub const TINT_NORMAL: Color = Color::srgb(0.88, 0.88, 0.88);
pub const TINT_HOVER: Color = Color::WHITE;
pub const TINT_PRESSED: Color = Color::srgb(0.62, 0.62, 0.62);
pub const TINT_DISABLED: Color = Color::srgba(0.42, 0.42, 0.46, 0.85);
pub const TINT_ACTIVE: Color = Color::srgb(1.0, 0.92, 0.55);
pub const TINT_RED: Color = Color::srgb(1.0, 0.55, 0.45);
pub const TINT_GREEN: Color = Color::srgb(0.65, 1.0, 0.6);

/// Hover/press feedback restores to this when the cursor leaves.
#[derive(Component, Clone, Copy)]
pub struct BtnTint(pub Color);

/// Marker so the interaction system skips disabled buttons (visual dimming is
/// baked at build time).
#[derive(Component, Clone, Copy)]
pub struct Disabled(pub bool);

pub struct BtnStyle {
    pub tint: Color,
    pub disabled: bool,
    pub active: bool,
    pub min_width: f32,
    /// Uniform card height keeps icon and text-only buttons aligned in a row.
    pub min_height: f32,
    pub icon: Option<Handle<Image>>,
}

impl Default for BtnStyle {
    fn default() -> Self {
        BtnStyle {
            tint: TINT_NORMAL,
            disabled: false,
            active: false,
            min_width: 86.0,
            min_height: 66.0,
            icon: None,
        }
    }
}

impl BtnStyle {
    /// Compact text chip (tab rows, inline toggles).
    pub fn chip() -> Self {
        BtnStyle { min_width: 64.0, min_height: 30.0, ..default() }
    }
}

fn resolved_tint(style: &BtnStyle) -> Color {
    if style.disabled {
        TINT_DISABLED
    } else if style.active {
        TINT_ACTIVE
    } else {
        style.tint
    }
}

fn button_image(assets: &UiAssets, tint: Color) -> ImageNode {
    ImageNode::new(assets.button.clone())
        .with_mode(NodeImageMode::Sliced(UiAssets::button_slicer()))
        .with_color(tint)
}

/// A tool button: optional icon, label, optional cost/sub line.
pub fn tool_button(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: UiAction,
    title: &str,
    sub: Option<String>,
    style: BtnStyle,
) {
    let tint = resolved_tint(&style);
    let text_color = if style.disabled { TEXT_DIM } else { TEXT };
    let mut e = p.spawn((
        Button,
        action,
        button_image(assets, tint),
        BtnTint(tint),
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            min_width: Val::Px(style.min_width),
            min_height: Val::Px(style.min_height),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            margin: UiRect::all(Val::Px(2.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
        Disabled(style.disabled),
    ));
    e.with_children(|p| {
        if let Some(icon) = &style.icon {
            p.spawn((
                Node { width: Val::Px(28.0), height: Val::Px(28.0), ..default() },
                icon_image(icon.clone(), style.disabled),
            ));
        }
        label(p, font, title, FONT_SM, text_color);
        if let Some(sub) = sub {
            label(p, font, &sub, 10.0, if style.disabled { TEXT_DIM } else { GOLD });
        }
    });
}

fn icon_image(handle: Handle<Image>, disabled: bool) -> ImageNode {
    let mut img = ImageNode::new(handle);
    // nearest-neighbour look comes from the texture itself; dim when disabled
    if disabled {
        img = img.with_color(Color::srgba(0.6, 0.6, 0.6, 0.7));
    }
    img
}

/// A generic bronze button for menu/lobby/pause screens — same look as
/// `tool_button` but carries the screen's own action component.
pub fn screen_button<C: Component>(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: C,
    title: &str,
    active: bool,
    disabled: bool,
) {
    screen_button_sized(p, font, assets, action, title, active, disabled, None);
}

/// Compact fixed-width toggle (option rows: factions, difficulties, presets).
pub fn option_button<C: Component>(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: C,
    title: &str,
    active: bool,
    width: f32,
) {
    let style = BtnStyle { active, disabled: false, ..default() };
    let tint = resolved_tint(&style);
    p.spawn((
        Button,
        action,
        button_image(assets, tint),
        BtnTint(tint),
        Disabled(false),
        Node {
            width: Val::Px(width),
            min_height: Val::Px(32.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    ))
    .with_children(|p| label(p, font, title, FONT_SM, TEXT));
}

/// Fixed-width variant: stacked menu lists read better when every button is
/// the same width.
pub fn wide_button<C: Component>(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: C,
    title: &str,
    active: bool,
    disabled: bool,
) {
    screen_button_sized(p, font, assets, action, title, active, disabled, Some(230.0));
}

#[allow(clippy::too_many_arguments)]
fn screen_button_sized<C: Component>(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    action: C,
    title: &str,
    active: bool,
    disabled: bool,
    width: Option<f32>,
) {
    let style = BtnStyle { active, disabled, ..default() };
    let tint = resolved_tint(&style);
    p.spawn((
        Button,
        action,
        button_image(assets, tint),
        BtnTint(tint),
        Disabled(disabled),
        Node {
            padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
            margin: UiRect::all(Val::Px(3.0)),
            min_width: Val::Px(72.0),
            min_height: Val::Px(38.0),
            width: width.map(Val::Px).unwrap_or(Val::Auto),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    ))
    .with_children(|p| label(p, font, title, FONT_MD, if disabled { TEXT_DIM } else { TEXT }));
}

/// Hover/press tinting for ALL image buttons (HUD + menus).
pub fn button_feedback(
    mut q: Query<(&Interaction, &Disabled, &BtnTint, &mut ImageNode), Changed<Interaction>>,
) {
    for (i, disabled, base, mut img) in &mut q {
        if disabled.0 {
            continue;
        }
        img.color = match i {
            Interaction::Hovered => TINT_HOVER,
            Interaction::Pressed => TINT_PRESSED,
            Interaction::None => base.0,
        };
    }
}

// ── bars ─────────────────────────────────────────────────────────────────────

/// A horizontal ratio bar (HP / morale / research progress) in a bronze frame.
pub fn ratio_bar(
    p: &mut ChildSpawnerCommands,
    assets: &UiAssets,
    width: f32,
    ratio: f32,
    color: Color,
) {
    p.spawn((
        Node {
            width: Val::Px(width),
            height: Val::Px(9.0),
            margin: UiRect::vertical(Val::Px(2.0)),
            padding: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        ImageNode::new(assets.bar_frame.clone())
            .with_mode(NodeImageMode::Sliced(UiAssets::bar_slicer())),
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
