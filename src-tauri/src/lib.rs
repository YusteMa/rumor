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
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            state::second_instance(app, &args);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                // Our own debug lines (timings, second-instance args) in dev builds.
                .level_for(
                    "rumor_lib",
                    if cfg!(debug_assertions) {
                        log::LevelFilter::Debug
                    } else {
                        log::LevelFilter::Info
                    },
                )
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(shortcut::plugin())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::toggle,
            commands::set_playing,
            commands::set_color,
            commands::set_volume,
            commands::set_timer,
            commands::hide_panel,
            commands::panel_ready,
            commands::panel_painted,
            commands::quit,
        ])
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
