//! Persistent settings (SPEC §4.4). Pure model: defaults, validation and
//! partial updates live here and are tested; reading and writing the JSON
//! store is the only I/O.
//!
//! Every field has a sensible default, so nobody needs to open Settings for
//! Rumor to work well, and unknown or missing fields in an old or imported
//! file never break loading.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::audio::device_watch::{LockPolicy, OutputChangePolicy};
use crate::audio::noise::NoiseColor;
use crate::study::machine::{Mode as StudyMode, Plan};

/// Bumped when the shape changes in a way `#[serde(default)]` can't absorb.
pub const VERSION: u32 = 1;
pub const FADE_MS: (u32, u32) = (200, 3_000);
pub const MAX_VOLUME: (f32, f32) = (0.1, 1.0);
pub const END_FADE_S: (u32, u32) = (2, 60);
pub const PRESET_MINUTES: (u32, u32) = (1, 600);
pub const WORK_MINUTES: (u32, u32) = (5, 180);
pub const BREAK_MINUTES: (u32, u32) = (1, 60);
pub const LONG_BREAK_MINUTES: (u32, u32) = (1, 90);
pub const CYCLES: (u32, u32) = (1, 12);
/// Idle detection: 0 = off.
pub const IDLE_MINUTES: (u32, u32) = (5, 120);
/// Ear break: 0 = off.
pub const EAR_BREAK_HOURS: (u32, u32) = (1, 8);

/// "Color por defecto": the one used last, or a fixed one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartColor {
    #[default]
    Last,
    Pink,
    Brown,
    White,
}

/// "Al iniciar Rumor" (SPEC §4.4, merged with "recordar el último estado").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnLaunch {
    #[default]
    Silent,
    Play,
    /// Playing again only if it was playing when Rumor closed.
    Resume,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeftClick {
    #[default]
    Toggle,
    Panel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccentChoice {
    /// Follows the noise: dusty pink, toasted caramel, blue-grey pearl.
    #[default]
    Noise,
    /// Windows' accent colour.
    System,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageChoice {
    #[default]
    Auto,
    Es,
    En,
}

/// How Rumor tells you something (SPEC §4.3b "Avisos"): one choice for the
/// sleep timer, the Pomodoro and the ear break.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alerts {
    /// The singing bowl.
    Bell,
    Notification,
    #[default]
    Both,
    None,
}

impl Alerts {
    pub fn bell(self) -> bool {
        matches!(self, Self::Bell | Self::Both)
    }

    pub fn notification(self) -> bool {
        matches!(self, Self::Notification | Self::Both)
    }
}

/// What the noise does during Pomodoro breaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakNoise {
    /// −10 dB: half as loud.
    #[default]
    Half,
    Pause,
    Keep,
}

/// Output buffer: Rumor's default (`DEFAULT_BUFFER_MS`) or a fixed length.
/// Values from older versions (`ms10`) read as `Auto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BufferChoice {
    Ms20,
    Ms50,
    Ms100,
    // Last: `serde(other)` has to be.
    #[default]
    #[serde(other)]
    Auto,
}

impl BufferChoice {
    pub fn millis(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Ms20 => Some(20),
            Self::Ms50 => Some(50),
            Self::Ms100 => Some(100),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sound {
    pub start_color: StartColor,
    /// Fade in and out, in ms.
    pub fade_ms: u32,
    /// The slider's 100 % maps to this (0.1..=1.0).
    pub max_volume: f32,
    /// Remember a volume for each output device (SPEC §3.3).
    pub per_output_volume: bool,
    /// Same signal in both ears, for a single earbud.
    pub mono: bool,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            start_color: StartColor::Last,
            fade_ms: 400,
            max_volume: 1.0,
            per_output_volume: true,
            mono: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Playback {
    pub on_output_change: OutputChangePolicy,
    pub resume_on_reconnect: bool,
    pub on_lock: LockPolicy,
    pub on_launch: OnLaunch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Timer {
    /// The three quick choices next to ∞, in minutes.
    pub presets: [u32; 3],
    pub end_fade_s: u32,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            presets: [25, 50, 90],
            end_fade_s: 10,
        }
    }
}

/// Settings → General, beyond playback.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct General {
    pub alerts: Alerts,
    /// Remind to rest the ears after this many hours on headphones; 0 = off.
    pub ear_break_hours: u32,
}

impl Default for General {
    fn default() -> Self {
        Self {
            alerts: Alerts::Both,
            ear_break_hours: 3,
        }
    }
}

/// Settings → Study (SPEC §4.3b, §4.3c).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Study {
    /// "Modo estudio": off by default (SPEC §9 decision 9).
    pub enabled: bool,
    /// Play and pause the noise and the stopwatch together.
    pub link_noise: bool,
    pub work_min: u32,
    pub short_break_min: u32,
    pub long_break_min: u32,
    /// Work blocks before the long break.
    pub cycles: u32,
    pub break_noise: BreakNoise,
    /// Ask after this many minutes without input; 0 = off (the default:
    /// paper notes don't touch the computer).
    pub idle_minutes: u32,
}

