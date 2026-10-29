//! `#[tauri::command]` handlers exposed to the panel (CLAUDE.md contract).
//! Each one only forwards to the coordinator in `state.rs`.

use tauri::AppHandle;

use crate::audio::noise::NoiseColor;
use crate::panel;
use crate::state::{self, PanelState};

#[tauri::command]
pub fn get_state(app: AppHandle) -> Option<PanelState> {
    state::panel_state(&app)
}

#[tauri::command]
pub fn toggle(app: AppHandle) {
    state::toggle(&app);
}

#[tauri::command]
pub fn set_playing(app: AppHandle, playing: bool) {
    state::set_playing(&app, playing);
}

#[tauri::command]
pub fn set_color(app: AppHandle, color: NoiseColor) {
    state::set_color(&app, color);
}

/// `volume` is the slider position in 0..=1.
#[tauri::command]
pub fn set_volume(app: AppHandle, volume: f32) {
    state::set_volume(&app, volume);
}

/// `minutes` = 0 turns the timer off.
#[tauri::command]
pub fn set_timer(app: AppHandle, minutes: u32) {
    state::set_timer(&app, minutes);
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    panel::hide(&app);
}

/// The panel page has loaded and rendered its first state.
#[tauri::command]
pub fn panel_ready(app: AppHandle) {
    panel::on_ready(&app);
}

/// The panel painted its first frame after being shown (timing log).
#[tauri::command]
pub fn panel_painted(app: AppHandle) {
    panel::on_painted(&app);
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    state::quit(&app);
}
