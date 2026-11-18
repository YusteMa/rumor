//! The ambient feed thread: decodes the selected recording, shuffles it
//! (`Shuffler`), resamples it to the device rate and pushes it into a
//! lock-free ring the audio callback reads from (CLAUDE.md rule 1).
//!
//! Each frame in the ring is `[left, right, target]`. `target` is 1 while an
//! ambient is selected and 0 for noise, so the renderer's noise ↔ ambient
//! crossfade starts exactly when the new audio reaches it, not a ring's
//! length early.
//!
//! The feed outlives streams: when the engine rebuilds the stream (another
//! output, another rate) it attaches the new ring and the recording carries
//! on where it was.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ambient::{AMBIENT_RATE, FlacSource, Shuffler};
use super::resample::Resampler;

/// Audio queued ahead of the callback. Short, so a new selection is heard
/// quickly; the feed tops it up every few milliseconds.
pub const RING_MS: u32 = 200;
/// Samples per ring frame: left, right, target.
pub const RING_CHANNELS: usize = 3;
/// One ambient into another.
const SWITCH_S: f32 = 3.0;
/// After switching back to noise, the old ambient keeps playing this long so
/// the renderer's crossfade has something to fade out.
const TAIL_S: f32 = 2.0;
/// Frames rendered per step, at 48 kHz.
const CHUNK: usize = 1_024;
const IDLE: Duration = Duration::from_millis(15);

/// A ring frame count for `rate`.
pub fn ring_capacity(rate: u32) -> usize {
    (rate * RING_MS / 1_000) as usize * RING_CHANNELS
}

struct Sink {
    producer: rtrb::Producer<f32>,
    rate: u32,
}

struct Shared {
    selection: Mutex<Option<PathBuf>>,
    selection_gen: AtomicU64,
    sink: Mutex<Option<Sink>>,
    shutdown: AtomicBool,
    thread: Mutex<Option<Thread>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct AmbientFeed {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl AmbientFeed {
    pub fn start() -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            selection: Mutex::new(None),
            selection_gen: AtomicU64::new(0),
            sink: Mutex::new(None),
            shutdown: AtomicBool::new(false),
            thread: Mutex::new(None),
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("rumor-ambient".into())
            .spawn(move || run(&s))?;
        *lock(&shared.thread) = Some(thread.thread().clone());
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    fn wake(&self) {
        if let Some(t) = lock(&self.shared.thread).as_ref() {
            t.unpark();
        }
    }

    /// Plays the prepared file at `path`, or goes back to noise with `None`.
    pub fn select(&self, path: Option<PathBuf>) {
        *lock(&self.shared.selection) = path;
        self.shared.selection_gen.fetch_add(1, Ordering::Relaxed);
        self.wake();
    }

    /// Feeds a new stream's ring, at that stream's rate.
    pub fn attach(&self, producer: rtrb::Producer<f32>, rate: u32) {
        *lock(&self.shared.sink) = Some(Sink { producer, rate });
        self.wake();
    }
}

impl Drop for AmbientFeed {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        self.wake();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(path: &Path) -> Option<Shuffler<FlacSource>> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    let result = FlacSource::open(path)
        .and_then(|a| Ok((a, FlacSource::open(path)?)))
        .and_then(|(a, b)| Shuffler::new(a, b, seed));
    match result {
        Ok(s) => Some(s),
        Err(e) => {
            log::error!("ambient {}: {e}", path.display());
            None
        }
    }
}

/// What is playing, and what is fading out.
struct Player {
    current: Option<Shuffler<FlacSource>>,
    old: Option<Shuffler<FlacSource>>,
    /// Frames into the current transition, and its length.
    pos: u64,
    len: u64,
}

impl Player {
    fn select(&mut self, next: Option<Shuffler<FlacSource>>) {
        let rate = AMBIENT_RATE as f32;
        self.len = if next.is_some() && self.current.is_some() {
            (SWITCH_S * rate) as u64
        } else {
            (TAIL_S * rate) as u64
        };
        // An ambient selected again mid-transition replaces the fading one.
        self.old = self.current.take().or(self.old.take());
        self.current = next;
        self.pos = 0;
    }

    fn target(&self) -> f32 {
        if self.current.is_some() { 1.0 } else { 0.0 }
    }

    fn is_silent(&self) -> bool {
        self.current.is_none() && self.old.is_none()
    }

    fn next(&mut self) -> [f32; 2] {
        let cur = self.current.as_mut().map_or([0.0; 2], |s| s.next_frame());
        let Some(old) = self.old.as_mut() else {
            return cur;
        };
        let o = old.next_frame();
        self.pos += 1;
        let out = if self.current.is_some() {
            // Ambient to ambient: equal-power crossfade here.
            let t = self.pos as f32 / self.len as f32 * std::f32::consts::FRAC_PI_2;
            [
                o[0] * t.cos() + cur[0] * t.sin(),
                o[1] * t.cos() + cur[1] * t.sin(),
            ]
        } else {
            // Back to noise: the renderer fades this out.
            o
        };
        if self.pos >= self.len {
            self.old = None;
        }
        out
    }
}

fn run(shared: &Shared) {
    let mut player = Player {
        current: None,
        old: None,
        pos: 0,
        len: 1,
    };
    let mut seen_gen = 0;
    let mut resampler = Resampler::new(AMBIENT_RATE, AMBIENT_RATE);
    let mut block = vec![[0.0f32; 2]; CHUNK];
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(CHUNK * 5);

    while !shared.shutdown.load(Ordering::Relaxed) {
        let generation = shared.selection_gen.load(Ordering::Relaxed);
        if generation != seen_gen {
            seen_gen = generation;
            let path = lock(&shared.selection).clone();
            // Decoding starts here, off the audio thread.
            player.select(path.as_deref().and_then(open));
        }

        {
            let mut sink = lock(&shared.sink);
            if let Some(sink) = sink.as_mut() {
                if resampler.to_rate() != sink.rate {
                    resampler = Resampler::new(AMBIENT_RATE, sink.rate);
                }
                // Room for the worst case of one chunk after resampling.
                let worst = (CHUNK * sink.rate as usize).div_ceil(AMBIENT_RATE as usize) + 2;
                while sink.producer.slots() >= worst * RING_CHANNELS {
                    let target = player.target();
                    out.clear();
                    if player.is_silent() {
                        // Noise only: keep the ring flowing with silence.
                        out.resize(worst - 2, [0.0; 2]);
                    } else {
                        for f in &mut block {
                            *f = player.next();
                        }
                        resampler.process(&block, &mut out);
                    }
                    for [l, r] in &out {
                        // Never fails: the room was checked above.
                        let _ = sink.producer.push(*l);
                        let _ = sink.producer.push(*r);
                        let _ = sink.producer.push(target);
                    }
                    if generation != shared.selection_gen.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }
        std::thread::park_timeout(IDLE);
    }
}
