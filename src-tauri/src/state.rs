//! Shared `AppState` and the coordinator: every user action (tray, shortcut,
//! panel, Settings) goes through here, which updates the engine and then
//! everything that shows the state. Modules never call each other directly
//! (CLAUDE.md rule 12).

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

use crate::audio::device_watch::{OutputKind, PauseReason};
use crate::audio::engine::{AudioEngine, EngineConfig, EngineEvent};
use crate::audio::noise::{Controls, NoiseColor};
use crate::i18n::{Lang, Strings};
use crate::settings::{self, LanguageChoice, Settings};
use crate::timer::Timer;
use crate::{panel, settings_window, shortcut, tray};

/// Volume shortcuts move the slider by this much.
const VOLUME_STEP: f32 = 0.05;

/// What is playing, as the tray shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub color: NoiseColor,
    /// Slider position in 0..=1.
    pub volume: f32,
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
    pub timer_presets: [u32; 3],
    pub timer_remaining: Option<u64>,
    pub paused_for: Option<PauseReason>,
    pub output: Option<OutputSummary>,
    pub shortcut: Option<String>,
    /// The preferred shortcut, when another app owns it.
    pub shortcut_taken: Option<String>,
    pub lang: Lang,
    pub mica: bool,
    pub appearance: settings::Appearance,
    /// Windows' accent colour, when Settings → Appearance asks for it.
    pub system_accent: Option<String>,
}

