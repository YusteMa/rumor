//! White, pink (Kellet refined) and brown generators with per-channel state,
//! and the renderer that mixes them into the stereo signal you hear.
//!
//! Pure DSP: no allocation after construction, no locks, no I/O and no `cpal`
//! or Tauri, so it runs on the audio thread and is tested by rendering to
//! buffers (`tests/dsp.rs`).

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use serde::{Deserialize, Serialize};

use super::smoothing::{
    OnePole, Ramp, equal_power, fade_curve, soft_clip, step_per_sample, volume_gain,
};

/// Default fade-in when playback starts (SPEC §3.2; adjustable 0.2–3 s, §4.4).
pub const FADE_IN_MS: f32 = 400.0;
/// Default fade-out when playback stops (SPEC §3.2; adjustable 0.2–3 s, §4.4).
pub const FADE_OUT_MS: f32 = 400.0;
/// Colour crossfade (SPEC §3.2).
pub const CROSSFADE_MS: f32 = 150.0;
/// Mono on/off blend, so switching it never clicks.
pub const MONO_BLEND_MS: f32 = 50.0;
/// Time constant of the volume smoother (SPEC §3.2).
pub const VOLUME_SMOOTHING_MS: f32 = 30.0;
/// Level every colour is normalised to at full volume: K-weighted RMS per
/// channel (as in BS.1770 / LUFS), in dBFS. Low enough that brown noise, the
/// colour with the highest peaks for a given loudness, stays clear of the soft
/// clipper's knee.
pub const TARGET_K_RMS_DB: f32 = -24.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseColor {
    #[default]
    Pink,
    Brown,
    White,
}

impl NoiseColor {
    pub const ALL: [NoiseColor; 3] = [Self::Pink, Self::Brown, Self::White];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Brown,
            2 => Self::White,
            _ => Self::Pink,
        }
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// xorshift128+ seeded through splitmix64: fast, allocation-free and
/// deterministic for a given seed (the tests rely on that).
#[derive(Clone, Debug)]
pub struct Rng {
    s0: u64,
    s1: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let s0 = splitmix64(&mut sm);
        let s1 = splitmix64(&mut sm);
        // The state must never be all zeros.
        let s1 = if (s0 | s1) == 0 { 1 } else { s1 };
        Self { s0, s1 }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s0;
        let y = self.s1;
        self.s0 = y;
        x ^= x << 23;
        self.s1 = x ^ y ^ (x >> 17) ^ (y >> 26);
        self.s1.wrapping_add(y)
    }

    /// Uniform in [−1, 1), from the top 24 bits (the best ones in xorshift+).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (2.0 / 16_777_216.0) - 1.0
    }
}

/// Pink noise: Paul Kellet's refined filter (±0.05 dB above 9.2 Hz at 44.1 kHz).
#[derive(Clone, Debug)]
pub struct Pink {
    rng: Rng,
    b: [f32; 7],
}

impl Pink {
    /// Brings the filter to roughly unit level; `calibrate` sets the real gain.
    const SCALE: f32 = 0.11;

    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            b: [0.0; 7],
        }
    }

    #[inline]
    pub fn sample(&mut self) -> f32 {
        let w = self.rng.bipolar();
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + w * 0.055_517_9;
        b[1] = 0.99332 * b[1] + w * 0.075_075_9;
        b[2] = 0.96900 * b[2] + w * 0.153_852;
        b[3] = 0.86650 * b[3] + w * 0.310_485_6;
        b[4] = 0.55000 * b[4] + w * 0.532_952_2;
        b[5] = -0.7616 * b[5] - w * 0.016_898;
        let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115_926;
        out * Self::SCALE
    }
}

/// Brown (red) noise: a leaky integrator of white noise and a DC blocker.
///
/// SPEC §3.1 gives `y = (y + 0.02·w) / 1.02`. That leak puts the corner at
/// ~150 Hz at 48 kHz, so below it the noise is flat rather than brown. Here the
/// leak comes from the sample rate, putting the corner at ~10 Hz at any rate,
/// and a 20 Hz high-pass removes the sub-sonic rumble and the drift.
#[derive(Clone, Debug)]
pub struct Brown {
    rng: Rng,
    leak: f32,
    y: f32,
    hp_coef: f32,
    hp_x1: f32,
    hp_y1: f32,
}

impl Brown {
    const CORNER_HZ: f32 = 10.0;
    const HIGHPASS_HZ: f32 = 20.0;
    const INPUT_GAIN: f32 = 0.02;

