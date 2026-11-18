//! Ambients: real recordings (rain, storms…) played so the loop is never
//! heard. Instead of repeating the file, `Shuffler` keeps jumping to random
//! segments of it (25–70 s long) and joins them with long equal-power
//! crossfades, avoiding the stretches it played most recently.
//!
//! The recordings are prepared offline (`examples/ambient_prep.rs`): 48 kHz
//! stereo FLAC, loudness-matched to the noises. Decoding and shuffling run on
//! the ambient feed thread (`ambient_feed.rs`), never on the audio thread.
//!
//! `Shuffler` is pure and generic over `Source`, so it is tested with
//! synthetic sources; `FlacSource` reads the real files.

use std::f32::consts::FRAC_PI_2;
use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::units::Timestamp;
use symphonia::default::formats::FlacReader;

/// Rate every prepared ambient is stored at.
pub const AMBIENT_RATE: u32 = 48_000;
/// Crossfade between two segments of the same recording.
pub const SEGMENT_CROSSFADE_S: f32 = 4.0;
/// Shortest and longest segment played before jumping elsewhere.
pub const SEGMENT_MIN_S: f32 = 25.0;
pub const SEGMENT_MAX_S: f32 = 70.0;
/// Segments remembered so the next jump avoids them.
const RECENT: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum AmbientError {
    #[error("could not open the ambient file: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not decode the ambient file: {0}")]
    Decode(#[from] symphonia::core::errors::Error),
    #[error("the ambient file has no audio track")]
    NoTrack,
    #[error("the ambient file is not {AMBIENT_RATE} Hz")]
    Rate,
    #[error("the ambient file has no length")]
    Empty,
}

/// Random-access stereo audio at `AMBIENT_RATE`.
pub trait Source {
    /// Length in frames.
    fn frames(&self) -> u64;
    /// Moves to `frame` (0..frames).
    fn seek(&mut self, frame: u64) -> Result<(), AmbientError>;
    /// Reads up to `out.len()` frames; 0 means the end.
    fn read(&mut self, out: &mut [[f32; 2]]) -> Result<usize, AmbientError>;
}

/// A source with a read-ahead buffer, so the shuffler can take one frame at
/// a time cheaply.
struct Voice<S> {
    src: S,
    buf: Vec<[f32; 2]>,
    pos: usize,
    len: usize,
    /// The source failed or ended early: play silence until the next seek.
    dead: bool,
}

impl<S: Source> Voice<S> {
    fn new(src: S) -> Self {
        Self {
            src,
            buf: vec![[0.0; 2]; 4_096],
            pos: 0,
            len: 0,
            dead: false,
        }
    }

    fn seek(&mut self, frame: u64) {
        self.pos = 0;
        self.len = 0;
        self.dead = self.src.seek(frame).is_err();
        if self.dead {
            log::warn!("ambient: seek to frame {frame} failed");
        }
    }

    #[inline]
    fn next(&mut self) -> [f32; 2] {
        if self.pos == self.len {
            if self.dead {
                return [0.0; 2];
            }
            self.pos = 0;
            self.len = match self.src.read(&mut self.buf) {
                Ok(n) => n,
                Err(e) => {
                    log::warn!("ambient: {e}");
                    0
                }
            };
            if self.len == 0 {
                self.dead = true;
                return [0.0; 2];
            }
        }
        let s = self.buf[self.pos];
        self.pos += 1;
        s
    }
}

/// Plays a recording as an endless, non-repeating stream.
pub struct Shuffler<S> {
    voices: [Voice<S>; 2],
    /// The voice playing the current segment.
    active: usize,
    frames: u64,
    xfade: u64,
    seg_min: u64,
    seg_max: u64,
    /// Frames of the current segment left before the next crossfade starts.
    left: u64,
    /// Position within a crossfade, while one is running.
    fading: Option<u64>,
    /// Length of the segment the crossfade is fading into.
    next_len: u64,
    recent: [(u64, u64); RECENT],
    rng: u64,
}

impl<S: Source> Shuffler<S> {
    /// `a` and `b` are two readers of the same recording (one per side of a
    /// crossfade). Starts at a random point.
    pub fn new(a: S, b: S, seed: u64) -> Result<Self, AmbientError> {
        let frames = a.frames();
        if frames == 0 {
            return Err(AmbientError::Empty);
        }
        let rate = AMBIENT_RATE as f32;
        // Short recordings get shorter segments and crossfades, so a segment
        // is always at least three crossfades long.
        let xfade = ((SEGMENT_CROSSFADE_S * rate) as u64).min(frames / 6).max(1);
        let seg_max = ((SEGMENT_MAX_S * rate) as u64).min(frames);
        let seg_min = ((SEGMENT_MIN_S * rate) as u64).min(seg_max).max(3 * xfade);
        let mut s = Self {
            voices: [Voice::new(a), Voice::new(b)],
            active: 0,
            frames,
            xfade,
            seg_min,
            seg_max: seg_max.max(seg_min),
            left: 0,
            fading: None,
            next_len: 0,
            recent: [(u64::MAX, 0); RECENT],
            rng: seed | 1,
        };
        let (start, len) = s.pick();
        s.voices[0].seek(start);
        s.left = len - s.xfade;
        Ok(s)
    }

    fn rand(&mut self) -> u64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn uniform(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            lo
        } else {
            lo + self.rand() % (hi - lo + 1)
        }
    }

    /// A new segment `(start, len)` that overlaps the recent ones as little
    /// as possible.
    fn pick(&mut self) -> (u64, u64) {
        let len = self.uniform(self.seg_min, self.seg_max).min(self.frames);
        let mut best = (0, u64::MAX);
        for _ in 0..16 {
            let start = self.uniform(0, self.frames - len);
            let overlap: u64 = self
                .recent
                .iter()
                .filter(|r| r.0 != u64::MAX)
                .map(|&(s, l)| (start + len).min(s + l).saturating_sub(start.max(s)))
                .sum();
            if overlap < best.1 {
                best = (start, overlap);
                if overlap == 0 {
                    break;
                }
            }
        }
        self.recent.rotate_right(1);
        self.recent[0] = (best.0, len);
        (best.0, len)
    }

    /// The next stereo frame.
    #[inline]
    pub fn next_frame(&mut self) -> [f32; 2] {
        match self.fading {
            None => {
                let s = self.voices[self.active].next();
                self.left -= 1;
                if self.left == 0 {
                    let (start, len) = self.pick();
                    self.voices[1 - self.active].seek(start);
                    self.next_len = len;
                    self.fading = Some(0);
                }
                s
            }
            Some(k) => {
                let t = (k as f32 + 0.5) / self.xfade as f32;
                let (g_out, g_in) = ((t * FRAC_PI_2).cos(), (t * FRAC_PI_2).sin());
                let a = self.voices[self.active].next();
                let b = self.voices[1 - self.active].next();
                if k + 1 == self.xfade {
                    self.active = 1 - self.active;
                    self.fading = None;
                    // The first crossfade of the new segment is already played.
                    self.left = self.next_len - 2 * self.xfade;
                } else {
                    self.fading = Some(k + 1);
                }
                [a[0] * g_out + b[0] * g_in, a[1] * g_out + b[1] * g_in]
            }
        }
    }

    /// Fills `out` with the next frames.
    pub fn fill(&mut self, out: &mut [[f32; 2]]) {
        for o in out {
            *o = self.next_frame();
        }
    }
}

