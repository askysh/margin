//! The notepad view without AppKit (DECISIONS.md D57, D61): the main window at a narrow
//! width, beside a call or a browser. notepad.rs does this on macOS with NSWindow frames.
//! This module does the same with Tauri's own window calls, for Windows.
//!
//! Differences from macOS:
//!
//! - No app menu. On Windows a menu is a bar inside the window, and Margin has none. The
//!   window's own button, Ctrl+Shift+N in the window and "Open as Notepad" in the
//!   notification area menu switch the view.
//! - No animation. Windows resizes a window in one step.
//! - Sizes are in physical pixels with the top left corner as the origin, as Tauri gives
//!   them. The narrow limit and the first sizes are multiplied by the scale of the monitor.

use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

/// Under this width the page is the notepad. It must match `COMPACT_QUERY` in
/// src/components/Compact.tsx.
pub const NARROW_BELOW: f64 = 760.0;
/// The notepad's first width. The window can go down to 440 (tauri.conf.json).
const NOTEPAD_WIDTH: f64 = 480.0;
/// The wide size when Margin has no earlier one: the size the window opens at.
const WIDE_SIZE: (f64, f64) = (1180.0, 760.0);

/// A window frame: the outer position and the inner size, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// The last frame of each view. Kept for this run only: the window opens wide at each
/// launch.
struct Frames {
    wide: Option<Frame>,
    notepad: Option<Frame>,
}
static FRAMES: Mutex<Frames> = Mutex::new(Frames { wide: None, notepad: None });

/// Windows has no app menu, so there is nothing to install.
pub fn install_menu(_app: &AppHandle) -> tauri::Result<()> {
    Ok(())
}

/// No menu item to keep in step with the width.
pub fn sync_menu(_app: &AppHandle) {}

/// Enter the notepad view from a wide window, or leave it.
pub fn toggle(app: &AppHandle) {
    switch(app, None);
}

/// Go to one view. A window already in it stays as it is.
pub fn set(app: &AppHandle, notepad: bool) {
    switch(app, Some(notepad));
}

fn switch(app: &AppHandle, want: Option<bool>) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = a.get_webview_window("main") else { return };
        // A full-screen or maximized window has no frame to change. Leave that state first.
        if window.is_fullscreen().unwrap_or(false) {
            let _ = window.set_fullscreen(false);
            return;
        }
        if window.is_maximized().unwrap_or(false) {
            let _ = window.unmaximize();
        }
        let (Ok(pos), Ok(size), Ok(scale)) = (window.outer_position(), window.inner_size(), window.scale_factor()) else { return };
        let frame = Frame { x: pos.x as f64, y: pos.y as f64, w: size.width as f64, h: size.height as f64 };
        let narrow = frame.w / scale < NARROW_BELOW;
        if want == Some(narrow) {
            return;
        }
        tracing::info!(to_notepad = !narrow, "notepad view");
        let Ok(Some(monitor)) = window.current_monitor() else { return };
        let area = monitor.work_area();
        let visible = Frame { x: area.position.x as f64, y: area.position.y as f64, w: area.size.width as f64, h: area.size.height as f64 };
        let target = {
            let mut frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
            if narrow {
                frames.notepad = Some(frame);
                frames.wide.unwrap_or_else(|| wide_from(frame, scale))
            } else {
                frames.wide = Some(frame);
                frames.notepad.unwrap_or_else(|| notepad_from(frame, scale))
            }
        };
        let target = fit(target, visible);
        let _ = window.set_size(PhysicalSize::new(target.w.round() as u32, target.h.round() as u32));
        let _ = window.set_position(PhysicalPosition::new(target.x.round() as i32, target.y.round() as i32));
    });
}

/// The first notepad frame: the same top, height and right edge as the wide window.
fn notepad_from(wide: Frame, scale: f64) -> Frame {
    let w = NOTEPAD_WIDTH * scale;
    Frame { x: wide.x + wide.w - w, y: wide.y, w, h: wide.h }
}

/// A wide frame when Margin has none: the default size, with the notepad's top and right
/// edge. `fit` then moves it onto the screen.
fn wide_from(notepad: Frame, scale: f64) -> Frame {
    let (w, h) = (WIDE_SIZE.0 * scale, (WIDE_SIZE.1 * scale).max(notepad.h));
    Frame { x: notepad.x + notepad.w - w, y: notepad.y, w, h }
}

/// The frame made to fit the work area of the monitor. A saved frame can be from another
/// monitor, or from before the taskbar moved.
fn fit(f: Frame, visible: Frame) -> Frame {
    let w = f.w.min(visible.w);
    let h = f.h.min(visible.h);
    let x = f.x.clamp(visible.x, visible.x + visible.w - w);
    let y = f.y.clamp(visible.y, visible.y + visible.h - h);
    Frame { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame { x, y, w, h }
    }

    #[test]
    fn the_notepad_keeps_the_right_edge_top_and_height() {
        let n = notepad_from(frame(200.0, 100.0, 1180.0, 760.0), 1.0);
        assert_eq!(n, frame(900.0, 100.0, 480.0, 760.0));
        assert!(n.w < NARROW_BELOW);
        // At 150% the notepad is 720 pixels wide, which is 480 points.
        let n = notepad_from(frame(300.0, 150.0, 1770.0, 1140.0), 1.5);
        assert_eq!(n, frame(1350.0, 150.0, 720.0, 1140.0));
    }

    #[test]
    fn a_wide_frame_from_nothing_keeps_the_top_and_fits_the_screen() {
        let screen = frame(0.0, 0.0, 1440.0, 875.0);
        // A tall notepad at the right edge of the screen.
        let w = fit(wide_from(frame(960.0, 20.0, 480.0, 820.0), 1.0), screen);
        assert_eq!((w.w, w.h), (1180.0, 820.0));
        assert_eq!(w.x + w.w, 1440.0);
        assert_eq!(w.y, 20.0);
        // A notepad near the left edge: the wide window moves right onto the screen.
        let w = fit(wide_from(frame(10.0, 20.0, 480.0, 600.0), 1.0), screen);
        assert_eq!(w.x, 0.0);
        assert!(w.w >= NARROW_BELOW);
    }

    #[test]
    fn fit_shrinks_a_frame_from_a_larger_display() {
        let f = fit(frame(-300.0, 900.0, 2000.0, 1200.0), frame(0.0, 0.0, 1440.0, 875.0));
        assert_eq!(f, frame(0.0, 0.0, 1440.0, 875.0));
    }
}
