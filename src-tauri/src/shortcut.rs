//! Global shortcuts (SPEC §4.3): one per action, conflict detection, and the
//! AltGr rule (CLAUDE.md rule 13): a shortcut must never be a key combination
//! that types a character on the user's keyboard layout.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::panel;
use crate::settings::ShortcutAction;
use crate::state::{self, AppState};

/// Toggle-noise candidates when the default is taken, preferred first. None
/// of them types a character with AltGr (= Ctrl+Alt) on Spanish or other
/// European layouts.
const TOGGLE_CANDIDATES: [&str; 3] = ["Ctrl+Alt+P", "Ctrl+Alt+N", "Ctrl+Shift+Alt+P"];

#[derive(Default)]
struct Bindings {
    bound: Mutex<HashMap<ShortcutAction, (String, Shortcut)>>,
    /// The preferred toggle shortcut, if another app owns it.
    taken: Mutex<Option<String>>,
    /// Actions whose saved shortcut could not be registered.
    failed: Mutex<HashMap<ShortcutAction, String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a shortcut was refused (the Settings window shows each case).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShortcutError {
    #[error("not a valid shortcut")]
    Invalid,
    #[error("needs Ctrl, Alt, Shift or Win")]
    NoModifier,
    #[error("types a character with AltGr")]
    TypesCharacter,
    #[error("already used for another action")]
    Duplicate,
    #[error("already used by another app")]
    Taken,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = app.try_state::<Bindings>().and_then(|b| {
                lock(&b.bound)
                    .iter()
                    .find(|(_, (_, s))| s == shortcut)
                    .map(|(a, _)| *a)
            });
            match action {
                Some(ShortcutAction::Toggle) => state::toggle(app),
                Some(ShortcutAction::VolumeUp) => state::nudge_volume(app, true),
                Some(ShortcutAction::VolumeDown) => state::nudge_volume(app, false),
                Some(ShortcutAction::NextColor) => state::next_color(app),
                Some(ShortcutAction::OpenPanel) => panel::open(app, None),
                None => {}
            }
        })
        .build()
}

fn parse(label: &str) -> Result<Shortcut, ShortcutError> {
    label.parse().map_err(|_| ShortcutError::Invalid)
}

fn bind(app: &AppHandle, action: ShortcutAction, label: &str) -> Result<(), ShortcutError> {
    let shortcut = parse(label)?;
    app.global_shortcut().register(shortcut).map_err(|e| {
        log::warn!("shortcut {label} unavailable: {e}");
        ShortcutError::Taken
    })?;
    log::info!("shortcut {action:?}: {label}");
    if let Some(b) = app.try_state::<Bindings>() {
        lock(&b.bound).insert(action, (label.to_owned(), shortcut));
        lock(&b.failed).remove(&action);
    }
    Ok(())
}

