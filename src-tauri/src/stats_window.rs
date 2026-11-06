//! The Statistics window (SPEC §4.3d): like Settings, a normal window with
//! the Windows title bar and Mica, one instance, opened from the study card.
//! Rust does all the sums; the page asks for each view and listens to
//! `stats-changed` (the history changed) and `stats-context` (language,
//! accent or Mica changed).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::audio::noise::NoiseColor;
use crate::i18n::Lang;
use crate::settings::{AccentChoice, Appearance};
use crate::settings_window::{fitted_sizes, set_mica, theme_of};
use crate::state::AppState;
use crate::taskbar;

pub const LABEL: &str = "stats";
/// Opening size, in logical pixels: the week fits without scrolling much.
const WIDTH: f64 = 820.0;
const HEIGHT: f64 = 720.0;
/// Two cards side by side still fit from here.
const MIN_WIDTH: f64 = 560.0;
const MIN_HEIGHT: f64 = 480.0;

static MICA: AtomicBool = AtomicBool::new(false);

/// What the page needs to look like the rest of Rumor.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub lang: Lang,
    /// The accent follows the noise, as in the panel.
    pub color: NoiseColor,
    pub system_accent: Option<String>,
    pub mica: bool,
}

pub fn context(app: &AppHandle) -> Option<Context> {
    let state = app.try_state::<AppState>()?;
    let settings = state.settings();
    Some(Context {
        lang: state.lang(),
        color: settings.last.color,
        system_accent: (settings.appearance.accent == AccentChoice::System)
            .then(taskbar::system_accent)
            .flatten(),
        mica: MICA.load(Ordering::Relaxed),
    })
}

pub fn open(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let app = app.clone();
    // Built off the event loop's thread, like Settings.
    std::thread::spawn(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let appearance = state.settings().appearance;
        let title = state.strings().get("statsTitle").to_owned();
        let ((width, height), (min_width, min_height)) =
            fitted_sizes(&app, (WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT));
        let built = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("stats.html".into()))
            .title(title)
            .inner_size(width, height)
            .min_inner_size(min_width, min_height)
            .transparent(true)
            .theme(theme_of(appearance.theme))
            .visible(false)
            .center()
            .build();
        match built {
            Ok(window) => {
                MICA.store(set_mica(&window, &appearance), Ordering::Relaxed);
                // Shown by the page once it has rendered (no white flash).
            }
            Err(e) => log::error!("statistics window: {e}"),
        }
    });
}

/// The page rendered its first view (`stats_ready`).
pub fn on_ready(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn apply_appearance(app: &AppHandle, appearance: &Appearance) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_theme(theme_of(appearance.theme));
        MICA.store(set_mica(&window, appearance), Ordering::Relaxed);
    }
}

/// The history changed: the page asks for its view again.
pub fn emit_changed(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, "stats-changed", ());
    }
}

/// Language, accent or Mica may have changed (cheap: no sums).
pub fn emit_context(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_none() {
        return;
    }
    if let Some(cx) = context(app) {
        let _ = app.emit_to(LABEL, "stats-context", cx);
    }
}
