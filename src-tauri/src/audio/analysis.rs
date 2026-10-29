//! Spectrum bands for the panel (SPEC §4.2): 64 log-spaced bands from 20 Hz
//! to 20 kHz, computed ~30 times a second and only while the panel is
//! visible. Runs on its own thread, never on the audio thread.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

pub const BANDS: usize = 64;
pub const F_MIN: f32 = 20.0;
pub const F_MAX: f32 = 20_000.0;
const FFT_SIZE: usize = 8192;
/// Level reported for bands with no energy (or above Nyquist).
pub const FLOOR_DB: f32 = -200.0;
/// Per-frame weight of a new spectrum in the running average: at ~30 fps the
/// curve settles in a few hundred ms and moves calmly.
const SMOOTHING: f32 = 0.12;

/// Centre frequency of each band (log-spaced).
pub fn band_centres() -> [f32; BANDS] {
    std::array::from_fn(|i| F_MIN * (F_MAX / F_MIN).powf((i as f32 + 0.5) / BANDS as f32))
}

pub struct Analyzer {
    sample_rate: f32,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    ring: Vec<f32>,
    pos: usize,
    filled: usize,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// For each band, the FFT bins it covers and how many Hz of each.
    weights: Vec<Vec<(usize, f32)>>,
    /// Converts |X|² of one bin to one-sided power density (per Hz).
    density_scale: f32,
    smoothed: [f32; BANDS],
    primed: bool,
}

impl Analyzer {
    pub fn new(sample_rate: f32) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        let window_power: f32 = window.iter().map(|w| w * w).sum();
        let bin_hz = sample_rate / FFT_SIZE as f32;
        let nyquist = sample_rate / 2.0;

        let edge = |i: usize| F_MIN * (F_MAX / F_MIN).powf(i as f32 / BANDS as f32);
        let weights = (0..BANDS)
            .map(|b| {
                let (lo, hi) = (edge(b), edge(b + 1).min(nyquist));
                let first = ((lo / bin_hz) - 0.5).floor().max(1.0) as usize;
                let last = ((hi / bin_hz) + 0.5).ceil() as usize;
                (first..=last.min(FFT_SIZE / 2))
                    .filter_map(|k| {
                        let (a, z) = ((k as f32 - 0.5) * bin_hz, (k as f32 + 0.5) * bin_hz);
                        let overlap = hi.min(z) - lo.max(a);
                        (overlap > 0.0).then_some((k, overlap))
                    })
                    .collect()
            })
            .collect();

        let scratch_len = fft.get_inplace_scratch_len();
        Self {
            sample_rate,
            fft,
            window,
            ring: vec![0.0; FFT_SIZE],
            pos: 0,
            filled: 0,
            buf: vec![Complex::default(); FFT_SIZE],
            scratch: vec![Complex::default(); scratch_len],
            weights,
            density_scale: 2.0 / (sample_rate * window_power),
            smoothed: [0.0; BANDS],
            primed: false,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn push(&mut self, x: f32) {
        self.ring[self.pos] = x;
        self.pos = (self.pos + 1) % FFT_SIZE;
        self.filled = (self.filled + 1).min(FFT_SIZE);
    }

    /// Power spectral density of each band in dB (re full scale, per Hz),
    /// averaged over the last frames. `None` until a full window is buffered.
    pub fn bands(&mut self) -> Option<[f32; BANDS]> {
        if self.filled < FFT_SIZE {
            return None;
        }
        // Oldest sample first.
        for (i, (b, w)) in self.buf.iter_mut().zip(&self.window).enumerate() {
            *b = Complex::new(self.ring[(self.pos + i) % FFT_SIZE] * w, 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buf, &mut self.scratch);

        for (s, bins) in self.smoothed.iter_mut().zip(&self.weights) {
            let hz: f32 = bins.iter().map(|(_, w)| w).sum();
            let density = if hz > 0.0 {
                bins.iter()
                    .map(|&(k, w)| self.buf[k].norm_sqr() * w)
                    .sum::<f32>()
                    * self.density_scale
                    / hz
            } else {
                0.0
            };
            *s = if self.primed {
                *s + SMOOTHING * (density - *s)
            } else {
                density
            };
        }
        self.primed = true;
        Some(self.smoothed.map(|p| {
            if p > 0.0 {
                (10.0 * p.log10()).max(FLOOR_DB)
            } else {
                FLOOR_DB
            }
        }))
    }

    /// Forget the history, e.g. after a pause, so the next curve starts fresh.
    pub fn reset(&mut self) {
        self.filled = 0;
        self.primed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::noise::Channel;

    const FS: f32 = 48_000.0;

    fn analyse(mut source: impl FnMut() -> f32, seconds: f32) -> [f32; BANDS] {
        let mut a = Analyzer::new(FS);
        let mut last = None;
        let hop = (FS / 30.0) as usize;
        for i in 0..(seconds * FS) as usize {
            a.push(source());
            if i % hop == 0 {
                last = a.bands().or(last);
            }
        }
        last.unwrap_or([FLOOR_DB; BANDS])
    }

    /// dB/oct between the bands nearest 100 Hz and 5 kHz.
    fn slope(bands: &[f32; BANDS]) -> f32 {
        let centres = band_centres();
        let nearest = |f: f32| {
            (0..BANDS)
                .min_by(|&a, &b| (centres[a] - f).abs().total_cmp(&(centres[b] - f).abs()))
                .unwrap_or(0)
        };
        // Average a few bands on each side to tame the noise.
        let avg = |c: usize| bands[c - 2..=c + 2].iter().sum::<f32>() / 5.0;
        let (lo, hi) = (nearest(100.0), nearest(5_000.0));
        (avg(hi) - avg(lo)) / (centres[hi] / centres[lo]).log2()
    }

    #[test]
    fn pink_noise_reads_minus_3_db_per_octave_and_white_reads_flat() {
        let mut pink = Channel::new(1, FS);
        let mut white = Channel::new(2, FS);
        let p = slope(&analyse(|| pink.sample()[0], 6.0));
        let w = slope(&analyse(|| white.sample()[2], 6.0));
        assert!((p + 3.0).abs() < 0.6, "pink {p:.2} dB/oct");
        assert!(w.abs() < 0.6, "white {w:.2} dB/oct");
    }

    #[test]
    fn a_sine_peaks_in_its_band() {
        let mut t = 0.0f32;
        let bands = analyse(
            || {
                t += 1.0 / FS;
                0.5 * (std::f32::consts::TAU * 1_000.0 * t).sin()
            },
            1.0,
        );
        let peak = (0..BANDS)
            .max_by(|&a, &b| bands[a].total_cmp(&bands[b]))
            .unwrap_or(0);
        let centre = band_centres()[peak];
        assert!((900.0..1_120.0).contains(&centre), "peak at {centre} Hz");
    }

    #[test]
    fn silence_is_the_floor_and_short_input_is_none() {
        let mut a = Analyzer::new(FS);
        for _ in 0..100 {
            a.push(0.0);
        }
        assert!(a.bands().is_none());
        assert_eq!(analyse(|| 0.0, 0.5), [FLOOR_DB; BANDS]);
    }

    #[test]
    fn every_band_maps_to_bins_below_nyquist() {
        for fs in [44_100.0, 48_000.0, 96_000.0] {
            let a = Analyzer::new(fs);
            assert!(a.weights.iter().all(|w| !w.is_empty()), "{fs}");
        }
    }
}
