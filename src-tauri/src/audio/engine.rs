//! The audio engine. A control thread owns the `cpal` stream, rebuilds it on
//! errors and output changes, and applies the headphones policy (SPEC §3.3).
//! The audio callback only renders: no allocation, locks, logging or I/O
//! (CLAUDE.md rule 1).
//!
//! Streams are always opened on a concrete endpoint, never on cpal's
//! "default device", which silently follows the system default. Following it
//! would move the noise to the speakers the instant headphones are unplugged,
//! before the policy had a say.

use std::collections::HashMap;
use std::f32::consts::FRAC_1_SQRT_2;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use serde::{Deserialize, Serialize};

use super::analysis::{Analyzer, BANDS};
use super::device_watch::{
    Endpoint, LockPolicy, OutputChangePolicy, OutputKind, PauseReason, SystemEvent, SystemWatch,
    Verdict, on_output_change, resumes_after_unlock, resumes_on,
};
use super::noise::{Controls, NoiseColor, NoiseRenderer, calibrate};
use super::params::Params;

/// How often the control thread checks the stream when nothing wakes it.
const TICK: Duration = Duration::from_millis(50);
/// After a stream error, how long to wait for Windows to report the new
/// default output before deciding on our own.
const FAILURE_GRACE: Duration = Duration::from_millis(300);
/// Retry interval while there is no usable output.
const RETRY: Duration = Duration::from_secs(2);
/// Backstop poll of the default output, in case a notification is missed.
const DEFAULT_POLL: Duration = Duration::from_secs(1);
/// How often the spectrum is sent to the panel (~30 fps).
const SPECTRUM_FRAME: Duration = Duration::from_millis(33);
/// Longest wait for a fade-out on the old output before switching.
const SWITCH_FADE_LIMIT: Duration = Duration::from_millis(3_200);

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("could not start the audio control thread: {0}")]
    Spawn(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
enum StreamError {
    #[error(transparent)]
    Cpal(#[from] cpal::Error),
    #[error("unsupported sample format {0}")]
    Format(SampleFormat),
}

/// Engine settings from SPEC §3.3 / §4.4 "Playback" and "Advanced".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineConfig {
    pub on_output_change: OutputChangePolicy,
    pub resume_on_reconnect: bool,
    /// What locking the PC does (SPEC §3.3).
    pub on_lock: LockPolicy,
    /// Endpoint id of a fixed output; `None` follows Windows' default. A
    /// fixed output that disappears pauses the noise: it never falls back to
    /// another device (CLAUDE.md rule 10).
    pub output_device: Option<String>,
    /// Fixed buffer length, if the device supports it; `None` = Windows'.
    pub buffer_ms: Option<u32>,
}

/// The output currently in use, for the panel and Settings → Advanced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    pub endpoint: Endpoint,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// Things the engine did on its own. User actions are not echoed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    AutoPaused(PauseReason),
    AutoResumed,
    OutputChanged(Option<OutputInfo>),
    StreamError(String),
}

type EventSink = Box<dyn Fn(EngineEvent) + Send>;

enum Command {
    Wake,
    /// The output device or buffer setting changed: reopen the stream.
    Reopen,
    System(SystemEvent),
    Shutdown,
}

/// State shared by the API, the control thread and the audio callback.
struct Shared {
    params: Params,
    /// Set by the callback once a fade-out has finished.
    silent: AtomicBool,
    /// Generation of a stream whose error callback fired (0 = none).
    failed: AtomicU64,
    frames: AtomicU64,
    config: Mutex<EngineConfig>,
    paused_for: Mutex<Option<PauseReason>>,
    output: Mutex<Option<OutputInfo>>,
    /// The callback feeds the spectrum tap only while this is set.
    spectrum_on: AtomicBool,
    /// Development aid: render (and analyse) but send silence to the device.
    muted: AtomicBool,
    shutdown: AtomicBool,
    tap: Mutex<Option<Tap>>,
}