impl Default for Study {
    fn default() -> Self {
        Self {
            enabled: false,
            link_noise: true,
            work_min: 25,
            short_break_min: 5,
            long_break_min: 15,
            cycles: 4,
            break_noise: BreakNoise::Half,
            idle_minutes: 0,
        }
    }
}

impl Study {
    pub fn plan(&self) -> Plan {
        let min = |m: u32| std::time::Duration::from_secs(u64::from(m) * 60);
        Plan {
            work: min(self.work_min),
            short_break: min(self.short_break_min),
            long_break: min(self.long_break_min),
            cycles: self.cycles,
        }
    }
}

/// Global shortcuts as shown to the user ("Ctrl+Alt+P"); `None` = unset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Shortcuts {
    pub toggle: Option<String>,
    pub volume_up: Option<String>,
    pub volume_down: Option<String>,
    pub next_color: Option<String>,
    pub open_panel: Option<String>,
    /// Start or pause the study stopwatch (only with "Modo estudio" on).
    pub study: Option<String>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            toggle: Some("Ctrl+Alt+P".into()),
            volume_up: None,
            volume_down: None,
            next_color: None,
            open_panel: None,
            study: Some("Ctrl+Alt+T".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutAction {
    Toggle,
    VolumeUp,
    VolumeDown,
    NextColor,
    OpenPanel,
    Study,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 6] = [
        Self::Toggle,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::NextColor,
        Self::OpenPanel,
        Self::Study,
    ];
}

impl Shortcuts {
    pub fn get(&self, action: ShortcutAction) -> Option<&str> {
        match action {
            ShortcutAction::Toggle => self.toggle.as_deref(),
            ShortcutAction::VolumeUp => self.volume_up.as_deref(),
            ShortcutAction::VolumeDown => self.volume_down.as_deref(),
            ShortcutAction::NextColor => self.next_color.as_deref(),
            ShortcutAction::OpenPanel => self.open_panel.as_deref(),
            ShortcutAction::Study => self.study.as_deref(),
        }
    }

