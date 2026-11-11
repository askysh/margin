//! The notepad view: the main window at a narrow width, beside a call or a browser (D51,
//! D57). The width alone decides the layout, so a resize by hand still works. This module
//! adds a direct way in and out: View → Enter Notepad View (⌘⇧N), the window's own
//! button and the menu bar's "Open as Notepad". Leaving puts the window back where it was.

use objc2_app_kit::NSWindow;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Manager, WebviewWindow, Wry};

/// Under this width the page is the notepad. It must match `COMPACT_QUERY` in
/// src/components/Compact.tsx.
pub const NARROW_BELOW: f64 = 760.0;
/// The notepad's first width. The window can go down to 440 (tauri.conf.json).
const NOTEPAD_WIDTH: f64 = 480.0;
/// The wide size when Margin has no earlier one: the size the window opens at.
const WIDE_SIZE: (f64, f64) = (1180.0, 760.0);
const MENU_ID: &str = "notepad-view";

/// The last frame of each view, in screen points. Kept for this run only: the window opens
/// wide at each launch.
#[derive(Default)]
struct Frames {
    wide: Option<NSRect>,
    notepad: Option<NSRect>,
}
static FRAMES: Mutex<Frames> = Mutex::new(Frames { wide: None, notepad: None });

/// The View menu item, kept so its title can follow the window width.
struct ViewItem(MenuItem<Wry>);

/// The app menu: macOS's standard menus, with the notepad item first in View.
pub fn install_menu(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::default(app)?;
    let item = MenuItem::with_id(app, MENU_ID, "Enter Notepad View", true, Some("CmdOrCtrl+Shift+N"))?;
    for entry in menu.items()? {
        if let MenuItemKind::Submenu(sub) = entry {
            if sub.text()? == "View" {
                sub.insert(&item, 0)?;
                sub.insert(&PredefinedMenuItem::separator(app)?, 1)?;
            }
        }
    }
    app.set_menu(menu)?;
    app.manage(ViewItem(item));
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == MENU_ID {
            toggle(app);
        }
    });
    sync_menu(app);
    Ok(())
}

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
        // A full-screen window has no frame to change. Leave full screen first.
        if window.is_fullscreen().unwrap_or(false) {
            let _ = window.set_fullscreen(false);
            return;
        }
        let Some(w) = ns(&window) else { return };
        let frame = w.frame();
        let narrow = frame.size.width < NARROW_BELOW;
        if want == Some(narrow) {
            return;
        }
        tracing::info!(to_notepad = !narrow, "notepad view");
        let Some(visible) = w.screen().map(|s| s.visibleFrame()) else { return };
        let target = {
            let mut frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
            if narrow {
                frames.notepad = Some(frame);
                frames.wide.unwrap_or_else(|| wide_from(frame))
            } else {
                frames.wide = Some(frame);
                frames.notepad.unwrap_or_else(|| notepad_from(frame))
            }
        };
        w.setFrame_display_animate(fit(target, visible), true, true);
    });
}

/// The first notepad frame: the same top, bottom and right edge as the wide window.
fn notepad_from(wide: NSRect) -> NSRect {
    NSRect::new(NSPoint::new(wide.origin.x + wide.size.width - NOTEPAD_WIDTH, wide.origin.y), NSSize::new(NOTEPAD_WIDTH, wide.size.height))
}

/// A wide frame when Margin has none: the default size, with the notepad's top and right
/// edge. `fit` then moves it onto the screen.
fn wide_from(notepad: NSRect) -> NSRect {
    let (w, h) = (WIDE_SIZE.0, WIDE_SIZE.1.max(notepad.size.height));
    let top = notepad.origin.y + notepad.size.height;
    NSRect::new(NSPoint::new(notepad.origin.x + notepad.size.width - w, top - h), NSSize::new(w, h))
}

/// The frame made to fit the visible part of the screen. A saved frame can be from another
/// display, or from before the Dock moved.
fn fit(f: NSRect, visible: NSRect) -> NSRect {
    let w = f.size.width.min(visible.size.width);
    let h = f.size.height.min(visible.size.height);
    let x = f.origin.x.clamp(visible.origin.x, visible.origin.x + visible.size.width - w);
    let y = f.origin.y.clamp(visible.origin.y, visible.origin.y + visible.size.height - h);
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// The View menu says what the item does now. Called after each resize.
pub fn sync_menu(app: &AppHandle) {
    let (Some(window), Some(item)) = (app.get_webview_window("main"), app.try_state::<ViewItem>()) else { return };
    let narrow = match (window.inner_size(), window.scale_factor()) {
        (Ok(size), Ok(scale)) => (size.width as f64 / scale) < NARROW_BELOW,
        _ => return,
    };
    let _ = item.0.set_text(if narrow { "Leave Notepad View" } else { "Enter Notepad View" });
}

fn ns(window: &WebviewWindow) -> Option<&NSWindow> {
    let ptr = window.ns_window().ok()?;
    // SAFETY: Tauri gives the NSWindow of a live window, and the reference lives no longer
    // than the WebviewWindow it came from.
    (!ptr.is_null()).then(|| unsafe { &*(ptr as *const NSWindow) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    #[test]
    fn the_notepad_keeps_the_right_edge_top_and_height() {
        let n = notepad_from(rect(200.0, 100.0, 1180.0, 760.0));
        assert_eq!((n.origin.x, n.origin.y, n.size.width, n.size.height), (900.0, 100.0, 480.0, 760.0));
        assert!(n.size.width < NARROW_BELOW);
    }

    #[test]
    fn a_wide_frame_from_nothing_keeps_the_top_and_fits_the_screen() {
        let screen = rect(0.0, 0.0, 1440.0, 875.0);
        // A tall notepad at the right edge of the screen.
        let w = fit(wide_from(rect(960.0, 20.0, 480.0, 820.0)), screen);
        assert_eq!((w.size.width, w.size.height), (1180.0, 820.0));
        assert_eq!(w.origin.x + w.size.width, 1440.0);
        assert_eq!(w.origin.y + w.size.height, 840.0);
        // A notepad near the left edge: the wide window moves right onto the screen.
        let w = fit(wide_from(rect(10.0, 20.0, 480.0, 600.0)), screen);
        assert_eq!(w.origin.x, 0.0);
        assert!(w.size.width >= NARROW_BELOW);
    }

    #[test]
    fn fit_shrinks_a_frame_from_a_larger_display() {
        let f = fit(rect(-300.0, 900.0, 2000.0, 1200.0), rect(0.0, 0.0, 1440.0, 875.0));
        assert_eq!((f.origin.x, f.origin.y, f.size.width, f.size.height), (0.0, 0.0, 1440.0, 875.0));
    }
}
