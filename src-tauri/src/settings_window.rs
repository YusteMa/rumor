//! The Settings window (SPEC §4.4): a normal window with the Windows title
//! bar and Mica, one instance, opened from the panel. Changes apply at once;
//! it listens to `settings-changed` to stay in step with the rest of the app.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::audio::engine::OutputInfo;
use crate::i18n::Lang;
use crate::settings::{Appearance, Settings, ThemeChoice};
use crate::shortcut::{self, ShortcutStatus};
use crate::state::{self, AppState};
use crate::study::db::Tag;
use crate::taskbar;
use crate::update::{self, UpdateView};

pub const LABEL: &str = "settings";
/// Opening size, in logical pixels: room for text and control side by side.
const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 640.0;
/// Smallest size: below it rows would stop fitting (controls already wrap
/// under their text from here up to about WIDTH).
const MIN_WIDTH: f64 = 520.0;
const MIN_HEIGHT: f64 = 440.0;
/// Kept free around the window inside the work area (title bar and air).
const SCREEN_MARGIN: f64 = 48.0;

/// Whether Mica is actually behind the window (fails on Windows 10).
static MICA: AtomicBool = AtomicBool::new(false);

/// Everything the Settings window shows.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub shortcuts: Vec<ShortcutStatus>,
    pub output: Option<OutputInfo>,
    pub lang: Lang,
    pub version: String,
    pub system_accent: Option<String>,
    pub mica: bool,
    /// Study tags, for Settings → Study.
    pub tags: Vec<Tag>,
    /// A newer version, for Settings → About.
    pub update: Option<UpdateView>,
}

pub fn view(app: &AppHandle) -> Option<SettingsView> {
    let state = app.try_state::<AppState>()?;
    Some(SettingsView {
        settings: state.settings(),
        shortcuts: shortcut::status(app),
        output: state.engine.output(),
        lang: state.lang(),
        version: app.package_info().version.to_string(),
        system_accent: taskbar::system_accent(),
        mica: MICA.load(Ordering::Relaxed),
        tags: state::tags(app),
        update: update::view(app),
    })
}

pub fn theme_of(choice: ThemeChoice) -> Option<Theme> {
    match choice {
        ThemeChoice::System => None,
        ThemeChoice::Light => Some(Theme::Light),
        ThemeChoice::Dark => Some(Theme::Dark),
    }
}

fn apply_mica(window: &WebviewWindow, appearance: &Appearance) {
    let dark = theme_of(appearance.theme).map(|t| t == Theme::Dark);
    let on = if appearance.mica {
        window_vibrancy::apply_mica(window, dark).is_ok()
    } else {
        let _ = window_vibrancy::clear_mica(window);
        false
    };
    MICA.store(on, Ordering::Relaxed);
}

/// Opening and minimum sizes that fit the monitor under the cursor at any
/// scaling (a 1080p screen at 200 % has only ~960 × 516 logical pixels).
fn fitted_sizes(app: &AppHandle) -> ((f64, f64), (f64, f64)) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return ((WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT));
    };
    let work = monitor
        .work_area()
        .size
        .to_logical::<f64>(monitor.scale_factor());
    let room = (work.width - SCREEN_MARGIN, work.height - SCREEN_MARGIN);
    let min = (MIN_WIDTH.min(room.0), MIN_HEIGHT.min(room.1));
    let size = (WIDTH.min(room.0).max(min.0), HEIGHT.min(room.1).max(min.1));
    (size, min)
}

pub fn open(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let app = app.clone();
    // Built off the event loop's thread, like the panel (see panel.rs).
    std::thread::spawn(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let appearance = state.settings().appearance;
        let title = state.strings().get("settingsTitle").to_owned();
        let ((width, height), (min_width, min_height)) = fitted_sizes(&app);
        let built = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("settings.html".into()))
            .title(title)
            .inner_size(width, height)
            .min_inner_size(min_width, min_height)
            .maximizable(false)
            .transparent(true)
            .theme(theme_of(appearance.theme))
            .visible(false)
            .center()
            .build();
        match built {
            Ok(window) => {
                apply_mica(&window, &appearance);
                // Shown by the page once it has rendered (no white flash).
            }
            Err(e) => log::error!("settings window: {e}"),
        }
    });
}

/// The page rendered its first state (`settings_ready`).
pub fn on_ready(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.close();
    }
}

pub fn apply_appearance(app: &AppHandle, appearance: &Appearance) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_theme(theme_of(appearance.theme));
        apply_mica(&window, appearance);
    }
}

pub fn emit_changed(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_none() {
        return;
    }
    if let Some(view) = view(app) {
        let _ = app.emit_to(LABEL, "settings-changed", view);
    }
}
