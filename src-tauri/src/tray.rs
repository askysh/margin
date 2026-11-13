//! Menu-bar item: start and stop recordings without opening the window, see how long the
//! recording has run (next to the icon), and open a recent meeting.
//!
//! On Windows (D61) this is the icon in the notification area. The name is Margin Notes.
//! A click opens the window, and a right-click opens the menu, as Windows users expect.
//! Windows shows no text next to the icon, so the elapsed time is in the tooltip. The
//! icon is the app icon, with a red dot while Margin records: a black template image
//! does not show on a dark taskbar.

use crate::commands::ops;
use crate::events;
use crate::recorder::{recorder, Kind};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main";
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray-template.png");
const ICON_REC: &[u8] = include_bytes!("../icons/tray-recording.png");
#[cfg(windows)]
const ICON_WINDOWS: &[u8] = include_bytes!("../icons/32x32.png");
/// The name the app shows: Margin on macOS, Margin Notes on Windows (D61).
pub const APP_NAME: &str = if cfg!(windows) { "Margin Notes" } else { "Margin" };
const RECENT: usize = 5;
const RECENT_PREFIX: &str = "recent:";

/// What the menu shows, gathered off the main thread (listing meetings reads files).
#[derive(Default)]
struct MenuState {
    recording: bool,
    title: Option<String>,
    onboarded: bool,
    /// A downloaded update waiting for a restart.
    update: Option<String>,
    /// (path, label), newest first.
    recent: Vec<(String, String)>,
}

/// Paths behind the "recent:<n>" items of the menu now showing.
static RECENT_PATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The clock shown next to the icon, so it is set only when it changes.
static SHOWN_CLOCK: Mutex<Option<String>> = Mutex::new(None);

fn gather() -> MenuState {
    let view = recorder().view();
    let onboarded = crate::prefs::load().onboarding_complete;
    let recent = if onboarded {
        crate::library::list().into_iter().take(RECENT).map(|m| (m.path, recent_label(&m.title, m.date.as_deref()))).collect()
    } else {
        vec![]
    };
    MenuState { recording: view.recording, title: view.title, onboarded, update: crate::updater::ready_version(), recent }
}

fn recent_label(title: &str, date: Option<&str>) -> String {
    let title: String = if title.chars().count() > 40 { format!("{}…", title.chars().take(39).collect::<String>()) } else { title.to_string() };
    match date.and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()) {
        Some(d) => format!("{title}  ·  {}", d.with_timezone(&chrono::Local).format("%-d %b")),
        None => title,
    }
}

fn menu(app: &AppHandle, state: &MenuState) -> tauri::Result<Menu<Wry>> {
    let can_start = state.onboarded && !state.recording;
    let menu = Menu::new(app)?;
    if state.recording {
        let label = format!("Recording: {}", state.title.as_deref().unwrap_or("Untitled meeting"));
        menu.append(&MenuItem::with_id(app, "now", label, false, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, "stop", "Stop recording", true, None::<&str>)?)?;
    } else {
        // Windows cannot record a call yet (D61), so the menu does not offer it.
        if !cfg!(windows) {
            menu.append(&MenuItem::with_id(app, "start-call", "Record call (mic + system audio)", can_start, None::<&str>)?)?;
        }
        menu.append(&MenuItem::with_id(app, "start-in-person", "Record in-person meeting", can_start, None::<&str>)?)?;
    }
    if !state.recent.is_empty() {
        let recent = Submenu::with_id(app, "recent", "Recent meetings", true)?;
        for (i, (_, label)) in state.recent.iter().enumerate() {
            recent.append(&MenuItem::with_id(app, format!("{RECENT_PREFIX}{i}"), label, true, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&recent)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    if let Some(version) = &state.update {
        // Not while recording: the restart would cut the meeting.
        menu.append(&MenuItem::with_id(app, "update", format!("Restart to update to {version}"), !state.recording, None::<&str>)?)?;
    }
    menu.append(&MenuItem::with_id(app, "open", format!("Open {APP_NAME}"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "open-notepad", "Open as Notepad", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?)?;
    // A shortcut in this menu works on macOS only: there it is also in the app menu.
    let quit_key = (!cfg!(windows)).then_some("CmdOrCtrl+Q");
    menu.append(&MenuItem::with_id(app, "quit", format!("Quit {APP_NAME}"), true, quit_key)?)?;
    *RECENT_PATHS.lock().unwrap() = state.recent.iter().map(|(p, _)| p.clone()).collect();
    Ok(menu)
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false)?)
        .icon_as_template(true)
        .tooltip(APP_NAME)
        .menu(&menu(app, &MenuState::default())?)
        .show_menu_on_left_click(!cfg!(windows))
        .on_menu_event(|app, event| handle(app, event.id().as_ref()));
    #[cfg(windows)]
    let builder = builder.on_tray_icon_event(|tray, event| {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            show_main(tray.app_handle());
        }
    });
    builder.build(app)?;
    // Recent meetings need a folder listing; fill them in off the main thread.
    refresh(app);
    Ok(())
}

#[cfg(not(windows))]
fn icon(recording: bool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if recording { ICON_REC } else { ICON_IDLE })
}

/// Windows: the app icon, with a red dot in the lower right corner while Margin records.
#[cfg(windows)]
fn icon(recording: bool) -> tauri::Result<Image<'static>> {
    let _ = (ICON_IDLE, ICON_REC);
    let base = Image::from_bytes(ICON_WINDOWS)?;
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    if recording {
        red_dot(&mut rgba, w, h);
    }
    Ok(Image::new_owned(rgba, w, h))
}

