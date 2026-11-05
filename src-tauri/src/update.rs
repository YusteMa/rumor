//! Self-update (ROADMAP v0.2): looks for a newer signed build on GitHub a
//! little after launch and every few hours, announces it once per version and
//! installs it on one click from the panel or Settings → About.
//!
//! The manifest (`latest.json`) lives on the repo's `updater` branch, written
//! by the release workflow; the installer it points to is checked against the
//! public key in `tauri.conf.json` before anything runs. Settings → About can
//! turn the checks off; it's the only network request Rumor makes.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::settings_window;
use crate::state::{self, AppState};

/// The first look waits a little, so launching stays quiet.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Available,
    /// Downloading, then fading out and handing over to the installer.
    Installing,
    /// The last try failed; the button tries again.
    Failed,
}

/// What the panel and Settings show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub version: String,
    pub phase: Phase,
}

/// The update found by the last check.
#[derive(Default)]
pub struct Updates(Mutex<Option<(Update, Phase)>>);

impl Updates {
    fn lock(&self) -> MutexGuard<'_, Option<(Update, Phase)>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn view(app: &AppHandle) -> Option<UpdateView> {
    let updates = app.try_state::<Updates>()?;
    let found = updates.lock();
    found.as_ref().map(|(update, phase)| UpdateView {
        version: update.version.clone(),
        phase: *phase,
    })
}

/// Pushes a change of phase to the panel and Settings.
fn show(app: &AppHandle) {
    state::refresh(app);
    settings_window::emit_changed(app);
}

/// Starts the periodic checks. Debug builds only check from Settings, so
/// `tauri dev` never offers to replace itself with a release.
pub fn start(app: &AppHandle) {
    app.manage(Updates::default());
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                if app.state::<AppState>().settings().startup.check_updates {
                    // Failures are logged in `check`; the next round tries again.
                    let _ = tauri::async_runtime::block_on(check(&app));
                }
                std::thread::sleep(EVERY);
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start the update checks: {e}");
    }
}

/// Asks GitHub for a newer version. `Ok(None)` when this one is the latest.
pub async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    let failed = |e: tauri_plugin_updater::Error| {
        log::warn!("update check failed: {e}");
        e.to_string()
    };
    let cleanup = app.clone();
    let updater = app
        .updater_builder()
        // The installer ends the process right away: take the tray icon
        // with it, so no ghost stays in the notification area.
        .on_before_exit(move || cleanup.cleanup_before_exit())
        .build()
        .map_err(failed)?;
    let update = updater.check().await.map_err(failed)?;
    let Some(update) = update else {
        log::info!("update check: {} is the latest", app.package_info().version);
        return Ok(None);
    };
    let version = update.version.clone();
    let Some(updates) = app.try_state::<Updates>() else {
        return Ok(Some(version));
    };
    let new = {
        let mut found = updates.lock();
        match found.as_ref() {
            Some((_, Phase::Installing)) => false,
            Some((known, _)) if known.version == version => false,
            _ => {
                *found = Some((update, Phase::Available));
                true
            }
        }
    };
    if new {
        log::info!("update available: {version}");
        state::update_found(app, &version);
    }
    Ok(Some(version))
}

/// Downloads the update while the noise keeps playing, then fades out and
/// hands over to the installer, which reopens Rumor. Returns only on failure.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = {
        let updates = app.state::<Updates>();
        let mut found = updates.lock();
        match found.as_mut() {
            Some((_, Phase::Installing)) | None => return Err("no update to install".into()),
            Some((update, phase)) => {
                *phase = Phase::Installing;
                update.clone()
            }
        }
    };
    show(app);
    log::info!("downloading update {}", update.version);
    let result = async {
        let bytes = update.download(|_, _| {}, || {}).await?;
        let was_playing = state::prepare_for_update(app);
        log::info!("installing update {}", update.version);
        // On success the installer takes over and this process ends here.
        update
            .install(bytes)
            .inspect_err(|_| state::update_failed(app, was_playing))
    }
    .await;
    let Err(e) = result else {
        return Ok(());
    };
    log::error!("update failed: {e}");
    if let Some((_, phase)) = app.state::<Updates>().lock().as_mut() {
        *phase = Phase::Failed;
    }
    show(app);
    Err(e.to_string())
}
