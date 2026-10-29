//! Lock-free parameters shared with the audio thread. The control side writes
//! them; the audio callback reads a `Controls` snapshot once per buffer.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use super::noise::{Controls, NoiseColor};

/// An `f32` stored as its bits in an `AtomicU32`.
#[derive(Debug)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Each field is independent and every change is smoothed by the renderer,
/// so relaxed ordering is enough: a buffer seeing one change a buffer late is
/// inaudible.
#[derive(Debug)]
pub struct Params {
    playing: AtomicBool,
    color: AtomicU8,
    volume: AtomicF32,
    fade_out_ms: AtomicF32,
}

impl Params {
    pub fn new(initial: &Controls) -> Self {
        Self {
            playing: AtomicBool::new(initial.playing),
            color: AtomicU8::new(initial.color.index() as u8),
            volume: AtomicF32::new(initial.volume.clamp(0.0, 1.0)),
            fade_out_ms: AtomicF32::new(initial.fade_out_ms),
        }
    }

    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    pub fn set_color(&self, color: NoiseColor) {
        self.color.store(color.index() as u8, Ordering::Relaxed);
    }

    /// Slider position in 0..=1. Non-finite values are ignored.
    pub fn set_volume(&self, volume: f32) {
        if volume.is_finite() {
            self.volume.store(volume.clamp(0.0, 1.0));
        }
    }

    /// Length of the next fade-out, e.g. a long one when the sleep timer ends.
    pub fn set_fade_out_ms(&self, ms: f32) {
        if ms.is_finite() && ms > 0.0 {
            self.fade_out_ms.store(ms);
        }
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Everything the renderer needs, read once per audio buffer.
    #[inline]
    pub fn snapshot(&self) -> Controls {
        Controls {
            playing: self.playing.load(Ordering::Relaxed),
            color: NoiseColor::from_index(self.color.load(Ordering::Relaxed)),
            volume: self.volume.load(),
            fade_out_ms: self.fade_out_ms.load(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_every_field() {
        let p = Params::new(&Controls::default());
        p.set_playing(true);
        p.set_color(NoiseColor::White);
        p.set_volume(0.75);
        p.set_fade_out_ms(10_000.0);
        assert_eq!(
            p.snapshot(),
            Controls {
                playing: true,
                color: NoiseColor::White,
                volume: 0.75,
                fade_out_ms: 10_000.0,
            }
        );
    }

    #[test]
    fn bad_values_are_clamped_or_ignored() {
        let p = Params::new(&Controls::default());
        p.set_volume(3.0);
        assert_eq!(p.snapshot().volume, 1.0);
        p.set_volume(f32::NAN);
        assert_eq!(p.snapshot().volume, 1.0);
        p.set_fade_out_ms(-5.0);
        assert_eq!(p.snapshot().fade_out_ms, Controls::default().fade_out_ms);
    }
}