    pub fn set(&mut self, action: ShortcutAction, value: Option<String>) {
        let slot = match action {
            ShortcutAction::Toggle => &mut self.toggle,
            ShortcutAction::VolumeUp => &mut self.volume_up,
            ShortcutAction::VolumeDown => &mut self.volume_down,
            ShortcutAction::NextColor => &mut self.next_color,
            ShortcutAction::OpenPanel => &mut self.open_panel,
            ShortcutAction::Study => &mut self.study,
        };
        *slot = value;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Tray {
    pub left_click: LeftClick,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    pub theme: ThemeChoice,
    pub accent: AccentChoice,
    pub mica: bool,
    pub spectrum: bool,
    pub language: LanguageChoice,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            accent: AccentChoice::Noise,
            mica: true,
            spectrum: true,
            language: LanguageChoice::Auto,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Startup {
    pub launch_at_login: bool,
    /// Look for a newer version on GitHub (the only network request Rumor
    /// makes, CLAUDE.md rule 8).
    pub check_updates: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            check_updates: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Advanced {
    /// Endpoint id of a fixed output, or `None` for Windows' default.
    pub output_device: Option<String>,
    pub buffer: BufferChoice,
}

/// What Rumor remembers between runs (not shown in Settings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LastSession {
    pub color: NoiseColor,
    /// Slider position in 0..=1.
    pub volume: f32,
    pub timer_minutes: u32,
    pub was_playing: bool,
    /// Slider position per output endpoint id (SPEC §3.3).
    pub output_volumes: BTreeMap<String, f32>,
    /// The panel remembers the study mode last used.
    pub study_mode: StudyMode,
    pub study_tag: Option<i64>,
    /// The last version announced by a notification, so each one is
    /// announced once.
    pub update_notified: Option<String>,
    /// The output an update interrupted the noise on: play again on the next
    /// launch, if it is still the output then (CLAUDE.md rule 10).
    pub resume_after_update: Option<String>,
}

impl Default for LastSession {
    fn default() -> Self {
        Self {
            color: NoiseColor::Pink,
            volume: 0.4,
            timer_minutes: 0,
            was_playing: false,
            output_volumes: BTreeMap::new(),
            study_mode: StudyMode::Stopwatch,
            study_tag: None,
            update_notified: None,
            resume_after_update: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub general: General,
    pub sound: Sound,
    pub playback: Playback,
    pub timer: Timer,
    pub study: Study,
    pub shortcuts: Shortcuts,
    pub tray: Tray,
    pub appearance: Appearance,
    pub startup: Startup,
    pub advanced: Advanced,
    pub last: LastSession,
    /// The welcome notification has been shown.
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            general: General::default(),
            sound: Sound::default(),
            playback: Playback::default(),
            timer: Timer::default(),
            study: Study::default(),
            shortcuts: Shortcuts::default(),
            tray: Tray::default(),
            appearance: Appearance::default(),
            startup: Startup::default(),
            advanced: Advanced::default(),
            last: LastSession::default(),
            onboarded: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("not a settings object: {0}")]
    Shape(#[from] serde_json::Error),
    #[error("the update must be a JSON object")]
    NotAnObject,
}

fn clamp_f(v: f32, (lo, hi): (f32, f32), fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

impl Settings {
    /// Puts every value back inside its allowed range.
    pub fn sanitize(mut self) -> Self {
        self.version = VERSION;
        self.sound.fade_ms = self.sound.fade_ms.clamp(FADE_MS.0, FADE_MS.1);
        self.sound.max_volume = clamp_f(self.sound.max_volume, MAX_VOLUME, 1.0);
        self.timer.end_fade_s = self.timer.end_fade_s.clamp(END_FADE_S.0, END_FADE_S.1);
        for m in &mut self.timer.presets {
            *m = (*m).clamp(PRESET_MINUTES.0, PRESET_MINUTES.1);
        }
        self.last.volume = clamp_f(self.last.volume, (0.0, 1.0), 0.4);
        self.last.timer_minutes = self.last.timer_minutes.min(PRESET_MINUTES.1);
        let st = &mut self.study;
        st.work_min = st.work_min.clamp(WORK_MINUTES.0, WORK_MINUTES.1);
        st.short_break_min = st.short_break_min.clamp(BREAK_MINUTES.0, BREAK_MINUTES.1);
        st.long_break_min = st
            .long_break_min
            .clamp(LONG_BREAK_MINUTES.0, LONG_BREAK_MINUTES.1);
        st.cycles = st.cycles.clamp(CYCLES.0, CYCLES.1);
        if st.idle_minutes != 0 {
            st.idle_minutes = st.idle_minutes.clamp(IDLE_MINUTES.0, IDLE_MINUTES.1);
        }
        let ear = &mut self.general.ear_break_hours;
        if *ear != 0 {
            *ear = (*ear).clamp(EAR_BREAK_HOURS.0, EAR_BREAK_HOURS.1);
        }
        for v in self.last.output_volumes.values_mut() {
            *v = clamp_f(*v, (0.0, 1.0), 0.4);
        }
        for action in ShortcutAction::ALL {
            let blank = self
                .shortcuts
                .get(action)
                .is_some_and(|s| s.trim().is_empty());
            if blank {
                self.shortcuts.set(action, None);
            }
        }
        if self
            .advanced
            .output_device
            .as_ref()
            .is_some_and(|d| d.trim().is_empty())
        {
            self.advanced.output_device = None;
        }
        self
    }

    /// Parses a stored or imported file. Missing fields take their defaults,
    /// unknown ones are ignored, out-of-range values are clamped.
    pub fn from_json(value: Value) -> Result<Self, SettingsError> {
        Ok(serde_json::from_value::<Settings>(value)?.sanitize())
    }

    /// Applies a partial update such as `{"sound": {"fadeMs": 800}}`.
    pub fn merged(&self, patch: &Value) -> Result<Self, SettingsError> {
        if !patch.is_object() {
            return Err(SettingsError::NotAnObject);
        }
        let mut current = serde_json::to_value(self)?;
        merge(&mut current, patch);
        Self::from_json(current)
    }

    /// The colour to start with.
    pub fn start_color(&self) -> NoiseColor {
        match self.sound.start_color {
            StartColor::Last => self.last.color,
            StartColor::Pink => NoiseColor::Pink,
            StartColor::Brown => NoiseColor::Brown,
            StartColor::White => NoiseColor::White,
        }
    }

    /// Whether to start playing right away.
    pub fn start_playing(&self) -> bool {
        match self.playback.on_launch {
            OnLaunch::Silent => false,
            OnLaunch::Play => true,
            OnLaunch::Resume => self.last.was_playing,
        }
    }

    /// The volume actually sent to the engine for a slider position: the
    /// slider's full travel covers 0..=max (SPEC §4.4).
    pub fn effective_volume(&self, slider: f32) -> f32 {
        slider.clamp(0.0, 1.0) * self.sound.max_volume
    }
}

/// RFC 7396-style merge: objects merge key by key, anything else replaces.
fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => merge(existing, v),
                    _ => {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

/// Reads the saved settings (`%APPDATA%\<id>\settings.json`). A missing or
/// unreadable file gives the defaults; it never stops Rumor from starting.
pub fn load(app: &tauri::AppHandle) -> Settings {
    use tauri_plugin_store::StoreExt;
    let store = match app.store(STORE_FILE) {
        Ok(store) => store,
        Err(e) => {
            log::warn!("settings store unavailable, using defaults: {e}");
            return Settings::default();
        }
    };
    match store.get(STORE_KEY).map(Settings::from_json) {
        Some(Ok(settings)) => settings,
        Some(Err(e)) => {
            log::warn!("settings unreadable, using defaults: {e}");
            Settings::default()
        }
        None => Settings::default(),
    }
}

/// Stores the settings. The store writes to disk shortly after (debounced),
/// so a volume drag doesn't hammer the disk; `flush` writes now.
pub fn save(app: &tauri::AppHandle, settings: &Settings) {
    use tauri_plugin_store::StoreExt;
    match (app.store(STORE_FILE), serde_json::to_value(settings)) {
        (Ok(store), Ok(value)) => store.set(STORE_KEY, value),
        (Err(e), _) => log::warn!("settings not saved: {e}"),
        (_, Err(e)) => log::warn!("settings not saved: {e}"),
    }
}

/// Writes pending changes to disk right away (on quit).
pub fn flush(app: &tauri::AppHandle) {
    use tauri_plugin_store::StoreExt;
    if let Ok(store) = app.store(STORE_FILE) {
        if let Err(e) = store.save() {
            log::warn!("settings not written: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_spec() {
        let s = Settings::default();
        assert_eq!(s.sound.fade_ms, 400);
        assert_eq!(s.sound.max_volume, 1.0);
        assert_eq!(
            s.playback.on_output_change,
            OutputChangePolicy::PauseOnSpeakers
        );
        assert!(!s.playback.resume_on_reconnect);
        assert_eq!(s.playback.on_launch, OnLaunch::Silent);
        assert_eq!(s.timer.presets, [25, 50, 90]);
        assert_eq!(s.timer.end_fade_s, 10);
        assert_eq!(s.shortcuts.toggle.as_deref(), Some("Ctrl+Alt+P"));
        assert_eq!(s.tray.left_click, LeftClick::Toggle);
        assert!(!s.startup.launch_at_login);
        assert!(s.startup.check_updates);
        assert_eq!(s.appearance.accent, AccentChoice::Noise);
        assert!(s.sound.per_output_volume);
        assert!(!s.sound.mono);
        assert_eq!(s.playback.on_lock, LockPolicy::PauseResumeWithHeadphones);
        assert_eq!(s.general.alerts, Alerts::Both);
        assert_eq!(s.general.ear_break_hours, 3);
        assert!(!s.study.enabled, "Modo estudio is off by default");
        assert!(s.study.link_noise);
        assert_eq!(s.study.plan(), Plan::default());
        assert_eq!(s.study.break_noise, BreakNoise::Half);
        assert_eq!(s.study.idle_minutes, 0);
        assert_eq!(s.shortcuts.study.as_deref(), Some("Ctrl+Alt+T"));
        assert_eq!(s.last.study_mode, StudyMode::Stopwatch);
    }

    #[test]
    fn settings_from_before_updates_check_for_them() {
        let s = Settings::from_json(json!({ "startup": { "launchAtLogin": true } })).unwrap();
        assert!(s.startup.launch_at_login);
        assert!(s.startup.check_updates);
    }

    #[test]
    fn the_removed_10_ms_buffer_reads_as_auto() {
        let s = Settings::from_json(json!({ "advanced": { "buffer": "ms10" } })).unwrap();
        assert_eq!(s.advanced.buffer, BufferChoice::Auto);
        assert_eq!(BufferChoice::Auto.millis(), None);
        let s = Settings::from_json(json!({ "advanced": { "buffer": "ms100" } })).unwrap();
        assert_eq!(s.advanced.buffer.millis(), Some(100));
    }

    #[test]
    fn old_files_lose_the_removed_options_quietly() {
        let s = Settings::from_json(json!({
            "timer": { "notifyOnEnd": false, "presets": [10, 20, 30] },
            "tray": { "showRemaining": false, "leftClick": "panel" },
            "appearance": { "reduceMotion": true, "theme": "dark" }
        }))
        .unwrap();
        assert_eq!(s.timer.presets, [10, 20, 30]);
        assert_eq!(s.tray.left_click, LeftClick::Panel);
        assert_eq!(s.appearance.theme, ThemeChoice::Dark);
        assert_eq!(s.general, General::default());
    }

    #[test]
    fn study_values_are_clamped_and_zero_means_off() {
        let s = Settings::from_json(json!({
            "study": { "workMin": 1, "shortBreakMin": 500, "cycles": 0, "idleMinutes": 2 },
            "general": { "earBreakHours": 0 }
        }))
        .unwrap();
        assert_eq!(s.study.work_min, WORK_MINUTES.0);
        assert_eq!(s.study.short_break_min, BREAK_MINUTES.1);
        assert_eq!(s.study.cycles, CYCLES.0);
        assert_eq!(s.study.idle_minutes, IDLE_MINUTES.0);
        assert_eq!(s.general.ear_break_hours, 0);
        let off = Settings::from_json(json!({ "study": { "idleMinutes": 0 } })).unwrap();
        assert_eq!(off.study.idle_minutes, 0);
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings::default();
        s.sound.fade_ms = 1_200;
        s.last.color = NoiseColor::Brown;
        s.advanced.output_device = Some("{0.0.0.00000000}.{abc}".into());
        let back = Settings::from_json(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn missing_and_unknown_fields_are_fine() {
        let s =
            Settings::from_json(json!({ "sound": { "fadeMs": 900 }, "fromTheFuture": 1 })).unwrap();
        assert_eq!(s.sound.fade_ms, 900);
        assert_eq!(s.sound.max_volume, 1.0);
        assert_eq!(s.timer, Timer::default());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = Settings::from_json(json!({
            "sound": { "fadeMs": 50_000, "maxVolume": 0.0 },
            "timer": { "presets": [0, 50, 100_000], "endFadeS": 0 },
            "last": { "volume": 7.0 },
            "shortcuts": { "toggle": "  " },
            "advanced": { "outputDevice": "" }
        }))
        .unwrap();
        assert_eq!(s.sound.fade_ms, FADE_MS.1);
        assert_eq!(s.sound.max_volume, MAX_VOLUME.0);
        assert_eq!(s.timer.presets, [1, 50, 600]);
        assert_eq!(s.timer.end_fade_s, END_FADE_S.0);
        assert_eq!(s.last.volume, 1.0);
        assert_eq!(s.shortcuts.toggle, None);
        assert_eq!(s.advanced.output_device, None);
    }

    #[test]
    fn wrong_types_are_rejected_not_half_applied() {
        assert!(Settings::from_json(json!({ "sound": { "fadeMs": "slow" } })).is_err());
        assert!(Settings::default().merged(&json!(3)).is_err());
    }

    #[test]
    fn partial_updates_touch_only_what_they_name() {
        let base = Settings::default();
        let next = base
            .merged(&json!({ "appearance": { "theme": "dark" }, "tray": { "leftClick": "panel" } }))
            .unwrap();
        assert_eq!(next.appearance.theme, ThemeChoice::Dark);
        assert_eq!(next.tray.left_click, LeftClick::Panel);
        assert_eq!(next.appearance.mica, base.appearance.mica);
        assert_eq!(next.sound, base.sound);
        // null clears an optional value.
        let cleared = base
            .merged(&json!({ "shortcuts": { "toggle": null } }))
            .unwrap();
        assert_eq!(cleared.shortcuts.toggle, None);
    }

    #[test]
    fn start_colour_and_playing_follow_the_choices() {
        let mut s = Settings::default();
        s.last.color = NoiseColor::White;
        s.last.was_playing = true;
        assert_eq!(s.start_color(), NoiseColor::White);
        s.sound.start_color = StartColor::Brown;
        assert_eq!(s.start_color(), NoiseColor::Brown);
        assert!(!s.start_playing());
        s.playback.on_launch = OnLaunch::Resume;
        assert!(s.start_playing());
        s.last.was_playing = false;
        assert!(!s.start_playing());
        s.playback.on_launch = OnLaunch::Play;
        assert!(s.start_playing());
    }

    #[test]
    fn max_volume_scales_the_whole_slider() {
        let mut s = Settings::default();
        s.sound.max_volume = 0.6;
        assert_eq!(s.effective_volume(1.0), 0.6);
        assert_eq!(s.effective_volume(0.5), 0.3);
        assert_eq!(s.effective_volume(2.0), 0.6);
    }
}
