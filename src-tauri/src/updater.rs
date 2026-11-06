//! Auto-update (DECISIONS.md D38). Margin checks GitHub's latest release a short while
//! after launch and then every six hours, downloads a newer version in the background,
//! and says so. The update installs when you choose "Restart to update", or when you
//! quit Margin. It never installs or restarts during a recording.
//!
//! The download is a `.app.tar.gz` signed with Margin's updater key (minisign); the
//! plugin refuses anything that the public key in tauri.conf.json does not verify.

use crate::{events, prefs, recorder::recorder};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Points the check at another latest.json (tests, a staging channel). The download
/// must still verify against the built-in public key.
const ENDPOINT_ENV: &str = "MARGIN_UPDATE_ENDPOINT";
/// Test only: install and restart as soon as an update is downloaded.
const AUTO_INSTALL_ENV: &str = "MARGIN_UPDATE_AUTO_INSTALL";

struct Ready {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct State {
    ready: Option<Ready>,
    checking: bool,
    last_checked: Option<SystemTime>,
    error: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { ready: None, checking: false, last_checked: None, error: None });

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    /// False in the QA bridge, which has no app to update.
    pub supported: bool,
    pub current: String,
    /// A newer version that is downloaded and ready to install.
    pub ready: Option<String>,
    pub notes: Option<String>,
    pub checking: bool,
    /// Seconds since the last finished check.
    pub checked_secs_ago: Option<u64>,
    pub error: Option<String>,
}

pub fn status() -> UpdateStatus {
    let s = STATE.lock().unwrap();
    UpdateStatus {
        supported: true,
        current: env!("CARGO_PKG_VERSION").into(),
        ready: s.ready.as_ref().map(|r| r.update.version.clone()),
        notes: s.ready.as_ref().and_then(|r| r.update.body.clone()),
        checking: s.checking,
        checked_secs_ago: s.last_checked.and_then(|t| t.elapsed().ok()).map(|d| d.as_secs()),
        error: s.error.clone(),
    }
}

/// The status the QA bridge reports: no updater there.
pub fn unsupported() -> UpdateStatus {
    UpdateStatus {
        supported: false,
        current: env!("CARGO_PKG_VERSION").into(),
        ready: None,
        notes: None,
        checking: false,
        checked_secs_ago: None,
        error: None,
    }
}

pub fn ready_version() -> Option<String> {
    STATE.lock().unwrap().ready.as_ref().map(|r| r.update.version.clone())
}

/// The background loop: a first check soon after launch, then one every six hours.
pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("margin-updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                if !prefs::load().no_auto_update {
                    check(&app);
                }
                std::thread::sleep(EVERY);
            }
        })
        .expect("spawn updater");
}

/// Look for a newer version and download it. Safe to call from any thread; a second
/// call while one runs returns at once.
pub fn check(app: &AppHandle) -> UpdateStatus {
    {
        let mut s = STATE.lock().unwrap();
        if s.checking || s.ready.is_some() {
            drop(s);
            return status();
        }
        s.checking = true;
        s.error = None;
    }
    events::emit("update-status", status());
    let result = tauri::async_runtime::block_on(find_and_download(app));
    {
        let mut s = STATE.lock().unwrap();
        s.checking = false;
        s.last_checked = Some(SystemTime::now());
        match result {
            Ok(Some(ready)) => {
                tracing::info!(version = %ready.update.version, "update downloaded");
                s.ready = Some(ready);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "update check failed");
                s.error = Some(error);
            }
        }
    }
    let now = status();
    events::emit("update-status", now.clone());
    if now.ready.is_some() {
        events::refresh_tray();
        if std::env::var_os(AUTO_INSTALL_ENV).is_some() {
            let _ = install_and_restart(app);
        }
    }
    now
}

async fn find_and_download(app: &AppHandle) -> Result<Option<Ready>, String> {
    let mut builder = app.updater_builder();
    if let Ok(url) = std::env::var(ENDPOINT_ENV) {
        let url = url.parse().map_err(|e| format!("{ENDPOINT_ENV}: {e}"))?;
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    let updater = builder.build().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(friendly)? else {
        return Ok(None);
    };
    let bytes = update.download(|_, _| {}, || {}).await.map_err(friendly)?;
    Ok(Some(Ready { update, bytes }))
}

fn friendly(error: tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match error {
        E::Reqwest(_) | E::Network(_) => "Couldn't reach GitHub to check for updates.".into(),
        E::Minisign(_) | E::SignatureUtf8(_) => "The update's signature didn't match, so Margin didn't install it.".into(),
        other => other.to_string(),
    }
}

/// "Restart to update": install the downloaded version and start it.
pub fn install_and_restart(app: &AppHandle) -> Result<(), String> {
    if recorder().is_recording() {
        return Err("Stop the recording first. Margin updates once it's saved.".into());
    }
    install()?;
    app.restart();
}

/// Quitting with an update downloaded installs it, so the next launch is the new one.
pub fn install_on_quit() {
    if recorder().is_recording() {
        return;
    }
    if let Err(error) = install() {
        tracing::warn!(%error, "update not installed on quit");
    }
}

fn install() -> Result<(), String> {
    let Some(ready) = STATE.lock().unwrap().ready.take() else {
        return Err("No update is ready.".into());
    };
    tracing::info!(version = %ready.update.version, "installing update");
    ready.update.install(&ready.bytes).map_err(|e| format!("The update couldn't be installed: {e}"))
}
