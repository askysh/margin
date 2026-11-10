//! The recording chip (DECISIONS.md D53): a small pill on the right edge of the screen. It
//! shows while Margin records and the main window is not visible (hidden, minimized or
//! closed to the menu bar). It shows the elapsed time, and Stop and Notes on hover. A click
//! on it brings the window back on the live notes.
//!
//! The chip is a second webview window (`chip.html`). The page draws its states from the
//! same recorder events as the main window. This module decides when the window shows and
//! where it sits. The macOS calls go through objc2, which Tauri already uses.
//!
//! The window is a plain NSWindow, not a non-activating NSPanel: tao's window class keeps
//! its own state, so it cannot be changed to a panel after it is made. A click on the chip
//! makes Margin the active app. The page gives the focus back (`chip_release_focus`) when
//! the pointer leaves the chip or on Escape.

use crate::{prefs, recorder::recorder};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApp, NSEvent, NSScreen, NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "chip";
/// The page's slide-out takes 240 ms (150 ms with reduced motion). The window hides after it.
const SLIDE_OUT: Duration = Duration::from_millis(280);
/// After Stop, "Saving" shows at least this long, so the chip does not just blink away.
const SAVING_MIN: Duration = Duration::from_millis(1400);
/// The window at rest. The page sets the width to fit the pill (`chip_width`).
const WIDTH: f64 = 96.0;
const HEIGHT: f64 = 52.0;
/// Room left of the pill and above and below it for its shadow. The same as in chip.css.
const PAD_LEFT: f64 = 20.0;
const PAD_Y: f64 = 11.0;
/// About a third of the way down the screen.
pub const DEFAULT_PLACE: f64 = 0.33;

struct State {
    shown: bool,
    /// A slide-out runs. The window hides when it ends.
    hiding: bool,
    /// Counts shows and hides, so a late slide-out never hides a chip that came back.
    generation: u64,
    stopping_since: Option<Instant>,
    /// The window top when a drag started, in points below the top of the visible frame.
    drag_from: Option<f64>,
    pointer_inside: bool,
}

static STATE: Mutex<State> =
    Mutex::new(State { shown: false, hiding: false, generation: 0, stopping_since: None, drag_from: None, pointer_inside: false });
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Whether the chip shows. While Margin saves after Stop, a chip that shows stays (it says
/// "Saving"), and a chip that does not show does not appear.
pub fn should_show(enabled: bool, recording: bool, stopping: bool, main_visible: bool, shown: bool) -> bool {
    enabled && recording && !main_visible && (!stopping || shown)
}

/// The window top for a saved place, in points below the top of the visible frame. The
/// place is the middle of the window as a fraction of the visible height, so it survives a
/// change of display. The window always stays inside the visible frame.
pub fn top_for(place: f64, visible_h: f64, win_h: f64) -> f64 {
    let top = place.clamp(0.0, 1.0) * visible_h - win_h / 2.0;
    top.clamp(0.0, (visible_h - win_h).max(0.0))
}

/// The place to save for a window top. The inverse of `top_for`.
pub fn place_for(top: f64, visible_h: f64, win_h: f64) -> f64 {
    if visible_h <= 0.0 {
        return DEFAULT_PLACE;
    }
    ((top + win_h / 2.0) / visible_h).clamp(0.0, 1.0)
}

/// Look at the recorder and the main window, and show or hide the chip to match. Safe from
/// any thread: the work runs on the main thread.
pub fn sync(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || apply(&a));
}

/// Hide the main window, so only the chip shows. If Margin is the active app, it also hides
/// itself, so macOS gives the focus back to the app you were in, such as the call.
pub fn shrink(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if let Some(w) = a.get_webview_window("main") {
            let _ = w.hide();
        }
        if NSApp(mtm).isActive() {
            let _ = a.hide();
        }
        apply(&a);
    });
}

fn apply(app: &AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let enabled = prefs::load().recording_chip();
    let (recording, stopping) = recorder().status();
    let main_visible = main_visible(app, mtm);
    let mut st = STATE.lock().unwrap();
    if (stopping || (!recording && st.shown)) && st.stopping_since.is_none() {
        st.stopping_since = Some(Instant::now());
    }
    let hold = (!recording && st.shown).then(|| st.stopping_since.map(|t| SAVING_MIN.saturating_sub(t.elapsed()))).flatten();
    let holding = hold.is_some_and(|left| !left.is_zero()) && enabled && !main_visible;
    let want = should_show(enabled, recording, stopping, main_visible, st.shown) || holding;
    if want && !st.shown {
        show(app, mtm, &mut st);
    } else if !want && st.shown {
        hide(app, &mut st);
    }
    if holding {
        let left = hold.unwrap_or_default();
        let a = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(left + Duration::from_millis(20));
            sync(&a);
        });
    }
    if !recording && !st.shown {
        st.stopping_since = None;
        if !st.hiding {
            if let Some(w) = app.get_webview_window(LABEL) {
                let _ = w.destroy();
            }
        }
    }
}