fn unbind(app: &AppHandle, action: ShortcutAction) {
    let Some(b) = app.try_state::<Bindings>() else {
        return;
    };
    let removed = lock(&b.bound).remove(&action);
    if let Some((_, shortcut)) = removed {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

/// Registers every saved shortcut (at start, or after an import or reset).
pub fn register_all(app: &AppHandle) {
    if app.try_state::<Bindings>().is_none() {
        app.manage(Bindings::default());
    }
    for action in ShortcutAction::ALL {
        unbind(app, action);
    }
    let bindings = app.state::<Bindings>();
    *lock(&bindings.taken) = None;
    lock(&bindings.failed).clear();

    let saved = app.state::<AppState>().settings().shortcuts;
    for action in ShortcutAction::ALL {
        let Some(label) = saved.get(action) else {
            continue;
        };
        // The default toggle shortcut falls back to an alternative if taken.
        let candidates: Vec<&str> =
            if action == ShortcutAction::Toggle && label == TOGGLE_CANDIDATES[0] {
                TOGGLE_CANDIDATES.to_vec()
            } else {
                vec![label]
            };
        let used = candidates
            .iter()
            .copied()
            .find(|c| bind(app, action, c).is_ok());
        if used == Some(label) {
            continue;
        }
        lock(&bindings.failed).insert(action, label.to_owned());
        if action == ShortcutAction::Toggle {
            *lock(&bindings.taken) = Some(label.to_owned());
        }
        let state = app.state::<AppState>();
        let strings = state.strings();
        let body = match used {
            Some(used) => {
                strings.format("shortcutTakenFallback", &[("taken", label), ("used", used)])
            }
            None => strings.format("shortcutTakenNone", &[("taken", label)]),
        };
        state::notify(app, strings.get("shortcutTakenTitle"), &body);
    }
}

/// Changes one action's shortcut (`None` clears it) after checking it is
/// valid, safe with AltGr, not used by another action and free.
pub fn set(
    app: &AppHandle,
    action: ShortcutAction,
    label: Option<String>,
) -> Result<(), ShortcutError> {
    let Some(label) = label.map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()) else {
        unbind(app, action);
        store(app, action, None);
        return Ok(());
    };
    let shortcut = parse(&label)?;
    check(&shortcut)?;
    let bindings = app.state::<Bindings>();
    let duplicate = lock(&bindings.bound)
        .iter()
        .any(|(a, (_, s))| *a != action && *s == shortcut);
    if duplicate {
        return Err(ShortcutError::Duplicate);
    }

    let previous = lock(&bindings.bound).get(&action).map(|(l, _)| l.clone());
    unbind(app, action);
    if let Err(e) = bind(app, action, &label) {
        if let Some(previous) = previous {
            let _ = bind(app, action, &previous);
        }
        return Err(e);
    }
    if action == ShortcutAction::Toggle {
        *lock(&bindings.taken) = None;
    }
    store(app, action, Some(label));
    Ok(())
}

fn store(app: &AppHandle, action: ShortcutAction, label: Option<String>) {
    let mut shortcuts = app.state::<AppState>().settings().shortcuts;
    shortcuts.set(action, label);
    state::set_shortcuts(app, shortcuts);
}

/// Needs a modifier, and must not type a character with AltGr.
fn check(shortcut: &Shortcut) -> Result<(), ShortcutError> {
    use tauri_plugin_global_shortcut::Modifiers;
    let mods = shortcut.mods;
    // Shift alone is how capitals and symbols are typed.
    if mods.is_empty() || mods == Modifiers::SHIFT {
        return Err(ShortcutError::NoModifier);
    }
    if mods.contains(Modifiers::CONTROL | Modifiers::ALT) && types_character(shortcut) {
        return Err(ShortcutError::TypesCharacter);
    }
    Ok(())
}

/// Whether Ctrl+Alt (= AltGr) plus this key types something on the keyboard
/// layout in use, asked to Windows itself.
#[cfg(windows)]
fn types_character(shortcut: &Shortcut) -> bool {
    use tauri_plugin_global_shortcut::Modifiers;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MAPVK_VK_TO_VSC, MapVirtualKeyExW, ToUnicodeEx,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let Some(vk) = virtual_key(shortcut.key) else {
        return false;
    };
    const DOWN: u8 = 0x80;
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let layout = GetKeyboardLayout(thread);
        let scan = MapVirtualKeyExW(vk, MAPVK_VK_TO_VSC, Some(layout));
        let mut keys = [0u8; 256];
        keys[0x11] = DOWN; // VK_CONTROL
        keys[0x12] = DOWN; // VK_MENU
        keys[0xA2] = DOWN; // VK_LCONTROL
        keys[0xA5] = DOWN; // VK_RMENU: AltGr
        if shortcut.mods.contains(Modifiers::SHIFT) {
            keys[0x10] = DOWN;
        }
        let mut out = [0u16; 8];
        // Flag 4: leave the keyboard state alone (no dead-key side effects).
        let n = ToUnicodeEx(vk, scan, &keys, &mut out, 4, Some(layout));
        n != 0
            && out
                .iter()
                .take(n.unsigned_abs() as usize)
                .any(|&c| c >= 0x20)
    }
}

