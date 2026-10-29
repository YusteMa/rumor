//! Tray icon, tooltip and clicks (SPEC §4.1). Left click toggles the noise;
//! right click shows a provisional menu until the panel exists (phase 3).

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::audio::device_watch::PauseReason;
use crate::audio::noise::NoiseColor;
use crate::state::{self, AppState, Snapshot};
use crate::{shortcut, taskbar};

const ID: &str = "main";

macro_rules! icons {
    ($($state:literal $theme:literal $size:literal),* $(,)?) => {
        &[$((
            $state,
            $theme,
            $size,
            include_bytes!(concat!("../icons/tray/", $state, "-", $theme, "-", $size, ".png")) as &[u8],
        )),*]
    };
}

/// (state, taskbar theme, size in px, PNG), drawn by `scripts/gen-tray-icons.mjs`.
const ICONS: &[(&str, &str, u32, &[u8])] = icons![
    "active" "light" 16, "active" "light" 20, "active" "light" 24,
    "active" "light" 32, "active" "light" 40, "active" "light" 48,
    "active" "dark" 16, "active" "dark" 20, "active" "dark" 24,
    "active" "dark" 32, "active" "dark" 40, "active" "dark" 48,
    "idle" "light" 16, "idle" "light" 20, "idle" "light" 24,
    "idle" "light" 32, "idle" "light" 40, "idle" "light" 48,
    "idle" "dark" 16, "idle" "dark" 20, "idle" "dark" 24,
    "idle" "dark" 32, "idle" "dark" 40, "idle" "dark" 48,
];

/// Menu items whose text follows the state.
struct TrayMenu {
    toggle: MenuItem<Wry>,
}

/// The icon drawn for the current scaling, so Windows never resamples it.
fn icon(app: &AppHandle, playing: bool) -> Option<Image<'static>> {
    let state = if playing { "active" } else { "idle" };
    let theme = if taskbar::uses_light_theme() {
        "light"
    } else {
        "dark"
    };
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |m| m.scale_factor());
    let wanted = (16.0 * scale).round() as u32;
    let mut candidates = ICONS
        .iter()
        .filter(|(s, t, _, _)| *s == state && *t == theme);
    let best = candidates
        .clone()
        .filter(|(_, _, size, _)| *size >= wanted)
        .min_by_key(|(_, _, size, _)| *size)
        .or_else(|| candidates.next_back())?;
    match Image::from_bytes(best.3) {
        Ok(image) => Some(image.to_owned()),
        Err(e) => {
            log::error!("tray icon: {e}");
            None
        }
    }
}

fn color_key(color: NoiseColor) -> &'static str {
    match color {
        NoiseColor::Pink => "colorPink",
        NoiseColor::Brown => "colorBrown",
        NoiseColor::White => "colorWhite",
    }
}

fn tooltip(state: &AppState, s: &Snapshot) -> String {
    let t = &state.strings;
    if s.playing {
        let volume = (s.sound.volume * 100.0).round().to_string();
        return t.format(
            "trayPlaying",
            &[
                ("color", t.get(color_key(s.sound.color))),
                ("volume", &volume),
            ],
        );
    }
    let reason = match s.paused_for {
        Some(PauseReason::HeadphonesDisconnected) => Some("pausedHeadphones"),
        Some(PauseReason::NoOutput) => Some("pausedNoOutput"),
        Some(PauseReason::OutputChanged) => Some("pausedOutputChanged"),
        Some(PauseReason::Suspended) | None => None,
    };
    match reason {
        Some(key) => format!("Rumor · {}", t.get(key)),
        None => t.get("trayPaused").to_owned(),
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot();
    let accelerator = shortcut::toggle_label(app);
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        state.strings.get(if snapshot.playing {
            "menuPause"
        } else {
            "menuPlay"
        }),
        true,
        accelerator.as_deref(),
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        state.strings.get("menuQuit"),
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&toggle, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id(ID)
        .tooltip(tooltip(&state, &snapshot))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => state::toggle(app),
            "quit" => state::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                state::toggle(tray.app_handle());
            }
        });
    if let Some(image) = icon(app, snapshot.playing) {
        builder = builder.icon(image);
    }
    builder.build(app)?;
    app.manage(TrayMenu { toggle });

    let handle = app.clone();
    taskbar::watch_theme(move || refresh(&handle));
    Ok(())
}

pub fn refresh(app: &AppHandle) {
    let (Some(tray), Some(state)) = (app.tray_by_id(ID), app.try_state::<AppState>()) else {
        return;
    };
    let snapshot = state.snapshot();
    if let Err(e) = tray.set_icon(icon(app, snapshot.playing)) {
        log::warn!("tray icon: {e}");
    }
    if let Err(e) = tray.set_tooltip(Some(tooltip(&state, &snapshot))) {
        log::warn!("tray tooltip: {e}");
    }
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let key = if snapshot.playing {
            "menuPause"
        } else {
            "menuPlay"
        };
        let _ = menu.toggle.set_text(state.strings.get(key));
    }
}
