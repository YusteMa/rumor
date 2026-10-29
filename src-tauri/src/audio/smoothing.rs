//! Click-free parameter changes: one-pole smoothing, linear ramps and the
//! curves applied on top of them. Pure and allocation-free, so everything here
//! is safe to call on the audio thread.

use std::f32::consts::FRAC_PI_2;

/// Per-sample increment that takes a 0..=1 ramp from end to end in `ms`.
#[inline]
pub fn step_per_sample(ms: f32, fs: f32) -> f32 {
    1000.0 / (ms.max(1e-3) * fs)
}

/// One-pole low-pass used to smooth a parameter towards its target.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    y: f32,
    coef: f32,
}

impl OnePole {
    /// `time_ms` is the time constant: after it the value has covered ~63 % of
    /// the distance to the target.
    pub fn new(time_ms: f32, fs: f32, initial: f32) -> Self {
        Self {
            y: initial,
            coef: (-1000.0 / (time_ms.max(1e-3) * fs)).exp(),
        }
    }

    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        let next = target + (self.y - target) * self.coef;
        // Near the target the per-sample step drops below half an f32 ulp and
        // the value stalls (~1e-5 away, −100 dB): snap instead of stalling.
        self.y = if next == self.y { target } else { next };
        self.y
    }

    pub fn value(&self) -> f32 {
        self.y
    }
}

/// Linear ramp position in 0..=1 that walks towards a target at a given step.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    pos: f32,
}

impl Ramp {
    pub const fn new(pos: f32) -> Self {
        Self { pos }
    }

    #[inline]
    pub fn next(&mut self, target: f32, step: f32) -> f32 {
        if self.pos < target {
            self.pos = (self.pos + step).min(target);
        } else if self.pos > target {
            self.pos = (self.pos - step).max(target);
        }
        self.pos
    }

    pub fn value(&self) -> f32 {
        self.pos
    }
}

/// 3·ln(10): the fade curve spans 60 dB.
const FADE_K: f32 = 6.907_755;

/// Exponential fade: maps a linear ramp position to a gain that moves roughly
/// linearly in dB over the top 60 dB. It is exactly 0 and 1 at the ends, so a
/// fade never finishes with a step.
#[inline]
pub fn fade_curve(pos: f32) -> f32 {
    if pos <= 0.0 {
        0.0
    } else if pos >= 1.0 {
        1.0
    } else {
        (FADE_K * pos).exp_m1() / FADE_K.exp_m1()
    }
}

/// Equal-power crossfade weight. Two uncorrelated noises crossfaded with
/// `equal_power(p)` and `equal_power(1 - p)` keep a constant level.
#[inline]
pub fn equal_power(pos: f32) -> f32 {
    if pos <= 0.0 {
        0.0
    } else if pos >= 1.0 {
        1.0
    } else {
        (pos * FRAC_PI_2).sin()
    }
}

/// Perceptual volume curve for the 0..=1 slider (SPEC §3.2: `gain = v²`).
#[inline]
pub fn volume_gain(slider: f32) -> f32 {
    let v = slider.clamp(0.0, 1.0);
    v * v
}

/// Below this the soft clipper is bit-transparent.
pub const CLIP_KNEE: f32 = 0.5;
/// −3 dBFS: the output never goes above it.
pub const CLIP_CEILING: f32 = 0.707_945_8;

/// Safety net (SPEC §3.2): transparent below the knee, then a tanh shoulder
/// that approaches −3 dBFS and never exceeds it. The slope is continuous at the
/// knee, so engaging it doesn't click.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        x
    } else {
        let range = CLIP_CEILING - CLIP_KNEE;
        (CLIP_KNEE + range * ((a - CLIP_KNEE) / range).tanh()).copysign(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    #[test]
    fn one_pole_covers_63_percent_after_one_time_constant() {
        let mut p = OnePole::new(30.0, FS, 0.0);
        for _ in 0..(0.030 * FS) as usize {
            p.next(1.0);
        }
        assert!((p.value() - 0.632).abs() < 0.01, "{}", p.value());
    }

    #[test]
    fn one_pole_settles_exactly_on_target() {
        let mut p = OnePole::new(30.0, FS, 0.0);
        for _ in 0..FS as usize {
            p.next(0.25);
        }
        assert_eq!(p.value(), 0.25);
    }

    #[test]
    fn ramp_reaches_target_in_the_requested_time() {
        let step = step_per_sample(400.0, FS);
        let mut r = Ramp::new(0.0);
        let n = (0.4 * FS) as usize;
        // f32 accumulation may finish a few samples early, never noticeably.
        for _ in 0..n * 99 / 100 {
            r.next(1.0, step);
        }
        assert!(r.value() < 1.0);
        for _ in 0..n / 100 + 2 {
            r.next(1.0, step);
        }
        assert_eq!(r.value(), 1.0);
        r.next(1.0, step);
        assert_eq!(r.value(), 1.0);
    }

    #[test]
    fn fade_curve_is_monotonic_with_exact_ends() {
        assert_eq!(fade_curve(0.0), 0.0);
        assert_eq!(fade_curve(1.0), 1.0);
        let mut prev = 0.0;
        for i in 1..=1000 {
            let g = fade_curve(i as f32 / 1000.0);
            assert!(g > prev);
            prev = g;
        }
        // Halfway through the ramp is about 30 dB down: an exponential curve.
        let half_db = 20.0 * fade_curve(0.5).log10();
        assert!((-31.0..-29.0).contains(&half_db), "{half_db}");
    }

    #[test]
    fn equal_power_keeps_constant_power() {
        for i in 0..=100 {
            let p = i as f32 / 100.0;
            let power = equal_power(p).powi(2) + equal_power(1.0 - p).powi(2);
            assert!((power - 1.0).abs() < 1e-5, "{p}: {power}");
        }
    }

    #[test]
    fn soft_clip_is_transparent_below_the_knee_and_bounded_above() {
        for i in -500..=500 {
            let x = i as f32 / 1000.0;
            assert_eq!(soft_clip(x), x);
        }
        for x in [0.6, 0.9, 1.0, 2.0, 10.0, 1e6] {
            let y = soft_clip(x);
            assert!(y > CLIP_KNEE && y <= CLIP_CEILING, "{x} -> {y}");
            assert_eq!(soft_clip(-x), -y);
        }
        // Continuous slope at the knee: no kink to click on.
        let d = 1e-3;
        let slope = (soft_clip(CLIP_KNEE + d) - soft_clip(CLIP_KNEE)) / d;
        assert!((slope - 1.0).abs() < 0.02, "{slope}");
    }

    #[test]
    fn volume_curve_is_quadratic_and_clamped() {
        assert_eq!(volume_gain(0.0), 0.0);
        assert_eq!(volume_gain(0.5), 0.25);
        assert_eq!(volume_gain(1.0), 1.0);
        assert_eq!(volume_gain(2.0), 1.0);
        assert_eq!(volume_gain(-1.0), 0.0);
    }
}
