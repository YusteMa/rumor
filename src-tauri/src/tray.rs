//! Tray icon, tooltip and clicks (SPEC §4.1, §4.3c). Left click toggles the
//! noise; right click opens the panel next to the icon. On play the bars
//! rise and on pause they settle, then the icon keeps still (no motion at
//! all with Windows' animation effects off). With "Modo estudio" on, a dot on the icon and the tooltip show the
//! stopwatch.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::audio::device_watch::PauseReason;
use crate::audio::noise::NoiseColor;
use crate::settings::LeftClick;
use crate::state::{self, AppState, Snapshot, StudyBrief};
use crate::study::machine::{Phase, Run};
use crate::tray_glyph::{self, Pose};
use crate::{panel, taskbar};

const ID: &str = "main";
/// Ink colours are the --ink tokens of SPEC §4.5: dark ink on a light
/// taskbar, light ink on a dark one.
const INK_LIGHT_TASKBAR: [u8; 3] = [0x2b, 0x25, 0x23];
const INK_DARK_TASKBAR: [u8; 3] = [0xf2, 0xec, 0xe9];
/// 25 frames a second, only while the bars move.
const FRAME: Duration = Duration::from_millis(40);
/// Play and pause glide between the idle and the active bars.
const GLIDE_SECS: f32 = 0.4;

/// What the icon should show; the animator thread draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    playing: bool,
    dot: bool,
    light: bool,
    size: u32,
}

struct Animator {
    look: Mutex<Look>,
    wake: Condvar,
}

/// The tooltip shown now, so a refresh only touches it when it changes
/// (volume drags refresh many times a second).
struct Shown {
    tooltip: Mutex<String>,
    animator: Arc<Animator>,
}

/// The dot: the stopwatch is running, or the idle question waits.
fn has_dot(s: &Snapshot) -> bool {
    s.study
        .is_some_and(|st| st.run == Run::Running || st.idle_pending)
}

/// The icon for the current scaling, drawn at its exact size so Windows
/// never resamples it.
fn look(app: &AppHandle, s: &Snapshot) -> Look {
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |m| m.scale_factor());
    Look {
        playing: s.playing,
        dot: has_dot(s),
        light: taskbar::uses_light_theme(),
        size: ((16.0 * scale).round() as u32).clamp(16, 64),
    }
}

fn image(look: Look, pose: &Pose) -> Image<'static> {
    let ink = if look.light {
        INK_LIGHT_TASKBAR
    } else {
        INK_DARK_TASKBAR
    };
    Image::new_owned(
        tray_glyph::draw(look.size, ink, pose, look.dot),
        look.size,
        look.size,
    )
}

/// Draws the icon whenever the look changes, frame by frame while the bars
/// glide. Sleeps on the condvar the rest of the time: a still icon costs
/// nothing.
fn animate(app: AppHandle, animator: Arc<Animator>, first: Look) {
    let step = FRAME.as_secs_f32() / GLIDE_SECS;
    let mut drawn = first;
    // 0 idle, 1 active.
    let mut glide: f32 = if first.playing { 1.0 } else { 0.0 };
    loop {
        let look = {
            let Ok(mut current) = animator.look.lock() else {
                return;
            };
            loop {
                let target = if current.playing { 1.0 } else { 0.0 };
                if glide != target || *current != drawn {
                    break *current;
                }
                current = match animator.wake.wait(current) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
        };
        let target = if look.playing { 1.0 } else { 0.0 };
        glide = if !taskbar::animations_enabled() {
            target
        } else if target > glide {
            (glide + step).min(target)
        } else {
            (glide - step).max(target)
        };
        let pose = Pose::blend(&Pose::idle(), &Pose::active(), glide);
        if let Some(tray) = app.tray_by_id(ID) {
            if let Err(e) = tray.set_icon(Some(image(look, &pose))) {
                log::warn!("tray icon: {e}");
            }
        }
        drawn = look;
        if glide != target {
            std::thread::sleep(FRAME);
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
    let first = look(app, &snapshot);
    let pose = if first.playing {
        Pose::active()
    } else {
        Pose::idle()
    };

    TrayIconBuilder::with_id(ID)
        .icon(image(first, &pose))
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
        })
        .build(app)?;
    let animator = Arc::new(Animator {
        look: Mutex::new(first),
        wake: Condvar::new(),
    });
    app.manage(Shown {
        tooltip: Mutex::new(text),
        animator: animator.clone(),
    });
    let handle = app.clone();
    std::thread::spawn(move || animate(handle, animator, first));

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

    let wanted = look(app, &snapshot);
    if let Ok(mut current) = shown.animator.look.lock() {
        if *current != wanted {
            *current = wanted;
            shown.animator.wake.notify_one();
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
