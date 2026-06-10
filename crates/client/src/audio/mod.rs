//! Procedural audio: music, unit voices, ambience and combat SFX — every
//! sample synthesized at startup (`synth`), nothing on disk. Read-only over
//! render state; the sim never hears any of this.

pub mod music;
pub mod sfx;
pub mod synth;
pub mod voice;

use crate::GameState;
use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;

pub use voice::{Bark, VoiceQueue};

pub fn register(app: &mut App) {
    app.init_resource::<VoiceQueue>();
    app.add_systems(Startup, (music::bake_music, sfx::bake_sfx, voice::bake_voices, apply_volume));
    // music plays everywhere (menu + match); the world sounds only in-game
    app.add_systems(Update, music::sequence_music);
    app.add_systems(
        Update,
        (
            voice::play_voices,
            sfx::projectile_sfx,
            sfx::death_sfx,
            sfx::crowd_sfx,
            sfx::ambience,
            apply_volume,
        )
            .run_if(in_state(GameState::Playing)),
    );
    app.add_systems(OnExit(GameState::Playing), sfx::stop_ambience);
}

/// The settings-screen master volume, applied globally (cheap compare-set).
fn apply_volume(cfg: Res<crate::config::UserConfig>, mut vol: ResMut<GlobalVolume>) {
    let want = Volume::Linear(cfg.master_volume.clamp(0.0, 1.0));
    if vol.volume != want {
        vol.volume = want;
    }
}
