//! Shared `AppState` and the coordinator: every user action (tray, shortcut,
//! panel, Settings) goes through here, which updates the engine and the study
//! module and then everything that shows the state. Sound and Study never
//! call each other: the linking between them lives here (CLAUDE.md rule 12).

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

use crate::audio::device_watch::{OutputKind, PauseReason, SystemEvent};
use crate::audio::engine::{AudioEngine, EngineConfig, EngineEvent};
use crate::audio::noise::{Controls, NoiseColor};
use crate::ear_break::EarBreak;
use crate::i18n::{Lang, Strings};
use crate::settings::{self, BreakNoise, LanguageChoice, Settings};
use crate::study::db::{DbError, Tag};
use crate::study::machine::{Mode, Phase, Run};
use crate::study::{self, Study, StudyView};
use crate::timer::Timer;
use crate::update::{self, UpdateView};
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

/// The stopwatch, as the tray shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StudyBrief {
    pub run: Run,
    pub phase: Phase,
    pub worked_secs: u64,
    pub phase_left_secs: Option<u64>,
    /// "¿Seguías estudiando?" waits for an answer.
    pub idle_pending: bool,
}

/// Everything the tray needs to draw itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub playing: bool,
    pub sound: Sound,
    pub paused_for: Option<PauseReason>,
    pub timer_remaining: Option<u64>,
    /// `None` with "Modo estudio" off.
    pub study: Option<StudyBrief>,
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
    /// The study card; `None` with "Modo estudio" off (SPEC §4.3c).
    pub study: Option<StudyView>,
    /// A newer version, once a check found one.
    pub update: Option<UpdateView>,
}

/// What the coordinator did on its own, to undo it at the right moment.
#[derive(Debug, Default)]
struct Link {
    /// A Pomodoro break ("Pausar") paused the noise; the next work block
    /// plays it again.
    noise_paused_for_break: bool,
    /// Locking the PC paused the stopwatch; unlocking continues it.
    study_paused_by_lock: bool,
    /// An update interrupted the noise on this output: play again once the
    /// engine opens it, and only there.
    resume_on: Option<String>,
}

pub struct AppState {
    pub engine: AudioEngine,
    strings: RwLock<Strings>,
    lang: Mutex<Lang>,
    settings: Mutex<Settings>,
    timer: Mutex<Timer>,
    study: Mutex<Study>,
    link: Mutex<Link>,
    ears: Mutex<EarBreak>,
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
        on_lock: s.playback.on_lock,
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

    fn study_on(&self) -> bool {
        lock(&self.settings).study.enabled
    }

    /// Noise and stopwatch play and pause together.
    fn linked(&self) -> bool {
        let s = lock(&self.settings);
        s.study.enabled && s.study.link_noise
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = Instant::now();
        let (sound, study_on) = {
            let settings = lock(&self.settings);
            (
                Sound {
                    color: settings.last.color,
                    volume: settings.last.volume,
                },
                settings.study.enabled,
            )
        };
        let study = study_on.then(|| {
            let st = lock(&self.study);
            StudyBrief {
                run: st.run(),
                phase: st.phase(),
                worked_secs: st.worked(now).as_secs(),
                phase_left_secs: st.phase_left_secs(now),
                idle_pending: st.idle_pending(),
            }
        });
        Snapshot {
            playing: self.engine.is_playing(),
            sound,
            paused_for: self.engine.paused_for(),
            timer_remaining: lock(&self.timer).remaining_secs(now),
            study,
        }
    }

    fn study_view(&self) -> Option<StudyView> {
        self.study_on()
            .then(|| lock(&self.study).view(Instant::now(), SystemTime::now()))
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
        study: state.study_view(),
        update: update::view(app),
    })
}