pub struct AppState {
    pub engine: AudioEngine,
    strings: RwLock<Strings>,
    lang: Mutex<Lang>,
    settings: Mutex<Settings>,
    timer: Mutex<Timer>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn resolve_lang(choice: LanguageChoice) -> Lang {
    match choice {
        LanguageChoice::Auto => Lang::system(),
        LanguageChoice::Es => Lang::Es,
        LanguageChoice::En => Lang::En,
    }
}

fn engine_config(s: &Settings) -> EngineConfig {
    EngineConfig {
        on_output_change: s.playback.on_output_change,
        resume_on_reconnect: s.playback.resume_on_reconnect,
        output_device: s.advanced.output_device.clone(),
        buffer_ms: s.advanced.buffer.millis(),
    }
}

impl AppState {
    pub fn strings(&self) -> RwLockReadGuard<'_, Strings> {
        self.strings.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn lang(&self) -> Lang {
        *lock(&self.lang)
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let settings = lock(&self.settings);
        Snapshot {
            playing: self.engine.is_playing(),
            sound: Sound {
                color: settings.last.color,
                volume: settings.last.volume,
            },
            paused_for: self.engine.paused_for(),
            timer_remaining: lock(&self.timer).remaining_secs(Instant::now()),
        }
    }
}

pub fn panel_state(app: &AppHandle) -> Option<PanelState> {
    let state = app.try_state::<AppState>()?;
    let snapshot = state.snapshot();
    let settings = state.settings();
    Some(PanelState {
        playing: snapshot.playing,
        color: snapshot.sound.color,
        volume: snapshot.sound.volume,
        timer_minutes: lock(&state.timer).minutes(),
        timer_presets: settings.timer.presets,
        timer_remaining: snapshot.timer_remaining,
        paused_for: snapshot.paused_for,
        output: state.engine.output().map(|o| OutputSummary {
            name: o.endpoint.name,
            kind: o.endpoint.kind,
        }),
        shortcut: shortcut::label(app, settings::ShortcutAction::Toggle),
        shortcut_taken: shortcut::preferred_taken(app),
        lang: state.lang(),
        mica: panel::uses_mica(app),
        system_accent: (settings.appearance.accent == settings::AccentChoice::System)
            .then(crate::taskbar::system_accent)
            .flatten(),
        appearance: settings.appearance,
    })
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut settings = settings::load(app);
    let color = settings.start_color();
    settings.last.color = color;
    let playing = settings.start_playing();
    let fade = settings.sound.fade_ms as f32;
    let lang = resolve_lang(settings.appearance.language);

    let events = app.clone();
    let spectrum = app.clone();
    let engine = AudioEngine::start(
        Controls {
            playing,
            color,
            volume: settings.effective_volume(settings.last.volume),
            fade_in_ms: fade,
            fade_out_ms: fade,
        },
        engine_config(&settings),
        move |event| on_engine_event(&events, event),
        move |bands| panel::emit_spectrum(&spectrum, &bands),
    )?;
    let mut timer = Timer::default();
    timer.set(settings.last.timer_minutes, playing, Instant::now());
    let welcome = !settings.onboarded;
    settings.onboarded = true;
    settings::save(app, &settings);
    let launch_at_login = settings.startup.launch_at_login;

    app.manage(AppState {
        engine,
        strings: RwLock::new(Strings::new(lang)),
        lang: Mutex::new(lang),
        settings: Mutex::new(settings),
        timer: Mutex::new(timer),
    });
    // Shortcuts first, so the tray and panel can show them.
    shortcut::register_all(app);
    tray::create(app)?;
    panel::init(app);
    sync_autostart(app, launch_at_login);
    start_ticker(app);
    if welcome {
        let state = app.state::<AppState>();
        let strings = state.strings();
        notify(app, "Rumor", strings.get("welcome"));
    }
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

/// Keeps the Windows "Run" entry in line with the setting (re-registered on
/// every start, so it follows the executable if it moved).
fn sync_autostart(app: &AppHandle, enabled: bool) {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else if manager.is_enabled().unwrap_or(false) {
        manager.disable()
    } else {
        Ok(())
    };
    if let Err(e) = result {
        log::warn!("launch at login: {e}");
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
        EngineEvent::AutoPaused(_) => {
            lock(&state.timer).on_playing(false, Instant::now());
            remember(app, |last| last.was_playing = false);
        }
        EngineEvent::AutoResumed => {
            lock(&state.timer).on_playing(true, Instant::now());
            remember(app, |last| last.was_playing = true);
        }
        EngineEvent::OutputChanged(_) => settings_window::emit_changed(app),
    }
    refresh(app);
}

/// Updates and stores what Rumor remembers between runs.
fn remember(app: &AppHandle, f: impl FnOnce(&mut settings::LastSession)) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut current = lock(&state.settings);
    f(&mut current.last);
    settings::save(app, &current);
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
    state
        .engine
        .set_fade_out_ms(state.settings().sound.fade_ms as f32);
    state.engine.set_playing(playing);
    lock(&state.timer).on_playing(playing, Instant::now());
    remember(app, |last| last.was_playing = playing);
    refresh(app);
}

pub fn set_color(app: &AppHandle, color: NoiseColor) {
    let state = app.state::<AppState>();
    state.engine.set_color(color);
    remember(app, |last| last.color = color);
    refresh(app);
}

/// The next colour, for the "next colour" shortcut.
pub fn next_color(app: &AppHandle) {
    let current = app.state::<AppState>().settings().last.color;
    let next = NoiseColor::from_index(((current.index() + 1) % NoiseColor::ALL.len()) as u8);
    set_color(app, next);
}

/// `volume` is the slider position in 0..=1; the engine gets it scaled by the
/// maximum volume setting.
pub fn set_volume(app: &AppHandle, volume: f32) {
    if !volume.is_finite() {
        return;
    }
    let volume = volume.clamp(0.0, 1.0);
    let state = app.state::<AppState>();
    state
        .engine
        .set_volume(state.settings().effective_volume(volume));
    remember(app, |last| last.volume = volume);
    refresh(app);
}

/// Volume shortcuts: one step up or down.
pub fn nudge_volume(app: &AppHandle, up: bool) {
    let current = app.state::<AppState>().settings().last.volume;
    let step = if up { VOLUME_STEP } else { -VOLUME_STEP };
    set_volume(app, ((current + step) * 100.0).round() / 100.0);
}

pub fn set_timer(app: &AppHandle, minutes: u32) {
    let state = app.state::<AppState>();
    let playing = state.engine.is_playing();
    lock(&state.timer).set(minutes, playing, Instant::now());
    remember(app, |last| last.timer_minutes = minutes);
    refresh(app);
}

/// Keys of the settings that only the app writes, or that have their own
/// command (`set_shortcut`), dropped from a Settings update.
const INTERNAL_KEYS: [&str; 4] = ["last", "onboarded", "version", "shortcuts"];

/// Applies a partial update from the Settings window, stores it, and
/// returns the new settings.
pub fn update_settings(app: &AppHandle, mut patch: Value) -> Result<Settings, String> {
    if let Some(object) = patch.as_object_mut() {
        for key in INTERNAL_KEYS {
            object.remove(key);
        }
    }
    let state = app.state::<AppState>();
    let (old, new) = {
        let mut current = lock(&state.settings);
        let old = current.clone();
        let new = old.merged(&patch).map_err(|e| e.to_string())?;
        *current = new.clone();
        (old, new)
    };
    settings::save(app, &new);
    apply(app, &old, &new);
    Ok(new)
}

/// Stores the shortcuts once `shortcut::set` has registered them.
pub fn set_shortcuts(app: &AppHandle, shortcuts: settings::Shortcuts) {
    let state = app.state::<AppState>();
    {
        let mut current = lock(&state.settings);
        current.shortcuts = shortcuts;
        settings::save(app, &current);
    }
    refresh(app);
    settings_window::emit_changed(app);
}

/// Replaces every setting (import or reset), keeping what Rumor remembers.
pub fn replace_settings(app: &AppHandle, mut new: Settings) {
    let state = app.state::<AppState>();
    let old = {
        let mut current = lock(&state.settings);
        new.last = current.last.clone();
        new.onboarded = true;
        std::mem::replace(&mut *current, new.clone())
    };
    settings::save(app, &new);
    settings::flush(app);
    if old.shortcuts != new.shortcuts {
        shortcut::register_all(app);
    }
    apply(app, &old, &new);
}

/// Pushes setting changes to the parts they affect.
fn apply(app: &AppHandle, old: &Settings, new: &Settings) {
    let state = app.state::<AppState>();
    let config = engine_config(new);
    if engine_config(old) != config {
        state.engine.set_config(config);
    }
    if old.sound.fade_ms != new.sound.fade_ms {
        state.engine.set_fade_in_ms(new.sound.fade_ms as f32);
        state.engine.set_fade_out_ms(new.sound.fade_ms as f32);
    }
    if old.sound.max_volume != new.sound.max_volume {
        state
            .engine
            .set_volume(new.effective_volume(new.last.volume));
    }
    if old.appearance.language != new.appearance.language {
        let lang = resolve_lang(new.appearance.language);
        *lock(&state.lang) = lang;
        *state.strings.write().unwrap_or_else(|e| e.into_inner()) = Strings::new(lang);
    }
    if old.startup.launch_at_login != new.startup.launch_at_login {
        sync_autostart(app, new.startup.launch_at_login);
    }
    if old.appearance != new.appearance {
        panel::apply_appearance(app, &new.appearance);
        settings_window::apply_appearance(app, &new.appearance);
    }
    refresh(app);
    settings_window::emit_changed(app);
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
                    timer_finished(&app);
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

fn timer_finished(app: &AppHandle) {
    log::info!("sleep timer finished");
    let state = app.state::<AppState>();
    let settings = state.settings();
    state
        .engine
        .set_fade_out_ms(settings.timer.end_fade_s as f32 * 1000.0);
    state.engine.set_playing(false);
    remember(app, |last| last.was_playing = false);
    refresh(app);
    if settings.timer.notify_on_end {
        let strings = state.strings();
        notify(
            app,
            strings.get("timerEndedTitle"),
            strings.get("timerEndedBody"),
        );
    }
}

/// Fades out before exiting, so quitting never clicks.
pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let playing = state.engine.is_playing();
    // "As I left it" needs to know whether it was playing.
    remember(app, |last| last.was_playing = playing);
    settings::flush(app);
    panel::hide(app);
    settings_window::close(app);
    if !playing {
        app.exit(0);
        return;
    }
    let fade = state.settings().sound.fade_ms;
    state.engine.set_fade_out_ms(fade as f32);
    state.engine.set_playing(false);
    tray::refresh(app);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(u64::from(fade) + 150));
        app.exit(0);
    });
}

/// Someone launched Rumor again (single instance): show the panel.
///
/// Debug builds also take `--dev-hide`, `--dev-prepare` and
/// `--dev-color <pink|brown|white>`, to drive the panel for screenshots and
/// timings, and `--dev-settings` to open Settings.
pub fn second_instance(app: &AppHandle, args: &[String]) {
    log::debug!("second instance: {args:?}");
    if cfg!(debug_assertions) {
        if args.iter().any(|a| a == "--dev-hide") {
            panel::hide(app);
            return;
        }
        // What hovering the tray icon does.
        if args.iter().any(|a| a == "--dev-prepare") {
            panel::prepare(app);
            return;
        }
        if args.iter().any(|a| a == "--dev-settings") {
            settings_window::open(app);
            return;
        }
        let color = args
            .iter()
            .position(|a| a == "--dev-color")
            .and_then(|i| args.get(i + 1))
            .and_then(|c| serde_json::from_value(serde_json::Value::String(c.clone())).ok());
        if let Some(color) = color {
            set_color(app, color);
        }
    }
    panel::open(app, None);
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}
