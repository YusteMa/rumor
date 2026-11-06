//! The panel window (SPEC §4.2): frameless, anchored next to the tray on
//! whichever side the taskbar is, Mica on Windows 11 (solid colour on 10),
//! hidden when it loses focus. The UI animates in on `panel-shown` and out
//! on `panel-hiding`.
//!
//! Memory (SPEC §3.4): WebView2 costs well over 100 MB, so the panel only
//! exists while it is useful. It is built when the pointer enters the tray
//! icon, so it is usually ready by the time the click lands; while hidden it
//! asks WebView2 for low memory; after five minutes closed it is destroyed.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, Rect, Theme, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::settings::Appearance;
use crate::settings_window;
use crate::state::{self, AppState};

pub const LABEL: &str = "panel";
/// Logical size; the content is laid out for exactly this.
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 540.0;
/// Room for the study card, with "Modo estudio" on (SPEC §4.3b, §4.3c):
/// the card and the gap above it.
const STUDY_HEIGHT: f64 = 176.0;
/// Gap between the panel, the taskbar and the screen edge (logical px).
const MARGIN: f64 = 12.0;
/// Must match `--dur-close` in tokens.css: the UI fades out, then we hide.
const CLOSE_MS: u64 = 120;
/// A click on the tray icon blurs the panel just before the click arrives;
/// within this window that click must not reopen it.
const REOPEN_GUARD: Duration = Duration::from_millis(300);
/// After a tray click Explorer takes the foreground back once it has handled
/// the click: within ~100 ms for a pinned icon, later when the icon sits in
/// the hidden-icons flyout, which closes with an animation first. A loss to
/// another window this soon after opening is that, not the user clicking
/// away, so the panel takes the foreground back (at most `MAX_REFOCUS` times).
const FOCUS_GUARD: Duration = Duration::from_millis(500);
/// Times the panel may take the foreground back within `FOCUS_GUARD`.
const MAX_REFOCUS: u8 = 2;
/// Wait before judging a focus loss, so the foreground window is settled.
const BLUR_SETTLE: Duration = Duration::from_millis(40);
/// How long a closed panel is kept before its webview is destroyed.
const IDLE_TTL: Duration = Duration::from_secs(5 * 60);

enum Life {
    Absent,
    Loading { started: Instant },
    Ready,
}

struct Panel {
    mica: AtomicBool,
    open: AtomicBool,
    /// Bumped on every open/hide so stale delayed work does nothing.
    generation: AtomicU64,
    last_hidden: Mutex<Option<Instant>>,
    life: Mutex<Life>,
    /// An open requested while the webview was still loading.
    pending: Mutex<Option<(Option<Rect>, Instant)>>,
    /// When the current open was requested, to log how long it took to show.
    requested: Mutex<Option<Instant>>,
    /// When the panel was last shown, for `FOCUS_GUARD`.
    shown_at: Mutex<Option<Instant>>,
    /// Foreground take-backs since it was shown, up to `MAX_REFOCUS`.
    refocused: AtomicU8,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The panel's theme: Settings → Appearance, or in debug builds
/// `RUMOR_THEME=light|dark` (for screenshots).
fn panel_theme(app: &AppHandle) -> Option<Theme> {
    forced_theme().or_else(|| {
        app.try_state::<AppState>()
            .and_then(|s| settings_window::theme_of(s.settings().appearance.theme))
    })
}

pub fn forced_theme() -> Option<Theme> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("RUMOR_THEME").ok()?.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

/// Debug builds only: `RUMOR_DEV_PANEL_TTL=<seconds>` shortens the idle
/// time before the panel is destroyed, to test it.
fn idle_ttl() -> Duration {
    if cfg!(debug_assertions) {
        if let Some(secs) = std::env::var("RUMOR_DEV_PANEL_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            return Duration::from_secs(secs);
        }
    }
    IDLE_TTL
}

/// Registers the panel's state; the window itself is built on demand.
pub fn init(app: &AppHandle) {
    app.manage(Panel {
        mica: AtomicBool::new(false),
        open: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        last_hidden: Mutex::new(None),
        life: Mutex::new(Life::Absent),
        pending: Mutex::new(None),
        requested: Mutex::new(None),
        shown_at: Mutex::new(None),
        refocused: AtomicU8::new(0),
    });
}

/// Starts building the panel if it does not exist. Called when the pointer
/// enters the tray icon, so the click usually finds it ready.
pub fn prepare(app: &AppHandle) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    {
        let mut life = lock(&panel.life);
        if !matches!(*life, Life::Absent) {
            return;
        }
        *life = Life::Loading {
            started: Instant::now(),
        };
    }
    let app = app.clone();
    // Building a webview on the event loop's own thread can deadlock on
    // Windows; from another thread Tauri hands the work to the loop.
    std::thread::spawn(move || {
        if let Err(e) = build(&app) {
            log::error!("panel: {e}");
            if let Some(panel) = app.try_state::<Panel>() {
                *lock(&panel.life) = Life::Absent;
            }
        }
    });
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let theme = panel_theme(app);
    let wants_mica = app
        .try_state::<AppState>()
        .is_none_or(|s| s.settings().appearance.mica);
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Rumor")
        .inner_size(WIDTH, height(app))
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
    let mica =
        wants_mica && window_vibrancy::apply_mica(&window, theme.map(|t| t == Theme::Dark)).is_ok();
    if let Some(panel) = app.try_state::<Panel>() {
        panel.mica.store(mica, Ordering::Relaxed);
    }
    set_border(&window, window.theme().unwrap_or(Theme::Light));