/// The study card's live numbers (`study-tick`, `study_totals`).
pub fn study_view(app: &AppHandle) -> Option<StudyView> {
    app.try_state::<AppState>()?.study_view()
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut settings = settings::load(app);
    let color = settings.start_color();
    settings.last.color = color;
    let playing = settings.start_playing();
    let resume_on = settings.last.resume_after_update.take();
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
            mono: settings.sound.mono,
            ..Controls::default()
        },
        engine_config(&settings),
        move |event| on_engine_event(&events, event),
        move |bands| panel::emit_spectrum(&spectrum, &bands),
    )?;
    let mut timer = Timer::default();
    timer.set(settings.last.timer_minutes, playing, Instant::now());
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| log::error!("no data folder for the study history: {e}"))
        .ok();
    let study = Study::open(
        data_dir.as_deref(),
        settings.last.study_mode,
        settings.study.plan(),
        settings.last.study_tag,
    );
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
        study: Mutex::new(study),
        link: Mutex::new(Link {
            resume_on,
            ..Link::default()
        }),
        ears: Mutex::new(EarBreak::default()),
    });
    // Shortcuts first, so the tray and panel can show them.
    shortcut::register_all(app);
    tray::create(app)?;
    panel::init(app);
    sync_autostart(app, launch_at_login);
    start_ticker(app);
    update::start(app);
    if let Some(output) = app.state::<AppState>().engine.output() {
        resume_after_update(app, &output.endpoint.id);
    }
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
            // Headphones out, PC locked or asleep: the stopwatch stops too.
            if state.linked() {
                study_pause_only(app);
            }
        }
        EngineEvent::AutoResumed => {
            lock(&state.timer).on_playing(true, Instant::now());
            remember(app, |last| last.was_playing = true);
            // Continues a paused session; never starts a new one by itself.
            if state.linked() && lock(&state.study).run() == Run::Paused {
                study_start_only(app);
            }
        }
        EngineEvent::OutputChanged(output) => {
            if let Some(output) = output {
                apply_output_volume(app, &output.endpoint.id);
                resume_after_update(app, &output.endpoint.id);
            }
            settings_window::emit_changed(app);
        }
        EngineEvent::Session(event) => on_session(app, *event),
    }
    refresh(app);
}

/// Locked or asleep time never counts as study (SPEC §4.3b), linked or not.
fn on_session(app: &AppHandle, event: SystemEvent) {
    let state = app.state::<AppState>();
    if !state.study_on() {
        return;
    }
    match event {
        SystemEvent::Locked | SystemEvent::Suspending => {
            if lock(&state.study).run() == Run::Running {
                study_pause_only(app);
                // After sleep it stays paused, like the noise.
                lock(&state.link).study_paused_by_lock = event == SystemEvent::Locked;
            }
        }
        SystemEvent::Unlocked => {
            let by_lock = std::mem::take(&mut lock(&state.link).study_paused_by_lock);
            // Linked, it follows the noise: if the noise stayed paused (not
            // on headphones any more), so does the stopwatch.
            let follow = !state.linked() || state.engine.is_playing();
            if by_lock && follow && lock(&state.study).run() == Run::Paused {
                study_start_only(app);
            }
        }
        SystemEvent::Resumed | SystemEvent::OutputChanged => {}
    }
}

