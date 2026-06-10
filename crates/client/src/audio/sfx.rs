//! One-shot SFX + world ambience, all rendered at boot. Hooks are render-side
//! observations (new arrows, dying units, combat animation flags), never sim
//! state — audio can desync from nothing.

use super::synth::*;
use crate::camera::CameraState;
use crate::environment::ShoreList;
use crate::render::sync::{AnimState, Dying};
use bevy::audio::{AudioPlayer, AudioSink, AudioSource, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;

const SFX_GAIN: f32 = 0.65;
const AMBIENCE_GAIN: f32 = 0.16;
/// Sounds farther than this from the camera focus are skipped entirely.
const EARSHOT: f32 = 46.0;

#[derive(Resource)]
pub struct SfxBank {
    bow: Vec<Handle<AudioSource>>,
    siege_launch: Handle<AudioSource>,
    clash: Vec<Handle<AudioSource>>,
    chop: Vec<Handle<AudioSource>>,
    death: Vec<Handle<AudioSource>>,
    collapse: Handle<AudioSource>,
    bird: Vec<Handle<AudioSource>>,
    dove: Handle<AudioSource>,
    gull: Handle<AudioSource>,
    wind: Handle<AudioSource>,
    waves: Handle<AudioSource>,
}

// ── synthesis ────────────────────────────────────────────────────────────────

fn bow_shot(rng: &mut Rng32) -> Vec<f32> {
    // string twang + a short air whoosh
    let mut out = pluck(rng.range(520.0, 640.0), 0.18, 0.99, rng);
    for s in out.iter_mut() {
        *s *= 0.8;
    }
    let mut wh = noise(0.16, rng);
    bandpass(&mut wh, 1800.0, 1.2);
    for (i, s) in wh.iter_mut().enumerate() {
        let t = i as f32 / SRF;
        *s *= adsr(t, 0.16, 0.01, 0.1, 0.3, 0.05) * 0.5;
    }
    mix(&mut out, &wh, 0.01, 1.0);
    finalize(&mut out, 0.6);
    out
}

fn siege_launch(rng: &mut Rng32) -> Vec<f32> {
    // arm creak, heavy frame thunk, then air
    let mut out = render(0.5, |t| {
        let f = 70.0 - 50.0 * t;
        (t * f.max(28.0) * core::f32::consts::TAU).sin() * adsr(t, 0.5, 0.01, 0.3, 0.3, 0.15)
    });
    lowpass(&mut out, 300.0);
    let mut creak = noise(0.25, rng);
    bandpass(&mut creak, 420.0, 8.0);
    for (i, s) in creak.iter_mut().enumerate() {
        *s *= adsr(i as f32 / SRF, 0.25, 0.05, 0.1, 0.5, 0.08) * 0.5;
    }
    mix(&mut out, &creak, 0.0, 1.0);
    let mut wh = noise(0.3, rng);
    bandpass(&mut wh, 900.0, 1.0);
    for (i, s) in wh.iter_mut().enumerate() {
        *s *= adsr(i as f32 / SRF, 0.3, 0.05, 0.15, 0.3, 0.1) * 0.4;
    }
    mix(&mut out, &wh, 0.12, 1.0);
    finalize(&mut out, 0.75);
    out
}

fn sword_clash(rng: &mut Rng32) -> Vec<f32> {
    // inharmonic metal partials + a hard noise transient
    let partials: [f32; 4] = [
        rng.range(1900.0, 2300.0),
        rng.range(3000.0, 3600.0),
        rng.range(4400.0, 5200.0),
        rng.range(6300.0, 7000.0),
    ];
    let dur = 0.22;
    let mut out = render(dur, |t| {
        let mut v = 0.0;
        for (i, f) in partials.iter().enumerate() {
            let decay = (-(t) * (26.0 + i as f32 * 14.0)).exp();
            v += (t * f * core::f32::consts::TAU).sin() * decay / (i + 1) as f32;
        }
        v
    });
    let mut tr = noise(0.02, rng);
    highpass(&mut tr, 2000.0);
    mix(&mut out, &tr, 0.0, 0.8);
    finalize(&mut out, 0.55);
    out
}

fn chop(rng: &mut Rng32) -> Vec<f32> {
    let mut out = noise(0.07, rng);
    bandpass(&mut out, rng.range(620.0, 900.0), 3.0);
    for (i, s) in out.iter_mut().enumerate() {
        *s *= adsr(i as f32 / SRF, 0.07, 0.002, 0.04, 0.15, 0.02);
    }
    // body knock underneath
    let knock = render(0.06, |t| {
        (t * 180.0 * core::f32::consts::TAU).sin() * adsr(t, 0.06, 0.002, 0.04, 0.1, 0.02)
    });
    mix(&mut out, &knock, 0.0, 0.8);
    finalize(&mut out, 0.6);
    out
}

fn death_cry(rng: &mut Rng32) -> Vec<f32> {
    // a falling "aagh" — same glottal trick as the barks, darker
    let f0 = rng.range(120.0, 180.0);
    let dur = 0.5;
    let mut phase = 0.0_f32;
    let mut src = render(dur, |t| {
        let f = f0 * (1.25 - 0.5 * (t / dur));
        phase = (phase + f / SRF) % 1.0;
        2.0 * phase - 1.0
    });
    let b = noise(dur, rng);
    for (s, n) in src.iter_mut().zip(b.iter()) {
        *s += n * 0.2;
    }
    let mut f1 = src.clone();
    bandpass(&mut f1, 700.0, 5.0);
    let mut f2 = src;
    bandpass(&mut f2, 1100.0, 6.0);
    let mut out: Vec<f32> = f1.iter().zip(f2.iter()).map(|(a, b)| a + b * 0.6).collect();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SRF;
        *s *= adsr(t, dur, 0.03, 0.1, 0.7, 0.25);
    }
    finalize(&mut out, 0.5);
    out
}