    pub fn new(seed: u64, fs: f32) -> Self {
        Self {
            rng: Rng::new(seed),
            leak: (-TAU * Self::CORNER_HZ / fs).exp(),
            y: 0.0,
            hp_coef: (-TAU * Self::HIGHPASS_HZ / fs).exp(),
            hp_x1: 0.0,
            hp_y1: 0.0,
        }
    }

    #[inline]
    pub fn sample(&mut self) -> f32 {
        let w = self.rng.bipolar();
        self.y = self.leak * self.y + Self::INPUT_GAIN * w;
        // One-pole, one-zero high-pass (DC blocker).
        let out = self.hp_coef * (self.hp_y1 + self.y - self.hp_x1);
        self.hp_x1 = self.y;
        self.hp_y1 = out;
        out
    }
}

/// The three generators for one output channel, each with its own random
/// stream. Separate channels with separate seeds give truly decorrelated
/// stereo (SPEC §3.1).
#[derive(Clone, Debug)]
pub struct Channel {
    pink: Pink,
    brown: Brown,
    white: Rng,
}

impl Channel {
    pub fn new(seed: u64, fs: f32) -> Self {
        let mut sm = seed;
        Self {
            pink: Pink::new(splitmix64(&mut sm)),
            brown: Brown::new(splitmix64(&mut sm), fs),
            white: Rng::new(splitmix64(&mut sm)),
        }
    }

    /// One raw sample of every colour, in `NoiseColor` order. All three always
    /// run, so a crossfade never starts from a cold filter.
    #[inline]
    pub fn sample(&mut self) -> [f32; 3] {
        [
            self.pink.sample(),
            self.brown.sample(),
            self.white.bipolar(),
        ]
    }
}

/// Direct-form-I biquad in f64 (used for measurement, not on the audio path).
#[derive(Clone, Copy, Debug)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn process(&mut self, x0: f64) -> f64 {
        let y0 = self.b[0] * x0 + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x0, self.x[0]];
        self.y = [y0, self.y[0]];
        y0
    }
}

/// ITU-R BS.1770 K-weighting (high shelf + high-pass) at any sample rate, with
/// the coefficient derivation used by libebur128.
#[derive(Clone, Copy, Debug)]
pub struct KWeighting {
    shelf: Biquad,
    highpass: Biquad,
}

impl KWeighting {
    pub fn new(fs: f64) -> Self {
        let (f0, gain_db, q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let vh = 10f64.powf(gain_db / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b: [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        };

        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        let highpass = Biquad {
            b: [1.0, -2.0, 1.0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        };

        Self { shelf, highpass }
    }

    pub fn process(&mut self, x: f64) -> f64 {
        self.highpass.process(self.shelf.process(x))
    }
}

/// Seed for calibration; the tests measure with other seeds.
const CALIBRATION_SEED: u64 = 0x5EED_CA11_B4A7_E000;

/// Per-colour gains that bring each raw generator to `TARGET_K_RMS_DB`, so
/// all three sound equally loud at the same volume (SPEC §3.2).
///
/// Renders ~33 s of noise, so it takes a few milliseconds: call it when the
/// stream is built, never on the audio thread.
pub fn calibrate(fs: f32) -> [f32; 3] {
    const SETTLE_S: f32 = 1.0;
    const MEASURE_S: f32 = 10.0;

    let mut channel = Channel::new(CALIBRATION_SEED, fs);
    let mut weighting = [KWeighting::new(f64::from(fs)); 3];
    let mut energy = [0f64; 3];
    let settle = (fs * SETTLE_S) as usize;
    let n = (fs * MEASURE_S) as usize;

    for i in 0..settle + n {
        let samples = channel.sample();
        for ((k, e), s) in weighting.iter_mut().zip(&mut energy).zip(samples) {
            let y = k.process(f64::from(s));
            if i >= settle {
                *e += y * y;
            }
        }
    }

    let target = 10f64.powf(f64::from(TARGET_K_RMS_DB) / 20.0);
    energy.map(|e| (target / (e / n as f64).sqrt()) as f32)
}

/// What the renderer should be doing, read once per audio buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    pub playing: bool,
    pub color: NoiseColor,
    /// Slider position in 0..=1; the perceptual curve is applied here.
    pub volume: f32,
    /// Fade-in length for the next start.
    pub fade_in_ms: f32,
    /// Fade-out length for the next stop (the sleep timer uses a longer one).
    pub fade_out_ms: f32,
    /// Both channels carry the same signal, for a single earbud.
    pub mono: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            playing: false,
            color: NoiseColor::Pink,
            volume: 0.4,
            fade_in_ms: FADE_IN_MS,
            fade_out_ms: FADE_OUT_MS,
            mono: false,
        }
    }
}

