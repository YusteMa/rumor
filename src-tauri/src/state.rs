//! Shared `AppState` and the coordinator: every user action (tray, shortcut,
//! later the panel) goes through here, which updates the engine and then
//! everything that shows the state. Modules never call each other directly
//! (CLAUDE.md rule 12).

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::audio::device_watch::PauseReason;
use crate::audio::engine::{AudioEngine, EngineConfig, EngineEvent};
use crate::audio::noise::{Controls, FADE_OUT_MS, NoiseColor};
use crate::i18n::{Lang, Strings};
use crate::{shortcut, tray};

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
}

pub struct AppState {
    pub engine: AudioEngine,
    pub strings: Strings,
    sound: Mutex<Sound>,
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
        }
    }
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let sound = Sound::default();
    let handle = app.clone();
    let engine = AudioEngine::start(
        Controls {
            playing: false,
            color: sound.color,
            volume: sound.volume,
            ..Controls::default()
        },
        EngineConfig::default(),
        move |event| on_engine_event(&handle, event),
    )?;
    app.manage(AppState {
        engine,
        strings: Strings::new(Lang::system()),
        sound: Mutex::new(sound),
    });
    // The shortcut first, so the tray menu can show it.
    shortcut::register_defaults(app);
    tray::create(app)?;
    Ok(())
}

/// Runs on the engine's control thread.
fn on_engine_event(app: &AppHandle, event: EngineEvent) {
    if let EngineEvent::StreamError(e) = &event {
        log::warn!("engine: {e}");
    }
    // Events can arrive before setup has managed the state.
    if app.try_state::<AppState>().is_some() {
        refresh(app);
    }
}

/// Pushes the current state to everything that displays it.
pub fn refresh(app: &AppHandle) {
    tray::refresh(app);
}

pub fn toggle(app: &AppHandle) {
    app.state::<AppState>().engine.toggle();
    refresh(app);
}

/// Fades out before exiting, so quitting never clicks.
pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.engine.is_playing() {
        app.exit(0);
        return;
    }
    state.engine.set_playing(false);
    refresh(app);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FADE_OUT_MS as u64 + 150));
        app.exit(0);
    });
}

/// Someone launched Rumor again (single instance).
pub fn second_instance(app: &AppHandle) {
    // Phase 3 opens the panel here; until then, say where Rumor is.
    let strings = &app.state::<AppState>().strings;
    notify(app, "Rumor", strings.get("alreadyRunning"));
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}
