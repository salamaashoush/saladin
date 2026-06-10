//! Procedural unit voices, Stronghold-style: every order gets a short spoken
//! acknowledgment, distinct per unit kind and per command. No samples — a
//! Rosenberg glottal pulse driven through moving formant resonators speaks
//! Arabic-flavored barks ("na'am", "hadir", "yallah", "hatab"...) with stop
//! bursts, aspiration and nasal tails, rendered once at boot.

use super::synth::*;
use bevy::audio::{AudioPlayer, AudioSource, PlaybackMode, PlaybackSettings, Volume};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use saladin_sim::UnitKind;

pub const VOICE_GAIN: f32 = 0.85;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bark {
    Ack = 0,
    Attack = 1,
    Wood = 2,
    Food = 3,
    Stone = 4,
    Gold = 5,
}

/// Commands push here (client-side only); the playback system drains it.
#[derive(Resource, Default)]
pub struct VoiceQueue(pub Vec<(UnitKind, Bark)>);

#[derive(Resource)]
pub struct VoiceBank {
    clips: HashMap<(u8, u8), Vec<Handle<AudioSource>>>,
}

// ── articulation model ───────────────────────────────────────────────────────

/// (F1, F2, F3) and per-formant amplitude.
#[derive(Clone, Copy)]
struct Vowel {
    f: (f32, f32, f32),
    a: (f32, f32, f32),
}

const V_A: Vowel = Vowel { f: (700.0, 1150.0, 2600.0), a: (1.0, 0.65, 0.25) };
const V_I: Vowel = Vowel { f: (290.0, 2150.0, 2900.0), a: (1.0, 0.5, 0.3) };
const V_U: Vowel = Vowel { f: (330.0, 820.0, 2350.0), a: (1.0, 0.55, 0.2) };

/// Consonant onsets: where the burst sits and where the formants start.
#[derive(Clone, Copy, PartialEq)]
enum Onset {
    None,
    /// glottal stop — a hard 25 ms silence before the vowel
    Stop,
    /// aspiration shaped by the coming vowel
    H,
    /// voiced nasal murmur (m/n)
    N,
    /// alveolar stop burst (t/d) — locus pulls F2 toward 1800
    T,
    /// labial stop burst (b) — locus pulls F2 down toward 900
    B,
    /// palatal glide (y) — starts from /i/
    Y,
}

/// Word-final closures.
#[derive(Clone, Copy, PartialEq)]
enum Coda {
    Open,
    /// nasal hum tail (m)
    M,
    /// tapped r — a fast amplitude flutter on the tail
    R,
    /// final stop (b/t) — abrupt cut + tiny low burst
    Stop,
}

#[derive(Clone, Copy)]
struct Syl {
    onset: Onset,
    vowel: Vowel,
    /// f0 multipliers across the vowel
    p0: f32,
    p1: f32,
    dur: f32,
    coda: Coda,
}

fn syl(onset: Onset, vowel: Vowel, p0: f32, p1: f32, dur: f32, coda: Coda) -> Syl {
    Syl { onset, vowel, p0, p1, dur, coda }
}