#[cfg(not(windows))]
fn types_character(_shortcut: &Shortcut) -> bool {
    false
}

/// Windows virtual-key code of the keys that can type a character.
#[cfg(windows)]
fn virtual_key(code: tauri_plugin_global_shortcut::Code) -> Option<u32> {
    use tauri_plugin_global_shortcut::Code as C;
    const LETTERS: [C; 26] = [
        C::KeyA,
        C::KeyB,
        C::KeyC,
        C::KeyD,
        C::KeyE,
        C::KeyF,
        C::KeyG,
        C::KeyH,
        C::KeyI,
        C::KeyJ,
        C::KeyK,
        C::KeyL,
        C::KeyM,
        C::KeyN,
        C::KeyO,
        C::KeyP,
        C::KeyQ,
        C::KeyR,
        C::KeyS,
        C::KeyT,
        C::KeyU,
        C::KeyV,
        C::KeyW,
        C::KeyX,
        C::KeyY,
        C::KeyZ,
    ];
    const DIGITS: [C; 10] = [
        C::Digit0,
        C::Digit1,
        C::Digit2,
        C::Digit3,
        C::Digit4,
        C::Digit5,
        C::Digit6,
        C::Digit7,
        C::Digit8,
        C::Digit9,
    ];
    if let Some(i) = LETTERS.iter().position(|&c| c == code) {
        return Some(0x41 + i as u32);
    }
    if let Some(i) = DIGITS.iter().position(|&c| c == code) {
        return Some(0x30 + i as u32);
    }
    Some(match code {
        C::Semicolon => 0xBA,
        C::Equal => 0xBB,
        C::Comma => 0xBC,
        C::Minus => 0xBD,
        C::Period => 0xBE,
        C::Slash => 0xBF,
        C::Backquote => 0xC0,
        C::BracketLeft => 0xDB,
        C::Backslash => 0xDC,
        C::BracketRight => 0xDD,
        C::Quote => 0xDE,
        C::IntlBackslash => 0xE2,
        _ => return None,
    })
}

/// The shortcut bound to `action`, as shown to the user.
pub fn label(app: &AppHandle, action: ShortcutAction) -> Option<String> {
    let b = app.try_state::<Bindings>()?;
    lock(&b.bound).get(&action).map(|(l, _)| l.clone())
}

/// The preferred toggle shortcut when another app owns it.
pub fn preferred_taken(app: &AppHandle) -> Option<String> {
    let b = app.try_state::<Bindings>()?;
    lock(&b.taken).clone()
}

/// Per action: the shortcut in use and, if the saved one couldn't be
/// registered, which one that was (Settings → Shortcuts).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub action: ShortcutAction,
    pub active: Option<String>,
    pub unavailable: Option<String>,
}

pub fn status(app: &AppHandle) -> Vec<ShortcutStatus> {
    let Some(b) = app.try_state::<Bindings>() else {
        return Vec::new();
    };
    let bound = lock(&b.bound);
    let failed = lock(&b.failed);
    ShortcutAction::ALL
        .iter()
        .map(|&action| ShortcutStatus {
            action,
            active: bound.get(&action).map(|(l, _)| l.clone()),
            unavailable: failed.get(&action).cloned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_real_modifier() {
        assert_eq!(check(&parse("P").unwrap()), Err(ShortcutError::NoModifier));
        assert_eq!(
            check(&parse("Shift+P").unwrap()),
            Err(ShortcutError::NoModifier)
        );
        assert!(check(&parse("Ctrl+Shift+F9").unwrap()).is_ok());
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse("Ctrl+Alt+Nope").err(), Some(ShortcutError::Invalid));
    }

    #[test]
    fn function_keys_never_type_characters() {
        assert!(!types_character(&parse("Ctrl+Alt+F5").unwrap()));
    }
}
