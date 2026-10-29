//! The panel window (SPEC §4.2): frameless, anchored next to the tray on
//! whichever side the taskbar is, Mica on Windows 11 (solid colour on 10),
//! hidden when it loses focus. It is created hidden at startup so it opens
//! instantly; the UI animates in on `panel-shown` and out on `panel-hiding`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, Rect, Theme, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::state::{self, AppState};

pub const LABEL: &str = "panel";
/// Logical size; the content is laid out for exactly this.
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 540.0;
/// Gap between the panel, the taskbar and the screen edge (logical px).
const MARGIN: f64 = 12.0;
/// Must match `--dur-close` in tokens.css: the UI fades out, then we hide.
const CLOSE_MS: u64 = 120;
/// A click on the tray icon blurs the panel just before the click arrives;
/// within this window that click must not reopen it.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

struct Panel {
    mica: bool,
    open: AtomicBool,
    /// Bumped on every open/hide so a stale delayed hide does nothing.
    generation: AtomicU64,
    last_hidden: Mutex<Option<Instant>>,
}

/// Debug builds only: `RUMOR_THEME=light|dark` forces the theme (the
/// Appearance setting replaces this in phase 4).
fn forced_theme() -> Option<Theme> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("RUMOR_THEME").ok()?.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let theme = forced_theme();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Rumor")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .theme(theme)
        .build()?;

    // Fails on Windows 10, where the CSS falls back to a solid colour.
    let mica = window_vibrancy::apply_mica(&window, theme.map(|t| t == Theme::Dark)).is_ok();
    log::info!("panel ready (mica: {mica})");

    app.manage(Panel {
        mica,
        open: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        last_hidden: Mutex::new(None),
    });

    set_border(&window, window.theme().unwrap_or(Theme::Light));

    let handle = app.clone();
    let themed = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => hide(&handle),
        WindowEvent::ThemeChanged(theme) => set_border(&themed, *theme),
        _ => {}
    });
    Ok(())
}

/// Windows 11 draws a 1 px border round the panel in its own colour, dark
/// whenever the system is dark. Match it to the palette (no effect on 10).
fn set_border(window: &WebviewWindow, theme: Theme) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DwmSetWindowAttribute};

        // A step darker (light) or lighter (dark) than --line, so the edge
        // still reads against the desktop. COLORREF is 0x00BBGGRR.
        let color: u32 = match theme {
            Theme::Dark => 0x002f_3a43,
            _ => 0x00cf_d6dc,
        };
        let Ok(hwnd) = window.hwnd() else { return };
        let hwnd = HWND(hwnd.0 as _);
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &color as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            )
        };
    }
    #[cfg(not(windows))]
    let _ = (window, theme);
}

pub fn uses_mica(app: &AppHandle) -> bool {
    app.try_state::<Panel>().is_some_and(|p| p.mica)
}

fn is_open(app: &AppHandle) -> bool {
    app.try_state::<Panel>()
        .is_some_and(|p| p.open.load(Ordering::Relaxed))
}

/// Right click on the tray icon.
pub fn toggle(app: &AppHandle, anchor: Option<Rect>) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    if panel.open.load(Ordering::Relaxed) {
        hide(app);
        return;
    }
    let just_hidden = panel
        .last_hidden
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < REOPEN_GUARD);
    if !just_hidden {
        open(app, anchor);
    }
}

pub fn open(app: &AppHandle, anchor: Option<Rect>) {
    let (Some(window), Some(panel)) = (app.get_webview_window(LABEL), app.try_state::<Panel>())
    else {
        return;
    };
    let started = Instant::now();
    panel.generation.fetch_add(1, Ordering::Relaxed);
    panel.open.store(true, Ordering::Relaxed);
    place(app, &window, anchor);
    emit_state(app);
    if let Err(e) = window.show() {
        log::warn!("panel show: {e}");
    }
    let _ = window.set_focus();
    let _ = window.emit_to(LABEL, "panel-shown", ());
    if let Some(state) = app.try_state::<AppState>() {
        state.engine.set_spectrum(true);
    }
    log::debug!("panel opened in {:?}", started.elapsed());
}

