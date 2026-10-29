//! Spectral and level tests for the noise chain (SPEC §6). Everything renders
//! to buffers; no audio device is needed.

use rumor_lib::audio::noise::{
    Controls, KWeighting, NoiseColor, NoiseRenderer, Rng, TARGET_K_RMS_DB,
};
use rumor_lib::audio::smoothing::{CLIP_CEILING, CLIP_KNEE};
use rustfft::{FftPlanner, num_complex::Complex};

const RATES: [f32; 2] = [44_100.0, 48_000.0];

fn playing(color: NoiseColor) -> Controls {
    Controls {
        playing: true,
        color,
        volume: 1.0,
        ..Controls::default()
    }
}

/// Renders `seconds` of steady-state stereo output at full volume.
fn render(color: NoiseColor, fs: f32, seconds: f32, seed: u64) -> (Vec<f32>, Vec<f32>) {
    let c = playing(color);
    let mut r = NoiseRenderer::new(fs, seed, &c);
    // Skip the fade-in and let every filter settle.
    for _ in 0..fs as usize {
        r.next_frame(&c);
    }
    let n = (seconds * fs) as usize;
    let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for _ in 0..n {
        let [l, rr] = r.next_frame(&c);
        left.push(l);
        right.push(rr);
    }
    (left, right)
}

/// Welch power spectrum: Hann window, 50 % overlap, bins 0..=n/2.
fn welch_psd(x: &[f32], n_fft: usize) -> Vec<f64> {
    let fft = FftPlanner::<f64>::new().plan_fft_forward(n_fft);
    let window: Vec<f64> = (0..n_fft)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n_fft as f64).cos())
        .collect();
    let mut psd = vec![0.0; n_fft / 2 + 1];
    let mut buf = vec![Complex::new(0.0, 0.0); n_fft];
    let mut frames = 0;
    for start in (0..=x.len() - n_fft).step_by(n_fft / 2) {
        for ((b, &s), w) in buf.iter_mut().zip(&x[start..start + n_fft]).zip(&window) {
            *b = Complex::new(f64::from(s) * w, 0.0);
        }
        fft.process(&mut buf);
        for (p, b) in psd.iter_mut().zip(&buf) {
            *p += b.norm_sqr();
        }
        frames += 1;
    }
    psd.iter().map(|p| p / frames as f64).collect()
}

/// Least-squares slope in dB per octave of the PSD averaged in third-octave
/// bands, so every octave weighs the same in the fit.
fn slope_db_per_octave(psd: &[f64], fs: f32, lo: f64, hi: f64) -> f64 {
    let bin_hz = f64::from(fs) / ((psd.len() - 1) * 2) as f64;
    let mut points = Vec::new();
    let mut fc = lo;
    while fc <= hi * 1.0001 {
        let (a, b) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
        let bins: Vec<f64> = (0..psd.len())
            .filter(|&k| (a..b).contains(&(k as f64 * bin_hz)))
            .map(|k| psd[k])
            .collect();
        assert!(!bins.is_empty(), "no FFT bins in the band around {fc} Hz");
        let mean = bins.iter().sum::<f64>() / bins.len() as f64;
        points.push((fc.log2(), 10.0 * mean.log10()));
        fc *= 2f64.powf(1.0 / 3.0);
    }
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    sxy / sxx
}

fn assert_slope(color: NoiseColor, expected: f64, tolerance: f64) {
    for fs in RATES {
        let (left, _) = render(color, fs, 60.0, 1);
        let slope = slope_db_per_octave(&welch_psd(&left, 8192), fs, 50.0, 10_000.0);
        eprintln!("{color:?} at {fs} Hz: {slope:.3} dB/oct");
        assert!(
            (slope - expected).abs() <= tolerance,
            "{color:?} at {fs} Hz: {slope:.2} dB/oct, expected {expected} ± {tolerance}"
        );
    }
}

#[test]
fn pink_falls_3_db_per_octave() {
    assert_slope(NoiseColor::Pink, -3.0, 0.5);
}

#[test]
fn brown_falls_6_db_per_octave() {
    assert_slope(NoiseColor::Brown, -6.0, 0.7);
}

#[test]
fn white_is_flat() {
    assert_slope(NoiseColor::White, 0.0, 0.5);
}

fn k_weighted_rms_db(x: &[f32], fs: f32) -> f64 {
    let mut k = KWeighting::new(f64::from(fs));
    let energy: f64 = x.iter().map(|&s| k.process(f64::from(s)).powi(2)).sum();
    10.0 * (energy / x.len() as f64).log10()
}

#[test]
fn every_colour_is_equally_loud() {
    for fs in RATES {
        let levels: Vec<f64> = NoiseColor::ALL
            .iter()
            .map(|&c| {
                let (l, r) = render(c, fs, 20.0, 7);
                (k_weighted_rms_db(&l, fs) + k_weighted_rms_db(&r, fs)) / 2.0
            })
            .collect();
        let max = levels.iter().cloned().fold(f64::MIN, f64::max);
        let min = levels.iter().cloned().fold(f64::MAX, f64::min);
        eprintln!(
            "{fs} Hz: K-weighted RMS {levels:.2?} dB, spread {:.2} dB",
            max - min
        );
        assert!(
            max - min <= 1.0,
            "{fs} Hz: K-weighted levels {levels:.2?} differ by more than 1 dB"
        );
        for level in &levels {
            assert!(
                (level - f64::from(TARGET_K_RMS_DB)).abs() <= 1.0,
                "{fs} Hz: {levels:.2?} not at the {TARGET_K_RMS_DB} dB target"
            );
        }
    }
}