fn collapse(rng: &mut Rng32) -> Vec<f32> {
    // low rumble + a scatter of stone impacts, with a short room tail
    let mut out = brown_noise(1.3, rng);
    lowpass(&mut out, 200.0);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SRF;
        *s *= adsr(t, 1.3, 0.04, 0.5, 0.5, 0.5) * 3.0;
    }
    for _ in 0..9 {
        let at = rng.range(0.05, 0.8);
        let mut hit = noise(0.05, rng);
        bandpass(&mut hit, rng.range(300.0, 900.0), 3.0);
        for (i, s) in hit.iter_mut().enumerate() {
            *s *= adsr(i as f32 / SRF, 0.05, 0.002, 0.03, 0.1, 0.01);
        }
        mix(&mut out, &hit, at, rng.range(0.4, 0.9));
    }
    echo(&mut out, 0.11, 0.35, 2);
    finalize(&mut out, 0.8);
    out
}

/// Collared dove: two soft falling coos — the inland calm between bird song.
fn dove_coo(rng: &mut Rng32) -> Vec<f32> {
    let mut out = Vec::new();
    let mut t0 = 0.0;
    for k in 0..2 {
        let f0 = rng.range(430.0, 470.0);
        let dur = if k == 0 { 0.22 } else { 0.34 };
        let c = render(dur, |t| {
            let f = f0 * (1.06 - 0.1 * (t / dur)) * (1.0 + 0.02 * (t * 34.0).sin());
            (t * f * core::f32::consts::TAU).sin() * adsr(t, dur, 0.05, 0.08, 0.8, 0.1)
        });
        mix(&mut out, &c, t0, 0.5);
        t0 += dur + 0.12;
    }
    lowpass(&mut out, 1200.0);
    out
}

fn bird_chirp(rng: &mut Rng32) -> Vec<f32> {
    let n = 2 + (rng.next_u32() % 3) as usize;
    let mut out = Vec::new();
    let mut t0 = 0.0;
    for _ in 0..n {
        let f = rng.range(2400.0, 3800.0);
        let warble = rng.range(300.0, 900.0);
        let dur = rng.range(0.06, 0.13);
        let c = render(dur, |t| {
            let fr = f + warble * (t * 38.0 * core::f32::consts::TAU).sin();
            (t * fr * core::f32::consts::TAU).sin() * adsr(t, dur, 0.01, 0.03, 0.7, 0.03)
        });
        mix(&mut out, &c, t0, 0.5);
        t0 += dur + rng.range(0.04, 0.12);
    }
    out
}

