//! Rumor: real-time noise generator that lives in the Windows tray.
//!
//! All state lives here in Rust; the panel is only a remote control
//! (see `CLAUDE.md`). Modules are filled in phase by phase (`ROADMAP.md`).

pub mod audio;
mod commands;
mod panel;
mod settings;
mod shortcut;
mod state;
mod study;
mod timer;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(err) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("rumor: failed to start: {err}");
        std::process::exit(1);
    }
}
