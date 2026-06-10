//! Procedural audio: music, unit voices, ambience and combat SFX — every
//! sample synthesized at startup (`synth`), nothing on disk. Read-only over
//! render state; the sim never hears any of this.

pub mod music;
pub mod sfx;
pub mod synth;
pub mod voice;

use crate::GameState;
use bevy::audio::{AudioSink, Volume};
use bevy::prelude::*;

pub use voice::{Bark, VoiceQueue};

pub fn register(app: &mut App) {
    app.init_resource::<VoiceQueue>();
    app.add_systems(Startup, (music::bake_music, sfx::bake_sfx, voice::bake_voices));
    // music plays everywhere (menu + match); the world sounds only in-game
    app.add_systems(Update, (music::sequence_music, live_music_volume));
    app.add_systems(
        Update,
        (
            voice::play_voices,
            sfx::projectile_sfx,
            sfx::death_sfx,
            sfx::crowd_sfx,
            sfx::ambience,
        )
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(OnExit(GameState::Playing), sfx::stop_ambience);
}

/// Settings master volume, 0..1. Bevy's GlobalVolume only applies when a sink
/// SPAWNS, so every spawn multiplies this in and the persistent sinks retune
/// per frame — the settings buttons take effect immediately.
pub fn master(cfg: &crate::config::UserConfig) -> f32 {
    cfg.master_volume.clamp(0.0, 1.0)
}

/// Retune the currently-playing music phrase when the user moves the slider.
fn live_music_volume(
    cfg: Res<crate::config::UserConfig>,
    mut q: Query<&mut AudioSink, With<music::MusicPhrase>>,
) {
    if !cfg.is_changed() {
        return;
    }
    for mut sink in &mut q {
        sink.set_volume(Volume::Linear(music::MUSIC_GAIN * master(&cfg)));
    }
}
