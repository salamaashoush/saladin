//! Tiny offline DSP toolkit: every sound in the game is rendered from these
//! primitives at startup (no binary audio assets, matching the baked-art rule).
//! Output is 22 kHz mono f32, encoded to in-memory WAV for bevy_audio/rodio.

pub const SR: u32 = 22_050;
pub const SRF: f32 = SR as f32;

/// Deterministic xorshift — sounds bake identically every launch.
#[derive(Clone, Copy)]
pub struct Rng32(pub u32);

impl Rng32 {
    pub fn new(seed: u32) -> Self {
        Rng32(seed.max(1))
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// [0, 1)
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f32() * (hi - lo)
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next_u32() as usize) % xs.len()]
    }
}

pub fn samples(secs: f32) -> usize {
    (secs * SRF) as usize
}

/// Render `secs` of audio from a per-sample closure of time in seconds.
pub fn render(secs: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    (0..samples(secs)).map(|i| f(i as f32 / SRF)).collect()
}

/// Additively mix `src` into `dst` starting at `at` seconds (grows dst).
pub fn mix(dst: &mut Vec<f32>, src: &[f32], at: f32, gain: f32) {
    let off = samples(at);
    if dst.len() < off + src.len() {
        dst.resize(off + src.len(), 0.0);
    }
    for (i, s) in src.iter().enumerate() {
        dst[off + i] += s * gain;
    }
}

/// Peak-normalize to `peak`, then a soft clip for safety.
pub fn finalize(buf: &mut [f32], peak: f32) {
    let max = buf.iter().fold(1e-6_f32, |m, s| m.max(s.abs()));
    let g = peak / max;
    for s in buf.iter_mut() {
        let v = *s * g;
        *s = v.tanh();
    }
}

/// Attack/decay/sustain/release position envelope (all in seconds).
pub fn adsr(t: f32, dur: f32, a: f32, d: f32, s: f32, r: f32) -> f32 {
    if t < 0.0 || t >= dur {
        return 0.0;
    }
    if t < a {
        return t / a.max(1e-5);
    }
    if t < a + d {
        return 1.0 + (s - 1.0) * (t - a) / d.max(1e-5);
    }
    if t > dur - r {
        return s * (dur - t) / r.max(1e-5);
    }
    s
}

/// White noise buffer.
pub fn noise(secs: f32, rng: &mut Rng32) -> Vec<f32> {
    (0..samples(secs)).map(|_| rng.range(-1.0, 1.0)).collect()
}

/// One-pole lowpass (in place). `cutoff` in Hz.
pub fn lowpass(buf: &mut [f32], cutoff: f32) {
    let k = (1.0 - (-2.0 * core::f32::consts::PI * cutoff / SRF).exp()).clamp(0.0, 1.0);
    let mut y = 0.0;
    for s in buf.iter_mut() {
        y += k * (*s - y);
        *s = y;
    }
}

/// One-pole highpass (in place).
pub fn highpass(buf: &mut [f32], cutoff: f32) {
    let k = (1.0 - (-2.0 * core::f32::consts::PI * cutoff / SRF).exp()).clamp(0.0, 1.0);
    let mut low = 0.0;
    for s in buf.iter_mut() {
        low += k * (*s - low);
        *s -= low;
    }
}