/// Volume per output (SPEC §3.3): a device keeps its own volume; one never
/// used before starts at the current volume.
fn apply_output_volume(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.sound.per_output_volume {
        return;
    }
    match settings.last.output_volumes.get(id).copied() {
        Some(saved) if saved != settings.last.volume => {
            log::info!("volume for this output: {:.0} %", saved * 100.0);
            // The engine's volume smoothing makes this a soft change.
            state.engine.set_volume(settings.effective_volume(saved));
            remember(app, |last| last.volume = saved);
        }
        Some(_) => {}
        None => {
            let id = id.to_owned();
            remember(app, |last| {
                last.output_volumes.insert(id, last.volume);
            });
        }
    }
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

// ---- Noise ---------------------------------------------------------------------

/// Plays or pauses the noise, without touching the stopwatch.
fn noise_only(app: &AppHandle, playing: bool) {
    let state = app.state::<AppState>();
    // A user stop always uses the normal fade, even right after a timer end.
    state
        .engine
        .set_fade_out_ms(state.settings().sound.fade_ms as f32);
    state.engine.set_playing(playing);
    lock(&state.timer).on_playing(playing, Instant::now());
    remember(app, |last| last.was_playing = playing);
}

/// The noise changed by the user's hand: a linked stopwatch follows.
fn link_noise_to_study(app: &AppHandle, playing: bool) {
    let state = app.state::<AppState>();
    if !state.linked() {
        return;
    }
    if playing {
        study_start_only(app);
    } else {
        study_pause_only(app);
    }
}

pub fn toggle(app: &AppHandle) {
    let playing = app.state::<AppState>().engine.is_playing();
    set_playing(app, !playing);
}

pub fn set_playing(app: &AppHandle, playing: bool) {
    let state = app.state::<AppState>();
    noise_only(app, playing);
    // The user took over from whatever a break did.
    lock(&state.link).noise_paused_for_break = false;
    link_noise_to_study(app, playing);
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
    let settings = state.settings();
    state.engine.set_volume(settings.effective_volume(volume));
    let output = settings
        .sound
        .per_output_volume
        .then(|| state.engine.output().map(|o| o.endpoint.id))
        .flatten();
    remember(app, |last| {
        last.volume = volume;
        if let Some(id) = output {
            last.output_volumes.insert(id, volume);
        }
    });
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

// ---- Study ---------------------------------------------------------------------

/// Starts or continues the stopwatch, without touching the noise.
fn study_start_only(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.study_on() {
        return;
    }
    lock(&state.study).start(Instant::now(), SystemTime::now());
    lock(&state.link).study_paused_by_lock = false;
}

fn study_pause_only(app: &AppHandle) {
    let state = app.state::<AppState>();
    lock(&state.study).pause(Instant::now());
}

/// The stopwatch's play/pause (panel, tray, `Ctrl+Alt+T`).
pub fn study_toggle(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.study_on() {
        return;
    }
    let running = lock(&state.study).run() == Run::Running;
    if running {
        study_pause_only(app);
        if state.linked() && state.engine.is_playing() {
            noise_only(app, false);
        }
    } else {
        study_start_only(app);
        // A break set to "Pausar" keeps the noise quiet until work resumes.
        let quiet_break = {
            let st = lock(&state.study);
            st.phase().is_break() && state.settings().study.break_noise == BreakNoise::Pause
        };
        if state.linked() && !state.engine.is_playing() && !quiet_break {
            noise_only(app, true);
        }
    }
    refresh(app);
}

/// Ends the session and stores it. Linked, the noise stops too: "I'm done".
pub fn study_finish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let had_session = lock(&state.study).run() != Run::Stopped;
    lock(&state.study).finish(Instant::now());
    end_break_effects(app);
    if had_session && state.linked() && state.engine.is_playing() {
        noise_only(app, false);
    }
    refresh(app);
}

/// Undoes what a break did to the noise.
fn end_break_effects(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.engine.set_duck(false);
    lock(&state.link).noise_paused_for_break = false;
}

/// Between sessions only; the panel remembers the last one used.
pub fn study_set_mode(app: &AppHandle, mode: Mode) {
    let state = app.state::<AppState>();
    if lock(&state.study).set_mode(mode) {
        remember(app, |last| last.study_mode = mode);
        refresh(app);
    }
}

pub fn study_set_tag(app: &AppHandle, tag: Option<i64>) {
    let state = app.state::<AppState>();
    let tag = {
        let mut st = lock(&state.study);
        st.set_tag(tag);
        st.tag()
    };
    remember(app, |last| last.study_tag = tag);
    refresh(app);
}

/// "¿Seguías estudiando?": `keep` = "Sí, contar".
pub fn study_resolve_idle(app: &AppHandle, keep: bool) {
    let state = app.state::<AppState>();
    lock(&state.study).resolve_idle(keep, Instant::now());
    refresh(app);
}

pub fn tags(app: &AppHandle) -> Vec<Tag> {
    app.state::<AppState>()
        .study
        .lock()
        .map_or_else(|e| e.into_inner().tags().to_vec(), |st| st.tags().to_vec())
}

pub fn tags_upsert(
    app: &AppHandle,
    id: Option<i64>,
    name: &str,
    color: &str,
) -> Result<Tag, DbError> {
    let state = app.state::<AppState>();
    let tag = lock(&state.study).upsert_tag(id, name, color)?;
    refresh(app);
    settings_window::emit_changed(app);
    Ok(tag)
}

pub fn tags_delete(app: &AppHandle, id: i64) -> Result<(), DbError> {
    let state = app.state::<AppState>();
    let current = {
        let mut st = lock(&state.study);
        st.delete_tag(id)?;
        st.tag()
    };
    remember(app, |last| last.study_tag = current);
    refresh(app);
    settings_window::emit_changed(app);
    Ok(())
}

/// A Pomodoro phase began: tell the user, and (linked) do what Settings
/// says with the noise during breaks.
fn on_phase(app: &AppHandle, phase: Phase, cycle: u32, cycles: u32) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let (title, body) = {
        let t = state.strings();
        match phase {
            Phase::Work => (
                t.get("alertWorkTitle").to_owned(),
                t.format(
                    "alertWorkBody",
                    &[("n", &cycle.to_string()), ("total", &cycles.to_string())],
                ),
            ),
            Phase::ShortBreak => (
                t.get("alertBreakTitle").to_owned(),
                t.format(
                    "alertBreakBody",
                    &[("n", &(settings.study.short_break_min).to_string())],
                ),
            ),
            Phase::LongBreak => (
                t.get("alertLongBreakTitle").to_owned(),
                t.format(
                    "alertBreakBody",
                    &[("n", &(settings.study.long_break_min).to_string())],
                ),
            ),
        }
    };
    log::info!("pomodoro: {phase:?} {cycle}/{cycles}");
    alert(app, &title, &body);
    if !state.linked() {
        return;
    }
    if phase.is_break() {
        match settings.study.break_noise {
            BreakNoise::Half => state.engine.set_duck(true),
            BreakNoise::Pause if state.engine.is_playing() => {
                noise_only(app, false);
                lock(&state.link).noise_paused_for_break = true;
            }
            BreakNoise::Pause | BreakNoise::Keep => {}
        }
    } else {
        state.engine.set_duck(false);
        if std::mem::take(&mut lock(&state.link).noise_paused_for_break) {
            noise_only(app, true);
        }
    }
}

