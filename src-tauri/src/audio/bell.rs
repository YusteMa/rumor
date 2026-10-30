//! The alert bell (SPEC §4.3b): a Tibetan singing bowl struck softly,
//! synthesised on the spot: no audio files, like the noise.
//!
//! A bowl's partials are inharmonic (about 1 : 2.76 : 5.1 : 8.1 : 11.6) and
//! each rings for less time the higher it is. Its slight asymmetry splits
//! every partial into two close modes, which beat slowly ("wah-wah"). Each
//! mode is a decaying phasor: one complex multiply per sample, no `sin()`,
//! no allocation, so it can run on the audio thread.

use super::smoothing::fade_curve;

/// Fundamental: a warm, medium bowl.
const F0: f64 = 220.0;
/// (frequency ratio, amplitude, seconds to fall 60 dB, beat in Hz).
const PARTIALS: [(f64, f64, f64, f64); 5] = [
    (1.0, 1.0, 8.0, 0.7),
    (2.76, 0.55, 5.0, 1.4),
    (5.12, 0.28, 3.0, 2.1),
    (8.07, 0.14, 1.8, 2.9),
    (11.6, 0.07, 1.1, 3.6),
];
/// The two modes of a split partial share its amplitude unevenly, so the
/// beating is a gentle swell, never a full tremolo.
const SPLIT: (f64, f64) = (0.62, 0.38);
/// A soft mallet: the bowl swells in over this, so the strike never clicks.
const ATTACK_S: f64 = 0.012;
const LENGTH_S: f64 = 9.0;
/// The tail is faded to exactly zero over the last stretch.
const RELEASE_S: f64 = 1.5;
const MODES: usize = PARTIALS.len() * 2;

#[derive(Clone, Copy, Debug, Default)]
struct Mode {
    re: f64,
    im: f64,
    /// Per-sample rotation and decay: `r·e^{iω}`.
    c: f64,
    s: f64,
    amp: f64,
}

#[derive(Clone, Debug)]
pub struct Bowl {
    modes: [Mode; MODES],
    pos: u32,
    attack: u32,
    len: u32,
    release: u32,
    level: f32,
    norm: f64,
}

impl Bowl {
    pub fn new(fs: f32) -> Self {
        let fs = f64::from(fs);
        let mut modes = [Mode::default(); MODES];
        for (i, &(ratio, amp, t60, beat)) in PARTIALS.iter().enumerate() {
            // −60 dB after t60 seconds.
            let r = (-6.907_755_278_982_137 / (t60 * fs)).exp();
            for (j, (offset, share)) in [(-beat / 2.0, SPLIT.0), (beat / 2.0, SPLIT.1)]
                .into_iter()
                .enumerate()
            {
                // Above Nyquist (very low rates only): leave it silent.
                let f = F0 * ratio + offset;
                let w = std::f64::consts::TAU * f / fs;
                let amp = if f < fs / 2.0 { amp * share } else { 0.0 };
                modes[i * 2 + j] = Mode {
                    c: r * w.cos(),
                    s: r * w.sin(),
                    amp,
                    ..Mode::default()
                };
            }
        }
        let norm = 1.0 / PARTIALS.iter().map(|p| p.1).sum::<f64>();
        let len = (LENGTH_S * fs) as u32;
        Self {
            modes,
            // Silent until struck.
            pos: len,
            attack: ((ATTACK_S * fs) as u32).max(1),
            len,
            release: ((RELEASE_S * fs) as u32).max(1),
            level: 0.0,
            norm,
        }
    }

    /// Strikes the bowl (again, if it is still ringing) with peak `level`.
    pub fn strike(&mut self, level: f32) {
        for m in &mut self.modes {
            // Sine phase: every mode starts at zero.
            m.re = m.amp;
            m.im = 0.0;
        }
        self.pos = 0;
        self.level = level.max(0.0);
    }

    pub fn is_done(&self) -> bool {
        self.pos >= self.len
    }

    /// One sample, or exactly 0 when not ringing.
    #[inline]
    pub fn sample(&mut self) -> f32 {
        if self.pos >= self.len {
            return 0.0;
        }
        let mut sum = 0.0;
        for m in &mut self.modes {
            sum += m.im;
            let re = m.re * m.c - m.im * m.s;
            m.im = m.re * m.s + m.im * m.c;
            m.re = re;
        }
        let pos = self.pos;
        self.pos += 1;
        // Raised-cosine swell in; exponential fade to zero at the end.
        let attack = if pos < self.attack {
            let x = f64::from(pos) / f64::from(self.attack);
            0.5 - 0.5 * (std::f64::consts::PI * x).cos()
        } else {
            1.0
        };
        // The last sample is exactly 0.
        let left = self.len - pos - 1;
        let release = if left < self.release {
            f64::from(fade_curve(left as f32 / self.release as f32))
        } else {
            1.0
        };
        (sum * self.norm * attack * release) as f32 * self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn ring(level: f32) -> Vec<f32> {
        let mut b = Bowl::new(FS);
        b.strike(level);
        let mut out = Vec::new();
        while !b.is_done() {
            out.push(b.sample());
        }
        out
    }

    #[test]
    fn silent_until_struck_and_exactly_zero_after() {
        let mut b = Bowl::new(FS);
        assert!(b.is_done());
        assert_eq!(b.sample(), 0.0);
        let out = ring(0.25);
        assert_eq!(out.len(), (LENGTH_S * f64::from(FS)) as usize);
        assert_eq!(out[0], 0.0);
        assert_eq!(*out.last().unwrap(), 0.0);
    }

    #[test]
    fn stays_under_its_level_and_never_clicks() {
        let level = 0.25;
        let out = ring(level);
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak <= level && peak > level * 0.5, "peak {peak}");
        // The biggest step between samples is what a 2.5 kHz sine at this
        // level does anyway: no discontinuity at the strike or the end.
        let step = out.windows(2).fold(0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(step < level * 0.35, "step {step}");
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn rings_at_its_fundamental_and_fades() {
        let out = ring(1.0);
        // Goertzel-style energy at a frequency over the first second.
        let energy = |f: f64| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (n, x) in out.iter().take(FS as usize).enumerate() {
                let p = std::f64::consts::TAU * f * n as f64 / f64::from(FS);
                re += f64::from(*x) * p.cos();
                im += f64::from(*x) * p.sin();
            }
            re.hypot(im)
        };
        assert!(energy(F0) > 10.0 * energy(300.0));
        assert!(energy(F0 * 2.76) > 10.0 * energy(F0 * 2.0));
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let first = rms(&out[..48_000]);
        let late = rms(&out[6 * 48_000..7 * 48_000]);
        assert!(late < first / 10.0, "first {first}, late {late}");
    }
}