/// Biquad bandpass (constant skirt), in place — the formant building block.
pub fn bandpass(buf: &mut [f32], freq: f32, q: f32) {
    let w0 = 2.0 * core::f32::consts::PI * freq / SRF;
    let alpha = w0.sin() / (2.0 * q.max(0.1));
    let b0 = alpha;
    let b2 = -alpha;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    for s in buf.iter_mut() {
        let x0 = *s;
        let y0 = (b0 * x0 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        *s = y0;
    }
}

/// Karplus-Strong plucked string — the oud. `damp` ~0.992..0.999 sets decay.
pub fn pluck(freq: f32, secs: f32, damp: f32, rng: &mut Rng32) -> Vec<f32> {
    let period = (SRF / freq).max(2.0) as usize;
    let mut line: Vec<f32> = (0..period).map(|_| rng.range(-1.0, 1.0)).collect();
    let n = samples(secs);
    let mut out = Vec::with_capacity(n);
    let mut idx = 0;
    for _ in 0..n {
        let next = (idx + 1) % period;
        let v = line[idx];
        line[idx] = (v + line[next]) * 0.5 * damp;
        out.push(v);
        idx = next;
    }
    // pick attack transient off, gentle body
    lowpass(&mut out, 4200.0);
    out
}

/// A simple echo tail — the "stone hall" feel on music and big impacts.
pub fn echo(buf: &mut Vec<f32>, delay: f32, feedback: f32, taps: usize) {
    let d = samples(delay);
    let extra = d * taps;
    let orig = buf.clone();
    buf.resize(buf.len() + extra, 0.0);
    let mut g = feedback;
    for tap in 1..=taps {
        let off = d * tap;
        for (i, s) in orig.iter().enumerate() {
            buf[off + i] += s * g;
        }
        g *= feedback;
    }
}

/// Brown(ish) noise — integrated white, the natural wind/rumble source.
pub fn brown_noise(secs: f32, rng: &mut Rng32) -> Vec<f32> {
    let mut acc = 0.0_f32;
    (0..samples(secs))
        .map(|_| {
            acc = (acc + rng.range(-1.0, 1.0) * 0.06).clamp(-1.0, 1.0);
            acc
        })
        .collect()
}

/// Two-pole resonator with per-sample retuning — the moving vocal-tract
/// formant. `traj(t) -> (freq, bandwidth)`; gain normalized at the pole.
pub struct Resonator {
    y1: f32,
    y2: f32,
}

impl Resonator {
    pub fn new() -> Self {
        Resonator { y1: 0.0, y2: 0.0 }
    }
    pub fn tick(&mut self, x: f32, freq: f32, bw: f32) -> f32 {
        let r = (-core::f32::consts::PI * bw / SRF).exp();
        let w = 2.0 * core::f32::consts::PI * freq / SRF;
        let a1 = 2.0 * r * w.cos();
        let a2 = -r * r;
        let g = (1.0 - r) * (1.0 - r); // rough level compensation
        let y = g * x + a1 * self.y1 + a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Rosenberg glottal pulse train: the open/close airflow shape real voices
/// have — far less buzzy than a sawtooth. `f0(t)` drives pitch; `jitter` and
/// `shimmer` add the per-cycle imperfections that read as human.
pub fn glottal(
    secs: f32,
    mut f0: impl FnMut(f32) -> f32,
    jitter: f32,
    shimmer: f32,
    rng: &mut Rng32,
) -> Vec<f32> {
    let mut phase = 0.0_f32;
    let mut cycle_gain = 1.0_f32;
    let mut cycle_pitch = 1.0_f32;
    let open = 0.6_f32; // open quotient
    render(secs, |t| {
        let f = f0(t).max(20.0) * cycle_pitch;
        let prev = phase;
        phase += f / SRF;
        if phase >= 1.0 {
            phase -= 1.0;
            cycle_gain = 1.0 + shimmer * rng.range(-1.0, 1.0);
            cycle_pitch = 1.0 + jitter * rng.range(-1.0, 1.0);
        }
        let _ = prev;
        // rising half-sinusoid while open, sharp closure, silence while closed
        let v = if phase < open {
            let x = phase / open;
            (core::f32::consts::PI * x).sin().powi(2)
        } else {
            0.0
        };
        // differentiate-ish for the radiated pressure feel
        (v - 0.35) * cycle_gain
    })
}

/// Drive `src` through three parallel tracked formants. `traj(t)` returns
/// [(freq, amp, bandwidth); 3] sampled per output sample.
pub fn formant_track(src: &[f32], mut traj: impl FnMut(f32) -> [(f32, f32, f32); 3]) -> Vec<f32> {
    let mut r1 = Resonator::new();
    let mut r2 = Resonator::new();
    let mut r3 = Resonator::new();
    src.iter()
        .enumerate()
        .map(|(i, &x)| {
            let t = i as f32 / SRF;
            let f = traj(t);
            r1.tick(x, f[0].0, f[0].2) * f[0].1 * 40.0
                + r2.tick(x, f[1].0, f[1].2) * f[1].1 * 40.0
                + r3.tick(x, f[2].0, f[2].2) * f[2].1 * 40.0
        })
        .collect()
}

/// Schroeder reverb (4 combs + 2 allpasses), slightly detuned per channel for
/// width. Returns (left, right) including the dry signal.
pub fn reverb_stereo(dry: &[f32], wet: f32, room: f32) -> (Vec<f32>, Vec<f32>) {
    fn comb(src: &[f32], delay_s: f32, fb: f32, out_len: usize) -> Vec<f32> {
        let d = samples(delay_s).max(1);
        let mut out = vec![0.0_f32; out_len];
        for i in 0..out_len {
            let x = if i < src.len() { src[i] } else { 0.0 };
            let echo = if i >= d { out[i - d] } else { 0.0 };
            out[i] = x + echo * fb;
        }
        out
    }
    fn allpass(buf: &mut [f32], delay_s: f32, g: f32) {
        let d = samples(delay_s).max(1);
        let mut delayed = vec![0.0_f32; buf.len()];
        for i in 0..buf.len() {
            let back = if i >= d { delayed[i - d] } else { 0.0 };
            let v = buf[i] + g * back;
            delayed[i] = v;
            buf[i] = back - g * v;
        }
    }
    let tail = samples(room * 1.8);
    let n = dry.len() + tail;
    let fb = 0.72 + room * 0.08;
    let make = |tunes: [f32; 4]| {
        let mut sum = vec![0.0_f32; n];
        for t in tunes {
            let c = comb(dry, t * (0.8 + room * 0.4), fb, n);
            for (s, v) in sum.iter_mut().zip(c.iter()) {
                *s += v * 0.25;
            }
        }
        allpass(&mut sum, 0.0051, 0.7);
        allpass(&mut sum, 0.0017, 0.7);
        lowpass(&mut sum, 4500.0);
        sum
    };
    let wl = make([0.0297, 0.0371, 0.0411, 0.0437]);
    let wr = make([0.0313, 0.0359, 0.0423, 0.0451]);
    let mut l = vec![0.0_f32; n];
    let mut r = vec![0.0_f32; n];
    for i in 0..n {
        let d = if i < dry.len() { dry[i] } else { 0.0 };
        l[i] = d + wl[i] * wet;
        r[i] = d + wr[i] * wet;
    }
    (l, r)
}

/// Encode stereo f32 to 16-bit PCM WAV in memory.
pub fn to_wav_stereo(left: &[f32], right: &[f32]) -> Vec<u8> {
    let n = left.len().min(right.len()) as u32;
    let data_len = n * 4;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&SR.to_le_bytes());
    out.extend_from_slice(&(SR * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n as usize {
        out.extend_from_slice(&((left[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
        out.extend_from_slice(&((right[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

/// Peak-normalize a stereo pair together (keeps the image).
pub fn finalize_stereo(l: &mut [f32], r: &mut [f32], peak: f32) {
    let max = l.iter().chain(r.iter()).fold(1e-6_f32, |m, s| m.max(s.abs()));
    let g = peak / max;
    for s in l.iter_mut().chain(r.iter_mut()) {
        *s = (*s * g).tanh();
    }
}

/// Encode mono f32 to a 16-bit PCM WAV file in memory.
pub fn to_wav(samplebuf: &[f32]) -> Vec<u8> {
    let n = samplebuf.len() as u32;
    let data_len = n * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SR.to_le_bytes());
    out.extend_from_slice(&(SR * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samplebuf {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_length() {
        let buf = render(0.1, |t| (t * 440.0 * core::f32::consts::TAU).sin());
        let wav = to_wav(&buf);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + buf.len() * 2);
    }

    #[test]
    fn pluck_decays() {
        let mut rng = Rng32::new(7);
        let p = pluck(220.0, 1.0, 0.995, &mut rng);
        let head: f32 = p[..2000].iter().map(|s| s.abs()).sum();
        let tail: f32 = p[p.len() - 2000..].iter().map(|s| s.abs()).sum();
        assert!(tail < head * 0.5, "string should ring down: head {head} tail {tail}");
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng32::new(42);
        let mut b = Rng32::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
