//! Shared `AppState` and the coordinator: every user action (tray, shortcut,
//! panel) goes through here, which updates the engine and then everything
//! that shows the state. Modules never call each other directly (CLAUDE.md
//! rule 12).

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::audio::device_watch::{OutputKind, PauseReason};
use crate::audio::engine::{AudioEngine, EngineConfig, EngineEvent};
use crate::audio::noise::{Controls, FADE_OUT_MS, NoiseColor};
use crate::i18n::{Lang, Strings};
use crate::timer::{END_FADE_MS, Timer};
use crate::{panel, shortcut, tray};

/// What the user chose for the sound (persisted in phase 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub color: NoiseColor,
    /// Slider position in 0..=1.
    pub volume: f32,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            color: NoiseColor::Pink,
            volume: 0.4,
        }
    }
}

/// Everything the tray needs to draw itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub playing: bool,
    pub sound: Sound,
    pub paused_for: Option<PauseReason>,
    pub timer_remaining: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSummary {
    pub name: String,
    pub kind: OutputKind,
}

/// The full state the panel renders (`get_state`, `state-changed`).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelState {
    pub playing: bool,
    pub color: NoiseColor,
    pub volume: f32,
    pub timer_minutes: u32,
    pub timer_remaining: Option<u64>,
    pub paused_for: Option<PauseReason>,
    pub output: Option<OutputSummary>,
    pub shortcut: Option<String>,
    /// The preferred shortcut, when another app owns it.
    pub shortcut_taken: Option<String>,
    pub lang: Lang,
    pub mica: bool,
}

pub struct AppState {
    pub engine: AudioEngine,
    pub strings: Strings,
    pub lang: Lang,
    sound: Mutex<Sound>,
    timer: Mutex<Timer>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            playing: self.engine.is_playing(),
            sound: *lock(&self.sound),
            paused_for: self.engine.paused_for(),
            timer_remaining: lock(&self.timer).remaining_secs(Instant::now()),
        }
    }
}

pub fn panel_state(app: &AppHandle) -> Option<PanelState> {
    let state = app.try_state::<AppState>()?;
    let snapshot = state.snapshot();
    Some(PanelState {
        playing: snapshot.playing,
        color: snapshot.sound.color,
        volume: snapshot.sound.volume,
        timer_minutes: lock(&state.timer).minutes(),
        timer_remaining: snapshot.timer_remaining,
        paused_for: snapshot.paused_for,
        output: state.engine.output().map(|o| OutputSummary {
            name: o.endpoint.name,
            kind: o.endpoint.kind,
        }),
        shortcut: shortcut::toggle_label(app),
        shortcut_taken: shortcut::preferred_taken(app),
        lang: state.lang,
        mica: panel::uses_mica(app),
    })
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let sound = Sound::default();
    let events = app.clone();
    let spectrum = app.clone();
    let engine = AudioEngine::start(
        Controls {
            playing: false,
            color: sound.color,
            volume: sound.volume,
            ..Controls::default()
        },
        EngineConfig::default(),
        move |event| on_engine_event(&events, event),
        move |bands| panel::emit_spectrum(&spectrum, &bands),
    )?;
    let lang = Lang::system();
    app.manage(AppState {
        engine,
        strings: Strings::new(lang),
        lang,
        sound: Mutex::new(sound),
        timer: Mutex::new(Timer::default()),
    });
    // The shortcut first, so the tray can show it.
    shortcut::register_defaults(app);
    tray::create(app)?;
    panel::create(app)?;
    start_ticker(app);
    #[cfg(debug_assertions)]
    dev_overrides(app);
    Ok(())
}

