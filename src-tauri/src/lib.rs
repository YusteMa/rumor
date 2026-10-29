//! Rumor: real-time noise generator that lives in the Windows tray.
//!
//! All state lives here in Rust; the panel is only a remote control
//! (see `CLAUDE.md`). Modules are filled in phase by phase (`ROADMAP.md`).

pub mod audio;
mod commands;
mod i18n;
mod panel;
mod settings;
mod shortcut;
mod state;
mod study;
mod taskbar;
mod timer;
mod tray;

use tauri::RunEvent;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // Must be first, so a second launch exits before doing anything else.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            state::second_instance(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(shortcut::plugin())
        .setup(|app| state::setup(app.handle()))
        .build(tauri::generate_context!());

    match app {
        // A tray app has no windows to close; only "Quit" ends it.
        Ok(app) => app.run(|_, event| {
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        }),
        Err(err) => {
            eprintln!("rumor: failed to start: {err}");
            std::process::exit(1);
        }
    }
}