    let handle = app.clone();
    let themed = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => log::debug!("panel focused"),
        WindowEvent::Focused(false) => on_blur(&handle),
        WindowEvent::ThemeChanged(theme) => set_border(&themed, *theme),
        _ => {}
    });
    Ok(())
}

/// The panel's window lost the focus. That also happens when the focus just
/// moves into the webview inside it, so look at which window is actually in
/// front once Windows has settled, and only close for another app's window.
fn on_blur(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(BLUR_SETTLE);
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || decide_blur(&handle));
    });
}

fn decide_blur(app: &AppHandle) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    if !panel.open.load(Ordering::Relaxed) {
        return;
    }
    let since = lock(&panel.shown_at).map_or(Duration::MAX, |t| t.elapsed());
    if foreground_is_ours() {
        log::debug!(
            "panel focus moved inside Rumor ({} ms after opening)",
            since.as_millis()
        );
        return;
    }
    // Right after a tray click Explorer takes the foreground back once it has
    // finished with the click: take it back, a bounded number of times.
    if since < FOCUS_GUARD && panel.refocused.fetch_add(1, Ordering::Relaxed) < MAX_REFOCUS {
        log::debug!(
            "another window took the foreground {} ms after opening; taking it back",
            since.as_millis()
        );
        if let Some(window) = app.get_webview_window(LABEL) {
            bring_to_front(&window);
        }
        return;
    }
    log::debug!(
        "panel lost the foreground after {} ms; closing",
        since.as_millis()
    );
    hide(app);
}

/// Whether the foreground window belongs to this process (the panel or the
/// webview inside it).
fn foreground_is_ours() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        }
        pid == std::process::id()
    }
    #[cfg(not(windows))]
    false
}

