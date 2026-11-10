//! "Hide Margin from screen sharing" (DECISIONS.md D54). It keeps Margin's windows, the
//! main window and the recording chip, out of a shared screen and out of screenshots.
//!
//! The value is minutes-core's `privacy.hide_from_screen_share`. Core only stores it. The
//! app that shows the windows must apply it, so Margin does that here with Tauri's content
//! protection (`NSWindow.sharingType = none`). macOS 27 leaves such a window out of a
//! ScreenCaptureKit capture of the display, which is what call apps share.

use crate::corecfg;
use tauri::{AppHandle, Manager};

/// The stored choice, off by default. `corecfg` copies it from Margin's prefs (D54).
pub fn hidden() -> bool {
    corecfg::load().privacy.hide_from_screen_share
}

/// Apply the stored choice to every open window. Windows made later read `hidden()` when
/// they are built.
pub fn apply(app: &AppHandle) {
    let on = hidden();
    for (label, window) in app.webview_windows() {
        if let Err(error) = window.set_content_protected(on) {
            tracing::warn!(%label, %error, "could not change screen sharing protection");
        }
    }
}
