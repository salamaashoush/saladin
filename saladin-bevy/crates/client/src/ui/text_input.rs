//! Reusable single-line text input for bevy UI: click to focus, typed
//! characters via `KeyboardInput` (layout-aware `text`), backspace, blinking
//! cursor, placeholder. One input is focused at a time; Escape/Enter blur.

use super::theme::*;
use crate::UiFont;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

#[derive(Component)]
pub struct TextInput {
    pub value: String,
    pub placeholder: String,
    pub max_len: usize,
    pub focused: bool,
    /// Filter applied per typed char (e.g. room codes take alphanumerics only).
    pub filter: fn(char) -> bool,
}

impl TextInput {
    pub fn new(value: &str, placeholder: &str, max_len: usize) -> Self {
        TextInput {
            value: value.into(),
            placeholder: placeholder.into(),
            max_len,
            focused: false,
            filter: |c| !c.is_control(),
        }
    }
    pub fn with_filter(mut self, filter: fn(char) -> bool) -> Self {
        self.filter = filter;
        self
    }
}

/// The text node inside the input box (kept in sync by `render_text_inputs`).
#[derive(Component)]
pub struct TextInputDisplay;

/// Blink phase for the cursor (shared by all inputs; cheap).
#[derive(Resource, Default)]
pub struct CursorBlink(pub f32);

/// Spawn an input box. The caller tags the returned entity (e.g. with an id
/// component) to find its value later.
pub fn text_input(
    p: &mut ChildSpawnerCommands,
    font: &UiFont,
    input: TextInput,
    width: f32,
) -> Entity {
    p.spawn((
        Button,
        input,
        Node {
            width: Val::Px(width),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            align_items: AlignItems::Center,
            overflow: Overflow::clip_x(),
            ..default()
        },
        BackgroundColor(Color::srgba(0.04, 0.04, 0.05, 0.95)),
        BorderColor::all(PANEL_BORDER),
    ))
    .with_children(|p| {
        p.spawn((
            TextInputDisplay,
            Text::new(""),
            TextFont {
                font: font.0.clone().into(),
                font_size: FontSize::Px(FONT_MD),
                font_smoothing: bevy::text::FontSmoothing::None,
                ..default()
            },
            TextColor(TEXT),
            bevy::text::LineHeight::RelativeToFont(1.3),
        ));
    })
    .id()
}

/// Click focuses the clicked input and blurs the rest.
pub fn focus_text_inputs(mut q: Query<(Entity, &Interaction, &mut TextInput)>) {
    let Some(target) =
        q.iter().find(|(_, i, _)| **i == Interaction::Pressed).map(|(e, _, _)| e)
    else {
        return;
    };
    for (e, _, mut t) in q.iter_mut() {
        let want = e == target;
        if t.focused != want {
            t.focused = want;
        }
    }
}

/// Route key presses into the focused input.
pub fn type_into_inputs(mut keys: MessageReader<KeyboardInput>, mut q: Query<&mut TextInput>) {
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        for mut input in q.iter_mut() {
            if !input.focused {
                continue;
            }
            match &ev.logical_key {
                Key::Backspace => {
                    input.value.pop();
                }
                Key::Escape | Key::Enter => {
                    input.focused = false;
                }
                _ => {
                    if let Some(text) = &ev.text {
                        for c in text.chars() {
                            if !c.is_control() && (input.filter)(c) && input.value.len() < input.max_len {
                                input.value.push(c);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Repaint value/placeholder/cursor + focus border.
pub fn render_text_inputs(
    time: Res<Time>,
    mut blink: ResMut<CursorBlink>,
    q_inputs: Query<(&TextInput, &Children, Entity)>,
    mut q_border: Query<&mut BorderColor>,
    mut q_text: Query<(&mut Text, &mut TextColor), With<TextInputDisplay>>,
) {
    blink.0 += time.delta_secs();
    let cursor_on = (blink.0 * 2.0) as u32 % 2 == 0;
    for (input, children, entity) in &q_inputs {
        for child in children.iter() {
            let Ok((mut text, mut color)) = q_text.get_mut(child) else { continue };
            let (content, c) = if input.value.is_empty() && !input.focused {
                (input.placeholder.clone(), TEXT_DIM)
            } else {
                let cursor = if input.focused && cursor_on { "_" } else { " " };
                (format!("{}{}", input.value, cursor), TEXT)
            };
            if text.0 != content {
                text.0 = content;
            }
            color.0 = c;
        }
        if let Ok(mut border) = q_border.get_mut(entity) {
            *border = BorderColor::all(if input.focused { GOLD } else { PANEL_BORDER });
        }
    }
}

/// While any input is focused, gameplay/menu hotkeys must not fire.
pub fn any_input_focused(q: Query<&TextInput>) -> bool {
    q.iter().any(|t| t.focused)
}