/// Activates the panel even though Explorer owns the foreground after a tray
/// click. Windows only lets the foreground thread hand the foreground over,
/// so for a moment our thread shares Explorer's input state
/// (`AttachThreadInput`), which makes `SetForegroundWindow` succeed. Must run
/// on the main thread, which owns the window.
fn bring_to_front(window: &WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
        };

        let Ok(hwnd) = window.hwnd() else { return };
        let hwnd = HWND(hwnd.0 as _);
        unsafe {
            let foreground = GetForegroundWindow();
            if foreground == hwnd {
                return;
            }
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(foreground, None);
            let attached =
                theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
            let ok = SetForegroundWindow(hwnd).as_bool();
            let _ = BringWindowToTop(hwnd);
            if attached {
                let _ = AttachThreadInput(ours, theirs, false);
            }
            log::debug!("panel to foreground: {ok} (attached: {attached})");
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// The page has loaded and rendered its first state (`panel_ready`).
pub fn on_ready(app: &AppHandle) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    let started = {
        let mut life = lock(&panel.life);
        let started = match *life {
            Life::Loading { started } => Some(started),
            _ => None,
        };
        *life = Life::Ready;
        started
    };
    if let Some(started) = started {
        log::info!(
            "panel built in {} ms (mica: {})",
            started.elapsed().as_millis(),
            panel.mica.load(Ordering::Relaxed)
        );
    }
    let pending = lock(&panel.pending).take();
    match pending {
        Some((anchor, clicked)) => {
            log::info!(
                "panel ready {} ms after the click",
                clicked.elapsed().as_millis()
            );
            show(app, anchor, clicked);
        }
        None => {
            // Built ahead of a click that may never come.
            set_low_memory(app, true);
            schedule_destroy(app, panel.generation.load(Ordering::Relaxed));
        }
    }
}

/// The UI painted its first frame after `panel-shown` (`panel_painted`).
pub fn on_painted(app: &AppHandle) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    if let Some(clicked) = lock(&panel.requested).take() {
        log::info!(
            "panel on screen {} ms after the click",
            clicked.elapsed().as_millis()
        );
    }
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

/// Hints WebView2 to trim its memory while the panel is hidden. Only a
/// hint: the webview stays visible to WebView2 and is never suspended, so it
/// always paints the moment the window shows. (Suspending it saved ~4 % and
/// could leave the panel blank; destroying it after `IDLE_TTL` is what frees
/// the memory.)
fn set_low_memory(app: &AppHandle, low: bool) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    #[cfg(windows)]
    {
        let result = window.with_webview(move |webview| {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, ICoreWebView2_19,
            };
            use windows_core::Interface;

            let level = if low {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            };
            unsafe {
                if let Ok(core) = webview
                    .controller()
                    .CoreWebView2()
                    .and_then(|c| c.cast::<ICoreWebView2_19>())
                {
                    let _ = core.SetMemoryUsageTargetLevel(level);
                }
            }
        });
        if let Err(e) = result {
            log::warn!("panel memory level: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = (window, low);
}

/// Destroys the panel's webview if it stays closed for `idle_ttl()`.
fn schedule_destroy(app: &AppHandle, generation: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        let ttl = idle_ttl();
        std::thread::sleep(ttl);
        let Some(panel) = app.try_state::<Panel>() else {
            return;
        };
        let untouched = panel.generation.load(Ordering::Relaxed) == generation
            && !panel.open.load(Ordering::Relaxed)
            && matches!(*lock(&panel.life), Life::Ready);
        if !untouched {
            return;
        }
        if let Some(window) = app.get_webview_window(LABEL) {
            if let Err(e) = window.destroy() {
                log::warn!("panel destroy: {e}");
                return;
            }
        }
        *lock(&panel.life) = Life::Absent;
        log::info!("panel released after {} s closed", ttl.as_secs());
    });
}

/// Settings → Appearance changed: theme and Mica follow at once; the page
/// gets the rest (accent, spectrum, motion) through `state-changed`.
pub fn apply_appearance(app: &AppHandle, appearance: &Appearance) {
    let (Some(window), Some(panel)) = (app.get_webview_window(LABEL), app.try_state::<Panel>())
    else {
        return;
    };
    let theme = forced_theme().or(settings_window::theme_of(appearance.theme));
    let _ = window.set_theme(theme);
    let mica = if appearance.mica {
        window_vibrancy::apply_mica(&window, theme.map(|t| t == Theme::Dark)).is_ok()
    } else {
        let _ = window_vibrancy::clear_mica(&window);
        false
    };
    panel.mica.store(mica, Ordering::Relaxed);
    set_border(&window, window.theme().unwrap_or(Theme::Light));
}

