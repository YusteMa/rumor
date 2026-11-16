//! Rumor's audio engine for the browser (WebAssembly).
//!
//! This file adds no DSP of its own: it includes the very same
//! `src-tauri/src/audio` files the app uses (`#[path]` below) and exposes a
//! handful of plain C functions that an AudioWorklet calls. Same generators,
//! crossfade, loudness calibration, fades, soft clip and spectrum analyser.
//!
//! What stays in the app: `cpal`, devices, threads, Tauri. The browser plays
//! the samples and runs the worklet; this module only computes them.

#![allow(dead_code)] // the app files have helpers the demo doesn't call

use std::cell::RefCell;

#[path = "../../src-tauri/src/audio"]
mod audio {
    pub mod analysis;
    pub mod bell;
    pub mod noise;
    pub mod smoothing;
}

use audio::analysis::{Analyzer, BANDS};
use audio::noise::{Controls, NoiseColor, NoiseRenderer};

/// Largest block the worklet may ask for (it uses 128).
const MAX_FRAMES: usize = 1024;
/// Push one analyser frame every this many rendered samples (~30 fps at 48 kHz,
/// as in the app).
const ANALYSIS_EVERY: usize = 1600;

struct Demo {
    renderer: NoiseRenderer,
    controls: Controls,
    analyzer: Analyzer,
    since_analysis: usize,
    /// Planar output: left in `0..MAX_FRAMES`, right in `MAX_FRAMES..`.
    out: [f32; 2 * MAX_FRAMES],
    bands: [f32; BANDS],
}

// Boxed so the buffers have a fixed address that JavaScript can read.
thread_local! {
    static DEMO: RefCell<Option<Box<Demo>>> = const { RefCell::new(None) };
}

fn with<R>(default: R, f: impl FnOnce(&mut Demo) -> R) -> R {
    DEMO.with_borrow_mut(|d| d.as_deref_mut().map_or(default, f))
}

/// Builds the engine for `sample_rate` (calibrates loudness: a few ms).
#[unsafe(no_mangle)]
pub extern "C" fn rumor_init(sample_rate: f32, seed: u32) {
    let controls = Controls::default();
    let seed = u64::from(seed) | 0x5241_5552_0000_0000; // "RUMR" in the high bits
    let demo = Demo {
        renderer: NoiseRenderer::new(sample_rate, seed, &controls),
        controls,
        analyzer: Analyzer::new(sample_rate),
        since_analysis: 0,
        out: [0.0; 2 * MAX_FRAMES],
        bands: [0.0; BANDS],
    };
    DEMO.with_borrow_mut(|d| *d = Some(Box::new(demo)));
}

#[unsafe(no_mangle)]
pub extern "C" fn rumor_max_frames() -> u32 {
    MAX_FRAMES as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn rumor_bands_len() -> u32 {
    BANDS as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn rumor_set_playing(playing: u32) {
    with((), |d| {
        d.controls.playing = playing != 0;
        if d.controls.playing {
            d.analyzer.reset();
        }
    });
}

/// 0 = pink, 1 = brown, 2 = white.
#[unsafe(no_mangle)]
pub extern "C" fn rumor_set_color(color: u32) {
    with((), |d| {
        d.controls.color = NoiseColor::from_index(color as u8)
    });
}

/// Slider position in 0..=1 (the engine applies its own curve).
#[unsafe(no_mangle)]
pub extern "C" fn rumor_set_volume(volume: f32) {
    if volume.is_finite() {
        with((), |d| d.controls.volume = volume.clamp(0.0, 1.0));
    }
}

/// Renders `frames` stereo frames and returns a pointer to the planar buffer
/// (left at the pointer, right `rumor_max_frames()` floats later).
#[unsafe(no_mangle)]
pub extern "C" fn rumor_render(frames: u32) -> *const f32 {
    with(std::ptr::null(), |d| {
        let n = (frames as usize).min(MAX_FRAMES);
        let (left, right) = d.out.split_at_mut(MAX_FRAMES);
        for i in 0..n {
            let [l, r] = d.renderer.next_frame(&d.controls);
            left[i] = l;
            right[i] = r;
            // Same tap as the app: the mid signal.
            d.analyzer.push(0.5 * (l + r));
        }
        d.since_analysis += n;
        d.out.as_ptr()
    })
}

/// When an analyser frame is due, computes it and returns a pointer to
/// `rumor_bands_len()` values in dB; otherwise returns null.
#[unsafe(no_mangle)]
pub extern "C" fn rumor_bands() -> *const f32 {
    with(std::ptr::null(), |d| {
        if d.since_analysis < ANALYSIS_EVERY {
            return std::ptr::null();
        }
        d.since_analysis = 0;
        match d.analyzer.bands() {
            Some(b) => {
                d.bands = b;
                d.bands.as_ptr()
            }
            None => std::ptr::null(),
        }
    })
}

/// True once a fade-out has finished: the output is exact zeros.
#[unsafe(no_mangle)]
pub extern "C" fn rumor_is_silent() -> u32 {
    with(1, |d| u32::from(d.renderer.is_silent()))
}