fn main_visible(app: &AppHandle, mtm: MainThreadMarker) -> bool {
    if NSApp(mtm).isHidden() {
        return false;
    }
    app.get_webview_window("main").is_some_and(|w| ns(&w).is_some_and(|w| w.isVisible() && !w.isMiniaturized()))
}

fn ns(window: &WebviewWindow) -> Option<&NSWindow> {
    let ptr = window.ns_window().ok()?;
    // SAFETY: Tauri gives the NSWindow of a live window, and the reference lives no longer
    // than the WebviewWindow it came from.
    (!ptr.is_null()).then(|| unsafe { &*(ptr as *const NSWindow) })
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("chip.html".into()))
        .title("Margin recording")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .accept_first_mouse(true)
        .content_protected(crate::screenshare::hidden())
        .visible(false)
        .build()?;
    if let Some(w) = ns(&window) {
        // Above full-screen apps and on every Space, out of ⌘` and of Mission Control, and
        // still there when Margin hides itself.
        w.setLevel(NSStatusWindowLevel);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        w.setCanHide(false);
        w.setHidesOnDeactivate(false);
    }
    Ok(window)
}

fn show(app: &AppHandle, mtm: MainThreadMarker, st: &mut State) {
    let window = match app.get_webview_window(LABEL) {
        Some(w) => w,
        None => match build(app) {
            Ok(w) => w,
            Err(error) => {
                tracing::warn!(%error, "could not make the recording chip");
                return;
            }
        },
    };
    let Some(w) = ns(&window) else { return };
    // The display of the main window, where you pressed Start.
    let screen = app.get_webview_window("main").and_then(|m| ns(&m).and_then(|m| m.screen())).or_else(|| primary_screen(mtm));
    if let Some(screen) = screen {
        let vis = screen.visibleFrame();
        let frame = w.frame();
        let place = prefs::load().chip_place.unwrap_or(DEFAULT_PLACE);
        let top = top_for(place, vis.size.height, frame.size.height);
        let origin = NSPoint::new(vis.origin.x + vis.size.width - frame.size.width, vis.origin.y + vis.size.height - top - frame.size.height);
        w.setFrame_display(NSRect::new(origin, frame.size), true);
    }
    // Shown without becoming key, so the call app keeps the keyboard.
    w.orderFrontRegardless();
    st.shown = true;
    st.hiding = false;
    st.generation += 1;
    let _ = app.emit_to(LABEL, "chip-visible", true);
    watch_pointer(app);
}

fn hide(app: &AppHandle, st: &mut State) {
    st.shown = false;
    st.hiding = true;
    st.generation += 1;
    let generation = st.generation;
    let _ = app.emit_to(LABEL, "chip-visible", false);
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SLIDE_OUT);
        let b = a.clone();
        let _ = a.run_on_main_thread(move || {
            let mut st = STATE.lock().unwrap();
            if st.generation != generation {
                return;
            }
            st.hiding = false;
            if let Some(window) = b.get_webview_window(LABEL) {
                if let Some(w) = ns(&window) {
                    w.orderOut(None);
                }
                if !recorder().is_recording() {
                    let _ = window.destroy();
                }
            }
        });
    });
}

fn primary_screen(mtm: MainThreadMarker) -> Option<objc2::rc::Retained<NSScreen>> {
    NSScreen::screens(mtm).firstObject()
}

/// The visible frame of the screen the chip is on.
fn chip_screen(w: &NSWindow, mtm: MainThreadMarker) -> Option<NSRect> {
    w.screen().or_else(|| primary_screen(mtm)).map(|s| s.visibleFrame())
}

/// The chip's top, in points below the top of the visible frame.
fn top_of(w: &NSWindow, vis: NSRect) -> f64 {
    let f = w.frame();
    vis.origin.y + vis.size.height - (f.origin.y + f.size.height)
}

fn move_to(w: &NSWindow, vis: NSRect, top: f64) -> f64 {
    let f = w.frame();
    let top = top.clamp(0.0, (vis.size.height - f.size.height).max(0.0));
    let x = vis.origin.x + vis.size.width - f.size.width;
    w.setFrameOrigin(NSPoint::new(x, vis.origin.y + vis.size.height - top - f.size.height));
    top
}

fn save_place(place: f64) {
    let mut p = prefs::load();
    p.chip_place = Some(place);
    if let Err(error) = prefs::save(&p) {
        tracing::warn!(%error, "could not save the chip's place");
    }
}

/// Run `f` with the chip's NSWindow, on the main thread (sync commands run there).
fn with_chip<T>(app: &AppHandle, f: impl FnOnce(&NSWindow, MainThreadMarker) -> T) -> Option<T> {
    let mtm = MainThreadMarker::new()?;
    let window = app.get_webview_window(LABEL)?;
    let w = ns(&window)?;
    Some(f(w, mtm))
}

