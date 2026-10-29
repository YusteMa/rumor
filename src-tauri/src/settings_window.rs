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
use crate::state::AppState;
use crate::taskbar;

pub const LABEL: &str = "settings";
const WIDTH: f64 = 560.0;
const HEIGHT: f64 = 640.0;

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
        let built = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("settings.html".into()))
            .title(title)
            .inner_size(WIDTH, HEIGHT)
            .min_inner_size(WIDTH, HEIGHT)
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