/// Mono copy of the output for the spectrum, from the current stream.
struct Tap {
    consumer: rtrb::Consumer<f32>,
    sample_rate: u32,
}

/// A poisoned lock only means another thread panicked mid-update of plain
/// data; keep going with what is there.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Handle to the engine. Dropping it stops the audio.
pub struct AudioEngine {
    shared: Arc<Shared>,
    tx: Sender<Command>,
    thread: Option<JoinHandle<()>>,
    spectrum_thread: Option<JoinHandle<()>>,
}

impl AudioEngine {
    /// Starts the control thread, which opens the default output. The stream
    /// is built paused unless `initial.playing` is set. `on_event` runs on the
    /// control thread; `on_spectrum` on the spectrum thread, ~30 times a
    /// second while `set_spectrum(true)`.
    pub fn start(
        initial: Controls,
        config: EngineConfig,
        on_event: impl Fn(EngineEvent) + Send + 'static,
        on_spectrum: impl Fn([f32; BANDS]) + Send + 'static,
    ) -> Result<Self, EngineError> {
        let shared = Arc::new(Shared {
            params: Params::new(&initial),
            silent: AtomicBool::new(true),
            failed: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            config: Mutex::new(config),
            paused_for: Mutex::new(None),
            output: Mutex::new(None),
            spectrum_on: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            tap: Mutex::new(None),
        });
        let (tx, rx) = mpsc::channel();
        let control = Control::new(shared.clone(), tx.clone(), rx, Box::new(on_event));
        let thread = std::thread::Builder::new()
            .name("rumor-audio-control".into())
            .spawn(move || control.run())?;
        let spectrum_shared = shared.clone();
        let spectrum_thread = std::thread::Builder::new()
            .name("rumor-spectrum".into())
            .spawn(move || run_spectrum(&spectrum_shared, on_spectrum))?;
        Ok(Self {
            shared,
            tx,
            thread: Some(thread),
            spectrum_thread: Some(spectrum_thread),
        })
    }

    fn wake(&self) {
        let _ = self.tx.send(Command::Wake);
    }

    pub fn set_playing(&self, playing: bool) {
        *lock(&self.shared.paused_for) = None;
        self.shared.params.set_playing(playing);
        self.wake();
    }

    /// Returns the new state.
    pub fn toggle(&self) -> bool {
        let playing = !self.is_playing();
        self.set_playing(playing);
        playing
    }

    pub fn is_playing(&self) -> bool {
        self.shared.params.playing()
    }

    pub fn set_color(&self, color: NoiseColor) {
        self.shared.params.set_color(color);
    }

    /// Slider position in 0..=1.
    pub fn set_volume(&self, volume: f32) {
        self.shared.params.set_volume(volume);
    }

    pub fn set_fade_in_ms(&self, ms: f32) {
        self.shared.params.set_fade_in_ms(ms);
    }

    pub fn set_fade_out_ms(&self, ms: f32) {
        self.shared.params.set_fade_out_ms(ms);
    }

    pub fn set_mono(&self, mono: bool) {
        self.shared.params.set_mono(mono);
    }

    pub fn set_config(&self, config: EngineConfig) {
        let reopen = {
            let mut current = lock(&self.shared.config);
            let reopen = current.output_device != config.output_device
                || current.buffer_ms != config.buffer_ms;
            *current = config;
            reopen
        };
        let _ = self.tx.send(if reopen {
            Command::Reopen
        } else {
            Command::Wake
        });
    }

    /// Why the engine paused itself, until the user plays or pauses again.
    pub fn paused_for(&self) -> Option<PauseReason> {
        *lock(&self.shared.paused_for)
    }

    pub fn output(&self) -> Option<OutputInfo> {
        lock(&self.shared.output).clone()
    }

    /// Turns the spectrum on while the panel is visible, off otherwise.
    pub fn set_spectrum(&self, on: bool) {
        self.shared.spectrum_on.store(on, Ordering::Relaxed);
        if let Some(t) = &self.spectrum_thread {
            t.thread().unpark();
        }
    }