fn gull_cry(rng: &mut Rng32) -> Vec<f32> {
    let f0 = rng.range(900.0, 1100.0);
    let dur = 0.55;
    let mut out = render(dur, |t| {
        let f = f0 * (1.3 - 0.45 * (t / dur)) * (1.0 + 0.05 * (t * 26.0).sin());
        (t * f * core::f32::consts::TAU).sin() * adsr(t, dur, 0.04, 0.1, 0.8, 0.2)
    });
    let mut rasp = noise(dur, rng);
    bandpass(&mut rasp, f0 * 1.5, 3.0);
    for (i, s) in rasp.iter_mut().enumerate() {
        *s *= adsr(i as f32 / SRF, dur, 0.04, 0.1, 0.8, 0.2) * 0.4;
    }
    mix(&mut out, &rasp, 0.0, 1.0);
    finalize(&mut out, 0.4);
    out
}

/// Three-band wind: a brown-noise body, a mid whoosh gusting on its own
/// cycle, and a high leaf-rustle band that only opens inside the gusts. Every
/// LFO is an integer multiple of the loop length, so the seam is silent.
fn wind_loop(rng: &mut Rng32) -> Vec<f32> {
    let secs = 16.0;
    let w = core::f32::consts::TAU / secs;
    let mut low = brown_noise(secs, rng);
    lowpass(&mut low, 240.0);
    let mut mid = noise(secs, rng);
    bandpass(&mut mid, 600.0, 0.8);
    let mut high = noise(secs, rng);
    bandpass(&mut high, 3000.0, 0.9);
    let n = samples(secs);
    let mut out = vec![0.0_f32; n];
    for i in 0..n {
        let t = i as f32 / SRF;
        let gust = (0.5 + 0.5 * (2.0 * w * t).sin()) * (0.5 + 0.5 * (3.0 * w * t + 1.1).sin());
        let body = 0.5 + 0.25 * (w * t + 0.4).sin();
        out[i] = low[i] * body * 1.6 + mid[i] * (0.12 + gust * 0.5) + high[i] * gust * gust * 0.22;
    }
    finalize(&mut out, 0.4);
    out
}

/// Shore loop built from discrete wave events: a rising rumble, the crash
/// (noise with a falling cutoff), then the long backwash hiss.
fn wave_loop(rng: &mut Rng32) -> Vec<f32> {
    let secs = 20.0;
    let mut out = vec![0.0_f32; samples(secs)];
    let starts = [0.4_f32, 5.6, 10.4, 15.2];
    for s0 in starts {
        let s0 = s0 + rng.range(-0.2, 0.2);
        // buildup
        let mut up = brown_noise(1.4, rng);
        lowpass(&mut up, 300.0);
        for (i, s) in up.iter_mut().enumerate() {
            let t = i as f32 / SRF;
            *s *= (t / 1.4).powi(2) * 1.6;
        }
        mix(&mut out, &up, s0, 0.7);
        // crash: cutoff sweeps down as the wave folds
        let crash_dur = 1.1;
        let raw = noise(crash_dur, rng);
        let mut crash = vec![0.0_f32; raw.len()];
        let mut y = 0.0_f32;
        for (i, &x) in raw.iter().enumerate() {
            let t = i as f32 / SRF;
            let cutoff = 2600.0 * (1.0 - t / crash_dur).powi(2) + 350.0;
            let k = (1.0 - (-2.0 * core::f32::consts::PI * cutoff / SRF).exp()).clamp(0.0, 1.0);
            y += k * (x - y);
            crash[i] = y * adsr(t, crash_dur, 0.03, 0.3, 0.5, 0.5);
        }
        mix(&mut out, &crash, s0 + 1.3, 1.0);
        // backwash hiss draining away
        let mut wash = noise(2.2, rng);
        bandpass(&mut wash, 1500.0, 0.7);
        for (i, s) in wash.iter_mut().enumerate() {
            let t = i as f32 / SRF;
            *s *= adsr(t, 2.2, 0.25, 0.5, 0.5, 1.2) * 0.5;
        }
        mix(&mut out, &wash, s0 + 2.1, 1.0);
    }
    out.truncate(samples(secs));
    finalize(&mut out, 0.45);
    out
}

pub fn bake_sfx(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut rng = Rng32::new(0xBEEF);
    let add = |sources: &mut Assets<AudioSource>, buf: Vec<f32>| {
        sources.add(AudioSource { bytes: to_wav(&buf).into() })
    };
    let bank = SfxBank {
        bow: (0..3).map(|_| add(&mut sources, bow_shot(&mut rng))).collect(),
        siege_launch: add(&mut sources, siege_launch(&mut rng)),
        clash: (0..4).map(|_| add(&mut sources, sword_clash(&mut rng))).collect(),
        chop: (0..3).map(|_| add(&mut sources, chop(&mut rng))).collect(),
        death: (0..3).map(|_| add(&mut sources, death_cry(&mut rng))).collect(),
        collapse: add(&mut sources, collapse(&mut rng)),
        bird: (0..3).map(|_| add(&mut sources, bird_chirp(&mut rng))).collect(),
        dove: add(&mut sources, dove_coo(&mut rng)),
        gull: add(&mut sources, gull_cry(&mut rng)),
        wind: add(&mut sources, wind_loop(&mut rng)),
        waves: add(&mut sources, wave_loop(&mut rng)),
    };
    commands.insert_resource(bank);
}

