//! Tray icon, tooltip and clicks (SPEC §4.1, §4.3c). Left click toggles the
//! noise; right click opens the panel next to the icon. With "Modo estudio"
//! on, a dot on the icon and the tooltip show the stopwatch.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::audio::device_watch::PauseReason;
use crate::audio::noise::NoiseColor;
use crate::settings::LeftClick;
use crate::state::{self, AppState, Snapshot, StudyBrief};
use crate::study::machine::{Phase, Run};
use crate::{panel, taskbar};

const ID: &str = "main";

macro_rules! icons {
    ($($state:literal $theme:literal $size:literal),* $(,)?) => {
        &[$((
            $state,
            $theme,
            $size,
            include_bytes!(concat!("../icons/tray/", $state, "-", $theme, "-", $size, ".png")) as &[u8],
        )),*]
    };
}

/// (state, taskbar theme, size in px, PNG), drawn by `scripts/gen-tray-icons.mjs`.
const ICONS: &[(&str, &str, u32, &[u8])] = icons![
    "active" "light" 16, "active" "light" 20, "active" "light" 24,
    "active" "light" 32, "active" "light" 40, "active" "light" 48,
    "active" "dark" 16, "active" "dark" 20, "active" "dark" 24,
    "active" "dark" 32, "active" "dark" 40, "active" "dark" 48,
    "idle" "light" 16, "idle" "light" 20, "idle" "light" 24,
    "idle" "light" 32, "idle" "light" 40, "idle" "light" 48,
    "idle" "dark" 16, "idle" "dark" 20, "idle" "dark" 24,
    "idle" "dark" 32, "idle" "dark" 40, "idle" "dark" 48,
    "active-dot" "light" 16, "active-dot" "light" 20, "active-dot" "light" 24,
    "active-dot" "light" 32, "active-dot" "light" 40, "active-dot" "light" 48,
    "active-dot" "dark" 16, "active-dot" "dark" 20, "active-dot" "dark" 24,
    "active-dot" "dark" 32, "active-dot" "dark" 40, "active-dot" "dark" 48,
    "idle-dot" "light" 16, "idle-dot" "light" 20, "idle-dot" "light" 24,
    "idle-dot" "light" 32, "idle-dot" "light" 40, "idle-dot" "light" 48,
    "idle-dot" "dark" 16, "idle-dot" "dark" 20, "idle-dot" "dark" 24,
    "idle-dot" "dark" 32, "idle-dot" "dark" 40, "idle-dot" "dark" 48,
];

/// What the tray shows now, so a refresh only touches what changed (volume
/// drags refresh many times a second).
#[derive(Default)]
struct Shown {
    icon: Mutex<Option<IconKey>>,
    tooltip: Mutex<String>,
}

/// (playing, dot, light taskbar, size).
type IconKey = (bool, bool, bool, u32);

/// The dot: the stopwatch is running, or the idle question waits.
fn has_dot(s: &Snapshot) -> bool {
    s.study
        .is_some_and(|st| st.run == Run::Running || st.idle_pending)
}

/// The icon drawn for the current scaling, so Windows never resamples it.
fn pick_icon(app: &AppHandle, s: &Snapshot) -> (IconKey, Option<&'static [u8]>) {
    let (playing, dot) = (s.playing, has_dot(s));
    let state = match (playing, dot) {
        (true, false) => "active",
        (false, false) => "idle",
        (true, true) => "active-dot",
        (false, true) => "idle-dot",
    };
    let light = taskbar::uses_light_theme();
    let theme = if light { "light" } else { "dark" };
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |m| m.scale_factor());
    let wanted = (16.0 * scale).round() as u32;
    let mut candidates = ICONS
        .iter()
        .filter(|(s, t, _, _)| *s == state && *t == theme);
    let best = candidates
        .clone()
        .filter(|(_, _, size, _)| *size >= wanted)
        .min_by_key(|(_, _, size, _)| *size)
        .or_else(|| candidates.next_back());
    match best {
        Some(&(_, _, size, png)) => ((playing, dot, light, size), Some(png)),
        None => ((playing, dot, light, 0), None),
    }
}

fn decode(png: &[u8]) -> Option<Image<'static>> {
    match Image::from_bytes(png) {
        Ok(image) => Some(image.to_owned()),
        Err(e) => {
            log::error!("tray icon: {e}");
            None
        }
    }
}