/// Development knobs for screenshots, debug builds only:
/// `RUMOR_DEV_SILENT=1` renders and analyses but sends silence to the device,
/// `RUMOR_DEV_PLAY=1` starts playing, `RUMOR_DEV_TIMER=<minutes>` arms the timer.
#[cfg(debug_assertions)]
fn dev_overrides(app: &AppHandle) {
    let state = app.state::<AppState>();
    if std::env::var("RUMOR_DEV_SILENT").is_ok_and(|v| v == "1") {
        state.engine.set_output_muted(true);
    }
    if let Some(minutes) = std::env::var("RUMOR_DEV_TIMER")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        set_timer(app, minutes);
    }
    if std::env::var("RUMOR_DEV_PLAY").is_ok_and(|v| v == "1") {
        set_playing(app, true);
    }
}

/// Runs on the engine's control thread.
fn on_engine_event(app: &AppHandle, event: EngineEvent) {
    let Some(state) = app.try_state::<AppState>() else {
        // Setup hasn't managed the state yet; it refreshes everything itself.
        return;
    };
    match &event {
        EngineEvent::StreamError(e) => log::warn!("engine: {e}"),
        EngineEvent::AutoPaused(_) => lock(&state.timer).on_playing(false, Instant::now()),
        EngineEvent::AutoResumed => lock(&state.timer).on_playing(true, Instant::now()),
        EngineEvent::OutputChanged(_) => {}
    }
    refresh(app);
}

/// Pushes the current state to everything that displays it.
pub fn refresh(app: &AppHandle) {
    tray::refresh(app);
    panel::emit_state(app);
}

pub fn toggle(app: &AppHandle) {
    let playing = app.state::<AppState>().engine.is_playing();
    set_playing(app, !playing);
}

pub fn set_playing(app: &AppHandle, playing: bool) {
    let state = app.state::<AppState>();
    // A user stop always uses the normal fade, even right after a timer end.
    state.engine.set_fade_out_ms(FADE_OUT_MS);
    state.engine.set_playing(playing);
    lock(&state.timer).on_playing(playing, Instant::now());
    refresh(app);
}

pub fn set_color(app: &AppHandle, color: NoiseColor) {
    let state = app.state::<AppState>();
    lock(&state.sound).color = color;
    state.engine.set_color(color);
    refresh(app);
}

pub fn set_volume(app: &AppHandle, volume: f32) {
    if !volume.is_finite() {
        return;
    }
    let volume = volume.clamp(0.0, 1.0);
    let state = app.state::<AppState>();
    lock(&state.sound).volume = volume;
    state.engine.set_volume(volume);
    refresh(app);
}

pub fn set_timer(app: &AppHandle, minutes: u32) {
    let state = app.state::<AppState>();
    let playing = state.engine.is_playing();
    lock(&state.timer).set(minutes, playing, Instant::now());
    refresh(app);
}

/// Checks the sleep timer and keeps the countdown on screen. The timer
/// itself is `Instant`-based; this only polls it.
fn start_ticker(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("rumor-ticker".into())
        .spawn(move || {
            let mut last_shown = None;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let state = app.state::<AppState>();
                let now = Instant::now();
                let (expired, remaining) = {
                    let mut timer = lock(&state.timer);
                    (timer.take_expired(now), timer.remaining_secs(now))
                };
                if expired {
                    log::info!("sleep timer finished");
                    state.engine.set_fade_out_ms(END_FADE_MS);
                    state.engine.set_playing(false);
                    refresh(&app);
                } else if remaining != last_shown {
                    tray::refresh(&app);
                    panel::emit_timer_tick(&app, remaining);
                }
                last_shown = remaining;
            }
        });
    if let Err(e) = spawned {
        log::error!("timer thread not started: {e}");
    }
}

/// Fades out before exiting, so quitting never clicks.
pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    panel::hide(app);
    if !state.engine.is_playing() {
        app.exit(0);
        return;
    }
    state.engine.set_fade_out_ms(FADE_OUT_MS);
    state.engine.set_playing(false);
    refresh(app);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FADE_OUT_MS as u64 + 150));
        app.exit(0);
    });
}

/// Someone launched Rumor again (single instance): show the panel.
pub fn second_instance(app: &AppHandle) {
    panel::open(app, None);
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}
