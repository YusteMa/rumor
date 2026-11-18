//! Prepares an ambient recording for Rumor (SPEC §3.5): from the original
//! WAVs to the 48 kHz, 16-bit FLAC the app downloads and plays.
//!
//! 16 bits, with TPDF dither: the quietest thing in a field recording, its
//! own background noise, sits some 30 dB above 16-bit dither, so more bits
//! would only store more of that noise (and flacenc packs 24-bit noise-like
//! audio worse than raw PCM).
//!
//!   cargo run --release --example ambient_prep -- <id> <out dir> <in.wav>... [options]
//!
//! Options:
//!   --trim <s>          seconds dropped at both ends of every input (default 0.5:
//!                       recorder handling noise)
//!   --cut <i>:<a>-<b>   drop seconds a..b of input i (0-based), e.g. a car going
//!                       by; repeatable
//!
//! Steps: read → stereo → resample to 48 kHz → trims and cuts → inputs joined
//! with 2 s crossfades → 25 Hz high-pass → loudness-matched to the noises
//! (K-weighted, the median of 400 ms windows, so thunder doesn't pull the
//! rain down) → rare peaks over −1 dBFS softened → whole 4096-frame blocks →
//! FLAC. Prints the size and SHA-256 for `ambients::CATALOG`.

use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use rumor_lib::audio::ambient::AMBIENT_RATE;
use rumor_lib::audio::noise::{KWeighting, TARGET_K_RMS_DB};
use rumor_lib::audio::resample::Resampler;
use sha2::{Digest, Sha256};

const JOIN_S: f32 = 2.0;
const BLOCK: usize = 4_096;
const PEAK_CEILING_DB: f32 = -1.0;

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("ambient_prep: {msg}");
    std::process::exit(1);
}

struct Args {
    id: String,
    out: PathBuf,
    inputs: Vec<PathBuf>,
    trim: f32,
    cuts: Vec<(usize, f32, f32)>,
}

fn args() -> Args {
    let mut it = std::env::args().skip(1);
    let id = it
        .next()
        .unwrap_or_else(|| fail("usage: <id> <out dir> <in.wav>..."));
    let out = PathBuf::from(it.next().unwrap_or_else(|| fail("missing <out dir>")));
    let mut a = Args {
        id,
        out,
        inputs: Vec::new(),
        trim: 0.5,
        cuts: Vec::new(),
    };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--trim" => {
                a.trim = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| fail("--trim <seconds>"));
            }
            "--cut" => {
                let v = it.next().unwrap_or_else(|| fail("--cut <i>:<a>-<b>"));
                let parsed = v.split_once(':').and_then(|(i, r)| {
                    let (s, e) = r.split_once('-')?;
                    Some((i.parse().ok()?, s.parse().ok()?, e.parse().ok()?))
                });
                a.cuts
                    .push(parsed.unwrap_or_else(|| fail(format!("bad --cut {v}"))));
            }
            _ => a.inputs.push(PathBuf::from(x)),
        }
    }
    if a.inputs.is_empty() {
        fail("no input files");
    }
    a
}

/// Reads a WAV as stereo f32 and its rate.
fn read(path: &PathBuf) -> (Vec<[f32; 2]>, u32) {
    let mut r =
        hound::WavReader::open(path).unwrap_or_else(|e| fail(format!("{}: {e}", path.display())));
    let spec = r.spec();
    let ch = usize::from(spec.channels);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 * scale)
                .collect()
        }
    };
    let frames = samples
        .chunks_exact(ch)
        .map(|f| if ch == 1 { [f[0], f[0]] } else { [f[0], f[1]] })
        .collect();
    println!(
        "  {}: {} Hz, {} bit, {} ch, {:.1} s",
        path.display(),
        spec.sample_rate,
        spec.bits_per_sample,
        ch,
        samples.len() as f32 / ch as f32 / spec.sample_rate as f32
    );
    (frames, spec.sample_rate)
}