fn on_idle(app: &AppHandle, away: Duration) {
    let state = app.state::<AppState>();
    let minutes = away.as_secs().div_ceil(60);
    log::info!("back after {minutes} min without input");
    panel::emit_study_idle(app, minutes);
    // A discreet notification only; never a sound, never the panel.
    if state.settings().general.alerts.notification() {
        let t = state.strings();
        notify(
            app,
            t.get("idleTitle"),
            &t.format("idleBody", &[("n", &minutes.to_string())]),
        );
    }
}

/// The one "Avisos" setting: the bell, a notification, both or nothing.
fn alert(app: &AppHandle, title: &str, body: &str) {
    let state = app.state::<AppState>();
    let alerts = state.settings().general.alerts;
    if alerts.bell() {
        state.engine.ring();
    }
    if alerts.notification() {
        notify(app, title, body);
    }
}

// ---- Settings ------------------------------------------------------------------

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
    if old.sound.mono != new.sound.mono {
        state.engine.set_mono(new.sound.mono);
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
    if old.study.plan() != new.study.plan() {
        lock(&state.study).set_plan(new.study.plan());
    }
    let was_linked = old.study.enabled && old.study.link_noise;
    let is_linked = new.study.enabled && new.study.link_noise;
    if old.study.enabled && !new.study.enabled {
        // "Modo estudio" off: the session in progress is stored and ends,
        // and nothing of the stopwatch is left anywhere (SPEC §4.3c).
        lock(&state.study).finish(Instant::now());
    }
    if was_linked && !is_linked {
        end_break_effects(app);
    }
    if old.study.enabled != new.study.enabled || old.shortcuts != new.shortcuts {
        shortcut::register_all(app);
    }
    if old.study.enabled != new.study.enabled {
        panel::resize(app);
    }
    refresh(app);
    settings_window::emit_changed(app);
}

// ---- Ticker --------------------------------------------------------------------

