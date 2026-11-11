//! The mini clock (SPEC §4.3b): a small floating window with the session's
//! time and its play/pause, top right by default, always on top and never
//! taking the focus. It never appears on its own: the study card's pop-out
//! button brings it out for the session in progress, and that button or the
//! clock's × puts it away. It goes when the session ends; the rest of the
//! time there is no webview at all.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::panel;
use crate::settings::{Appearance, ThemeChoice};
use crate::settings_window::theme_of;
use crate::state::{self, AppState};
use crate::study::StudyView;

pub const LABEL: &str = "mini";
/// Logical size: the pill, plus room round it for its shadow and the ×.
const WIDTH: f64 = 196.0;
const HEIGHT: f64 = 80.0;
/// Must match `--dur-close` in tokens.css: the page fades out, then the
/// window goes.
const CLOSE_MS: u64 = 120;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Life {
    #[default]
    Absent,
    /// The webview is loading; shown on `mini_ready`.
    Building,
    Shown,
    /// Fading out before the window is destroyed.
    Closing,
}

#[derive(Default)]
struct Mini {
    life: Mutex<Life>,
    /// Bumped on every change, so a stale close does nothing.
    generation: AtomicU64,
    /// The session (its start, UTC ms) the user brought the clock out for.
    shown_for: Mutex<Option<i64>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn init(app: &AppHandle) {
    app.manage(Mini::default());
}

/// Whether the clock should be on screen now.
fn wanted(app: &AppHandle) -> bool {
    let (Some(state), Some(mini)) = (app.try_state::<AppState>(), app.try_state::<Mini>()) else {
        return false;
    };
    if !state.settings().study.enabled {
        return false;
    }
    state::study_view(app)
        .and_then(|v| v.started_at)
        .is_some_and(|session| *lock(&mini.shown_for) == Some(session))
}

/// Whether the clock is out, for the panel's pop-out button.
pub fn is_out(app: &AppHandle) -> bool {
    wanted(app)
}

/// The study card's pop-out button: out for the session in progress, or put
/// away. Without a session there is nothing to bring out.
pub fn set_shown(app: &AppHandle, shown: bool) {
    if let Some(mini) = app.try_state::<Mini>() {
        let session = state::study_view(app).and_then(|v| v.started_at);
        *lock(&mini.shown_for) = session.filter(|_| shown);
    }
    // Shows or fades the window (`sync`) and tells the panel's button.
    state::refresh(app);
}

/// Shows, keeps or removes the clock to match the session and the button.
/// Called on every state change (`state::refresh`).
pub fn sync(app: &AppHandle) {
    let Some(mini) = app.try_state::<Mini>() else {
        return;
    };
    let want = wanted(app);
    let mut life = lock(&mini.life);
    match (want, *life) {
        (true, Life::Absent) => {
            *life = Life::Building;
            let app = app.clone();
            // Built off the event loop's thread, like the other windows.
            std::thread::spawn(move || {
                if let Err(e) = build(&app) {
                    log::error!("mini clock: {e}");
                    if let Some(mini) = app.try_state::<Mini>() {
                        *lock(&mini.life) = Life::Absent;
                    }
                }
            });
        }
        (true, Life::Closing) => {
            // Came back before it had gone: stay.
            mini.generation.fetch_add(1, Ordering::Relaxed);
            *life = Life::Shown;
            let _ = app.emit_to(LABEL, "mini-shown", ());
        }
        (false, Life::Shown) => {
            *life = Life::Closing;
            let generation = mini.generation.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = app.emit_to(LABEL, "mini-hiding", ());
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(CLOSE_MS));
                close(&app, generation);
            });
        }
        // Still loading: `on_ready` sees it isn't wanted any more.
        _ => {}
    }
}

fn close(app: &AppHandle, generation: u64) {
    let Some(mini) = app.try_state::<Mini>() else {
        return;
    };
    {
        let mut life = lock(&mini.life);
        if *life != Life::Closing || mini.generation.load(Ordering::Relaxed) != generation {
            return;
        }
        *life = Life::Absent;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(e) = window.destroy() {
            log::warn!("mini clock destroy: {e}");
        }
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let settings = state.settings();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("mini.html".into()))
        .title("Rumor · mini")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .theme(theme(settings.appearance.theme))
        .build()?;
    place(app, &window, settings.last.mini_position);
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Moved(at) = event {
            moved(&handle, [at.x, at.y]);
        }
    });
    Ok(())
}

/// Dragged by the user: remembered for next time.
fn moved(app: &AppHandle, at: [i32; 2]) {
    let shown = app
        .try_state::<Mini>()
        .is_some_and(|m| *lock(&m.life) == Life::Shown);
    if shown {
        state::remember_mini_position(app, at);
    }
}

/// Where it was left, if that is still on a screen; otherwise the top right
/// of the main screen's work area. The window's transparent edge is the
/// margin.
fn place(app: &AppHandle, window: &WebviewWindow, saved: Option<[i32; 2]>) {
    let Ok(size) = window.outer_size() else {
        return;
    };
    let (w, h) = (
        i32::try_from(size.width).unwrap_or(0),
        i32::try_from(size.height).unwrap_or(0),
    );
    let monitors = app.available_monitors().unwrap_or_default();
    let on_screen = |[x, y]: [i32; 2]| {
        monitors.iter().any(|m| {
            let a = m.work_area();
            let (left, top) = (a.position.x, a.position.y);
            let right = left + i32::try_from(a.size.width).unwrap_or(0);
            let bottom = top + i32::try_from(a.size.height).unwrap_or(0);
            x >= left && y >= top && x + w <= right && y + h <= bottom
        })
    };
    let corner = || {
        app.primary_monitor().ok().flatten().map(|m| {
            let a = m.work_area();
            [
                a.position.x + i32::try_from(a.size.width).unwrap_or(0) - w,
                a.position.y,
            ]
        })
    };
    if let Some([x, y]) = saved.filter(|&p| on_screen(p)).or_else(corner) {
        let at = Position::Physical(PhysicalPosition::new(x, y));
        if let Err(e) = window.set_position(at) {
            log::warn!("mini clock position: {e}");
        }
    }
}

/// The page rendered its first state (`mini_ready`).
pub fn on_ready(app: &AppHandle) {
    let (Some(mini), Some(window)) = (app.try_state::<Mini>(), app.get_webview_window(LABEL))
    else {
        return;
    };
    let want = wanted(app);
    {
        let mut life = lock(&mini.life);
        if *life != Life::Building {
            return;
        }
        *life = if want { Life::Shown } else { Life::Absent };
    }
    // Outside the lock: showing moves the window, and `moved` looks at it.
    if want {
        if let Err(e) = window.show() {
            log::warn!("mini clock show: {e}");
        }
    } else {
        // The session ended while it loaded.
        let _ = window.destroy();
    }
}

/// The ×: put away, like pressing the panel's button again.
pub fn dismiss(app: &AppHandle) {
    set_shown(app, false);
}

pub fn apply_appearance(app: &AppHandle, appearance: &Appearance) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_theme(theme(appearance.theme));
    }
}

/// Settings → Appearance, or the panel's `RUMOR_THEME` in debug builds.
fn theme(choice: ThemeChoice) -> Option<tauri::Theme> {
    panel::forced_theme().or(theme_of(choice))
}

pub fn emit_state(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_none() {
        return;
    }
    if let Some(state) = state::panel_state(app) {
        let _ = app.emit_to(LABEL, "state-changed", state);
    }
}

/// The time, once a second.
pub fn emit_study_tick(app: &AppHandle, view: &StudyView) {
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, "study-tick", view);
    }
}
