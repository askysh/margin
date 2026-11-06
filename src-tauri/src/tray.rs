//! Menu-bar item: start and stop recordings without opening the window, see how long the
//! recording has run (next to the icon), and open a recent meeting.

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
        menu.append(&MenuItem::with_id(app, "start-call", "Record call (mic + system audio)", can_start, None::<&str>)?)?;
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
    menu.append(&MenuItem::with_id(app, "open", "Open Margin", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit Margin", true, Some("CmdOrCtrl+Q"))?)?;
    *RECENT_PATHS.lock().unwrap() = state.recent.iter().map(|(p, _)| p.clone()).collect();
    Ok(menu)
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON_IDLE)?)
        .icon_as_template(true)
        .tooltip("Margin")
        .menu(&menu(app, &MenuState::default())?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle(app, event.id().as_ref()))
        .build(app)?;
    // Recent meetings need a folder listing; fill them in off the main thread.
    refresh(app);
    Ok(())
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
            let _ = tray.set_title(recorder().is_recording().then_some(text));
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
    if let Ok(icon) = Image::from_bytes(if recording { ICON_REC } else { ICON_IDLE }) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_icon_as_template(true);
    }
    let _ = tray.set_tooltip(Some(if recording { "Margin: recording" } else { "Margin" }));
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn handle(app: &AppHandle, id: &str) {
    match id {
        "start-call" | "start-in-person" => {
            let kind = if id == "start-call" { Kind::Call } else { Kind::InPerson };
            show_main(app);
            std::thread::spawn(move || {
                if let Err(error) = ops::start_recording(kind, None) {
                    events::emit("error-toast", error);
                }
            });
        }
        "stop" => {
            std::thread::spawn(|| {
                if let Err(error) = recorder().stop(None, None) {
                    events::emit("error-toast", error);
                }
            });
        }
        "open" => show_main(app),
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
            // Never lose a recording to Quit: finish saving it first, off the main thread.
            let app = app.clone();
            std::thread::spawn(move || {
                recorder().finish_before_exit(Duration::from_secs(120));
                app.exit(0);
            });
        }
        _ => {}
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
}
