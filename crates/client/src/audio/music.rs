//! Infinite background music, rendered at startup. The band: a double-course
//! oud (two detuned Karplus strings, pick-position comb, wooden body
//! resonance), a breathy ney with finger slides, and a darbuka playing maqsum
//! with human timing. Phrases are 8-bar arches in maqam hijaz that cadence
//! with a qafla, finished in a stone-hall Schroeder reverb (stereo). A
//! sequencer chains phrases forever with drone-only breathers.

use super::synth::*;
use bevy::audio::{AudioPlayer, AudioSource, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;

pub const MUSIC_GAIN: f32 = 0.42;

/// Hijaz on D. Semitone offsets from the root.
const HIJAZ: [i32; 7] = [0, 1, 4, 5, 7, 8, 10];
const ROOT: f32 = 146.83; // D3

fn degree(d: i32) -> f32 {
    let oct = d.div_euclid(7);
    let idx = d.rem_euclid(7) as usize;
    ROOT * (2.0_f32).powf((HIJAZ[idx] + 12 * oct) as f32 / 12.0)
}

pub struct Phrase {
    pub handle: Handle<AudioSource>,
    pub secs: f32,
}

#[derive(Resource)]
pub struct MusicBank {
    pub phrases: Vec<Phrase>,
    pub rest: Phrase,
}

#[derive(Resource)]
pub struct MusicState {
    pub until: f32,
    pub last: usize,
    pub rng: Rng32,
}

/// Marker on the playing phrase so the settings slider can retune it live.
#[derive(Component)]
pub struct MusicPhrase;

// ── instruments ──────────────────────────────────────────────────────────────

/// Double-course oud note: two strings a few cents apart, the pick-position
/// comb that gives plucked strings their hollow attack, and a touch of wooden
/// body resonance.
fn oud_note(freq: f32, dur: f32, rng: &mut Rng32) -> Vec<f32> {
    let detune = freq * 0.0025;
    let a = pluck(freq - detune * 0.5, dur, 0.9962, rng);
    let b = pluck(freq + detune * 0.5, dur, 0.9958, rng);
    let mut out: Vec<f32> = a.iter().zip(b.iter()).map(|(x, y)| (x + y) * 0.5).collect();
    // pick-position comb (picked at ~1/7 of the string)
    let d = ((SRF / freq) * 0.14).max(1.0) as usize;
    for i in (d..out.len()).rev() {
        out[i] -= out[i - d] * 0.65;
    }
    // body: low wooden bloom
    let mut body = out.clone();
    bandpass(&mut body, 210.0, 1.6);
    for (o, bd) in out.iter_mut().zip(body.iter()) {
        *o += bd * 0.9;
    }
    out
}

/// The ney line: one continuous breathy tone gliding between target degrees,
/// with a chiff of breath at onset and gentle vibrato that blooms late.
fn ney_line(out: &mut Vec<f32>, targets: &[i32], at: f32, note: f32, rng: &mut Rng32) {
    let total = note * targets.len() as f32;
    let freqs: Vec<f32> = targets.iter().map(|d| degree(*d) * 2.0).collect();
    let glide = 0.09_f32; // seconds of slide into each new note
    let mut tone = {
        let freqs = freqs.clone();
        let mut phase = 0.0_f32;
        render(total, move |t| {
            let idx = ((t / note) as usize).min(freqs.len() - 1);
            let f_now = freqs[idx];
            let f = if idx > 0 && t - idx as f32 * note < glide {
                let x = (t - idx as f32 * note) / glide;
                freqs[idx - 1] + (f_now - freqs[idx - 1]) * x * x * (3.0 - 2.0 * x)
            } else {
                f_now
            };
            let vib = 1.0 + 0.007 * (t * 5.4 * core::f32::consts::TAU).sin() * (t / total).powf(0.7);
            phase += f * vib / SRF;
            let p = phase * core::f32::consts::TAU;
            // fundamental + soft overblown 2nd harmonic = hollow reed
            (p.sin() + 0.35 * (2.0 * p).sin()) * adsr(t, total, 0.5, 0.2, 0.85, total * 0.3)
        })
    };
    // breath running the whole line, stronger at the start of each note
    let mut breath = noise(total, rng);
    bandpass(&mut breath, freqs[0] * 2.1, 1.6);
    for (i, b) in breath.iter_mut().enumerate() {
        let t = i as f32 / SRF;
        let in_note = (t / note).fract() * note;
        let chiff = 1.0 + 2.2 * (-in_note * 14.0).exp();
        *b *= 0.075 * chiff * adsr(t, total, 0.4, 0.2, 0.85, total * 0.3);
    }
    lowpass(&mut tone, 3800.0);
    mix(out, &breath, at, 1.0);
    mix(out, &tone, at, 0.42);
}

/// Darbuka voices: dum = pitched membrane with a fast drop; tek = the bright
/// rim crack (two narrow high bands).
fn dum(rng: &mut Rng32) -> Vec<f32> {
    let f0 = rng.range(86.0, 96.0);
    let mut d = render(0.22, |t| {
        let f = f0 * (1.0 + 2.2 * (-t * 30.0).exp());
        (t * f * core::f32::consts::TAU).sin() * adsr(t, 0.22, 0.003, 0.12, 0.2, 0.07)
    });
    let mut slap = noise(0.02, rng);
    bandpass(&mut slap, 1400.0, 1.5);
    mix(&mut d, &slap, 0.0, 0.4);
    lowpass(&mut d, 700.0);
    d
}

fn tek(rng: &mut Rng32) -> Vec<f32> {
    let mut a = noise(0.06, rng);
    bandpass(&mut a, rng.range(3000.0, 3500.0), 5.0);
    let mut b = noise(0.06, rng);
    bandpass(&mut b, rng.range(5200.0, 6000.0), 6.0);
    for (i, (x, y)) in a.iter_mut().zip(b.iter()).enumerate() {
        let e = adsr(i as f32 / SRF, 0.06, 0.001, 0.03, 0.12, 0.02);
        *x = (*x + y * 0.7) * e;
    }
    a
}

/// Maqsum at the phrase tempo, with human push-pull: DUM tek . tek DUM . tek .
fn drums(out: &mut Vec<f32>, bars: usize, bar: f32, rng: &mut Rng32) {
    let dum_s = dum(rng);
    let tek_s = tek(rng);
    let eighth = bar / 8.0;
    // (eighth index, is_dum, base gain)
    let pattern: [(usize, bool, f32); 5] =
        [(0, true, 0.9), (1, false, 0.4), (3, false, 0.45), (4, true, 0.75), (6, false, 0.5)];
    for b in 0..bars {
        for (idx, is_dum, g) in pattern {
            let humanize = rng.range(-0.011, 0.011);
            let gain = g * rng.range(0.85, 1.1);
            let at = b as f32 * bar + idx as f32 * eighth + humanize;
            mix(out, if is_dum { &dum_s } else { &tek_s }, at.max(0.0), gain);
        }
        // ghost tek on the last eighth, sometimes
        if rng.f32() < 0.4 {
            let at = b as f32 * bar + 7.0 * eighth + rng.range(-0.01, 0.01);
            mix(out, &tek_s, at, 0.2);
        }
    }
}

// ── phrases ──────────────────────────────────────────────────────────────────

/// An 8-bar arch: rise toward a chosen peak, hover, then a qafla — the
/// stepwise ornamented descent home every maqam phrase resolves with.
fn melody(out: &mut Vec<f32>, total: f32, bar: f32, rng: &mut Rng32) {
    let peak = *rng.pick(&[5, 6, 7, 8]);
    let beat = bar / 4.0;
    let cells: [&[f32]; 4] =
        [&[1.0, 0.5, 0.5], &[0.5, 0.5, 1.0], &[0.5, 0.25, 0.25, 1.0], &[1.5, 0.5]];
    let qafla_len = bar * 1.5;
    let mut deg: i32 = *rng.pick(&[0, 1, 2]);
    let mut t = 0.0;
    while t < total - qafla_len - bar * 0.5 {
        let cell = *rng.pick(&cells);
        for &dur_beats in cell {
            if t >= total - qafla_len - bar * 0.25 {
                break;
            }
            let dur = dur_beats * beat;
            let n = oud_note(degree(deg), (dur * 2.4).min(2.2), rng);
            mix(out, &n, t, 0.5 * rng.range(0.85, 1.05));
            // ornament: quick lower-neighbor turn on longer notes
            if dur_beats >= 1.0 && rng.f32() < 0.45 {
                let g1 = oud_note(degree(deg - 1), 0.22, rng);
                mix(out, &g1, t + dur * 0.45, 0.2);
                let g2 = oud_note(degree(deg), 0.3, rng);
                mix(out, &g2, t + dur * 0.62, 0.25);
            }
            // walk: pulled upward early, free near the peak
            let progress = t / (total - qafla_len);
            let bias_up = progress < 0.55 && deg < peak;
            let step = if bias_up {
                *rng.pick(&[1, 1, 2, -1])
            } else {
                *rng.pick(&[-2, -1, 1, -1, 2])
            };
            deg = (deg + step).clamp(-1, peak);
            t += dur;
        }
    }
    // qafla: 4-3-2-1 with a turn, landing long on the root
    let q_start = total - qafla_len;
    let mut qt = q_start;
    for (i, d) in [3, 2, 1, 0].into_iter().enumerate() {
        let dur = if i == 3 { qafla_len * 0.4 } else { qafla_len * 0.2 };
        let n = oud_note(degree(d), (dur * 2.5).min(2.4), rng);
        mix(out, &n, qt, if i == 3 { 0.6 } else { 0.5 });
        if i == 1 && rng.f32() < 0.7 {
            let turn = oud_note(degree(2), 0.18, rng);
            mix(out, &turn, qt + dur * 0.5, 0.22);
        }
        qt += dur;
    }
}

fn drone(out: &mut Vec<f32>, total: f32, bar: f32, rng: &mut Rng32) {
    let bars = (total / bar) as usize;
    for b in 0..bars {
        let d = oud_note(degree(-7), bar * 1.3, rng);
        mix(out, &d, b as f32 * bar, 0.26);
        if b % 2 == 1 {
            let fifth = oud_note(degree(-3), bar * 0.9, rng);
            mix(out, &fifth, b as f32 * bar + bar * 0.5, 0.13);
        }
    }
    let pad = render(total, |t| {
        let p = degree(-7) * 0.5 * t * core::f32::consts::TAU;
        (p.sin() + 0.3 * (2.0 * p).sin()) * adsr(t, total, 2.0, 0.1, 0.7, 2.2)
    });
    mix(out, &pad, 0.0, 0.07);
}

fn master(mut dry: Vec<f32>) -> Vec<u8> {
    finalize(&mut dry, 0.6);
    let (mut l, mut r) = reverb_stereo(&dry, 0.32, 0.6);
    finalize_stereo(&mut l, &mut r, 0.72);
    to_wav_stereo(&l, &r)
}

fn render_phrase(seed: u32, with_drums: bool, with_ney: bool) -> (Vec<u8>, f32) {
    let mut rng = Rng32::new(seed);
    let bar = 60.0 / 96.0 * 4.0; // 96 bpm, 4/4
    let bars = 8;
    let total = bars as f32 * bar;
    let mut out = vec![0.0_f32; samples(total + 2.5)];

    drone(&mut out, total, bar, &mut rng);
    melody(&mut out, total, bar, &mut rng);
    if with_ney {
        let targets: Vec<i32> = (0..3).map(|_| *rng.pick(&[4, 5, 7, 8])).collect();
        ney_line(&mut out, &targets, bar * 2.0, bar * 1.6, &mut rng);
    }
    if with_drums {
        drums(&mut out, bars, bar, &mut rng);
    }
    (master(out), total)
}

/// Drone-only breather so the score never wallpapers.
fn render_rest(seed: u32) -> (Vec<u8>, f32) {
    let mut rng = Rng32::new(seed);
    let total = 8.0;
    let mut out = vec![0.0_f32; samples(total + 2.5)];
    drone(&mut out, total, 2.5, &mut rng);
    ney_line(&mut out, &[0], 2.0, 4.0, &mut rng);
    (master(out), total)
}

pub fn bake_music(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut phrases = Vec::new();
    let specs: [(u32, bool, bool); 6] = [
        (211, false, false),
        (223, true, false),
        (237, true, true),
        (251, false, true),
        (267, true, true),
        (283, true, false),
    ];
    for (seed, dr, ny) in specs {
        let (wav, secs) = render_phrase(seed, dr, ny);
        phrases.push(Phrase { handle: sources.add(AudioSource { bytes: wav.into() }), secs });
    }
    let (wav, secs) = render_rest(297);
    let rest = Phrase { handle: sources.add(AudioSource { bytes: wav.into() }), secs };
    commands.insert_resource(MusicBank { phrases, rest });
    commands.insert_resource(MusicState { until: 0.0, last: usize::MAX, rng: Rng32::new(0xC0FFEE) });
}

/// Chain phrases forever: never the same phrase twice in a row, with a 1-in-4
/// drone breather. Runs in menu and match alike.
pub fn sequence_music(
    mut commands: Commands,
    time: Res<Time>,
    cfg: Res<crate::config::UserConfig>,
    bank: Option<Res<MusicBank>>,
    state: Option<ResMut<MusicState>>,
) {
    let (Some(bank), Some(mut state)) = (bank, state) else { return };
    let now = time.elapsed_secs();
    if now < state.until {
        return;
    }
    let rest = state.rng.f32() < 0.25;
    let phrase = if rest {
        &bank.rest
    } else {
        let mut idx = (state.rng.next_u32() as usize) % bank.phrases.len();
        if idx == state.last {
            idx = (idx + 1) % bank.phrases.len();
        }
        state.last = idx;
        &bank.phrases[idx]
    };
    commands.spawn((
        MusicPhrase,
        AudioPlayer(phrase.handle.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(MUSIC_GAIN * super::master(&cfg)),
            ..default()
        },
    ));
    state.until = now + phrase.secs + 0.8;
}