    /// Development aid (screenshots, tests): keep rendering and analysing,
    /// but send silence to the device.
    pub fn set_output_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    /// Frames rendered since start (diagnostics).
    pub fn frames_rendered(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        self.shared.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.spectrum_thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

struct Live {
    stream: cpal::Stream,
    generation: u64,
    endpoint: Endpoint,
    running: bool,
}

struct Control {
    shared: Arc<Shared>,
    tx: Sender<Command>,
    rx: Receiver<Command>,
    on_event: EventSink,
    host: cpal::Host,
    live: Option<Live>,
    generation: u64,
    gains: HashMap<u32, [f32; 3]>,
    /// The endpoint of the last stream, kept after it dies so the policy can
    /// compare against where the noise was playing.
    last_endpoint: Option<Endpoint>,
    retry_at: Option<Instant>,
    switch_at: Option<Instant>,
    next_poll: Instant,
}

impl Control {
    fn new(
        shared: Arc<Shared>,
        tx: Sender<Command>,
        rx: Receiver<Command>,
        on_event: EventSink,
    ) -> Self {
        Self {
            shared,
            tx,
            rx,
            on_event,
            host: cpal::default_host(),
            live: None,
            generation: 0,
            gains: HashMap::new(),
            last_endpoint: None,
            retry_at: Some(Instant::now()),
            switch_at: None,
            next_poll: Instant::now() + DEFAULT_POLL,
        }
    }

    fn run(mut self) {
        let tx = self.tx.clone();
        let _watch = SystemWatch::start(move |event| {
            let _ = tx.send(Command::System(event));
        })
        .map_err(|e| log::warn!("output notifications unavailable, polling only: {e}"))
        .ok();

        loop {
            match self.rx.recv_timeout(TICK) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Command::Wake) | Err(RecvTimeoutError::Timeout) => {}
                Ok(Command::Reopen) => {
                    log::info!("output settings changed; reopening");
                    self.switch_at = None;
                    self.drop_live();
                    self.reconcile();
                }
                Ok(Command::System(event)) => self.on_system(event),
            }
            self.tick();
        }
        // Stop the stream before the watch unregisters.
        self.live = None;
    }

    fn emit(&self, event: EngineEvent) {
        (self.on_event)(event);
    }

    fn on_system(&mut self, event: SystemEvent) {
        match event {
            SystemEvent::OutputChanged => self.reconcile(),
            SystemEvent::Suspending => {
                log::info!("system suspending");
                // SPEC §3.3: after sleep, stay paused.
                if self.shared.params.playing() {
                    self.auto_pause(PauseReason::Suspended);
                }
            }
            SystemEvent::Locked => {
                let policy = lock(&self.shared.config).on_lock;
                if policy != LockPolicy::Nothing && self.shared.params.playing() {
                    log::info!("session locked: pausing");
                    self.auto_pause(PauseReason::Locked);
                }
            }
            SystemEvent::Unlocked => {
                let policy = lock(&self.shared.config).on_lock;
                let paused_for = *lock(&self.shared.paused_for);
                let to = self.live.as_ref().map(|l| l.endpoint.clone());
                if resumes_after_unlock(policy, paused_for, to.as_ref()) {
                    log::info!("session unlocked, still on headphones: resuming");
                    self.auto_resume();
                } else if paused_for == Some(PauseReason::Locked) {
                    log::info!("session unlocked: staying paused");
                }
            }
            SystemEvent::Resumed => {
                log::info!("system resumed");
                // A stream that died during sleep reports an error; this just
                // checks sooner.
                self.retry_at = Some(Instant::now() + FAILURE_GRACE);
            }
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let playing = self.shared.params.playing();

        // A stream reported an error: drop it and give Windows a moment to
        // announce a new default output before reconnecting.
        let failed = self.shared.failed.swap(0, Ordering::Relaxed);
        if failed != 0 && self.live.as_ref().is_some_and(|l| l.generation == failed) {
            log::warn!("audio stream failed; reconnecting");
            self.drop_live();
            // A dead stream can't finish a fade-out, so don't wait for one.
            self.switch_at = None;
            self.retry_at = Some(now + FAILURE_GRACE);
        }

        if let Some(at) = self.switch_at {
            if now >= at || self.shared.silent.load(Ordering::Relaxed) {
                self.switch_at = None;
                self.drop_live();
                self.reconcile();
            }
        }

        if self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
            self.reconcile();
        }

        if now >= self.next_poll {
            self.next_poll = now + DEFAULT_POLL;
            if let Some(live) = &self.live
                && self.default_id().as_deref() != Some(live.endpoint.id.as_str())
            {
                self.reconcile();
            }
        }

        match &mut self.live {
            Some(live) if playing && !live.running => match live.stream.play() {
                Ok(()) => live.running = true,
                Err(e) => {
                    log::warn!("could not start the stream: {e}");
                    self.shared.failed.store(live.generation, Ordering::Relaxed);
                }
            },
            // Stop the device once the fade-out is over, so an idle Rumor never
            // keeps Windows' audio engine (and the PC) awake.
            Some(live)
                if !playing && live.running && self.shared.silent.load(Ordering::Relaxed) =>
            {
                if live.stream.pause().is_ok() {
                    live.running = false;
                }
            }
            None if playing && self.retry_at.is_none() && self.switch_at.is_none() => {
                self.retry_at = Some(now);
            }
            _ => {}
        }
    }