/// Render one syllable: optional burst/aspiration, then the voiced vowel with
/// formants gliding in from the consonant locus, then the coda.
fn render_syl(s: Syl, base: f32, grit: f32, rng: &mut Rng32) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let mut cursor = 0.0_f32;

    // onset
    match s.onset {
        Onset::None => {}
        Onset::Stop => cursor += 0.025,
        Onset::H => {
            let dur = 0.07;
            let h = noise(dur, rng);
            let mut shaped = formant_track(&h, |_| {
                [
                    (s.vowel.f.0, s.vowel.a.0 * 0.5, 160.0),
                    (s.vowel.f.1, s.vowel.a.1 * 0.5, 200.0),
                    (s.vowel.f.2, s.vowel.a.2 * 0.4, 260.0),
                ]
            });
            for (i, v) in shaped.iter_mut().enumerate() {
                *v *= adsr(i as f32 / SRF, dur, 0.015, 0.03, 0.6, 0.02) * 0.5;
            }
            mix(&mut out, &shaped, cursor, 1.0);
            cursor += dur * 0.8;
        }
        Onset::N => {
            let dur = 0.08;
            let src = glottal(dur, |_| base * s.p0 * 0.95, 0.01, 0.05, rng);
            let mut hum = formant_track(&src, |_| {
                [(280.0, 1.0, 90.0), (1100.0, 0.15, 200.0), (2300.0, 0.05, 300.0)]
            });
            for (i, v) in hum.iter_mut().enumerate() {
                *v *= adsr(i as f32 / SRF, dur, 0.02, 0.02, 0.9, 0.015);
            }
            mix(&mut out, &hum, cursor, 0.8);
            cursor += dur * 0.9;
        }
        Onset::T | Onset::B => {
            cursor += 0.018; // closure silence
            let center = if s.onset == Onset::T { 3300.0 } else { 750.0 };
            let dur = 0.014;
            let mut burst = noise(dur, rng);
            bandpass(&mut burst, center, 1.6);
            for (i, v) in burst.iter_mut().enumerate() {
                *v *= adsr(i as f32 / SRF, dur, 0.001, 0.008, 0.2, 0.004) * 0.9;
            }
            mix(&mut out, &burst, cursor, 1.0);
            cursor += dur;
        }
        Onset::Y => {} // handled by the formant glide below
    }

    // voiced vowel with onset formant glide
    let vdur = s.dur;
    let (gf1, gf2) = match s.onset {
        Onset::T => (s.vowel.f.0 * 0.6, 1800.0),
        Onset::B => (s.vowel.f.0 * 0.6, 900.0),
        Onset::N => (350.0, 1000.0),
        Onset::Y => (V_I.f.0, V_I.f.1),
        _ => (s.vowel.f.0, s.vowel.f.1),
    };
    let glide = if s.onset == Onset::Y { 0.09 } else { 0.05 };
    let jitter = 0.006 + grit * 0.012;
    let shimmer = 0.04 + grit * 0.07;
    let (p0, p1) = (base * s.p0, base * s.p1);
    let mut src = glottal(vdur, |t| p0 + (p1 - p0) * (t / vdur), jitter, shimmer, rng);
    // breathiness scales with grit
    let br = noise(vdur, rng);
    for (v, n) in src.iter_mut().zip(br.iter()) {
        *v += n * (0.03 + grit * 0.06);
    }
    let vw = s.vowel;
    let nasal_coda = s.coda == Coda::M;
    let mut voiced = formant_track(&src, move |t| {
        let k = (t / glide).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        // nasal coda pulls F1 down over the last 40%
        let endk = if nasal_coda { ((t - vdur * 0.6) / (vdur * 0.4)).clamp(0.0, 1.0) } else { 0.0 };
        let f1 = (gf1 + (vw.f.0 - gf1) * k) * (1.0 - 0.5 * endk);
        let f2 = gf2 + (vw.f.1 - gf2) * k;
        [
            (f1, vw.a.0, 80.0 + 60.0 * endk),
            (f2, vw.a.1 * (1.0 - 0.6 * endk), 110.0),
            (vw.f.2, vw.a.2 * (1.0 - 0.6 * endk), 170.0),
        ]
    });
    let rel = match s.coda {
        Coda::Stop => 0.012,
        Coda::R => 0.05,
        _ => vdur * 0.3,
    };
    for (i, v) in voiced.iter_mut().enumerate() {
        let t = i as f32 / SRF;
        let mut e = adsr(t, vdur, 0.018, vdur * 0.2, 0.85, rel);
        if s.coda == Coda::R {
            // tapped r: fast flutter over the tail
            let tail = ((t - vdur * 0.7) / (vdur * 0.3)).clamp(0.0, 1.0);
            e *= 1.0 - tail * 0.5 * (0.5 + 0.5 * (t * 27.0 * core::f32::consts::TAU).sin());
        }
        *v *= e;
    }
    mix(&mut out, &voiced, cursor, 1.0);
    cursor += vdur;

    // codas that add material
    match s.coda {
        Coda::M => {
            let dur = 0.09;
            let src = glottal(dur, |_| p1 * 0.95, 0.01, 0.05, rng);
            let mut hum = formant_track(&src, |_| {
                [(260.0, 1.0, 90.0), (1000.0, 0.12, 220.0), (2200.0, 0.04, 300.0)]
            });
            for (i, v) in hum.iter_mut().enumerate() {
                *v *= adsr(i as f32 / SRF, dur, 0.01, 0.02, 0.8, 0.04);
            }
            mix(&mut out, &hum, cursor - 0.01, 0.7);
        }
        Coda::Stop => {
            let mut burst = noise(0.01, rng);
            bandpass(&mut burst, 700.0, 2.0);
            for (i, v) in burst.iter_mut().enumerate() {
                *v *= adsr(i as f32 / SRF, 0.01, 0.001, 0.006, 0.2, 0.003) * 0.4;
            }
            mix(&mut out, &burst, cursor + 0.015, 1.0);
        }
        _ => {}
    }
    out
}

