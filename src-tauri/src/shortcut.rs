//! The global shortcut: ⌃⌥⌘R (Ctrl+Alt+Shift+R on Windows) starts a recording from any
//! app, and stops it again. Off
//! until you turn it on in Settings, because a global shortcut takes those keys from every
//! other app.

use crate::recorder::{recorder, Kind};
use crate::{events, prefs, tray};
use tauri::{AppHandle, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Rarely used by other apps; shown in Settings as ⌃⌥⌘R.
#[cfg(not(windows))]
pub const RECORD_SHORTCUT: &str = "Control+Alt+Super+R";
/// Windows keeps the shortcuts with the Windows key for itself. Shown in Settings as
/// Ctrl+Alt+Shift+R.
#[cfg(windows)]
pub const RECORD_SHORTCUT: &str = "Control+Alt+Shift+R";
#[cfg(not(windows))]
const SHORTCUT_TAKEN: &str = "Another app already uses ⌃⌥⌘R, so Margin couldn't take it.";
#[cfg(windows)]
const SHORTCUT_TAKEN: &str = "Another app already uses Ctrl+Alt+Shift+R, so Margin Notes couldn't take it.";

pub fn plugin() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle(app);
            }
        })
        .build()
}

/// Register or drop the shortcut to match prefs.json.
pub fn refresh(app: &AppHandle) {
    let prefs = prefs::load();
    let want = prefs.onboarding_complete && prefs.record_shortcut;
    let shortcuts = app.global_shortcut();
    let have = shortcuts.is_registered(RECORD_SHORTCUT);
    let result = match (want, have) {
        (true, false) => shortcuts.register(RECORD_SHORTCUT),
        (false, true) => shortcuts.unregister(RECORD_SHORTCUT),
        _ => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!(%error, "global shortcut");
        events::emit("error-toast", SHORTCUT_TAKEN);
    }
}

fn toggle(app: &AppHandle) {
    if !recorder().is_recording() {
        // The notepad, or the recording chip when it is on, is where a meeting starts.
        let kind = match prefs::load().default_kind.as_deref() {
            Some("in-person") => Kind::InPerson,
            // Windows cannot record a call yet (D61).
            _ if cfg!(windows) => Kind::InPerson,
            _ => Kind::Call,
        };
        tray::start_outside(app, kind);
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = recorder().stop(None, None) {
            if error != crate::recorder::ALREADY_STOPPING {
                events::emit("error-toast", error);
            }
        }
    });
}