    /// Makes the stream match the current default output, applying the
    /// headphones policy if the output changed.
    fn reconcile(&mut self) {
        if self.switch_at.is_some() {
            return;
        }
        let target = self.default_output();
        let to = target.as_ref().map(|(_, e)| e);
        if let (Some(live), Some(to)) = (&self.live, to) {
            if live.endpoint.id == to.id {
                return;
            }
        }

        let from = self
            .live
            .as_ref()
            .map(|l| &l.endpoint)
            .or(self.last_endpoint.as_ref())
            .cloned();
        let config = lock(&self.shared.config).clone();
        if self.shared.params.playing() {
            if let Verdict::Pause(reason) =
                on_output_change(config.on_output_change, from.as_ref(), to)
            {
                self.auto_pause(reason);
            }
        } else if resumes_on(
            config.resume_on_reconnect,
            *lock(&self.shared.paused_for),
            to,
        ) {
            self.auto_resume();
        }

        // If we just paused and the old output still works (a manual switch,
        // not an unplug), let it fade out instead of cutting it.
        let old_is_fading = self.live.as_ref().is_some_and(|l| l.running)
            && !self.shared.params.playing()
            && !self.shared.silent.load(Ordering::Relaxed);
        if old_is_fading {
            self.switch_at = Some(Instant::now() + SWITCH_FADE_LIMIT);
            return;
        }

        self.drop_live();
        match target {
            Some((device, endpoint)) => self.open(device, endpoint),
            None => {
                if lock(&self.shared.output).take().is_some() {
                    log::warn!("no audio output");
                    self.emit(EngineEvent::OutputChanged(None));
                }
                self.retry_at = Some(Instant::now() + RETRY);
            }
        }
    }

    fn auto_pause(&mut self, reason: PauseReason) {
        log::info!("pausing: {reason:?}");
        self.shared.params.set_playing(false);
        *lock(&self.shared.paused_for) = Some(reason);
        self.emit(EngineEvent::AutoPaused(reason));
    }

    fn auto_resume(&mut self) {
        log::info!("resuming on its own");
        self.shared.params.set_playing(true);
        *lock(&self.shared.paused_for) = None;
        self.emit(EngineEvent::AutoResumed);
    }

    fn drop_live(&mut self) {
        if let Some(live) = self.live.take() {
            self.last_endpoint = Some(live.endpoint);
        }
    }

    /// The output to use: the fixed one from Settings, or Windows' default.
    fn target_id(&self) -> Option<cpal::DeviceId> {
        let fixed = lock(&self.shared.config).output_device.clone();
        match fixed {
            Some(id) => id.parse().ok(),
            None => self.host.default_output_device()?.id().ok(),
        }
    }