/// Wooden creak + knock — the "voice" of the siege engines.
fn creak(rng: &mut Rng32) -> Vec<f32> {
    let dur = 0.5;
    let mut phase = 0.0_f32;
    let f = rng.range(26.0, 40.0);
    let mut out = render(dur, |t| {
        phase = (phase + (f + t * 30.0) / SRF) % 1.0;
        let p = if phase < 0.08 { 1.0 } else { -0.05 };
        p * adsr(t, dur, 0.05, 0.1, 0.7, 0.2)
    });
    lowpass(&mut out, 900.0);
    let mut knock = noise(0.06, rng);
    bandpass(&mut knock, 700.0, 5.0);
    for (i, s) in knock.iter_mut().enumerate() {
        *s *= adsr(i as f32 / SRF, 0.06, 0.002, 0.03, 0.2, 0.02);
    }
    mix(&mut out, &knock, 0.3, 1.2);
    out
}

/// Per-kind voice register: base pitch, grit, tempo.
fn register(kind: UnitKind) -> (f32, f32, f32) {
    match kind {
        UnitKind::Peasant => (175.0, 0.3, 1.05),
        UnitKind::Spearman => (135.0, 0.5, 1.0),
        UnitKind::Archer => (158.0, 0.35, 1.1),
        UnitKind::Knight => (105.0, 0.55, 0.9),
        UnitKind::HorseArcher => (150.0, 0.4, 1.15),
        UnitKind::Mamluk => (120.0, 0.6, 0.95),
        UnitKind::Crossbowman => (145.0, 0.45, 1.0),
        UnitKind::Imam => (128.0, 0.12, 0.78),
        UnitKind::Ram | UnitKind::Mangonel => (0.0, 0.0, 1.0), // creak instead
    }
}

/// The word said for each order — Arabic-flavored, two variants each.
fn word_plan(bark: Bark, variant: u32) -> Vec<Syl> {
    match (bark, variant % 2) {
        // na'am / hadir — "yes" / "ready"
        (Bark::Ack, 0) => vec![
            syl(Onset::N, V_A, 1.0, 1.08, 0.13, Coda::Open),
            syl(Onset::Stop, V_A, 1.1, 0.92, 0.2, Coda::M),
        ],
        (Bark::Ack, _) => vec![
            syl(Onset::H, V_A, 1.0, 1.05, 0.12, Coda::Open),
            syl(Onset::T, V_I, 1.12, 0.95, 0.18, Coda::R),
        ],
        // yallah! / hujum! — "let's go" / "attack"
        (Bark::Attack, 0) => vec![
            syl(Onset::Y, V_A, 1.25, 1.42, 0.13, Coda::Open),
            syl(Onset::None, V_A, 1.45, 0.85, 0.3, Coda::Open),
        ],
        (Bark::Attack, _) => vec![
            syl(Onset::H, V_U, 1.3, 1.4, 0.12, Coda::Open),
            syl(Onset::T, V_U, 1.42, 0.9, 0.26, Coda::M),
        ],
        // hatab — "wood"
        (Bark::Wood, 0) => vec![
            syl(Onset::H, V_A, 1.0, 1.06, 0.12, Coda::Open),
            syl(Onset::T, V_A, 1.08, 0.9, 0.18, Coda::Stop),
        ],
        (Bark::Wood, _) => vec![
            syl(Onset::H, V_A, 0.95, 1.0, 0.14, Coda::Open),
            syl(Onset::T, V_A, 1.02, 0.88, 0.2, Coda::Stop),
        ],
        // ta'am — "food"
        (Bark::Food, 0) => vec![
            syl(Onset::T, V_A, 1.05, 1.12, 0.12, Coda::Open),
            syl(Onset::Stop, V_A, 1.1, 0.9, 0.2, Coda::M),
        ],
        (Bark::Food, _) => vec![
            syl(Onset::T, V_A, 1.1, 1.15, 0.11, Coda::Open),
            syl(Onset::Stop, V_A, 1.05, 0.88, 0.22, Coda::M),
        ],
        // hajar — "stone"
        (Bark::Stone, 0) => vec![
            syl(Onset::H, V_A, 0.9, 0.96, 0.13, Coda::Open),
            syl(Onset::T, V_A, 1.0, 0.85, 0.2, Coda::R),
        ],
        (Bark::Stone, _) => vec![
            syl(Onset::H, V_A, 0.88, 0.94, 0.14, Coda::Open),
            syl(Onset::B, V_A, 0.98, 0.84, 0.2, Coda::R),
        ],
        // dhahab — "gold"
        (Bark::Gold, 0) => vec![
            syl(Onset::T, V_A, 1.15, 1.25, 0.11, Coda::Open),
            syl(Onset::H, V_A, 1.2, 1.0, 0.16, Coda::Stop),
        ],
        (Bark::Gold, _) => vec![
            syl(Onset::T, V_A, 1.18, 1.28, 0.1, Coda::Open),
            syl(Onset::H, V_A, 1.22, 1.02, 0.17, Coda::Stop),
        ],
    }
}

