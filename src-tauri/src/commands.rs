//! `#[tauri::command]` handlers exposed to the panel (CLAUDE.md contract).
//! Each one only forwards to the coordinator in `state.rs`.

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::audio::device_watch::Endpoint;
use crate::audio::engine;
use crate::audio::noise::NoiseColor;
use crate::panel;
use crate::settings::{Settings, ShortcutAction};
use crate::settings_window::{self, SettingsView};
use crate::shortcut::{self, ShortcutError};
use crate::state::{self, PanelState};
use crate::study::StudyView;
use crate::study::db::Tag;
use crate::study::machine::Mode;
use crate::update;

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

/// The stopwatch's play/pause (SPEC §4.3b).
#[tauri::command]
pub fn study_toggle(app: AppHandle) {
    state::study_toggle(&app);
}

/// Ends the session and stores it.
#[tauri::command]
pub fn study_finish(app: AppHandle) {
    state::study_finish(&app);
}

/// Only between sessions.
#[tauri::command]
pub fn study_set_mode(app: AppHandle, mode: Mode) {
    state::study_set_mode(&app, mode);
}

/// `tag` = `None` for no tag.
#[tauri::command]
pub fn study_set_tag(app: AppHandle, tag: Option<i64>) {
    state::study_set_tag(&app, tag);
}

/// "¿Seguías estudiando?": `keep` = "Sí, contar", otherwise the idle time
/// is taken out.
#[tauri::command]
pub fn study_resolve_idle(app: AppHandle, keep: bool) {
    state::study_resolve_idle(&app, keep);
}

/// Session, today and this week; `None` with "Modo estudio" off.
#[tauri::command]
pub fn study_totals(app: AppHandle) -> Option<StudyView> {
    state::study_view(&app)
}

#[tauri::command]
pub fn tags_list(app: AppHandle) -> Vec<Tag> {
    state::tags(&app)
}

/// Creates a tag (`id` = `None`) or renames / recolours one.
#[tauri::command]
pub fn tags_upsert(
    app: AppHandle,
    id: Option<i64>,
    name: String,
    color: String,
) -> Result<Tag, String> {
    state::tags_upsert(&app, id, &name, &color).map_err(|e| e.to_string())
}

/// Its sessions are kept, untagged.
#[tauri::command]
pub fn tags_delete(app: AppHandle, id: i64) -> Result<(), String> {
    state::tags_delete(&app, id).map_err(|e| e.to_string())
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

/// The panel's ⚙ button.
#[tauri::command]
pub fn open_settings(app: AppHandle) {
    panel::hide(&app);
    settings_window::open(&app);
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Option<SettingsView> {
    settings_window::view(&app)
}

/// The Settings page has rendered; show its window.
#[tauri::command]
pub fn settings_ready(app: AppHandle) {
    settings_window::on_ready(&app);
}

/// A partial update, e.g. `{"sound": {"fadeMs": 800}}`.
#[tauri::command]
pub fn update_settings(app: AppHandle, patch: Value) -> Result<Option<SettingsView>, String> {
    state::update_settings(&app, patch)?;
    Ok(settings_window::view(&app))
}

/// `shortcut` = `None` clears it.
#[tauri::command]
pub fn set_shortcut(
    app: AppHandle,
    action: ShortcutAction,
    shortcut: Option<String>,
) -> Result<Option<SettingsView>, ShortcutError> {
    shortcut::set(&app, action, shortcut)?;
    Ok(settings_window::view(&app))
}

#[tauri::command]
pub fn list_outputs() -> Vec<Endpoint> {
    engine::output_devices()
}

/// Exports every setting (not what Rumor remembers between runs) to a JSON
/// file the user picks. `false` if they cancelled.
#[tauri::command]
pub async fn export_settings(app: AppHandle) -> Result<bool, String> {
    let settings = app.state::<state::AppState>().settings();
    let mut value = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.remove("last");
        object.remove("onboarded");
    }
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name("rumor-settings.json")
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    log::info!("settings exported to {}", path.display());
    Ok(true)
}

/// Imports settings from a JSON file the user picks. Unknown fields are
/// ignored and bad values clamped; a file that isn't settings is refused.
#[tauri::command]
pub async fn import_settings(app: AppHandle) -> Result<bool, String> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .blocking_pick_file()
    else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let imported = Settings::from_json(value).map_err(|e| e.to_string())?;
    state::replace_settings(&app, imported);
    log::info!("settings imported from {}", path.display());
    Ok(true)
}

/// Back to defaults (what Rumor remembers between runs is kept).
#[tauri::command]
pub fn reset_settings(app: AppHandle) -> Option<SettingsView> {
    state::replace_settings(&app, Settings::default());
    settings_window::view(&app)
}

#[tauri::command]
pub fn open_logs(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Settings → About's "Buscar ahora": the newer version, or `None` when this
/// one is the latest.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, String> {
    update::check(&app).await
}

/// The update's "Actualizar" button. Rumor closes and reopens on success.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    update::install(&app).await
}
