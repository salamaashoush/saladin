//! Design tokens for the HUD (port of the CSS custom properties).

use bevy::prelude::*;

pub const PANEL_BORDER: Color = Color::srgba(0.55, 0.45, 0.25, 0.6);
pub const TEXT: Color = Color::srgb(0.92, 0.88, 0.78);
pub const TEXT_DIM: Color = Color::srgb(0.62, 0.58, 0.48);
pub const ACCENT: Color = Color::srgb(0.61, 0.94, 0.42);
pub const WARN: Color = Color::srgb(0.88, 0.42, 0.29);
pub const GOLD: Color = Color::srgb(1.0, 0.82, 0.29);
pub const HP_GREEN: Color = Color::srgb(0.36, 0.54, 0.23);
pub const HP_YELLOW: Color = Color::srgb(0.79, 0.64, 0.15);
pub const HP_RED: Color = Color::srgb(0.71, 0.25, 0.18);
pub const MORALE_BLUE: Color = Color::srgb(0.25, 0.45, 0.77);

pub const FONT_SM: f32 = 11.0;
pub const FONT_MD: f32 = 13.0;
pub const FONT_LG: f32 = 18.0;

pub fn hp_color(ratio: f32) -> Color {
    if ratio > 0.5 {
        HP_GREEN
    } else if ratio > 0.25 {
        HP_YELLOW
    } else {
        HP_RED
    }
}

pub fn morale_color(ratio: f32) -> Color {
    if ratio > 0.5 {
        MORALE_BLUE
    } else if ratio > 0.25 {
        HP_YELLOW
    } else {
        HP_RED
    }
}
