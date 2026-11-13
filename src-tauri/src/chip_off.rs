//! The recording chip on a platform that has none (DECISIONS.md D61).
//!
//! chip.rs places and shows the chip with AppKit. Windows has no chip yet. This module has
//! the same functions and commands, and each one does nothing, so the rest of Margin and
//! the chip page need no platform checks. `prefs::Prefs::recording_chip` is false on
//! Windows, so Start never hides the window in favour of a chip that cannot show.
//!
//! What a Windows chip needs: a window that is always on top, has no taskbar button and
//! takes no focus when it shows (`WS_EX_NOACTIVATE`), a place computed from the work area
//! of the monitor in physical pixels, and a test by a person that a click does not take
//! the keyboard from the call app.

use tauri::AppHandle;

/// Nothing to show or hide.
pub fn sync(_app: &AppHandle) {}

/// The window stays. With no chip, hiding it would leave nothing on screen.
pub fn shrink(_app: &AppHandle) {}

#[tauri::command]
pub fn chip_hello() -> bool {
    false
}

#[tauri::command]
pub fn chip_width(_width: f64) {}

#[tauri::command]
pub fn chip_drag_start() {}

#[tauri::command]
pub fn chip_drag(_dy: f64) {}

#[tauri::command]
pub fn chip_drag_end() {}

#[tauri::command]
pub fn chip_nudge(_dy: f64) {}

#[tauri::command]
pub fn chip_release_focus() {}
