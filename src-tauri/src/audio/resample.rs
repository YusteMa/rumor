//! Sample-rate conversion for the ambients, which are stored at 48 kHz and
//! may play on a device that runs at another rate (44.1, 88.2, 96 kHz…).
//!
//! A rational polyphase resampler: the ratio is reduced to `L / M`, and a
//! windowed-sinc low-pass designed at `from × L` is split into `L` phases.
//! Each output sample is one short dot product, so it runs comfortably on
//! the ambient feed thread. Never on the audio thread: `new` allocates.
//!
//! Pure DSP, tested by rendering to buffers.

use std::f64::consts::PI;

/// Taps per phase. With the Kaiser window below the pass band is flat to
/// ~18 kHz and the stop band starts below 22 kHz, more than 90 dB down.
const TAPS: usize = 96;
/// Kaiser β: ~95 dB of stopband attenuation.
const BETA: f64 = 9.5;
/// Pass band edge, as a fraction of the lower Nyquist frequency. Rain has
/// next to nothing up there, and nobody hears above 20 kHz anyway.
const PASS: f64 = 0.91;

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Zeroth-order modified Bessel function, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// Stereo resampler with its own history, so it can be fed in chunks of any
/// size and the output is seamless across them.
#[derive(Clone, Debug)]
pub struct Resampler {
    /// Up factor.
    l: usize,
    /// Down factor.
    m: usize,
    /// `coefs[p * TAPS + k]`: phase `p`, oldest-to-newest order.
    coefs: Vec<f32>,
    /// Input history, written twice so a window is always contiguous.
    hist: Vec<[f32; 2]>,
    /// Write position in `hist` (0..TAPS).
    w: usize,
    /// Position of the next output on the up-sampled grid, relative to the
    /// newest input.
    phase: usize,
    from: u32,
    to: u32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let g = gcd(from.max(1), to.max(1));
        let l = (to.max(1) / g) as usize;
        let m = (from.max(1) / g) as usize;
        let coefs = if l == m { Vec::new() } else { design(l, m) };
        Self {
            l,
            m,
            coefs,
            hist: vec![[0.0; 2]; 2 * TAPS],
            w: 0,
            phase: 0,
            from,
            to,
        }
    }

    pub fn from_rate(&self) -> u32 {
        self.from
    }

    pub fn to_rate(&self) -> u32 {
        self.to
    }

    /// Same rate in and out: samples pass through untouched.
    pub fn is_passthrough(&self) -> bool {
        self.l == self.m
    }

    /// Converts `input` and appends the result to `out`.
    pub fn process(&mut self, input: &[[f32; 2]], out: &mut Vec<[f32; 2]>) {
        if self.is_passthrough() {
            out.extend_from_slice(input);
            return;
        }
        for &x in input {
            self.hist[self.w] = x;
            self.hist[self.w + TAPS] = x;
            self.w = (self.w + 1) % TAPS;
            // The window, oldest to newest, ends at the sample just written.
            let window = &self.hist[self.w..self.w + TAPS];
            while self.phase < self.l {
                let c = &self.coefs[self.phase * TAPS..(self.phase + 1) * TAPS];
                let mut y = [0.0f32; 2];
                for (h, s) in c.iter().zip(window) {
                    y[0] += h * s[0];
                    y[1] += h * s[1];
                }
                out.push(y);
                self.phase += self.m;
            }
            self.phase -= self.l;
        }
    }
}

