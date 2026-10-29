//! Global shortcuts and conflict detection (SPEC §4.3).

use std::sync::Mutex;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::{self, AppState};

/// Toggle-noise candidates, preferred first. None of them types a character
/// with AltGr (= Ctrl+Alt) on Spanish or other European layouts (CLAUDE.md
/// rule 13).
const TOGGLE_CANDIDATES: [&str; 3] = ["Ctrl+Alt+P", "Ctrl+Alt+N", "Ctrl+Shift+Alt+P"];

/// The shortcut currently bound to "toggle noise", as shown to the user.
#[derive(Default)]
pub struct Bindings {
    toggle: Mutex<Option<(String, Shortcut)>>,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let bound = app.try_state::<Bindings>().and_then(|b| {
                b.toggle
                    .lock()
                    .ok()
                    .and_then(|t| t.as_ref().map(|(_, s)| *s))
            });
            if bound.as_ref() == Some(shortcut) {
                state::toggle(app);
            }
        })
        .build()
}

pub fn register_defaults(app: &AppHandle) {
    app.manage(Bindings::default());
    let Some(used) = TOGGLE_CANDIDATES.iter().find(|label| try_bind(app, label)) else {
        log::warn!("no free toggle shortcut");
        let strings = &app.state::<AppState>().strings;
        let body = strings.format("shortcutTakenNone", &[("taken", TOGGLE_CANDIDATES[0])]);
        state::notify(app, strings.get("shortcutTakenTitle"), &body);
        return;
    };
    if *used != TOGGLE_CANDIDATES[0] {
        let strings = &app.state::<AppState>().strings;
        let body = strings.format(
            "shortcutTakenFallback",
            &[("taken", TOGGLE_CANDIDATES[0]), ("used", used)],
        );
        state::notify(app, strings.get("shortcutTakenTitle"), &body);
    }
}

/// Registers `label` as the toggle shortcut. False if it can't be parsed or
/// another app already owns it.
fn try_bind(app: &AppHandle, label: &str) -> bool {
    let shortcut: Shortcut = match label.parse() {
        Ok(s) => s,
        Err(e) => {
            log::error!("bad shortcut {label}: {e}");
            return false;
        }
    };
    match app.global_shortcut().register(shortcut) {
        Ok(()) => {
            log::info!("toggle shortcut: {label}");
            if let Ok(mut t) = app.state::<Bindings>().toggle.lock() {
                *t = Some((label.to_owned(), shortcut));
            }
            true
        }
        Err(e) => {
            log::warn!("shortcut {label} unavailable: {e}");
            false
        }
    }
}

/// The toggle shortcut in use, for the tooltip and (later) Settings.
pub fn toggle_label(app: &AppHandle) -> Option<String> {
    let bindings = app.try_state::<Bindings>()?;
    let toggle = bindings.toggle.lock().ok()?;
    toggle.as_ref().map(|(label, _)| label.clone())
}