/// The whole signal chain: generators → colour crossfade → loudness
/// normalisation → fade → smoothed volume → soft clip.
#[derive(Clone, Debug)]
pub struct NoiseRenderer {
    channels: [Channel; 2],
    norm: [f32; 3],
    mix: [Ramp; 3],
    fade: Ramp,
    volume: OnePole,
    /// 0 = stereo, 1 = mono.
    mono: Ramp,
    crossfade_step: f32,
    mono_step: f32,
    fs: f32,
}

impl NoiseRenderer {
    /// Builds a renderer and calibrates loudness for `fs`. Starts silent and
    /// fades in once `playing` is set. Not for the audio thread (see
    /// `calibrate`).
    pub fn new(fs: f32, seed: u64, initial: &Controls) -> Self {
        Self::with_gains(fs, seed, calibrate(fs), initial)
    }

    /// Like `new`, with gains from an earlier `calibrate(fs)`.
    pub fn with_gains(fs: f32, seed: u64, norm: [f32; 3], initial: &Controls) -> Self {
        let mut sm = seed;
        let channels = [
            Channel::new(splitmix64(&mut sm), fs),
            Channel::new(splitmix64(&mut sm), fs),
        ];
        let mix = NoiseColor::ALL.map(|c| Ramp::new(if c == initial.color { 1.0 } else { 0.0 }));
        Self {
            channels,
            norm,
            mix,
            fade: Ramp::new(0.0),
            mono: Ramp::new(if initial.mono { 1.0 } else { 0.0 }),
            mono_step: step_per_sample(MONO_BLEND_MS, fs),
            volume: OnePole::new(VOLUME_SMOOTHING_MS, fs, volume_gain(initial.volume)),
            crossfade_step: step_per_sample(CROSSFADE_MS, fs),
            fs,
        }
    }

    /// Renders one stereo frame.
    #[inline]
    pub fn next_frame(&mut self, c: &Controls) -> [f32; 2] {
        let mut weights = [0.0f32; 3];
        for (i, (ramp, w)) in self.mix.iter_mut().zip(&mut weights).enumerate() {
            let target = if i == c.color.index() { 1.0 } else { 0.0 };
            *w = equal_power(ramp.next(target, self.crossfade_step)) * self.norm[i];
        }

        let fade = if c.playing {
            self.fade.next(1.0, step_per_sample(c.fade_in_ms, self.fs))
        } else {
            self.fade.next(0.0, step_per_sample(c.fade_out_ms, self.fs))
        };
        let gain = fade_curve(fade) * self.volume.next(volume_gain(c.volume));

        let mut raw = [0.0f32; 2];
        for (o, channel) in raw.iter_mut().zip(&mut self.channels) {
            let s = channel.sample();
            *o = (s[0] * weights[0] + s[1] * weights[1] + s[2] * weights[2]) * gain;
        }
        // Mono before the clipper, so the sum never goes past −3 dBFS. The
        // two channels are uncorrelated, so (L + R)/√2 is as loud as each.
        let m = self
            .mono
            .next(if c.mono { 1.0 } else { 0.0 }, self.mono_step);
        if m > 0.0 {
            let mono = (raw[0] + raw[1]) * FRAC_1_SQRT_2;
            for o in &mut raw {
                // Exactly equal once fully mono (no 1-ulp drift between ears).
                *o = if m >= 1.0 { mono } else { *o + (mono - *o) * m };
            }
        }
        raw.map(soft_clip)
    }

    /// True once a fade-out has fully finished: the output is exact zeros and
    /// the stream can be paused.
    pub fn is_silent(&self) -> bool {
        self.fade.value() == 0.0
    }

    /// Current overall gain (fade × smoothed volume), for click tests.
    pub fn envelope(&self) -> f32 {
        fade_curve(self.fade.value()) * self.volume.value()
    }

    /// Current crossfade weight of each colour, before normalisation.
    pub fn color_weights(&self) -> [f32; 3] {
        self.mix.map(|r| equal_power(r.value()))
    }
}
