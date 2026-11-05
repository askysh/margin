//! Menu-bar item: start and stop recordings without opening the window.

use crate::commands::ops;
use crate::events;
use crate::recorder::{recorder, Kind};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use std::time::Duration;
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main";
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray-template.png");
const ICON_REC: &[u8] = include_bytes!("../icons/tray-recording.png");

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let recording = recorder().is_recording();
    let onboarded = crate::prefs::load().onboarding_complete;
    let can_start = onboarded && !recording;
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "start-call", "Record call (mic + system audio)", can_start, None::<&str>)?,
            &MenuItem::with_id(app, "start-in-person", "Record in-person meeting", can_start, None::<&str>)?,
            &MenuItem::with_id(app, "stop", "Stop recording", recording, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open", "Open Margin", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit Margin", true, Some("CmdOrCtrl+Q"))?,
        ],
    )
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON_IDLE)?)
        .icon_as_template(true)
        .tooltip("Margin")
        .menu(&menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

pub fn refresh(app: &AppHandle) {
    let app2 = app.clone();
    // Menu objects must be touched on the main thread.
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = app2.tray_by_id(TRAY_ID) else { return };
        let recording = recorder().is_recording();
        if let Ok(m) = menu(&app2) {
            let _ = tray.set_menu(Some(m));
        }
        if let Ok(icon) = Image::from_bytes(if recording { ICON_REC } else { ICON_IDLE }) {
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_icon_as_template(true);
        }
        let _ = tray.set_tooltip(Some(if recording { "Margin: recording" } else { "Margin" }));
    });
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