fn main() {
    let a = args();
    let rate = AMBIENT_RATE as f32;
    println!("{}", a.id);

    // Read, resample, trim and cut every input.
    let mut parts: Vec<Vec<[f32; 2]>> = Vec::new();
    for (i, path) in a.inputs.iter().enumerate() {
        let (x, from) = read(path);
        let mut y = Vec::with_capacity(x.len() * AMBIENT_RATE as usize / from as usize + 1);
        let mut rs = Resampler::new(from, AMBIENT_RATE);
        rs.process(&x, &mut y);
        let n = y.len();
        let trim = ((a.trim * rate) as usize).min(n / 4);
        let mut keep = vec![true; n];
        keep[..trim].fill(false);
        keep[n - trim..].fill(false);
        for &(_, s, e) in a.cuts.iter().filter(|c| c.0 == i) {
            let (s, e) = (((s * rate) as usize).min(n), ((e * rate) as usize).min(n));
            keep[s..e].fill(false);
        }
        // Each kept run is its own part, so cuts are crossfaded, not spliced.
        let mut run = Vec::new();
        for (f, k) in y.into_iter().zip(keep) {
            if k {
                run.push(f);
            } else if !run.is_empty() {
                parts.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            parts.push(run);
        }
    }

    // Join with equal-power crossfades.
    let join = (JOIN_S * rate) as usize;
    let mut out: Vec<[f32; 2]> = Vec::new();
    for p in parts {
        let x = join.min(out.len()).min(p.len() / 2);
        let start = out.len() - x;
        for k in 0..x {
            let t = (k as f32 + 0.5) / x as f32 * FRAC_PI_2;
            for c in 0..2 {
                out[start + k][c] = out[start + k][c] * t.cos() + p[k][c] * t.sin();
            }
        }
        out.extend_from_slice(&p[x..]);
    }

    // 25 Hz high-pass (2nd order Butterworth): DC and wind rumble.
    let w = (std::f64::consts::PI * 25.0 / f64::from(AMBIENT_RATE)).tan();
    let k = 1.0 / (1.0 + std::f64::consts::SQRT_2 * w + w * w);
    let (b0, a1, a2) = (
        k,
        2.0 * (w * w - 1.0) * k,
        (1.0 - std::f64::consts::SQRT_2 * w + w * w) * k,
    );
    for c in 0..2 {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0, 0.0, 0.0);
        for f in &mut out {
            let x0 = f64::from(f[c]);
            let y0 = b0 * (x0 - 2.0 * x1 + x2) - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, x0, y1, y0);
            f[c] = y0 as f32;
        }
    }

    // Loudness: median K-weighted power of 400 ms windows, per channel.
    let mut kw = [KWeighting::new(f64::from(AMBIENT_RATE)); 2];
    let win = (0.4 * rate) as usize;
    let mut powers: Vec<f64> = out
        .chunks(win)
        .filter(|c| c.len() == win)
        .map(|c| {
            c.iter()
                .map(|f| {
                    let l = kw[0].process(f64::from(f[0]));
                    let r = kw[1].process(f64::from(f[1]));
                    (l * l + r * r) / 2.0
                })
                .sum::<f64>()
                / win as f64
        })
        .collect();
    powers.sort_by(|a, b| a.total_cmp(b));
    let median = powers
        .get(powers.len() / 2)
        .copied()
        .unwrap_or_else(|| fail("too short"));
    let gain = (f64::from(TARGET_K_RMS_DB) / 20.0 * std::f64::consts::LN_10).exp() / median.sqrt();
    println!(
        "  gain {:+.1} dB to {TARGET_K_RMS_DB} dB K-RMS",
        20.0 * gain.log10()
    );

    // Gain, then a soft knee above −4 dBFS that never passes the ceiling.
    let ceiling = 10f32.powf(PEAK_CEILING_DB / 20.0);
    let knee = 10f32.powf(-4.0 / 20.0);
    let (mut peak, mut softened) = (0.0f32, 0usize);
    for f in &mut out {
        for s in f.iter_mut() {
            let v = *s * gain as f32;
            peak = peak.max(v.abs());
            *s = if v.abs() <= knee {
                v
            } else {
                softened += 1;
                v.signum()
                    * (knee + (ceiling - knee) * ((v.abs() - knee) / (ceiling - knee)).tanh())
            };
        }
    }
    println!(
        "  peak {:+.1} dBFS before the knee, {:.4}% of samples softened",
        20.0 * peak.log10(),
        100.0 * softened as f64 / (2 * out.len()) as f64
    );

    // Whole blocks only (see the FLAC test in `audio::ambient`).
    out.truncate(out.len() / BLOCK * BLOCK);
    // TPDF dither: two uniform randoms, ±1 LSB peak.
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    let mut uniform = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / 16_777_216.0
    };
    let samples: Vec<i32> = out
        .iter()
        .flat_map(|f| f.map(|s| s * 32_768.0 + uniform() - uniform()))
        .map(|v| (v.round() as i32).clamp(-32_768, 32_767))
        .collect();
    let mut config = flacenc::config::Encoder::default();
    config.block_size = BLOCK;
    let config = config
        .into_verified()
        .unwrap_or_else(|e| fail(format!("{e:?}")));
    let source = flacenc::source::MemSource::from_samples(&samples, 2, 16, AMBIENT_RATE as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, BLOCK)
        .unwrap_or_else(|e| fail(format!("encode: {e:?}")));
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .unwrap_or_else(|e| fail(format!("write: {e:?}")));
    let bytes = sink.as_slice();

    std::fs::create_dir_all(&a.out).unwrap_or_else(|e| fail(e));
    let path = a.out.join(format!("{}.flac", a.id));
    std::fs::write(&path, bytes).unwrap_or_else(|e| fail(e));
    let hash: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!(
        "  {}: {:.1} min, {:.1} MB\n  bytes: {},\n  sha256: \"{hash}\",",
        path.display(),
        out.len() as f32 / rate / 60.0,
        bytes.len() as f32 / 1e6,
        bytes.len()
    );
}