pub fn hide(app: &AppHandle) {
    let (Some(window), Some(panel)) = (app.get_webview_window(LABEL), app.try_state::<Panel>())
    else {
        return;
    };
    if !panel.open.swap(false, Ordering::Relaxed) {
        return;
    }
    if let Ok(mut t) = panel.last_hidden.lock() {
        *t = Some(Instant::now());
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.engine.set_spectrum(false);
    }
    let generation = panel.generation.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = window.emit_to(LABEL, "panel-hiding", ());
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(CLOSE_MS));
        let current = app
            .try_state::<Panel>()
            .map(|p| p.generation.load(Ordering::Relaxed));
        if current == Some(generation) {
            let _ = window.hide();
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Puts the panel next to the tray icon, on the side where the taskbar is
/// (found from the gap between the monitor and its work area).
fn place(app: &AppHandle, window: &WebviewWindow, anchor: Option<Rect>) {
    let anchor = anchor.or_else(|| app.tray_by_id("main").and_then(|t| t.rect().ok().flatten()));
    let point = anchor.map(|r| {
        let scale = 1.0;
        let pos = r.position.to_physical::<f64>(scale);
        let size = r.size.to_physical::<f64>(scale);
        (pos.x + size.width / 2.0, pos.y + size.height / 2.0)
    });
    let monitor = point
        .and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };

    let scale = monitor.scale_factor();
    let (w, h) = (WIDTH * scale, HEIGHT * scale);
    let m = MARGIN * scale;
    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let work = monitor.work_area();
    let (left, top) = (f64::from(work.position.x), f64::from(work.position.y));
    let (right, bottom) = (
        left + f64::from(work.size.width),
        top + f64::from(work.size.height),
    );
    let (mon_right, mon_bottom) = (
        f64::from(mon_pos.x) + f64::from(mon_size.width),
        f64::from(mon_pos.y) + f64::from(mon_size.height),
    );

    let edge = if bottom < mon_bottom {
        Edge::Bottom
    } else if top > f64::from(mon_pos.y) {
        Edge::Top
    } else if left > f64::from(mon_pos.x) {
        Edge::Left
    } else if right < mon_right {
        Edge::Right
    } else {
        // Auto-hidden taskbar: the side nearest the icon, bottom by default.
        match point {
            Some((x, y)) => {
                let distances = [
                    (mon_bottom - y, Edge::Bottom),
                    (y - f64::from(mon_pos.y), Edge::Top),
                    (x - f64::from(mon_pos.x), Edge::Left),
                    (mon_right - x, Edge::Right),
                ];
                distances
                    .into_iter()
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map_or(Edge::Bottom, |d| d.1)
            }
            None => Edge::Bottom,
        }
    };

    let (cx, cy) = point.unwrap_or((right, bottom));
    let clamp_x = |x: f64| x.clamp(left + m, (right - w - m).max(left + m));
    let clamp_y = |y: f64| y.clamp(top + m, (bottom - h - m).max(top + m));
    let (x, y) = match edge {
        Edge::Bottom => (clamp_x(cx - w / 2.0), bottom - h - m),
        Edge::Top => (clamp_x(cx - w / 2.0), top + m),
        Edge::Left => (left + m, clamp_y(cy - h / 2.0)),
        Edge::Right => (right - w - m, clamp_y(cy - h / 2.0)),
    };
    let position = Position::Physical(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    if let Err(e) = window.set_position(position) {
        log::warn!("panel position: {e}");
    }
}

pub fn emit_state(app: &AppHandle) {
    if !is_open(app) {
        return;
    }
    if let Some(state) = state::panel_state(app) {
        let _ = app.emit_to(LABEL, "state-changed", state);
    }
}

pub fn emit_spectrum(app: &AppHandle, bands: &[f32]) {
    if is_open(app) {
        let _ = app.emit_to(LABEL, "spectrum", bands.to_vec());
    }
}

#[derive(Clone, Serialize)]
struct TimerTick {
    remaining: Option<u64>,
}

pub fn emit_timer_tick(app: &AppHandle, remaining: Option<u64>) {
    if is_open(app) {
        let _ = app.emit_to(LABEL, "timer-tick", TimerTick { remaining });
    }
}