/// Draws a red dot with a white ring on an RGBA image, in the lower right corner.
#[cfg(any(windows, test))]
fn red_dot(rgba: &mut [u8], w: u32, h: u32) {
    let r = w.min(h) as f32 * 0.25;
    let (cx, cy) = (w as f32 - r - 1.0, h as f32 - r - 1.0);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let colour = if d <= r - 1.5 { [226, 60, 50, 255] } else if d <= r { [255, 255, 255, 255] } else { continue };
            let at = ((y * w + x) * 4) as usize;
            if let Some(pixel) = rgba.get_mut(at..at + 4) {
                pixel.copy_from_slice(&colour);
            }
        }
    }
}

pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = gather();
        let app2 = app.clone();
        // Menu objects must be touched on the main thread.
        let _ = app.run_on_main_thread(move || apply(&app2, &state));
    });
}

/// Elapsed time next to the menu-bar icon while recording, from the recorder's ticks.
pub fn tick(app: &AppHandle, elapsed_secs: u64) {
    let text = clock(elapsed_secs);
    {
        let mut shown = SHOWN_CLOCK.lock().unwrap();
        if shown.as_deref() == Some(text.as_str()) {
            return;
        }
        *shown = Some(text.clone());
    }
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = app2.tray_by_id(TRAY_ID) {
            // The recorder may have stopped since the tick was sent.
            #[cfg(not(windows))]
            let _ = tray.set_title(recorder().is_recording().then_some(text));
            // Windows has no text next to the icon. The tooltip has the time.
            #[cfg(windows)]
            if recorder().is_recording() {
                let _ = tray.set_tooltip(Some(format!("{APP_NAME}: recording {text}")));
            }
        }
    });
}

fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn apply(app: &AppHandle, state: &MenuState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let recording = state.recording;
    if let Ok(m) = menu(app, state) {
        let _ = tray.set_menu(Some(m));
    }
    if !recording {
        *SHOWN_CLOCK.lock().unwrap() = None;
        let _ = tray.set_title(None::<String>);
    }
    if let Ok(icon) = icon(recording) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_icon_as_template(true);
    }
    let _ = tray.set_tooltip(Some(if recording { format!("{APP_NAME}: recording") } else { APP_NAME.to_string() }));
}

pub fn show_main(app: &AppHandle) {
    // Margin hides itself when the recording chip takes over (chip.rs).
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    crate::chip::sync(app);
}

/// Start from outside the window: the menu bar, the global shortcut or a notification.
/// With the recording chip on, the window stays away and the chip shows. Otherwise the
/// notepad comes forward, as before. An error always brings the window, so you see it.
pub fn start_outside(app: &AppHandle, kind: Kind) {
    let chip = crate::prefs::load().recording_chip();
    if !chip {
        show_main(app);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(error) = ops::start_recording(kind, None) {
            if chip {
                show_main(&app);
            }
            events::emit("error-toast", error);
        }
    });
}

fn handle(app: &AppHandle, id: &str) {
    match id {
        "start-call" | "start-in-person" => {
            start_outside(app, if id == "start-call" { Kind::Call } else { Kind::InPerson });
        }
        "stop" => {
            std::thread::spawn(|| {
                if let Err(error) = recorder().stop(None, None) {
                    events::emit("error-toast", error);
                }
            });
        }
        "open" => show_main(app),
        "open-notepad" => {
            show_main(app);
            crate::notepad::set(app, true);
        }
        "settings" => {
            show_main(app);
            events::emit("open-settings", ());
        }
        "update" => {
            let app = app.clone();
            std::thread::spawn(move || {
                if let Err(error) = crate::updater::install_and_restart(&app) {
                    events::emit("error-toast", error);
                }
            });
        }
        id if id.starts_with(RECENT_PREFIX) => {
            let path = id[RECENT_PREFIX.len()..].parse::<usize>().ok().and_then(|i| RECENT_PATHS.lock().unwrap().get(i).cloned());
            if let Some(path) = path {
                show_main(app);
                events::emit("open-meeting", path);
            }
        }
        "quit" => {
            let app = app.clone();
            std::thread::spawn(move || quit(&app));
        }
        _ => {}
    }
}

/// Quit from the menu or with ⌘Q. Run it off the main thread: it waits for the recording
/// to save, and the question about processing blocks until you answer.
pub fn quit(app: &AppHandle) {
    // Never lose a recording to Quit: finish saving it first.
    recorder().finish_before_exit(Duration::from_secs(120));
    if crate::processing::confirm_quit(app) {
        app.exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_labels() {
        assert_eq!(clock(5), "0:05");
        assert_eq!(clock(754), "12:34");
        assert_eq!(clock(3725), "1:02:05");
        let long = "a".repeat(60);
        assert_eq!(recent_label(&long, None).chars().count(), 40);
        assert!(recent_label("Weekly", Some("2026-11-06T10:15:04+05:30")).starts_with("Weekly  ·  "));
        assert_eq!(recent_label("Weekly", Some("not a date")), "Weekly");
    }

    #[test]
    fn the_red_dot_is_in_the_lower_right_corner() {
        let mut rgba = vec![0u8; 32 * 32 * 4];
        red_dot(&mut rgba, 32, 32);
        let pixel = |x: usize, y: usize| &rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(pixel(23, 23), [226, 60, 50, 255]);
        assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(8, 23), [0, 0, 0, 0]);
    }
}