/// Polls the sleep timer, the Pomodoro, idle detection and the ear break, and
/// keeps the countdowns on screen. Every duration is `Instant`-based; this
/// only looks at the clocks.
fn start_ticker(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("rumor-ticker".into())
        .spawn(move || {
            let mut last_shown = None;
            let mut last_second = Instant::now();
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

                study_tick(&app, now);
                if now.saturating_duration_since(last_second) >= Duration::from_secs(1) {
                    last_second = now;
                    ear_tick(&app, now);
                    if state.study_on() {
                        tray::refresh(&app);
                        if let Some(view) = state.study_view() {
                            panel::emit_study_tick(&app, &view);
                        }
                    }
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("timer thread not started: {e}");
    }
}

fn study_tick(app: &AppHandle, now: Instant) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.study.enabled {
        return;
    }
    let idle_after = (settings.study.idle_minutes > 0)
        .then(|| Duration::from_secs(u64::from(settings.study.idle_minutes) * 60));
    let events = lock(&state.study).tick(now, SystemTime::now(), idle_after);
    for event in &events {
        match *event {
            study::Event::Phase(change) => {
                on_phase(app, change.to, change.cycle, settings.study.cycles)
            }
            study::Event::Idle(away) => on_idle(app, away),
        }
    }
    if !events.is_empty() {
        refresh(app);
    }
}

/// SPEC §4.3b: after X hours on headphones, one reminder; it never stops
/// the noise.
fn ear_tick(app: &AppHandle, now: Instant) {
    let state = app.state::<AppState>();
    let hours = state.settings().general.ear_break_hours;
    let listening = state.engine.is_playing()
        && state
            .engine
            .output()
            .is_some_and(|o| o.endpoint.kind == OutputKind::Personal);
    let after = Duration::from_secs(u64::from(hours) * 3600);
    if hours > 0 && lock(&state.ears).update(now, listening, after) {
        log::info!("ear break reminder after {hours} h on headphones");
        let (title, body) = {
            let t = state.strings();
            (
                t.get("earBreakTitle").to_owned(),
                t.format("earBreakBody", &[("n", &hours.to_string())]),
            )
        };
        alert(app, &title, &body);
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
    if state.linked() {
        study_pause_only(app);
    }
    refresh(app);
    let (title, body) = {
        let t = state.strings();
        (
            t.get("timerEndedTitle").to_owned(),
            t.get("timerEndedBody").to_owned(),
        )
    };
    alert(app, &title, &body);
}

/// Fades out before exiting, so quitting never clicks. A study session in
/// progress is stored first.
pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    lock(&state.study).finish(Instant::now());
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

// ---- Updates -------------------------------------------------------------------

/// A check found a newer version (update.rs): show it, and announce each
/// version once.
pub fn update_found(app: &AppHandle, version: &str) {
    refresh(app);
    settings_window::emit_changed(app);
    let state = app.state::<AppState>();
    if state.settings().last.update_notified.as_deref() == Some(version) {
        return;
    }
    remember(app, |last| last.update_notified = Some(version.to_owned()));
    let (title, body) = {
        let t = state.strings();
        (
            t.get("updateTitle").to_owned(),
            t.format("updateBody", &[("v", version)]),
        )
    };
    notify(app, &title, &body);
}

/// The update is downloaded and about to install: store everything, close
/// the windows and fade out, as quitting does, and play again after the
/// restart if the noise was on. Returns whether it was.
pub fn prepare_for_update(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    lock(&state.study).finish(Instant::now());
    let playing = state.engine.is_playing();
    let output = state.engine.output().map(|o| o.endpoint.id);
    remember(app, |last| {
        last.was_playing = playing;
        last.resume_after_update = output.filter(|_| playing);
    });
    settings::flush(app);
    panel::hide(app);
    settings_window::close(app);
    if playing {
        let fade = state.settings().sound.fade_ms;
        state.engine.set_fade_out_ms(fade as f32);
        state.engine.set_playing(false);
        tray::refresh(app);
        std::thread::sleep(Duration::from_millis(u64::from(fade) + 150));
    }
    playing
}

/// The first output after an update: the noise comes back if it is where it
/// was playing. Only once, whichever of setup and the engine gets here first.
fn resume_after_update(app: &AppHandle, output: &str) {
    let state = app.state::<AppState>();
    let Some(id) = lock(&state.link).resume_on.take() else {
        return;
    };
    if id == output {
        log::info!("playing again after the update");
        noise_only(app, true);
        refresh(app);
    } else {
        log::info!("not playing after the update: the output changed");
    }
}

/// The installer didn't start: undo `prepare_for_update`.
pub fn update_failed(app: &AppHandle, was_playing: bool) {
    remember(app, |last| last.resume_after_update = None);
    if was_playing {
        noise_only(app, true);
    }
}

/// Someone launched Rumor again (single instance): show the panel.
///
/// Debug builds also take `--dev-hide`, `--dev-prepare` and
/// `--dev-color <pink|brown|white>`, to drive the panel for screenshots and
/// timings, `--dev-settings` to open Settings, `--dev-bell` to ring and
/// `--dev-study` / `--dev-study-finish` to drive the stopwatch.
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
        if args.iter().any(|a| a == "--dev-bell") {
            app.state::<AppState>().engine.ring();
            return;
        }
        if args.iter().any(|a| a == "--dev-study") {
            study_toggle(app);
            return;
        }
        if args.iter().any(|a| a == "--dev-study-finish") {
            study_finish(app);
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