    fn default_id(&self) -> Option<String> {
        self.target_id().map(|id| id.to_string())
    }

    /// The target output as a concrete device (see the module docs), or
    /// `None` if it isn't there.
    fn default_output(&self) -> Option<(cpal::Device, Endpoint)> {
        let device = self.host.device_by_id(&self.target_id()?)?;
        let endpoint = endpoint_of(&device)?;
        Some((device, endpoint))
    }

    fn open(&mut self, device: cpal::Device, endpoint: Endpoint) {
        match self.build(&device, &endpoint) {
            Ok((live, info)) => {
                log::info!(
                    "output: {} ({:?}, {} Hz, {} ch, {})",
                    endpoint.name,
                    endpoint.kind,
                    info.sample_rate,
                    info.channels,
                    info.sample_format
                );
                self.live = Some(live);
                *lock(&self.shared.output) = Some(info.clone());
                self.emit(EngineEvent::OutputChanged(Some(info)));
            }
            Err(e) => {
                log::warn!("could not open {}: {e}", endpoint.name);
                self.last_endpoint = Some(endpoint);
                self.emit(EngineEvent::StreamError(e.to_string()));
                self.retry_at = Some(Instant::now() + RETRY);
            }
        }
    }

    fn build(
        &mut self,
        device: &cpal::Device,
        endpoint: &Endpoint,
    ) -> Result<(Live, OutputInfo), StreamError> {
        let supported = device.default_output_config()?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let format = supported.sample_format();
        let wanted = lock(&self.shared.config).buffer_ms;
        let buffer_size = match (wanted, supported.buffer_size()) {
            (Some(ms), cpal::SupportedBufferSize::Range { min, max }) => {
                let frames = (sample_rate * ms / 1000).clamp(*min, *max);
                BufferSize::Fixed(frames)
            }
            _ => BufferSize::Default,
        };
        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size,
        };

        // Calibration renders ~30 s of noise: once per sample rate, here,
        // never on the audio thread.
        let gains = *self
            .gains
            .entry(sample_rate)
            .or_insert_with(|| calibrate(sample_rate as f32));
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64)
            ^ self.generation;
        let renderer = NoiseRenderer::with_gains(
            sample_rate as f32,
            seed,
            gains,
            &self.shared.params.snapshot(),
        );

        self.generation += 1;
        let generation = self.generation;
        self.shared.silent.store(true, Ordering::Relaxed);
        // A quarter second of headroom; the spectrum thread drains it every 33 ms.
        let (producer, consumer) = rtrb::RingBuffer::new(sample_rate as usize / 4);
        *lock(&self.shared.tap) = Some(Tap {
            consumer,
            sample_rate,
        });
        let shared = self.shared.clone();
        let stream = match format {
            SampleFormat::F32 => {
                build_stream::<f32>(device, config, renderer, producer, shared, generation)
            }
            SampleFormat::F64 => {
                build_stream::<f64>(device, config, renderer, producer, shared, generation)
            }
            SampleFormat::I16 => {
                build_stream::<i16>(device, config, renderer, producer, shared, generation)
            }
            SampleFormat::I32 => {
                build_stream::<i32>(device, config, renderer, producer, shared, generation)
            }
            SampleFormat::U16 => {
                build_stream::<u16>(device, config, renderer, producer, shared, generation)
            }
            other => return Err(StreamError::Format(other)),
        }?;

        let running = if self.shared.params.playing() {
            stream.play()?;
            true
        } else {
            // Built but idle: the next play starts instantly (SPEC §3.3).
            let _ = stream.pause();
            false
        };

        let info = OutputInfo {
            endpoint: endpoint.clone(),
            sample_rate,
            channels,
            sample_format: format.to_string(),
        };
        let live = Live {
            stream,
            generation,
            endpoint: endpoint.clone(),
            running,
        };
        Ok((live, info))
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: NoiseRenderer,
    mut tap: rtrb::Producer<f32>,
    shared: Arc<Shared>,
    generation: u64,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let errors = shared.clone();
    device.build_output_stream(
        config,
        // The audio thread: render only.
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let controls = shared.params.snapshot();
            let feed_tap = shared.spectrum_on.load(Ordering::Relaxed);
            let muted = shared.muted.load(Ordering::Relaxed);
            for frame in data.chunks_exact_mut(channels) {
                let [l, r] = renderer.next_frame(&controls);
                if feed_tap {
                    // Wait-free; if the analyser fell behind, drop the sample.
                    let _ = tap.push(0.5 * (l + r));
                }
                if muted {
                    write_frame(frame, 0.0, 0.0);
                } else {
                    write_frame(frame, l, r);
                }
            }
            shared
                .silent
                .store(!controls.playing && renderer.is_silent(), Ordering::Relaxed);
            shared
                .frames
                .fetch_add((data.len() / channels) as u64, Ordering::Relaxed);
        },
        move |err: cpal::Error| {
            // A stream on a concrete device never reroutes; anything else
            // means it is gone.
            if err.kind() != cpal::ErrorKind::DeviceChanged {
                errors.failed.store(generation, Ordering::Relaxed);
            }
        },
        None,
    )
}