fn render_bark(kind: UnitKind, bark: Bark, variant: u32) -> Vec<f32> {
    let mut rng = Rng32::new((kind as u32 + 1) * 7919 + bark as u32 * 131 + variant * 17);
    let (base, grit, tempo) = register(kind);
    if base == 0.0 {
        return creak(&mut rng);
    }
    let base = base * rng.range(0.96, 1.04);
    let plan = if kind == UnitKind::Imam {
        // the Imam answers everything with a small rising melisma
        vec![
            syl(Onset::Y, V_A, 1.0, 1.12, 0.24, Coda::Open),
            syl(Onset::None, V_I, 1.18, 1.05, 0.26, Coda::Open),
            syl(Onset::None, V_A, 1.08, 0.95, 0.3, Coda::M),
        ]
    } else {
        word_plan(bark, variant)
    };
    let mut out: Vec<f32> = Vec::new();
    let mut t = 0.0;
    for mut s in plan {
        s.dur /= tempo;
        let len = s.dur + 0.1;
        let rendered = render_syl(s, base, grit, &mut rng);
        mix(&mut out, &rendered, t, 1.0);
        t += len * rng.range(0.92, 1.05);
    }
    // shouted attack barks get a throat of drive
    if bark == Bark::Attack {
        for s in out.iter_mut() {
            *s = (*s * 2.6).tanh();
        }
    }
    highpass(&mut out, 90.0);
    finalize(&mut out, 0.62);
    out
}

pub fn bake_voices(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut clips = HashMap::default();
    for &kind in UnitKind::ALL {
        for bark in [Bark::Ack, Bark::Attack, Bark::Wood, Bark::Food, Bark::Stone, Bark::Gold] {
            let v: Vec<Handle<AudioSource>> = (0..2)
                .map(|i| {
                    sources.add(AudioSource { bytes: to_wav(&render_bark(kind, bark, i)).into() })
                })
                .collect();
            clips.insert((kind as u8, bark as u8), v);
        }
    }
    commands.insert_resource(VoiceBank { clips });
}

/// One bark per order, throttled so a spam of clicks stays one voice.
pub fn play_voices(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<VoiceQueue>,
    bank: Option<Res<VoiceBank>>,
    mut last: Local<f32>,
    mut flip: Local<u32>,
) {
    let Some(bank) = bank else {
        queue.0.clear();
        return;
    };
    let Some((kind, bark)) = queue.0.first().copied() else { return };
    queue.0.clear();
    let now = time.elapsed_secs();
    if now - *last < 0.45 {
        return;
    }
    *last = now;
    *flip = flip.wrapping_add(1);
    if let Some(v) = bank.clips.get(&(kind as u8, bark as u8)) {
        let h = v[*flip as usize % v.len()].clone();
        commands.spawn((
            AudioPlayer(h),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(VOICE_GAIN),
                ..default()
            },
        ));
    }
}