pub fn uses_mica(app: &AppHandle) -> bool {
    app.try_state::<Panel>()
        .is_some_and(|p| p.mica.load(Ordering::Relaxed))
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
    let just_hidden = lock(&panel.last_hidden).is_some_and(|t| t.elapsed() < REOPEN_GUARD);
    if !just_hidden {
        open(app, anchor);
    }
}

/// Shows the panel, building it first if needed.
pub fn open(app: &AppHandle, anchor: Option<Rect>) {
    let Some(panel) = app.try_state::<Panel>() else {
        return;
    };
    let clicked = Instant::now();
    let ready = matches!(*lock(&panel.life), Life::Ready);
    if ready {
        show(app, anchor, clicked);
    } else {
        // Shown by `on_ready` once the page has loaded.
        *lock(&panel.pending) = Some((anchor, clicked));
        prepare(app);
    }
}

fn show(app: &AppHandle, anchor: Option<Rect>, clicked: Instant) {
    let (Some(window), Some(panel)) = (app.get_webview_window(LABEL), app.try_state::<Panel>())
    else {
        return;
    };
    panel.generation.fetch_add(1, Ordering::Relaxed);
    panel.open.store(true, Ordering::Relaxed);
    *lock(&panel.requested) = Some(clicked);
    set_low_memory(app, false);
    place(app, &window, anchor);
    emit_state(app);
    *lock(&panel.shown_at) = Some(Instant::now());
    panel.refocused.store(0, Ordering::Relaxed);
    if let Err(e) = window.show() {
        log::warn!("panel show: {e}");
    }
    let front = window.clone();
    let _ = app.run_on_main_thread(move || bring_to_front(&front));
    // Keyboard focus into the page, so Space and Esc work at once.
    let _ = window.set_focus();
    let _ = window.emit_to(LABEL, "panel-shown", ());
    if let Some(state) = app.try_state::<AppState>() {
        state.engine.set_spectrum(true);
    }
}

pub fn hide(app: &AppHandle) {
    let (Some(window), Some(panel)) = (app.get_webview_window(LABEL), app.try_state::<Panel>())
    else {
        return;
    };
    if !panel.open.swap(false, Ordering::Relaxed) {
        return;
    }
    *lock(&panel.last_hidden) = Some(Instant::now());
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
            set_low_memory(&app, true);
            schedule_destroy(&app, generation);
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
    let m = MARGIN * scale;
    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let work = monitor.work_area();
    // Never taller than the screen leaves room for (a 1366×768 laptop with
    // the study card): the page scrolls instead.
    let w = WIDTH * scale;
    let h = (height(app) * scale).min(f64::from(work.size.height) - 2.0 * m);
    let size = tauri::PhysicalSize::new(w.round() as u32, h.round() as u32);
    if window.inner_size().ok() != Some(size) {
        if let Err(e) = window.set_size(size) {
            log::warn!("panel size: {e}");
        }
    }
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

/// Logical height: taller with the study card.
fn height(app: &AppHandle) -> f64 {
    let study = app
        .try_state::<AppState>()
        .is_some_and(|s| s.settings().study.enabled);
    if study { HEIGHT + STUDY_HEIGHT } else { HEIGHT }
}

/// "Modo estudio" was switched: the card comes or goes.
pub fn resize(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(e) = window.set_size(tauri::LogicalSize::new(WIDTH, height(app))) {
            log::warn!("panel size: {e}");
        }
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

/// The study card's numbers, once a second while visible.
pub fn emit_study_tick(app: &AppHandle, view: &crate::study::StudyView) {
    if is_open(app) {
        let _ = app.emit_to(LABEL, "study-tick", view);
    }
}

#[derive(Clone, Serialize)]
struct StudyIdle {
    minutes: u64,
}

/// The idle question is waiting (the panel shows it next time it is open;
/// it is never opened for it).
pub fn emit_study_idle(app: &AppHandle, minutes: u64) {
    if is_open(app) {
        let _ = app.emit_to(LABEL, "study-idle", StudyIdle { minutes });
    }
}
