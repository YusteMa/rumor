// Only the commands listed here can be granted, and the capability in
// capabilities/default.json grants them to the panel (CLAUDE.md rule 6).
const COMMANDS: &[&str] = &[
    "get_state",
    "toggle",
    "set_playing",
    "set_color",
    "set_volume",
    "set_timer",
    "study_toggle",
    "study_finish",
    "study_set_mode",
    "study_set_tag",
    "study_resolve_idle",
    "study_totals",
    "tags_list",
    "tags_upsert",
    "tags_delete",
    "hide_panel",
    "panel_ready",
    "panel_painted",
    "open_settings",
    "get_settings",
    "settings_ready",
    "update_settings",
    "set_shortcut",
    "list_outputs",
    "export_settings",
    "import_settings",
    "reset_settings",
    "open_logs",
    "quit",
];

fn main() {
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