/// The prototype low-pass at `from × L`, cut at the lower of the two Nyquist
/// frequencies, split into `L` phases of `TAPS` taps.
fn design(l: usize, m: usize) -> Vec<f32> {
    let n = TAPS * l;
    // Cutoff in cycles per up-sampled sample: Nyquist of the lower rate.
    let fc = PASS * 0.5 / l.max(m) as f64;
    let centre = (n - 1) as f64 / 2.0;
    let norm = bessel_i0(BETA);
    let h: Vec<f64> = (0..n)
        .map(|i| {
            let x = i as f64 - centre;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (2.0 * PI * fc * x).sin() / (PI * x)
            };
            let r = x / centre;
            let window = bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / norm;
            // × L: each phase keeps 1/L of the energy of the up-sampled signal.
            sinc * window * l as f64
        })
        .collect();
    let mut coefs = vec![0.0f32; n];
    for p in 0..l {
        for k in 0..TAPS {
            // Oldest-to-newest: the newest input meets tap p, the oldest
            // meets tap p + (TAPS − 1)·L.
            coefs[p * TAPS + k] = h[p + (TAPS - 1 - k) * l] as f32;
        }
    }
    coefs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, fs: u32, secs: f64) -> Vec<[f32; 2]> {
        let n = (fs as f64 * secs) as usize;
        (0..n)
            .map(|i| {
                let v = (2.0 * PI * f * i as f64 / fs as f64).sin() as f32;
                [v, -v]
            })
            .collect()
    }

    /// RMS of the left channel, skipping the filter's start-up.
    fn rms(x: &[[f32; 2]]) -> f64 {
        let x = &x[x.len() / 4..];
        (x.iter().map(|s| f64::from(s[0]).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// Amplitude of frequency `f` in `x` (a single-bin DFT).
    fn tone(x: &[[f32; 2]], f: f64, fs: u32) -> f64 {
        let x = &x[x.len() / 4..];
        let (mut re, mut im) = (0.0, 0.0);
        for (i, s) in x.iter().enumerate() {
            let a = 2.0 * PI * f * i as f64 / fs as f64;
            re += f64::from(s[0]) * a.cos();
            im += f64::from(s[0]) * a.sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f64
    }

    #[test]
    fn same_rate_is_untouched() {
        let mut r = Resampler::new(48_000, 48_000);
        let x = sine(1_000.0, 48_000, 0.1);
        let mut y = Vec::new();
        r.process(&x, &mut y);
        assert!(r.is_passthrough());
        assert_eq!(x, y);
    }

    #[test]
    fn output_length_follows_the_ratio() {
        for (from, to) in [
            (48_000, 44_100),
            (48_000, 96_000),
            (48_000, 88_200),
            (48_000, 192_000),
        ] {
            let mut r = Resampler::new(from, to);
            let mut y = Vec::new();
            // In odd chunks, to exercise the history across calls.
            let x = sine(500.0, from, 1.0);
            for chunk in x.chunks(777) {
                r.process(chunk, &mut y);
            }
            let expected = x.len() as f64 * to as f64 / from as f64;
            assert!(
                (y.len() as f64 - expected).abs() <= 1.0,
                "{from}→{to}: {}",
                y.len()
            );
        }
    }

    #[test]
    fn pass_band_is_flat() {
        for f in [100.0, 1_000.0, 8_000.0, 16_000.0, 17_000.0] {
            let mut r = Resampler::new(48_000, 44_100);
            let mut y = Vec::new();
            r.process(&sine(f, 48_000, 1.0), &mut y);
            let gain_db = 20.0 * tone(&y, f, 44_100).log10();
            assert!(gain_db.abs() < 0.05, "{f} Hz: {gain_db:.3} dB");
        }
    }

    #[test]
    fn content_above_the_new_nyquist_is_removed() {
        // 23 kHz fits in 48 kHz but not in 44.1 kHz: it would alias to 21.1 kHz.
        let mut r = Resampler::new(48_000, 44_100);
        let mut y = Vec::new();
        r.process(&sine(23_000.0, 48_000, 1.0), &mut y);
        let level_db = 20.0 * rms(&y).log10();
        assert!(level_db < -90.0, "alias at {level_db:.1} dB");
    }

    #[test]
    fn upsampling_keeps_the_level_and_adds_no_images() {
        let mut r = Resampler::new(48_000, 96_000);
        let mut y = Vec::new();
        r.process(&sine(5_000.0, 48_000, 1.0), &mut y);
        assert!((20.0 * tone(&y, 5_000.0, 96_000).log10()).abs() < 0.05);
        // The image of 5 kHz at 48 − 5 = 43 kHz.
        assert!(20.0 * tone(&y, 43_000.0, 96_000).log10() < -90.0);
    }
}