/// Stereo to the first two channels, silence on any others; mono gets an
/// equal-power downmix so it is as loud as each stereo channel.
#[inline]
fn write_frame<T: Sample + FromSample<f32>>(frame: &mut [T], l: f32, r: f32) {
    match frame {
        [mono] => *mono = T::from_sample((l + r) * FRAC_1_SQRT_2),
        [left, right, rest @ ..] => {
            *left = T::from_sample(l);
            *right = T::from_sample(r);
            rest.fill(T::EQUILIBRIUM);
        }
        [] => {}
    }
}

/// The spectrum thread: sleeps while the panel is hidden, otherwise drains
/// the tap and analyses ~30 times a second.
fn run_spectrum(shared: &Shared, on_spectrum: impl Fn([f32; BANDS])) {
    let mut analyzer: Option<Analyzer> = None;
    while !shared.shutdown.load(Ordering::Relaxed) {
        if !shared.spectrum_on.load(Ordering::Relaxed) {
            if let Some(a) = &mut analyzer {
                a.reset();
            }
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }
        std::thread::sleep(SPECTRUM_FRAME);

        let mut fresh = 0usize;
        {
            let mut tap = lock(&shared.tap);
            let Some(tap) = tap.as_mut() else { continue };
            let sample_rate = tap.sample_rate as f32;
            if analyzer
                .as_ref()
                .is_none_or(|a| a.sample_rate() != sample_rate)
            {
                analyzer = Some(Analyzer::new(sample_rate));
            }
            let Some(a) = analyzer.as_mut() else { continue };
            while let Ok(x) = tap.consumer.pop() {
                a.push(x);
                fresh += 1;
            }
        }
        let Some(a) = analyzer.as_mut() else { continue };
        if fresh == 0 {
            // Paused: start clean next time instead of mixing in stale audio.
            a.reset();
        } else if let Some(bands) = a.bands() {
            on_spectrum(bands);
        }
    }
}

/// Describes a device as an endpoint (id, friendly name, headphones or not).
fn endpoint_of(device: &cpal::Device) -> Option<Endpoint> {
    let id = device.id().ok()?;
    let description = device.description().ok();
    Some(Endpoint {
        id: id.to_string(),
        name: description
            .as_ref()
            .map_or_else(|| device.to_string(), |d| d.name().to_string()),
        kind: description.map_or(OutputKind::Public, |d| {
            OutputKind::from_device_type(d.device_type())
        }),
    })
}

/// Every output device, for Settings → Advanced.
pub fn output_devices() -> Vec<Endpoint> {
    cpal::default_host()
        .output_devices()
        .map(|devices| devices.filter_map(|d| endpoint_of(&d)).collect())
        .unwrap_or_default()
}