fn color_key(color: NoiseColor) -> &'static str {
    match color {
        NoiseColor::Pink => "colorPink",
        NoiseColor::Brown => "colorBrown",
        NoiseColor::White => "colorWhite",
    }
}

/// `m:ss` or `h:mm:ss`.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn tooltip(state: &AppState, s: &Snapshot) -> String {
    let t = state.strings();
    if s.study.is_some_and(|st| st.idle_pending) {
        return format!("Rumor · {}", t.get("idleTitle"));
    }
    let base = if s.playing {
        let volume = (s.sound.volume * 100.0).round().to_string();
        let base = t.format(
            "trayPlaying",
            &[
                ("color", t.get(color_key(s.sound.color))),
                ("volume", &volume),
            ],
        );
        match s.timer_remaining {
            Some(secs) => t.format("trayTimer", &[("base", &base), ("time", &clock(secs))]),
            None => base,
        }
    } else {
        let reason = match s.paused_for {
            Some(PauseReason::HeadphonesDisconnected) => Some("pausedHeadphones"),
            Some(PauseReason::NoOutput) => Some("pausedNoOutput"),
            Some(PauseReason::OutputChanged) => Some("pausedOutputChanged"),
            Some(PauseReason::Suspended | PauseReason::Locked) | None => None,
        };
        match reason {
            Some(key) => format!("Rumor · {}", t.get(key)),
            None => t.get("trayPaused").to_owned(),
        }
    };
    match s.study.and_then(|st| study_part(&t, st)) {
        Some(part) => format!("{base} · {part}"),
        None => base,
    }
}

/// "Estudiando 1:23:45", "Descanso 4:10", "Estudio en pausa 1:23:45".
fn study_part(t: &crate::i18n::Strings, st: StudyBrief) -> Option<String> {
    let worked = clock(st.worked_secs);
    Some(match (st.run, st.phase) {
        (Run::Stopped, _) => return None,
        (Run::Paused, _) => t.format("trayStudyPaused", &[("time", &worked)]),
        (Run::Running, Phase::Work) => t.format("trayStudying", &[("time", &worked)]),
        (Run::Running, Phase::ShortBreak | Phase::LongBreak) => t.format(
            "trayStudyBreak",
            &[("time", &clock(st.phase_left_secs.unwrap_or(0)))],
        ),
    })
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot();
    let text = tooltip(&state, &snapshot);
    let (key, png) = pick_icon(app, &snapshot);

    let mut builder = TrayIconBuilder::with_id(ID)
        .tooltip(&text)
        .on_tray_icon_event(|tray, event| {
            // Build the panel as soon as the pointer arrives, so the click
            // finds it ready.
            if let TrayIconEvent::Enter { .. } | TrayIconEvent::Move { .. } = event {
                panel::prepare(tray.app_handle());
                return;
            }
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle();
                let left = app
                    .try_state::<AppState>()
                    .map(|s| s.settings().tray.left_click);
                match (button, left) {
                    (MouseButton::Left, Some(LeftClick::Panel)) | (MouseButton::Right, _) => {
                        panel::toggle(app, Some(rect))
                    }
                    (MouseButton::Left, _) => state::toggle(app),
                    _ => {}
                }
            }
        });
    if let Some(image) = png.and_then(decode) {
        builder = builder.icon(image);
    }
    builder.build(app)?;
    app.manage(Shown {
        icon: Mutex::new(Some(key)),
        tooltip: Mutex::new(text),
    });

    let handle = app.clone();
    taskbar::watch_theme(move || refresh(&handle));
    Ok(())
}

pub fn refresh(app: &AppHandle) {
    let (Some(tray), Some(state), Some(shown)) = (
        app.tray_by_id(ID),
        app.try_state::<AppState>(),
        app.try_state::<Shown>(),
    ) else {
        return;
    };
    let snapshot = state.snapshot();

    let (key, png) = pick_icon(app, &snapshot);
    if let Ok(mut current) = shown.icon.lock() {
        if *current != Some(key) {
            *current = Some(key);
            if let Err(e) = tray.set_icon(png.and_then(decode)) {
                log::warn!("tray icon: {e}");
            }
        }
    }

    let text = tooltip(&state, &snapshot);
    if let Ok(mut current) = shown.tooltip.lock() {
        if *current != text {
            if let Err(e) = tray.set_tooltip(Some(&text)) {
                log::warn!("tray tooltip: {e}");
            }
            *current = text;
        }
    }
}