// ── playback hooks ───────────────────────────────────────────────────────────

/// Distance attenuation against the camera focus; None = inaudible.
fn earshot(cam: &CameraState, x: f32, z: f32) -> Option<f32> {
    let d = ((x - cam.center.x).powi(2) + (z - cam.center.z).powi(2)).sqrt();
    if d > EARSHOT {
        return None;
    }
    Some((1.0 - d / EARSHOT).powi(2))
}

fn one_shot(commands: &mut Commands, h: Handle<AudioSource>, vol: f32, speed: f32) {
    commands.spawn((
        AudioPlayer(h),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol * SFX_GAIN),
            speed,
            ..default()
        },
    ));
}

/// New projectiles: bow twang per arrow (throttled), heavy launch per boulder.
pub fn projectile_sfx(
    mut commands: Commands,
    bank: Option<Res<SfxBank>>,
    cfg: Res<crate::config::UserConfig>,
    cam: Res<CameraState>,
    q_new: Query<&crate::fx::Arrow, Added<crate::fx::Arrow>>,
    time: Res<Time>,
    mut last_bow: Local<f32>,
    mut flip: Local<u32>,
) {
    let Some(bank) = bank else { return };
    let m = super::master(&cfg);
    let now = time.elapsed_secs();
    for a in &q_new {
        let Some(vol) = earshot(&cam, a.from.x, a.from.y).map(|v| v * m) else { continue };
        if a.stone {
            one_shot(&mut commands, bank.siege_launch.clone(), vol, 1.0);
        } else if now - *last_bow > 0.09 {
            *last_bow = now;
            *flip = flip.wrapping_add(1);
            let h = bank.bow[*flip as usize % bank.bow.len()].clone();
            one_shot(&mut commands, h, vol * 0.8, 0.92 + (*flip % 5) as f32 * 0.04);
        }
    }
}

/// Deaths and building collapses, from the render layer's dying ghosts.
pub fn death_sfx(
    mut commands: Commands,
    bank: Option<Res<SfxBank>>,
    cfg: Res<crate::config::UserConfig>,
    cam: Res<CameraState>,
    q_new: Query<(&Dying, &Transform), Added<Dying>>,
    mut flip: Local<u32>,
) {
    let Some(bank) = bank else { return };
    let m = super::master(&cfg);
    for (d, tf) in &q_new {
        let Some(vol) = earshot(&cam, tf.translation.x, tf.translation.z).map(|v| v * m) else {
            continue;
        };
        if d.rubble > 0.0 {
            one_shot(&mut commands, bank.collapse.clone(), vol, 1.0);
        } else {
            *flip = flip.wrapping_add(1);
            let h = bank.death[*flip as usize % bank.death.len()].clone();
            one_shot(&mut commands, h, vol * 0.7, 0.9 + (*flip % 4) as f32 * 0.07);
        }
    }
}

/// The battle din: while fights animate near the camera, ring steel on a beat
/// whose density follows the number of melees. Chops tick the same way.
pub fn crowd_sfx(
    mut commands: Commands,
    bank: Option<Res<SfxBank>>,
    cfg: Res<crate::config::UserConfig>,
    cam: Res<CameraState>,
    time: Res<Time>,
    q_anim: Query<(&AnimState, &Transform)>,
    mut next_clash: Local<f32>,
    mut next_chop: Local<f32>,
    mut flip: Local<u32>,
) {
    let Some(bank) = bank else { return };
    let m = super::master(&cfg);
    let now = time.elapsed_secs();
    let (mut fights, mut chops) = (0.0_f32, 0.0_f32);
    let (mut fv, mut cv) = (0.0_f32, 0.0_f32);
    for (a, tf) in &q_anim {
        let Some(vol) = earshot(&cam, tf.translation.x, tf.translation.z) else { continue };
        if a.combat {
            fights += 1.0;
            fv = fv.max(vol);
        }
        if a.harvest {
            chops += 1.0;
            cv = cv.max(vol);
        }
    }
    if fights > 0.0 && now >= *next_clash {
        *flip = flip.wrapping_add(1);
        let h = bank.clash[*flip as usize % bank.clash.len()].clone();
        one_shot(&mut commands, h, fv * m * (0.4 + (fights * 0.06).min(0.5)), 0.9 + (*flip % 6) as f32 * 0.035);
        *next_clash = now + (0.55 / (1.0 + fights * 0.12)).max(0.12);
    }
    if chops > 0.0 && now >= *next_chop {
        *flip = flip.wrapping_add(1);
        let h = bank.chop[*flip as usize % bank.chop.len()].clone();
        one_shot(&mut commands, h, cv * m * 0.55, 0.92 + (*flip % 5) as f32 * 0.04);
        *next_chop = now + (0.8 / (1.0 + chops * 0.1)).max(0.3);
    }
}