#[test]
fn left_and_right_are_decorrelated() {
    for color in NoiseColor::ALL {
        let (l, r) = render(color, 48_000.0, 60.0, 3);
        let dot = |a: &[f32], b: &[f32]| -> f64 {
            a.iter()
                .zip(b)
                .map(|(&x, &y)| f64::from(x) * f64::from(y))
                .sum()
        };
        let corr = dot(&l, &r) / (dot(&l, &l) * dot(&r, &r)).sqrt();
        eprintln!("{color:?}: L/R correlation {corr:.4}");
        assert!(corr.abs() < 0.05, "{color:?}: L/R correlation {corr:.3}");
    }
}

#[test]
fn soft_clipper_almost_never_engages_at_full_volume() {
    // Quality guard: at the loudness target the clipper is a safety net, not
    // part of the sound. Brown has the highest peaks for a given loudness.
    for color in NoiseColor::ALL {
        let (l, r) = render(color, 48_000.0, 60.0, 13);
        let over = l.iter().chain(&r).filter(|s| s.abs() > CLIP_KNEE).count();
        let fraction = over as f64 / (l.len() + r.len()) as f64;
        eprintln!("{color:?}: {fraction:.2e} of samples above the clip knee");
        assert!(
            fraction < 1e-5,
            "{color:?}: clipper engaged on {fraction:.2e} of samples"
        );
    }
}

#[test]
fn ten_minutes_of_random_use_stay_finite_and_below_minus_3_dbfs() {
    let fs = 48_000.0;
    let mut rng = Rng::new(42);
    let mut c = playing(NoiseColor::Pink);
    let mut r = NoiseRenderer::new(fs, 9, &c);
    let mut peak = 0.0f32;
    let change_every = (1.5 * fs) as usize;
    for i in 0..(600.0 * fs) as usize {
        if i % change_every == 0 {
            let roll = rng.next_u64();
            c.color = NoiseColor::from_index((roll % 3) as u8);
            c.volume = (rng.bipolar() + 1.0) / 2.0;
            // Mostly playing, so most of the time is spent at full output.
            c.playing = roll % 7 != 0;
            c.fade_out_ms = if roll % 2 == 0 { 400.0 } else { 3_000.0 };
            if roll % 5 == 0 {
                c.volume = 1.0;
            }
        }
        for s in r.next_frame(&c) {
            assert!(s.is_finite(), "non-finite sample at frame {i}");
            peak = peak.max(s.abs());
        }
    }
    eprintln!("peak over 10 min: {:.2} dBFS", 20.0 * peak.log10());
    // CLIP_CEILING is −3 dBFS, so this also covers SPEC §6 "peak < 1.0".
    assert!(peak <= CLIP_CEILING, "peak {peak}");
}

#[test]
fn start_stop_and_volume_changes_are_smooth() {
    let fs = 48_000.0;
    let mut c = Controls {
        volume: 1.0,
        ..Controls::default()
    };
    let mut r = NoiseRenderer::new(fs, 5, &c);
    let mut prev = r.envelope();
    let mut step = |r: &mut NoiseRenderer, c: &Controls, frames: usize| {
        for _ in 0..frames {
            r.next_frame(c);
            let env = r.envelope();
            assert!((env - prev).abs() < 1e-3, "envelope jumped {prev} -> {env}");
            prev = env;
        }
    };

    c.playing = true;
    step(&mut r, &c, 24_000);
    assert_eq!(r.envelope(), 1.0, "fade-in should be done after 500 ms");
    c.volume = 0.0;
    step(&mut r, &c, 24_000);
    c.volume = 1.0;
    step(&mut r, &c, 24_000);
    c.playing = false;
    step(&mut r, &c, 400 * 48 + 48);
    assert!(r.is_silent(), "fade-out should be done after 401 ms");
    assert_eq!(r.next_frame(&c), [0.0, 0.0]);
}

#[test]
fn colour_changes_crossfade_smoothly() {
    let fs = 48_000.0;
    let mut c = playing(NoiseColor::Pink);
    let mut r = NoiseRenderer::new(fs, 11, &c);
    let mut prev = r.color_weights();
    // Including a change in the middle of another crossfade.
    for (color, frames) in [
        (NoiseColor::Brown, 7_200),
        (NoiseColor::White, 3_000),
        (NoiseColor::Pink, 9_600),
    ] {
        c.color = color;
        for _ in 0..frames {
            r.next_frame(&c);
            let w = r.color_weights();
            for (a, b) in prev.iter().zip(&w) {
                assert!((a - b).abs() < 1e-3, "weight jumped {prev:?} -> {w:?}");
            }
            prev = w;
        }
    }
    assert_eq!(r.color_weights(), [1.0, 0.0, 0.0]);
}