/// A prepared ambient file, decoded with symphonia.
pub struct FlacSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    frames: u64,
    channels: usize,
    /// Decoded samples not handed out yet (interleaved, file's channels).
    pending: Vec<f32>,
    pending_pos: usize,
    /// Frames still to drop after a seek landed before its target.
    skip: u64,
}

impl FlacSource {
    pub fn open(path: &Path) -> Result<Self, AmbientError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        // Only FLAC is ever stored, so skip the format probe.
        let format: Box<dyn FormatReader> =
            Box::new(FlacReader::try_new(mss, FormatOptions::default())?);
        let track = format
            .default_track(TrackType::Audio)
            .ok_or(AmbientError::NoTrack)?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or(AmbientError::NoTrack)?;
        if params.sample_rate != Some(AMBIENT_RATE) {
            return Err(AmbientError::Rate);
        }
        let channels = params.channels.as_ref().map_or(2, |c| c.count()).max(1);
        let frames = track.num_frames.ok_or(AmbientError::Empty)?;
        let id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())?;
        Ok(Self {
            format,
            decoder,
            track: id,
            frames,
            channels,
            pending: Vec::new(),
            pending_pos: 0,
            skip: 0,
        })
    }

    /// Decodes the next packet into `pending`. False at the end.
    fn decode_next(&mut self) -> Result<bool, AmbientError> {
        loop {
            let Some(packet) = self.format.next_packet()? else {
                return Ok(false);
            };
            if packet.track_id != self.track {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    buf.copy_to_vec_interleaved(&mut self.pending);
                    self.pending_pos = 0;
                    return Ok(true);
                }
                // A damaged packet: skip it, like every player does.
                Err(symphonia::core::errors::Error::DecodeError(e)) => {
                    log::warn!("ambient: bad packet skipped: {e}");
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Source for FlacSource {
    fn frames(&self) -> u64 {
        self.frames
    }

    fn seek(&mut self, frame: u64) -> Result<(), AmbientError> {
        let seeked = self.format.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts: Timestamp::new(frame.min(self.frames) as i64),
                track_id: self.track,
            },
        )?;
        self.decoder.reset();
        self.pending.clear();
        self.pending_pos = 0;
        self.skip = frame.saturating_sub(seeked.actual_ts.get().max(0) as u64);
        Ok(())
    }

    fn read(&mut self, out: &mut [[f32; 2]]) -> Result<usize, AmbientError> {
        let ch = self.channels;
        let mut n = 0;
        while n < out.len() {
            if self.pending_pos >= self.pending.len() && !self.decode_next()? {
                break;
            }
            let avail = (self.pending.len() - self.pending_pos) / ch;
            if self.skip > 0 {
                let drop = avail.min(self.skip as usize);
                self.pending_pos += drop * ch;
                self.skip -= drop as u64;
                continue;
            }
            let take = avail.min(out.len() - n);
            for (i, o) in out[n..n + take].iter_mut().enumerate() {
                let f = &self.pending[self.pending_pos + i * ch..];
                *o = if ch == 1 { [f[0], f[0]] } else { [f[0], f[1]] };
            }
            self.pending_pos += take * ch;
            n += take;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recording whose every frame says where it comes from: left = frame
    /// index, right = 1.
    struct Ramp {
        frames: u64,
        pos: u64,
    }

    impl Source for Ramp {
        fn frames(&self) -> u64 {
            self.frames
        }
        fn seek(&mut self, frame: u64) -> Result<(), AmbientError> {
            self.pos = frame;
            Ok(())
        }
        fn read(&mut self, out: &mut [[f32; 2]]) -> Result<usize, AmbientError> {
            let n = (out.len() as u64).min(self.frames - self.pos) as usize;
            for (i, o) in out[..n].iter_mut().enumerate() {
                *o = [(self.pos + i as u64) as f32, 1.0];
            }
            self.pos += n as u64;
            Ok(n)
        }
    }

    fn ramp(frames: u64) -> Ramp {
        Ramp { frames, pos: 0 }
    }

    const RATE: u64 = AMBIENT_RATE as u64;

    #[test]
    fn plays_contiguous_runs_and_never_reads_past_the_end() {
        let frames = 8 * 60 * RATE;
        let mut s = Shuffler::new(ramp(frames), ramp(frames), 7).unwrap();
        let mut prev: Option<f32> = None;
        let mut jumps = 0;
        let mut fades = 0;
        for _ in 0..10 * 60 * RATE {
            let fading = s.fading.is_some();
            if s.fading == Some(0) {
                fades += 1;
            }
            let [l, r] = s.next_frame();
            if !fading {
                assert!(l < frames as f32);
                assert!((r - 1.0).abs() < 1e-6, "a gap of silence");
                if let Some(p) = prev {
                    // f32 can't count every frame past 2^24, so allow slack.
                    if (l - p - 1.0).abs() > 4.0 {
                        jumps += 1;
                    }
                }
                prev = Some(l);
            } else {
                prev = None;
            }
        }
        assert_eq!(jumps, 0, "a jump outside a crossfade");
        // Segments of 25–70 s: at least eight in ten minutes.
        assert!(fades >= 8, "{fades} crossfades");
    }

    #[test]
    fn crossfades_keep_the_power_of_uncorrelated_audio() {
        // Equal power: g_out² + g_in² = 1 through the whole crossfade.
        let x = SEGMENT_CROSSFADE_S as u64 * RATE;
        for k in 0..x {
            let t = (k as f32 + 0.5) / x as f32;
            let p = (t * FRAC_PI_2).cos().powi(2) + (t * FRAC_PI_2).sin().powi(2);
            assert!((p - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn jumps_spread_over_the_whole_recording() {
        let frames = 10 * 60 * RATE;
        let mut s = Shuffler::new(ramp(frames), ramp(frames), 99).unwrap();
        let mut covered = vec![false; 10];
        for _ in 0..30 * 60 * RATE / 4_096 {
            let mut buf = [[0.0f32; 2]; 4_096];
            s.fill(&mut buf);
            let minute = (buf[0][0] as u64 / (60 * RATE)) as usize;
            covered[minute.min(9)] = true;
        }
        assert!(covered.iter().filter(|c| **c).count() >= 8, "{covered:?}");
    }

    #[test]
    fn short_recordings_still_shuffle() {
        // One minute: segments shrink so it still crossfades smoothly.
        let frames = 60 * RATE;
        let mut s = Shuffler::new(ramp(frames), ramp(frames), 3).unwrap();
        assert!(s.xfade <= frames / 6);
        for _ in 0..5 * 60 * RATE {
            let [_, r] = s.next_frame();
            assert!(r > 0.70, "level dipped to {r}");
        }
    }

    #[test]
    fn an_empty_recording_is_an_error() {
        assert!(matches!(
            Shuffler::new(ramp(0), ramp(0), 1),
            Err(AmbientError::Empty)
        ));
    }

    #[test]
    fn reads_a_flac_file_and_seeks_exactly() {
        use flacenc::component::BitRepr;
        use flacenc::error::Verify;

        // A whole number of 4096-frame blocks: flacenc writes a short last
        // block into STREAMINFO's minimum, which symphonia then rejects
        // (`ambient_prep` trims to whole blocks for the same reason).
        let frames: usize = 35 * 4_096;
        // A different value on every frame, to tell where a seek landed.
        let samples: Vec<i32> = (0..frames)
            .flat_map(|i| {
                let v = ((i * 7) % 30_000) as i32 - 15_000;
                [v, -v]
            })
            .collect();
        let config = flacenc::config::Encoder::default().into_verified().unwrap();
        let source = flacenc::source::MemSource::from_samples(&samples, 2, 16, RATE as usize);
        let stream =
            flacenc::encode_with_fixed_block_size(&config, source, config.block_size).unwrap();
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).unwrap();
        let path =
            std::env::temp_dir().join(format!("rumor-ambient-test-{}.flac", std::process::id()));
        std::fs::write(&path, sink.as_slice()).unwrap();

        let mut src = FlacSource::open(&path).unwrap();
        assert_eq!(src.frames(), frames as u64);
        for target in [0u64, 1, 4_095, 4_096, 77_777, frames as u64 - 10] {
            src.seek(target).unwrap();
            let mut out = [[0.0f32; 2]; 4];
            let n = src.read(&mut out).unwrap();
            assert!(n > 0);
            let expected = (((target as usize) * 7) % 30_000) as f32 - 15_000.0;
            assert!(
                (out[0][0] * 32_768.0 - expected).abs() < 0.5,
                "seek to {target}: got {}, expected {expected}",
                out[0][0] * 32_768.0
            );
            assert!((out[0][0] + out[0][1]).abs() < 1e-6);
        }
        let _ = std::fs::remove_file(&path);
    }
}