// ── ambience ─────────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct WindLoop;

#[derive(Component)]
pub struct WaveLoop;

/// The looping ambience must not follow the player back to the menu.
pub fn stop_ambience(mut commands: Commands, q: Query<Entity, Or<(With<WindLoop>, With<WaveLoop>)>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Persistent wind + shore loops whose volume follows the camera; sparse bird
/// and gull one-shots keep the world breathing.
pub fn ambience(
    mut commands: Commands,
    bank: Option<Res<SfxBank>>,
    cfg: Res<crate::config::UserConfig>,
    cam: Res<CameraState>,
    shore: Option<Res<ShoreList>>,
    time: Res<Time>,
    mut q_wind: Query<&mut AudioSink, (With<WindLoop>, Without<WaveLoop>)>,
    mut q_wave: Query<&mut AudioSink, (With<WaveLoop>, Without<WindLoop>)>,
    mut next_bird: Local<f32>,
    mut rng_state: Local<u32>,
) {
    let Some(bank) = bank else { return };
    let m = super::master(&cfg);
    if *rng_state == 0 {
        *rng_state = 0x5EED;
    }
    let mut rng = Rng32(*rng_state);

    // lazily spawn the loops
    if q_wind.is_empty() {
        commands.spawn((
            WindLoop,
            AudioPlayer(bank.wind.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(AMBIENCE_GAIN),
                ..default()
            },
        ));
    }
    if q_wave.is_empty() {
        commands.spawn((
            WaveLoop,
            AudioPlayer(bank.waves.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(0.0),
                ..default()
            },
        ));
    }

    // wind swells slightly as you zoom out (higher vantage, more weather)
    let zoom_t = ((cam.view_size - 10.0) / 75.0).clamp(0.0, 1.0);
    if let Ok(mut sink) = q_wind.single_mut() {
        sink.set_volume(Volume::Linear(AMBIENCE_GAIN * m * (0.5 + zoom_t * 0.5)));
    }

    // shore proximity drives the lapping loop
    let shore_near = shore
        .map(|s| {
            s.0.iter()
                .map(|p| ((p.x - cam.center.x).powi(2) + (p.z - cam.center.z).powi(2)).sqrt())
                .fold(f32::MAX, f32::min)
        })
        .unwrap_or(f32::MAX);
    if let Ok(mut sink) = q_wave.single_mut() {
        let v = if shore_near < 30.0 { (1.0 - shore_near / 30.0) * AMBIENCE_GAIN * m } else { 0.0 };
        sink.set_volume(Volume::Linear(v));
    }

    // sparse birds inland, gulls by the shore
    let now = time.elapsed_secs();
    if now >= *next_bird {
        *next_bird = now + rng.range(4.0, 10.0);
        let by_sea = shore_near < 22.0;
        if by_sea && rng.f32() < 0.5 {
            one_shot(&mut commands, bank.gull.clone(), AMBIENCE_GAIN * m * 0.9, rng.range(0.9, 1.1));
        } else if rng.f32() < 0.3 {
            one_shot(&mut commands, bank.dove.clone(), AMBIENCE_GAIN * m * 0.8, rng.range(0.92, 1.08));
        } else if rng.f32() < 0.6 {
            let h = bank.bird[(rng.next_u32() as usize) % bank.bird.len()].clone();
            one_shot(&mut commands, h, AMBIENCE_GAIN * m * 0.7, rng.range(0.9, 1.15));
        }
    }
    *rng_state = rng.0;
}