/// Tells the page where the pointer is, about 10 times a second, while the chip shows.
/// WebKit may send no hover events to a window of an app that is not active, and the call
/// app is the active one. This makes hover work all the same.
fn watch_pointer(app: &AppHandle) {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        while STATE.lock().unwrap().shown {
            let a = app.clone();
            let _ = app.run_on_main_thread(move || {
                let inside = with_chip(&a, |w, _| {
                    let f = w.frame();
                    let p = NSEvent::mouseLocation();
                    p.x >= f.origin.x + PAD_LEFT - 2.0
                        && p.x <= f.origin.x + f.size.width
                        && p.y >= f.origin.y + PAD_Y
                        && p.y <= f.origin.y + f.size.height - PAD_Y
                })
                .unwrap_or(false);
                let mut st = STATE.lock().unwrap();
                if st.pointer_inside != inside {
                    st.pointer_inside = inside;
                    let _ = a.emit_to(LABEL, "chip-pointer", inside);
                }
            });
            std::thread::sleep(Duration::from_millis(100));
        }
        STATE.lock().unwrap().pointer_inside = false;
        WATCHING.store(false, Ordering::SeqCst);
    });
}

/// The page asks once it listens: does the chip show now?
#[tauri::command]
pub fn chip_hello() -> bool {
    STATE.lock().unwrap().shown
}

/// The window follows the pill's width, anchored to the right edge, so no empty part of it
/// takes clicks meant for the app behind.
#[tauri::command]
pub fn chip_width(app: AppHandle, width: f64) {
    with_chip(&app, |w, _| {
        let mut f = w.frame();
        let width = width.clamp(40.0, 420.0).round();
        if (f.size.width - width).abs() < 0.5 {
            return;
        }
        f.origin.x += f.size.width - width;
        f.size = NSSize::new(width, f.size.height);
        w.setFrame_display(f, true);
    });
}

#[tauri::command]
pub fn chip_drag_start(app: AppHandle) {
    let top = with_chip(&app, |w, mtm| chip_screen(w, mtm).map(|vis| top_of(w, vis))).flatten();
    STATE.lock().unwrap().drag_from = top;
}

/// `dy`: points the pointer moved down since the drag started. Only y changes.
#[tauri::command]
pub fn chip_drag(app: AppHandle, dy: f64) {
    let Some(from) = STATE.lock().unwrap().drag_from else { return };
    with_chip(&app, |w, mtm| {
        if let Some(vis) = chip_screen(w, mtm) {
            move_to(w, vis, from + dy);
        }
    });
}

#[tauri::command]
pub fn chip_drag_end(app: AppHandle) {
    STATE.lock().unwrap().drag_from = None;
    let place = with_chip(&app, |w, mtm| chip_screen(w, mtm).map(|vis| place_for(top_of(w, vis), vis.size.height, w.frame().size.height))).flatten();
    if let Some(place) = place {
        save_place(place);
    }
}

/// The arrow keys: move by `dy` points and keep the place.
#[tauri::command]
pub fn chip_nudge(app: AppHandle, dy: f64) {
    let place = with_chip(&app, |w, mtm| {
        chip_screen(w, mtm).map(|vis| {
            let top = move_to(w, vis, top_of(w, vis) + dy);
            place_for(top, vis.size.height, w.frame().size.height)
        })
    })
    .flatten();
    if let Some(place) = place {
        save_place(place);
    }
}

/// A click on the chip made Margin the active app. Give the focus back to the app you were
/// in. Only while the main window is hidden: Margin hides itself to do it.
#[tauri::command]
pub fn chip_release_focus(app: AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if NSApp(mtm).isActive() && !main_visible(&app, mtm) {
        let _ = app.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_while_recording_with_the_window_away() {
        assert!(should_show(true, true, false, false, false));
        assert!(!should_show(true, true, false, true, false), "the window is visible");
        assert!(!should_show(false, true, false, false, false), "the setting is off");
        assert!(!should_show(true, false, false, false, true), "not recording");
        // Saving after Stop: a chip that shows stays, and one that does not stays away.
        assert!(should_show(true, true, true, false, true));
        assert!(!should_show(true, true, true, false, false));
        assert!(!should_show(true, true, true, true, true), "the window came back");
    }

    #[test]
    fn place_is_a_fraction_of_the_visible_height() {
        let (vis, h) = (900.0, 52.0);
        assert_eq!(top_for(DEFAULT_PLACE, vis, h), 0.33 * 900.0 - 26.0);
        // Kept inside the visible frame.
        assert_eq!(top_for(0.0, vis, h), 0.0);
        assert_eq!(top_for(1.0, vis, h), vis - h);
        assert_eq!(top_for(7.0, vis, h), vis - h);
        // A round trip keeps the place, and a smaller display keeps the fraction.
        let top = top_for(0.6, vis, h);
        assert!((place_for(top, vis, h) - 0.6).abs() < 1e-9);
        assert!((place_for(top_for(0.6, 600.0, h), 600.0, h) - 0.6).abs() < 1e-9);
        assert_eq!(place_for(10.0, 0.0, h), DEFAULT_PLACE);
    }
}
